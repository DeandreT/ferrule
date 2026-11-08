use std::io::Write;

use ir::{
    Instance, SchemaNode, Value, XML_ELEMENTS_FIELD, XML_MIXED_CONTENT_FIELD,
    XML_MIXED_CONTENT_VALUE_FIELD, XML_NAMESPACE_URI_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
    XmlNamespace,
};
use quick_xml::Writer;
use quick_xml::events::{BytesText, Event};

use super::generic::{GenericSiblings, surplus_wildcard, write_generic_element};
use super::write::write_single_node;
use super::{MAX_XML_RECURSION_DEPTH, XmlFormatError, resolve_recursive_schema, xml_text};

/// Whether the writer may indent between the items of an ordered content
/// stream.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum OrderedContentLayout {
    /// Element-only content keeps the writer's indentation, which the reader
    /// discards.
    Indented,
    /// Mixed content, declared or generic: the reader keeps every text node,
    /// including whitespace, so text items are written exactly and no
    /// indentation is added between or inside the items.
    Verbatim,
}

impl OrderedContentLayout {
    /// The layout for a declared parent. It follows the reader, which keeps
    /// text in the ordered stream whenever the parent declares a text child
    /// (XSD `mixed`, DTD mixed `#PCDATA`, or DTD `ANY`).
    pub(crate) fn for_children(children: &[SchemaNode]) -> Self {
        if children.iter().any(|child| child.text) {
            Self::Verbatim
        } else {
            Self::Indented
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn write_ordered_mixed_content<W: std::io::Write>(
    writer: &mut Writer<W>,
    schema: &SchemaNode,
    children: &[SchemaNode],
    root_schema: &SchemaNode,
    fields: &[(String, Instance)],
    recursion_depth: usize,
    inherited_namespace: Option<&str>,
    layout: OrderedContentLayout,
) -> Result<bool, XmlFormatError> {
    let Some((_, mixed_content)) = fields
        .iter()
        .find(|(name, _)| name == XML_MIXED_CONTENT_FIELD)
    else {
        return Ok(false);
    };
    let Instance::Repeated(items) = mixed_content else {
        return Err(invalid_mixed_content(
            schema,
            "the ordered content field must be a repeated sequence",
        ));
    };
    let mut ends_with_element = false;
    // Single-occurrence children already written; a later item with the same
    // name is an extra occurrence held by the wildcard, as on read.
    let mut written = Vec::<&str>::new();
    for (index, item) in items.iter().enumerate() {
        let Instance::Group(item_fields) = item else {
            return Err(invalid_mixed_content(
                schema,
                format!("item {index} must be a group"),
            ));
        };
        let name = item_fields
            .iter()
            .find(|(name, _)| name == XML_NODE_NAME_FIELD)
            .and_then(|(_, instance)| instance.as_scalar())
            .and_then(|value| match value {
                Value::String(name) => Some(name.as_str()),
                _ => None,
            })
            .ok_or_else(|| {
                invalid_mixed_content(schema, format!("item {index} has no string node name"))
            })?;
        if name.is_empty() {
            let text = item_fields
                .iter()
                .find(|(name, _)| name == XML_TEXT_FIELD)
                .and_then(|(_, instance)| instance.as_scalar())
                .and_then(|value| match value {
                    Value::String(text) => Some(text.as_str()),
                    _ => None,
                })
                .ok_or_else(|| {
                    invalid_mixed_content(
                        schema,
                        format!("text item {index} has no string text value"),
                    )
                })?;
            writer.write_event(Event::Text(xml_text(text)))?;
            ends_with_element = false;
            continue;
        }
        let retained_namespace = item_fields
            .iter()
            .find(|(name, _)| name == XML_NAMESPACE_URI_FIELD)
            .and_then(|(_, instance)| instance.as_scalar())
            .map(|namespace| match namespace {
                Value::Null => Ok(XmlNamespace::Unqualified),
                Value::String(namespace) => XmlNamespace::qualified(namespace.clone()).ok_or_else(
                    || {
                        invalid_mixed_content(
                            schema,
                            format!(
                                "element item {index} has an empty retained namespace for child `{name}`"
                            ),
                        )
                    },
                ),
                _ => Err(invalid_mixed_content(
                    schema,
                    format!(
                        "element item {index} has a non-string retained namespace for child `{name}`"
                    ),
                )),
            })
            .transpose()?;
        let child_schema = children
            .iter()
            .find(|child| {
                !child.attribute
                    && !child.text
                    && child.name == name
                    && retained_namespace.as_ref().is_none_or(|namespace| {
                        child.xml_namespace.is_some()
                            && child.xml_namespace_matches(namespace.uri())
                    })
            })
            .or_else(|| {
                children
                    .iter()
                    .find(|child| child.name == XML_ELEMENTS_FIELD)
            })
            .ok_or_else(|| {
                invalid_mixed_content(
                    schema,
                    format!("item {index} names undeclared child `{name}`"),
                )
            })?;
        let item_value = item_fields
            .iter()
            .find(|(name, _)| name == XML_MIXED_CONTENT_VALUE_FIELD)
            .map(|(_, instance)| instance);
        let child_schema = match surplus_wildcard(child_schema, children, &written, item_value)? {
            Some(wildcard) => wildcard,
            None => {
                if child_schema.name != XML_ELEMENTS_FIELD && !child_schema.repeating {
                    written.push(child_schema.name.as_str());
                }
                child_schema
            }
        };
        let exact_child;
        let child_schema = if child_schema.xml_name_alternatives.is_empty() {
            child_schema
        } else {
            let namespace = retained_namespace
                .clone()
                .ok_or_else(|| {
                    invalid_mixed_content(
                        schema,
                        format!(
                            "element item {index} has no retained namespace for ambiguous child `{name}`"
                        ),
                    )
                })?;
            if !child_schema.xml_namespace_matches(namespace.uri()) {
                return Err(invalid_mixed_content(
                    schema,
                    format!("element item {index} uses an undeclared namespace for child `{name}`"),
                ));
            }
            exact_child = {
                let mut child = child_schema.clone();
                child.xml_namespace = Some(namespace);
                child.xml_name_alternatives.clear();
                child
            };
            &exact_child
        };
        let child_instance = item_fields
            .iter()
            .find(|(name, _)| name == XML_MIXED_CONTENT_VALUE_FIELD)
            .map(|(_, instance)| instance)
            .ok_or_else(|| {
                invalid_mixed_content(
                    schema,
                    format!("element item {index} has no typed child value"),
                )
            })?;
        let child_depth = recursion_depth + usize::from(child_schema.recursive_ref.is_some());
        let resolved_child;
        let child_schema = if let Some(anchor) = &child_schema.recursive_ref {
            if child_depth >= MAX_XML_RECURSION_DEPTH {
                return Err(XmlFormatError::RecursionLimit {
                    limit: MAX_XML_RECURSION_DEPTH,
                });
            }
            resolved_child = resolve_recursive_schema(child_schema, root_schema, anchor)?;
            &resolved_child
        } else {
            child_schema
        };
        let siblings = GenericSiblings {
            elements: children,
            written: &written,
        };
        match layout {
            OrderedContentLayout::Indented => write_ordered_element(
                writer,
                child_schema,
                root_schema,
                child_instance,
                child_depth,
                inherited_namespace,
                siblings,
            )?,
            OrderedContentLayout::Verbatim => {
                // The reader keeps each element item's string value, so
                // indentation anywhere in its subtree, even inside
                // element-only children, would come back as text. Write the
                // subtree through an unindented writer on the same output,
                // leaving the outer writer's layout state as it was. The
                // trait object keeps one writer type at every nesting level.
                let output: &mut dyn Write = writer.get_mut();
                write_ordered_element(
                    &mut Writer::new(output),
                    child_schema,
                    root_schema,
                    child_instance,
                    child_depth,
                    inherited_namespace,
                    siblings,
                )?;
            }
        }
        ends_with_element = true;
    }
    if ends_with_element {
        // Mark the parent as text-adjacent so quick-xml's indentation layer
        // does not invent whitespace between the final child and its close.
        writer.write_event(Event::Text(BytesText::new("")))?;
    }
    Ok(true)
}

/// Writes one element item of an ordered content stream.
fn write_ordered_element<W: std::io::Write>(
    writer: &mut Writer<W>,
    child_schema: &SchemaNode,
    root_schema: &SchemaNode,
    child_instance: &Instance,
    child_depth: usize,
    inherited_namespace: Option<&str>,
    siblings: GenericSiblings<'_>,
) -> Result<(), XmlFormatError> {
    if child_schema.name == XML_ELEMENTS_FIELD {
        write_generic_element(
            writer,
            child_schema,
            root_schema,
            child_instance,
            child_depth,
            inherited_namespace,
            siblings,
        )
    } else {
        write_single_node(
            writer,
            child_schema,
            root_schema,
            child_instance,
            child_depth,
            inherited_namespace,
            (None, None),
        )
    }
}

fn invalid_mixed_content(schema: &SchemaNode, reason: impl Into<String>) -> XmlFormatError {
    XmlFormatError::InvalidMixedContent {
        group: schema.name.clone(),
        reason: reason.into(),
    }
}
