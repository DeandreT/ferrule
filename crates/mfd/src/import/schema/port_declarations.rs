use std::collections::{BTreeMap, BTreeSet};

use super::parse_u32;

/// Declaration facts for bounded proofs. Ordinary port normalization is unchanged.
#[derive(Default)]
pub(in crate::import) struct PortDeclarations {
    inputs: BTreeSet<u32>,
    outputs: BTreeSet<u32>,
    ambiguous: BTreeSet<u32>,
    ambiguous_paths: BTreeMap<u32, Vec<Vec<String>>>,
    unlowered: BTreeSet<u32>,
}

impl PortDeclarations {
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
}
