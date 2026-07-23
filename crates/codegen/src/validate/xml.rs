use std::collections::BTreeMap;

use std::collections::BTreeSet;

use ir::{SchemaKind, SchemaNode, XML_ELEMENTS_FIELD, XML_TEXT_FIELD};
use mapping::NodeId;

use super::{ProgramValidationError, SourceCatalog};
use crate::Expression;

pub(super) fn validate(
    sources: SourceCatalog<'_>,
    expressions: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        if let Expression::XmlMixedContent {
            frame,
            path,
            replacements,
        } = expression
        {
            let mut absolute = frame.clone().unwrap_or_default();
            absolute.extend(path.iter().cloned());
            let mixed_source = sources
                .path_targets(&absolute)
                .into_iter()
                .any(|candidate| {
                    candidate.resolved().is_some_and(|candidate| {
                        matches!(candidate.node().kind, SchemaKind::Group { .. })
                            && candidate
                                .node()
                                .child(XML_TEXT_FIELD)
                                .is_some_and(|text| text.text)
                    })
                });
            if !mixed_source {
                return Err(ProgramValidationError::InvalidXmlMixedContentSource {
                    node,
                    path: absolute,
                });
            }
            let mut elements = BTreeSet::new();
            for (replacement, rule) in replacements.iter().enumerate() {
                if rule.element.is_empty() {
                    return Err(ProgramValidationError::EmptyXmlMixedContentElement {
                        node,
                        replacement,
                    });
                }
                if !elements.insert(rule.element.as_str()) {
                    return Err(ProgramValidationError::DuplicateXmlMixedContentElement {
                        node,
                        element: rule.element.clone(),
                    });
                }
                if !rule.collection.is_empty()
                    && !sources
                        .path_targets(&rule.collection)
                        .into_iter()
                        .any(|candidate| candidate.node().repeating)
                {
                    return Err(ProgramValidationError::InvalidXmlMixedContentCollection {
                        node,
                        replacement,
                        collection: rule.collection.clone(),
                    });
                }
            }
            continue;
        }
        let Expression::XmlSerialize {
            frame,
            path,
            schema,
            namespace,
            ..
        } = expression
        else {
            continue;
        };
        if schema.repeating {
            return Err(ProgramValidationError::RepeatingXmlSerializeSchema {
                node,
                schema: schema.name.clone(),
            });
        }
        if namespace.as_ref().is_some_and(String::is_empty) {
            return Err(ProgramValidationError::EmptyXmlSerializeNamespace { node });
        }
        if let Some(feature) = unsupported_schema_feature(schema) {
            return Err(ProgramValidationError::UnsupportedXmlSerializeSchema {
                node,
                schema: schema.name.clone(),
                feature,
            });
        }
        let mut absolute = frame.clone().unwrap_or_default();
        absolute.extend(path.iter().cloned());
        let expected_group = matches!(schema.kind, SchemaKind::Group { .. });
        let matches = sources
            .path_targets(&absolute)
            .into_iter()
            .any(|candidate| {
                candidate.resolved().is_some_and(|candidate| {
                    candidate.node().name == schema.name
                        && matches!(candidate.node().kind, SchemaKind::Group { .. })
                            == expected_group
                })
            });
        if !matches {
            return Err(ProgramValidationError::InvalidXmlSerializeSource {
                node,
                path: absolute,
                schema: schema.name.clone(),
            });
        }
    }
    Ok(())
}

fn unsupported_schema_feature(schema: &SchemaNode) -> Option<&'static str> {
    if !schema.xml_repeating_sequences.is_empty() {
        return Some("anonymous repeating-sequence metadata");
    }
    let SchemaKind::Group {
        children,
        alternatives,
        dynamic,
    } = &schema.kind
    else {
        return None;
    };
    if !alternatives.is_empty() {
        return Some("schema alternatives");
    }
    if dynamic.is_some() {
        return Some("runtime-named fields");
    }
    if children
        .iter()
        .any(|child| child.name == XML_ELEMENTS_FIELD)
    {
        return Some("generic XML elements");
    }
    let has_text = children.iter().any(|child| child.text);
    let has_elements = children.iter().any(|child| !child.attribute && !child.text);
    if has_text && has_elements {
        return Some("ordered mixed element/text content");
    }
    children.iter().find_map(unsupported_schema_feature)
}
