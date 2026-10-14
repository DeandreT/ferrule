use std::collections::{BTreeMap, BTreeSet};

use super::parse_u32;

#[derive(Clone, Copy, PartialEq, Eq)]
enum JsonPortRole {
    Scalar(ir::ScalarType),
    ArrayObjectFrame,
}

/// Declaration facts for bounded proofs. Ordinary port normalization is unchanged.
#[derive(Default)]
pub(in crate::import) struct PortDeclarations {
    inputs: BTreeSet<u32>,
    outputs: BTreeSet<u32>,
    ambiguous: BTreeSet<u32>,
    ambiguous_paths: BTreeMap<u32, Vec<Vec<String>>>,
    unlowered: BTreeSet<u32>,
    json_roles: BTreeMap<u32, Option<JsonPortRole>>,
    complete_x12_config: bool,
    occurrence: Option<super::occurrence_provenance::RawOccurrenceOwner>,
}

impl PortDeclarations {
    pub(in crate::import) fn complete_x12_config(&self) -> bool {
        self.complete_x12_config
    }

    pub(in crate::import) fn occurrence(&self) -> Option<&super::occurrence_provenance::RawOccurrenceOwner> {
        self.occurrence.as_ref()
    }

    pub(in crate::import) fn json_scalar(&self, key: u32, ty: ir::ScalarType) -> bool {
        self.json_roles.get(&key) == Some(&Some(JsonPortRole::Scalar(ty)))
    }

    pub(in crate::import) fn json_array_object_frame(&self, key: u32) -> bool {
        self.json_roles.get(&key) == Some(&Some(JsonPortRole::ArrayObjectFrame))
    }

    pub(super) fn declares_output(&self, key: u32) -> bool {
        self.outputs.contains(&key)
    }
    pub(super) fn unambiguous_input(&self, key: u32) -> bool {
        self.inputs.contains(&key) && !self.ambiguous.contains(&key)
    }

    pub(super) fn unambiguous_output(&self, key: u32) -> bool {
        self.outputs.contains(&key) && !self.ambiguous.contains(&key)
    }

    pub(super) fn ambiguous_input_affects(&self, key: u32, path: &[String]) -> bool {
        self.inputs.contains(&key)
            && self.ambiguous.contains(&key)
            && (self.unlowered.contains(&key)
                || self.ambiguous_paths.get(&key).is_some_and(|paths| {
                    paths
                        .iter()
                        .any(|declared| declared.starts_with(path) || path.starts_with(declared))
                }))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Direction {
    Input,
    Output,
}

impl Direction {
    fn attribute(self) -> &'static str {
        match self {
            Self::Input => "inpkey",
            Self::Output => "outkey",
        }
    }
}

struct Declaration<'doc, 'input> {
    entry: roxmltree::Node<'doc, 'input>,
    path: Vec<String>,
    direction: Direction,
}

#[derive(Default)]
pub(in crate::import) struct PortDeclarationCensus<'doc, 'input> {
    first: BTreeMap<u32, Declaration<'doc, 'input>>,
    visited: BTreeSet<(usize, Direction)>,
    facts: PortDeclarations,
}

impl<'doc, 'input> PortDeclarationCensus<'doc, 'input> {
    /// Only successful physical X12 configuration compilation supplies this
    /// import-only witness. Entry-tree and embedded fallback schemas cannot.
    pub(in crate::import) fn mark_complete_x12_config(&mut self) {
        self.facts.complete_x12_config = true;
    }

    /// Called from the actual port walk, before it overwrites the normalized map.
    pub(in crate::import) fn record(
        &mut self,
        entry: roxmltree::Node<'doc, 'input>,
        path: &[String],
    ) {
        for direction in [Direction::Input, Direction::Output] {
            let Some(key) = parse_u32(entry.attribute(direction.attribute())) else {
                continue;
            };
            self.visited.insert((entry.range().start, direction));
            let role = json_port_role(entry);
            self.facts.json_roles.entry(key).and_modify(|previous| {
                if *previous != role { *previous = None; }
            }).or_insert(role);
            match direction {
                Direction::Input => self.facts.inputs.insert(key),
                Direction::Output => self.facts.outputs.insert(key),
            };
            if let Some(first) = self.first.get(&key) {
                if first.path != path
                    || first.direction != direction
                    || !same_raw_role(first.entry, entry, direction)
                {
                    self.facts.ambiguous.insert(key);
                    let paths = self.facts.ambiguous_paths.entry(key).or_default();
                    for path in [&first.path[..], path] {
                        if !paths.iter().any(|existing| existing == path) {
                            paths.push(path.to_vec());
                        }
                    }
                }
            } else {
                self.first.insert(
                    key,
                    Declaration {
                        entry,
                        path: path.to_vec(),
                        direction,
                    },
                );
            }
        }
    }

