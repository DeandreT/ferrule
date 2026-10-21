//! Exact owned XML mixed-content atomization in paired Basic contexts.
//!
//! The group is read with the owned element rules, so a missing group is a
//! typed failure and never borrows another owner or legacy frame. Parent
//! replacements evaluate in the receiving context. An occurrence replacement
//! evaluates with its retained element value as the latest occurrence of the
//! group's own owner at the group path plus the element name, numbered per
//! element name from one; the legacy view stacks the same item.

use std::collections::BTreeMap;

use ir::{
    XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
};

use super::*;
use crate::{OwnedXmlMixedContentReplacement, XmlMixedContentElement};

fn string_field<'a>(item: &'a Instance, name: &str) -> &'a str {
    match item.field(name).and_then(Instance::as_scalar) {
        Some(Value::String(value)) => value,
        _ => "",
    }
}

/// Null-like replacement values contribute nothing.
fn append(output: &mut String, value: Value) {
    match value {
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => {}
        Value::Bool(value) => output.push_str(if value { "true" } else { "false" }),
        Value::Int(value) => output.push_str(&value.to_string()),
        Value::Float(value) => output.push_str(&value.to_string()),
        Value::String(value) => output.push_str(&value),
    }
}

impl<'ctx> BasicScopeContext<'ctx> {
    /// Atomizes one owned mixed group. Failures of the group read and of an
    /// occurrence frame are attributed to `node`; replacement failures keep
    /// their own attribution.
    #[allow(clippy::too_many_arguments)]
    pub fn owned_xml_mixed_content(
        &self,
        node: u32,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
        frame: Option<&[&str]>,
        declared_nonrepeating: bool,
        replacements: &[OwnedXmlMixedContentReplacement<'_>],
    ) -> Result<Value, RuntimeError> {
        let at_node = |error: RuntimeError| error.with_owned_source_node(node);
        let group = self
            .owned_xml_instance(owner, path, frame, declared_nonrepeating)
            .map_err(at_node)?;
        let Some(items) = group
            .field(XML_MIXED_CONTENT_FIELD)
            .and_then(Instance::as_repeated)
        else {
            return Ok(group
                .field(XML_TEXT_FIELD)
                .and_then(Instance::as_scalar)
                .cloned()
                .unwrap_or(Value::Null));
        };
        let mut output = String::new();
        let mut occurrences = BTreeMap::<&str, usize>::new();
        for item in items {
            let name = string_field(item, XML_NODE_NAME_FIELD);
            let text = string_field(item, XML_TEXT_FIELD);
            let Some(replacement) = replacements.iter().find(|rule| rule.element == name) else {
                output.push_str(text);
                continue;
            };
            if !replacement.occurrence {
                append(&mut output, (replacement.expression)(self)?);
                continue;
            }
            // Readers retain every element's value; an item without one is
            // no occurrence and keeps its text.
            let Some(value) = item.field(XML_MIXED_CONTENT_VALUE_FIELD) else {
                output.push_str(text);
                continue;
            };
            let index = occurrences.entry(name).or_default();
            *index += 1;
            let occurrence = self
                .with_owned_mixed_occurrence(owner, path, name, value, *index)
                .map_err(at_node)?;
            append(&mut output, (replacement.expression)(&occurrence)?);
        }
        Ok(Value::String(output))
    }

    /// Attaches the ordered stream of the owner's current occurrence of
    /// `collection` (the scope's own owned selector) to a constructed group:
    /// its latest exact frame, a group-member frame's first member, or the
    /// registry root of an endpoint-root selection. Another owner's frame and
    /// the innermost legacy frame are never consulted.
    pub fn preserve_owned_xml_mixed_content(
        &self,
        owner: SourceOwnerRef<'_>,
        collection: &[&str],
        output: Instance,
        elements: &[XmlMixedContentElement<'_>],
    ) -> Result<Instance, RuntimeError> {
        self.budget(owner, collection, None, false)?;
        let (key, root) = self.owned.resolve_owned_owner(owner)?;
        let source = match self.owned.owned_frame_index(key, Some(collection)) {
            Some(index) => {
                let frame = &self.owned.frames[index];
                match (frame.kind, frame.instance) {
                    (
                        super::super::OwnedOccurrenceKind::GroupMembers,
                        Instance::Repeated(members),
                    ) => members.first(),
                    (_, instance) => Some(instance),
                }
            }
            None if collection.is_empty() => Some(root),
            None => {
                return Err(OwnedSourceError::InactiveSourceFrame {
                    owner: self.owned.owned_identity(key),
                    collection: Some(collection.iter().map(|part| (*part).to_owned()).collect()),
                }
                .into());
            }
        };
        Ok(crate::xml_mixed_content::preserve_from(
            source, output, elements,
        ))
    }

    /// Stacks one occurrence of `element` under the owner's group at `path`
    /// on both views, keeping every existing owned frame.
    pub(crate) fn with_owned_mixed_occurrence(
        &self,
        owner: SourceOwnerRef<'_>,
        path: &[&str],
        element: &str,
        item: &'ctx Instance,
        index: usize,
    ) -> Result<Self, RuntimeError> {
        let mut collection = path
            .iter()
            .map(|segment| (*segment).to_owned())
            .collect::<Vec<_>>();
        collection.push(element.to_owned());
        let borrowed = collection.iter().map(String::as_str).collect::<Vec<_>>();
        let (mut budget, bounds) = self.budget(owner, &borrowed, None, false)?;
        let (key, _) = self.owned.resolve_owned_owner(owner)?;
        length_limit("legacy frames", self.legacy.frames.len(), 1, bounds.legacy)?;
        let retained = &self.owned.frames;
        owned_charge(
            OwnedSourceLimitKind::Frames,
            0,
            add(retained.len(), 1)?,
            self.owned.registry.limits.max_active_frames,
        )?;
        budget.slots(2)?;
        let mut legacy = self.legacy_copy(&mut budget)?;
        let origins = self.origins_copy(&mut budget)?;
        grow(&mut legacy.frames, 1, &mut budget)?;
        let legacy_path = budget.path(&collection)?;
        legacy.frames.push(super::super::collection_frame(
            item,
            CollectionIdentity::Repeated {
                path: legacy_path,
                index,
            },
        ));
        budget.slots(add(retained.len(), 1)?)?;
        owned_frames_bytes(retained, &mut budget)?;
        let owned_path = budget.path(&collection)?;
        let mut frames = Vec::with_capacity(add(retained.len(), 1)?);
        frames.extend_from_slice(retained);
        frames.push(OwnedSourceFrame {
            owner: key,
            collection: owned_path,
            instance: item,
            index,
            kind: super::super::OwnedOccurrenceKind::Item,
        });
        Ok(Self {
            legacy,
            owned: OwnedScopeContext {
                registry: Arc::clone(&self.owned.registry),
                frames,
            },
            origins,
            bindings: self.bindings,
            projection_bytes: self.projection_bytes,
            identity: Arc::new(()),
            join: self.join.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use ir::{
        XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
    };

    use crate::{
        BasicScopeContext, BasicSourceRoots, DeclaredNamedSourceRoot, Instance, OwnedSourceError,
        OwnedSourceIdentity, OwnedSourceLimits, OwnedXmlMixedContentReplacement, RuntimeError,
        SourceOwnerRef, SourcePathError, Value, XmlMixedContentElement,
    };

    fn group(fields: Vec<(&str, Instance)>) -> Instance {
        Instance::Group(
            fields
                .into_iter()
                .map(|(name, value)| (name.to_string(), value))
                .collect::<Vec<_>>()
                .into(),
        )
    }

    fn text(value: &str) -> Instance {
        Instance::Scalar(Value::String(value.into()))
    }

    fn item(name: &str, text_value: &str, value: Instance) -> Instance {
        group(vec![
            (XML_NODE_NAME_FIELD, text(name)),
            (XML_TEXT_FIELD, text(text_value)),
            (XML_MIXED_CONTENT_VALUE_FIELD, value),
        ])
    }

    /// `Para` holds `prefix <b>…</b>… |` with the given bold texts.
    fn input(title: &str, prefix: &str, bold: &[&str]) -> Instance {
        let mut stream = vec![item("", prefix, text(prefix))];
        for value in bold {
            stream.push(item("b", value, text(value)));
            stream.push(item("", "|", text("|")));
        }
        group(vec![
            ("Title", text(title)),
            (
                "Para",
                group(vec![
                    (
                        "b",
                        Instance::Repeated(bold.iter().map(|b| text(b)).collect()),
                    ),
                    (XML_MIXED_CONTENT_FIELD, Instance::Repeated(stream)),
                ]),
            ),
        ])
    }

    const LIMITS: OwnedSourceLimits = OwnedSourceLimits {
        max_roots: 8,
        max_active_frames: 8,
        max_path_segments: 8,
        max_metadata_bytes: 4096,
        max_walk_contexts: 64,
    };

    fn occurrence(context: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        let position = context.owned_position(SourceOwnerRef::Primary, Some(&["Para", "b"]))?;
        let Value::String(value) =
            context.owned_field(SourceOwnerRef::Primary, &["Para", "b"], None)?
        else {
            unreachable!("authored text");
        };
        Ok(Value::String(format!("[{position}:{value}]")))
    }

    fn title(context: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        context.owned_field(SourceOwnerRef::Named("Para"), &["Title"], None)
    }

    fn foreign(context: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {
        context
            .owned_field(SourceOwnerRef::Named("Para"), &["Para", "b"], None)
            .map_err(|error| error.with_owned_source_node(9))
    }

    #[test]
    fn owned_mixed_content_reads_only_its_owner_and_occurrences() {
        let primary = input("P", "p ", &["x", "y"]);
        let alias = input("A", "a ", &["z"]);
        let declared = [DeclaredNamedSourceRoot {
            name: "Para",
            instance: Some(&alias),
        }];
        let roots = BasicSourceRoots::try_new(&primary, &declared, LIMITS).unwrap();
        let root = roots.root_context().unwrap();
        let atomize = |owner, rules: &[OwnedXmlMixedContentReplacement<'_>]| {
            root.owned_xml_mixed_content(4, owner, &["Para"], None, false, rules)
        };
        let rules = [OwnedXmlMixedContentReplacement {
            element: "b",
            occurrence: true,
            expression: occurrence,
        }];
        assert_eq!(
            atomize(SourceOwnerRef::Primary, &rules),
            Ok(Value::String("p [1:x]|[2:y]|".into()))
        );
        // A parent rule reads the receiving context; the alias owner keeps its own group.
        let parent = [OwnedXmlMixedContentReplacement {
            element: "b",
            occurrence: false,
            expression: title,
        }];
        assert_eq!(
            atomize(SourceOwnerRef::Named("Para"), &parent),
            Ok(Value::String("a A|".into()))
        );
        // The primary occurrence never serves another owner's read.
        let crossing = [OwnedXmlMixedContentReplacement {
            element: "b",
            occurrence: true,
            expression: foreign,
        }];
        assert!(matches!(
            atomize(SourceOwnerRef::Primary, &crossing),
            Err(RuntimeError::OwnedSourceAtNode {
                node: 9,
                source: OwnedSourceError::UniteratedRepetition { .. },
            })
        ));
        // The context itself is unchanged after the occurrences.
        assert!(matches!(
            root.owned_position(SourceOwnerRef::Primary, Some(&["Para", "b"])),
            Err(RuntimeError::OwnedSource(
                OwnedSourceError::InactiveSourceFrame { .. }
            ))
        ));
    }

    #[test]
    fn a_missing_group_is_attributed_to_the_atomizer_and_never_borrowed() {
        let primary = group(vec![("Title", text("P"))]);
        let alias = input("A", "a ", &["z"]);
        let declared = [DeclaredNamedSourceRoot {
            name: "Para",
            instance: Some(&alias),
        }];
        let roots = BasicSourceRoots::try_new(&primary, &declared, LIMITS).unwrap();
        let root = roots.root_context().unwrap();
        assert_eq!(
            root.owned_xml_mixed_content(4, SourceOwnerRef::Primary, &["Para"], None, false, &[]),
            Err(RuntimeError::OwnedSourceAtNode {
                node: 4,
                source: OwnedSourceError::Traversal {
                    owner: OwnedSourceIdentity::Primary,
                    source: SourcePathError::MissingInstance {
                        path: vec!["Para".into()],
                    },
                },
            })
        );
        // A group without a retained stream returns its text value.
        let plain = group(vec![("Para", group(vec![(XML_TEXT_FIELD, text(" t "))]))]);
        let roots = BasicSourceRoots::try_new(&plain, &[], LIMITS).unwrap();
        assert_eq!(
            roots.root_context().unwrap().owned_xml_mixed_content(
                4,
                SourceOwnerRef::Primary,
                &["Para"],
                None,
                false,
                &[],
            ),
            Ok(Value::String(" t ".into()))
        );
    }

    fn strong(values: &[&str]) -> Instance {
        group(vec![(
            "Strong",
            Instance::Repeated(values.iter().map(|value| text(value)).collect()),
        )])
    }

    fn stream_names(output: &Instance) -> Vec<(String, String)> {
        output
            .field(XML_MIXED_CONTENT_FIELD)
            .and_then(Instance::as_repeated)
            .unwrap_or_default()
            .iter()
            .map(|item| {
                let field = |name| match item.field(name).and_then(Instance::as_scalar) {
                    Some(Value::String(value)) => value.clone(),
                    other => format!("{other:?}"),
                };
                (field(XML_NODE_NAME_FIELD), field(XML_TEXT_FIELD))
            })
            .collect()
    }

    #[test]
    fn owned_construction_attaches_only_the_selected_occurrence_or_endpoint_root() {
        // Each `Para` occurrence of the primary attaches its own stream.
        let primary = group(vec![(
            "Para",
            Instance::Repeated(vec![
                input("P", "first ", &["x"]).field("Para").cloned().unwrap(),
                input("P", "second ", &["y"])
                    .field("Para")
                    .cloned()
                    .unwrap(),
            ]),
        )]);
        let note = input("N", "note ", &["z"]).field("Para").cloned().unwrap();
        let declared = [DeclaredNamedSourceRoot {
            name: "note",
            instance: Some(&note),
        }];
        let roots = BasicSourceRoots::try_new(&primary, &declared, LIMITS).unwrap();
        let root = roots.root_context().unwrap();
        let elements = [XmlMixedContentElement {
            source: "b",
            target: "Strong",
        }];
        let rows = root
            .walk_owned_source(SourceOwnerRef::Primary, &["Para"])
            .unwrap();
        let pair = |name: &str, value: &str| (name.to_owned(), value.to_owned());
        let first = rows[0]
            .context()
            .preserve_owned_xml_mixed_content(
                SourceOwnerRef::Primary,
                &["Para"],
                strong(&["X"]),
                &elements,
            )
            .unwrap();
        assert_eq!(
            stream_names(&first),
            [pair("", "first "), pair("Strong", "X"), pair("", "|")]
        );
        // An endpoint-root selection inside the primary row attaches the
        // named root, never the row item that is the innermost legacy frame.
        let nested = rows[1]
            .context()
            .preserve_owned_xml_mixed_content(
                SourceOwnerRef::Named("note"),
                &[],
                strong(&["Z"]),
                &elements,
            )
            .unwrap();
        assert_eq!(
            stream_names(&nested),
            [pair("", "note "), pair("Strong", "Z"), pair("", "|")]
        );
        // A non-root selector without its own occurrence is a typed failure.
        assert!(matches!(
            root.preserve_owned_xml_mixed_content(
                SourceOwnerRef::Primary,
                &["Para"],
                strong(&[]),
                &elements
            ),
            Err(RuntimeError::OwnedSource(
                OwnedSourceError::InactiveSourceFrame { .. }
            ))
        ));
    }
}