    /// First-entry and wrapper normalization may omit saved declarations. Such
    /// declarations cannot establish a bounded proof, even if the key survived
    /// in the reader's direction sets through another entry.
    pub(in crate::import) fn finish(
        mut self,
        root: roxmltree::Node<'doc, 'input>,
    ) -> PortDeclarations {
        self.facts.occurrence = root.ancestors()
            .find(|node| node.has_tag_name("component"))
            .and_then(super::occurrence_provenance::capture);
        for entry in root
            .descendants()
            .filter(|node| node.is_element() && node.tag_name().name() == "entry")
        {
            for direction in [Direction::Input, Direction::Output] {
                let Some(key) = parse_u32(entry.attribute(direction.attribute())) else {
                    continue;
                };
                if !self.visited.contains(&(entry.range().start, direction)) {
                    self.facts.ambiguous.insert(key);
                    self.facts.unlowered.insert(key);
                    match direction {
                        Direction::Input => self.facts.inputs.insert(key),
                        Direction::Output => self.facts.outputs.insert(key),
                    };
                }
            }
        }
        self.facts
    }
}

fn entry_children<'doc, 'input>(
    node: roxmltree::Node<'doc, 'input>,
) -> Vec<roxmltree::Node<'doc, 'input>> {
    node.children().filter(|child| child.has_tag_name("entry")).collect()
}

/// Keep raw wrapper evidence separate from the reader's transparent JSON path.
/// A duplicate object view is an occurrence only within one ordinary array/item.
pub(super) fn ordinary_json_object_item(object: roxmltree::Node<'_, '_>) -> bool {
    if object.attribute("name") != Some("object") || object.attribute("type").is_some()
        || object.ancestors().any(|node| node.attribute("type") == Some("json-subtype"))
    {
        return false;
    }
    let Some(item) = object.parent() else { return false; };
    let Some(array) = item.parent() else { return false; };
    let Some(property) = array.parent() else { return false; };
    item.has_tag_name("entry") && item.attribute("name") == Some("item")
        && item.attribute("type") == Some("json-item")
        && array.has_tag_name("entry") && array.attribute("name") == Some("array")
        && array.attribute("type").is_none()
        && entry_children(array).as_slice() == [item]
        && property.has_tag_name("entry") && property.attribute("type") == Some("json-property")
        && entry_children(property).as_slice() == [array]
        && entry_children(item).iter().all(|child| {
            child.attribute("name") == Some("object") && child.attribute("type").is_none()
        })
}

fn json_port_role(entry: roxmltree::Node<'_, '_>) -> Option<JsonPortRole> {
    if entry.attribute("type").is_some()
        || entry.ancestors().any(|node| node.attribute("type") == Some("json-subtype"))
    {
        return None;
    }
    let ty = match entry.attribute("name")? {
        "boolean" => Some(ir::ScalarType::Bool),
        "integer" => Some(ir::ScalarType::Int),
        "string" => Some(ir::ScalarType::String),
        _ => None,
    };
    if let Some(ty) = ty {
        let property = entry.parent()?;
        return (property.attribute("type") == Some("json-property")
            && entry_children(property).as_slice() == [entry]
            && entry_children(entry).is_empty())
            .then_some(JsonPortRole::Scalar(ty));
    }
    let object = if entry.attribute("name") == Some("array") {
        let children = entry_children(entry);
        let [item] = children.as_slice() else { return None; };
        let objects = entry_children(*item);
        let [object] = objects.as_slice() else { return None; };
        *object
    } else {
        entry
    };
    (ordinary_json_object_item(object) && entry_children(object.parent()?).len() == 1)
        .then_some(JsonPortRole::ArrayObjectFrame)
}

fn same_raw_role(
    left: roxmltree::Node<'_, '_>,
    right: roxmltree::Node<'_, '_>,
    direction: Direction,
) -> bool {
    let mut left_roles = std::iter::once(left).chain(left.ancestors().filter(|node| {
        node.is_element()
            && node.tag_name().name() == "entry"
            && node.range().start != left.range().start
    }));
    let mut right_roles = std::iter::once(right).chain(right.ancestors().filter(|node| {
        node.is_element()
            && node.tag_name().name() == "entry"
            && node.range().start != right.range().start
    }));
    let mut first = true;
    loop {
        match (left_roles.next(), right_roles.next()) {
            (None, None) => return true,
            (Some(left), Some(right)) => {
                let ignored = first.then_some(direction.attribute());
                let retained = |attr: &roxmltree::Attribute<'_, '_>| {
                    !(attr.namespace().is_none() && Some(attr.name()) == ignored)
                };
                if left.tag_name().name() != right.tag_name().name()
                    || left.tag_name().namespace() != right.tag_name().namespace()
                    || left.attributes().filter(retained).count()
                        != right.attributes().filter(retained).count()
                    || left.attributes().filter(retained).any(|attr| {
                        !right.attributes().any(|other| {
                            attr.name() == other.name()
                                && attr.namespace() == other.namespace()
                                && attr.value() == other.value()
                        })
                    })
                {
                    return false;
                }
                first = false;
            }
            _ => return false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(text: &str, paths: &[&[&str]]) -> PortDeclarations {
        let document = roxmltree::Document::parse(text).unwrap();
        let entries = document
            .root_element()
            .children()
            .filter(|node| node.has_tag_name("entry"));
        let mut census = PortDeclarationCensus::default();
        for (entry, path) in entries.zip(paths) {
            census.record(
                entry,
                &path
                    .iter()
                    .map(|part| String::from(*part))
                    .collect::<Vec<_>>(),
            );
        }
        census.finish(document.root_element())
    }

    #[test]
    fn identical_saved_references_share_one_physical_declaration() {
        let facts = capture(
            r#"<root><entry name="Value" type="attribute" outkey="7"/><entry outkey="7" type="attribute" name="Value"/></root>"#,
            &[&["Value"], &["Value"]],
        );
        assert!(facts.unambiguous_output(7));
    }

    #[test]
    fn differing_paths_roles_or_directions_cannot_be_normalized_into_one_proof() {
        for (text, paths) in [
            (
                r#"<root><entry name="First" outkey="7"/><entry name="Second" outkey="7"/></root>"#,
                [&["First"][..], &["Second"][..]],
            ),
            (
                r#"<root><entry name="Value" type="attribute" outkey="7"/><entry name="Value" outkey="7"/></root>"#,
                [&["Value"][..], &["Value"][..]],
            ),
            (
                r#"<root><entry name="Value" outkey="7"/><entry name="Value" inpkey="7"/></root>"#,
                [&["Value"][..], &["Value"][..]],
            ),
        ] {
            let facts = capture(text, &paths);
            assert!(!facts.unambiguous_output(7), "{text}");
            assert!(!facts.unambiguous_input(7), "{text}");
        }
    }

    #[test]
    fn differing_connected_input_paths_remain_ambiguous() {
        let facts = capture(
            r#"<root><entry name="First" inpkey="7"/><entry name="Second" inpkey="7"/></root>"#,
            &[&["First"], &["Second"]],
        );
        assert!(!facts.unambiguous_input(7));
        assert!(facts.ambiguous_input_affects(7, &[String::from("First")]));
        assert!(!facts.ambiguous_input_affects(7, &[String::from("Unrelated")]));
    }

    #[test]
    fn differing_enclosing_roles_cannot_share_one_selected_port() {
        let document = roxmltree::Document::parse(
            r#"<root><entry name="First"><entry name="Value" outkey="7"/></entry><entry name="Second"><entry name="Value" outkey="7"/></entry></root>"#,
        ).unwrap();
        let mut census = PortDeclarationCensus::default();
        for entry in document
            .descendants()
            .filter(|node| node.attribute("outkey").is_some())
        {
            census.record(entry, &[String::from("Value")]);
        }
        assert!(!census.finish(document.root_element()).unambiguous_output(7));
    }

    #[test]
    fn unvisited_declared_references_cannot_hide_behind_one_lowered_port() {
        let facts = capture(
            r#"<root><entry name="Value" outkey="7"/><entry name="Other" outkey="7"/><entry name="Missing" outkey="8"/></root>"#,
            &[&["Value"]],
        );
        assert!(!facts.unambiguous_output(7));
        assert!(!facts.unambiguous_output(8));
        assert!(facts.declares_output(7));
        assert!(facts.declares_output(8));
    }

    #[test]
    fn json_raw_role_proof_is_sticky_invalid_across_equivalent_saved_references() {
        for invalid_first in [false, true] {
            let valid = r#"<entry name="Value" type="json-property"><entry name="string" outkey="7"/></entry>"#;
            let invalid = r#"<entry name="Value" type="json-property"><entry name="string" outkey="7"><entry name="unexpected"/></entry></entry>"#;
            let text = if invalid_first {
                format!("<root>{invalid}{valid}</root>")
            } else {
                format!("<root>{valid}{invalid}</root>")
            };
            let document = roxmltree::Document::parse(&text).unwrap();
            let mut census = PortDeclarationCensus::default();
            for entry in document.descendants().filter(|node| node.attribute("outkey") == Some("7")) {
                census.record(entry, &["Value".into()]);
            }
            let facts = census.finish(document.root_element());
            assert!(facts.unambiguous_output(7));
            assert!(!facts.json_scalar(7, ir::ScalarType::String));
        }
    }
}
