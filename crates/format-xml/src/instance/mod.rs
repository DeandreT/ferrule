mod error;
mod finalization;
mod generic;
mod mixed_content;
mod read;
mod scalar_output;
mod schema_hints;
mod soap;
mod structured;
mod write;

pub use error::XmlFormatError;
pub use finalization::{
    XmlWriteFinalizationError, XmlWritePreflight, to_string_with_options_and_finalizer,
};
pub use read::{XmlReadOptions, from_str, from_str_with_options, read, read_with_options};
pub use soap::{from_wsdl_message_str, read_wsdl_message};
pub use structured::from_str_structured;
pub use write::{XmlWriteOptions, to_string, to_string_with_options, write, write_with_options};

pub(crate) use mixed_content::{OrderedContentLayout, write_ordered_mixed_content};
use read::read_node;
pub(super) use write::{NodeWriteContext, push_schema_attribute};
use write::{
    format_schema_scalar, push_attribute, push_element_namespace, render_with_options,
    validate_output_characters, write_node, xml_text,
};

use ir::{
    Instance, ScalarType, SchemaKind, SchemaNode, Value, XML_ATTRIBUTES_FIELD, XML_ELEMENTS_FIELD,
    XML_MIXED_CONTENT_FIELD, XML_SUBSTITUTION_FIELD, XML_TYPE_FIELD, XmlAlternativeKind,
};

const MAX_XML_RECURSION_DEPTH: usize = 64;
const MAX_XML_NODES: u32 = 1_000_000;
const MAX_ROOT_VIEW_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

fn expand_xml_qname(
    element: &roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
    value: &str,
) -> Result<String, XmlFormatError> {
    let invalid = || XmlFormatError::InvalidXmlType {
        name: schema.name.clone(),
        value: value.to_string(),
    };
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        return Err(invalid());
    }
    let (prefix, local) = match value.split_once(':') {
        Some((prefix, local))
            if !prefix.is_empty() && !local.is_empty() && !local.contains(':') =>
        {
            (Some(prefix), local)
        }
        Some(_) => return Err(invalid()),
        None => (None, value),
    };
    Ok(match element.lookup_namespace_uri(prefix) {
        Some(namespace) if !namespace.is_empty() => format!("{{{namespace}}}{local}"),
        Some(_) | None if prefix.is_none() => local.to_string(),
        Some(_) | None => return Err(invalid()),
    })
}

fn group_fields(instance: &Instance) -> &[(String, Instance)] {
    let Instance::Group(fields) = instance else {
        unreachable!("read_group_fields always returns a group")
    };
    fields
}

fn validate_alternative_fields(
    schema: &SchemaNode,
    alternative: &ir::GroupAlternative,
    fields: &[(String, Instance)],
) -> Result<(), XmlFormatError> {
    if fields.iter().any(|(name, instance)| {
        !is_xml_metadata_field(name)
            && instance_has_value(instance)
            && !alternative.members.contains(name)
    }) || alternative.required.iter().any(|required| {
        !fields
            .iter()
            .any(|(name, instance)| name == required && instance_has_value(instance))
    }) {
        return Err(XmlFormatError::NoMatchingAlternative {
            name: schema.name.clone(),
        });
    }
    Ok(())
}

fn resolve_recursive_schema(
    occurrence: &SchemaNode,
    root: &SchemaNode,
    anchor: &str,
) -> Result<SchemaNode, XmlFormatError> {
    let mut resolved = root
        .find_concrete_group(anchor)
        .cloned()
        .ok_or_else(|| XmlFormatError::MissingElement(format!("recursive anchor `{anchor}`")))?;
    resolved.name.clone_from(&occurrence.name);
    resolved.xml_namespace.clone_from(&occurrence.xml_namespace);
    resolved
        .xml_name_alternatives
        .clone_from(&occurrence.xml_name_alternatives);
    resolved
        .xml_wildcard_namespace
        .clone_from(&occurrence.xml_wildcard_namespace);
    resolved.xml_wildcard_process_contents = occurrence.xml_wildcard_process_contents;
    resolved
        .xml_wildcard_excluded_names
        .clone_from(&occurrence.xml_wildcard_excluded_names);
    resolved.repeating = occurrence.repeating;
    resolved.xml_optional = occurrence.xml_optional;
    resolved.nillable = occurrence.nillable;
    Ok(resolved)
}

pub(super) fn element_matches_schema(
    element: &roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
) -> bool {
    if schema.xml_alternative_kind == XmlAlternativeKind::SubstitutionGroup {
        let identity = expanded_node_name(element);
        return schema
            .alternatives()
            .iter()
            .any(|alternative| alternative.name == identity);
    }
    element.tag_name().name() == schema.name
        && declared_namespace_matches(schema, element_namespace(element))
}

/// An element's namespace name. roxmltree reports a name under an `xmlns=""`
/// undeclaration as `Some("")`, which is no namespace.
pub(super) fn element_namespace<'a>(element: &roxmltree::Node<'a, '_>) -> Option<&'a str> {
    element
        .tag_name()
        .namespace()
        .filter(|namespace| !namespace.is_empty())
}

/// An attribute's namespace name, normalised like [`element_namespace`] for a
/// prefix bound to the empty string.
pub(super) fn attribute_namespace<'a>(attribute: &roxmltree::Attribute<'a, '_>) -> Option<&'a str> {
    attribute
        .namespace()
        .filter(|namespace| !namespace.is_empty())
}

/// Whether an element namespace names this declaration. A declaration without
/// namespace identity (DTD, entry-tree, and closed XSD content without a target
/// namespace) matches its local name in any namespace; an explicitly
/// unqualified one matches only unqualified elements.
pub(super) fn declared_namespace_matches(schema: &SchemaNode, namespace: Option<&str>) -> bool {
    schema.xml_namespace.is_none() || schema.xml_namespace_matches(namespace)
}

pub(super) fn attribute_value<'a>(
    element: &'a roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
) -> Option<&'a str> {
    match &schema.xml_namespace {
        None => element.attribute(schema.name.as_str()),
        Some(namespace) => element
            .attributes()
            .find(|attribute| {
                attribute.name() == schema.name && namespace.matches(attribute_namespace(attribute))
            })
            .map(|attribute| attribute.value()),
    }
}

fn schema_xml_name(schema: &SchemaNode) -> String {
    schema.xml_namespace.as_ref().map_or_else(
        || schema.name.clone(),
        |namespace| expanded_name(namespace.uri(), &schema.name),
    )
}

fn expanded_node_name(element: &roxmltree::Node<'_, '_>) -> String {
    expanded_name(element_namespace(element), element.tag_name().name())
}

fn expanded_name(namespace: Option<&str>, local: &str) -> String {
    namespace.map_or_else(
        || local.to_string(),
        |namespace| format!("{{{namespace}}}{local}"),
    )
}

fn has_xml_nil(
    element: &roxmltree::Node<'_, '_>,
    schema: &SchemaNode,
) -> Result<bool, XmlFormatError> {
    const XSI: &str = "http://www.w3.org/2001/XMLSchema-instance";
    let Some(value) = element.attribute((XSI, "nil")) else {
        return Ok(false);
    };
    let is_nil = match value {
        "true" | "1" => true,
        "false" | "0" => false,
        _ => {
            return Err(XmlFormatError::InvalidXmlNil {
                name: schema.name.clone(),
                value: value.to_string(),
            });
        }
    };
    if !is_nil {
        return Ok(false);
    }
    if !crate::declaration::declaration(schema)?.nil {
        return Err(XmlFormatError::UnexpectedXmlNil {
            name: schema.name.clone(),
        });
    }
    if element
        .children()
        .any(|child| child.is_element() || child.text().is_some_and(|text| !text.trim().is_empty()))
    {
        return Err(XmlFormatError::XmlNilWithContent {
            name: schema.name.clone(),
        });
    }
    Ok(true)
}

fn parse_scalar(name: &str, ty: ScalarType, text: &str) -> Result<Value, XmlFormatError> {
    let bad = || XmlFormatError::ScalarParse {
        name: name.to_string(),
        ty,
        value: text.to_string(),
    };
    Ok(match ty {
        ScalarType::String => Value::String(text.to_string()),
        ScalarType::Int => Value::Int(text.trim().parse().map_err(|_| bad())?),
        ScalarType::Float => {
            let value = text.trim().parse::<f64>().map_err(|_| bad())?;
            if !value.is_finite() {
                return Err(bad());
            }
            Value::Float(value)
        }
        ScalarType::Bool => Value::Bool(xml_boolean(text).ok_or_else(bad)?),
    })
}

fn parse_schema_scalar(
    schema: &SchemaNode,
    ty: ScalarType,
    text: &str,
) -> Result<Value, XmlFormatError> {
    if let Some(expected) = schema.fixed.as_deref()
        && text != expected
    {
        return Err(XmlFormatError::FixedValue {
            name: schema.name.clone(),
            expected: expected.to_string(),
            got: text.to_string(),
        });
    }
    parse_scalar(&schema.name, ty, text)
}

fn parse_input_schema_scalar(
    schema: &SchemaNode,
    ty: ScalarType,
    text: &str,
) -> Result<Value, XmlFormatError> {
    let text = if text.is_empty() {
        schema.default.as_deref().unwrap_or(text)
    } else {
        text
    };
    parse_schema_scalar(schema, ty, text)
}

fn instance_kind(instance: &Instance) -> &'static str {
    match instance {
        Instance::Scalar(_) => "a scalar",
        Instance::Group(_) => "an element group",
        Instance::Repeated(_) => "repeating elements",
        Instance::MappedSequence(_) => "a mapped element sequence",
        Instance::DocumentSet(_) => "a document set",
    }
}

fn group_has_serialized_content(children: &[SchemaNode], fields: &[(String, Instance)]) -> bool {
    if fields
        .iter()
        .any(|(name, _)| name == XML_MIXED_CONTENT_FIELD)
    {
        return true;
    }
    children
        .iter()
        .filter(|child| !child.attribute && child.name != XML_ATTRIBUTES_FIELD)
        .any(|child| {
            let Some((_, instance)) = fields.iter().find(|(name, _)| name == &child.name) else {
                return false;
            };
            if child.text {
                return match instance {
                    Instance::Scalar(Value::Null | Value::JsonNull(_)) => false,
                    Instance::Scalar(Value::String(value))
                        if value.is_empty()
                            && matches!(
                                child.kind,
                                SchemaKind::Scalar {
                                    ty: ScalarType::String
                                }
                            )
                            && child.fixed.as_deref().is_none_or(str::is_empty) =>
                    {
                        false
                    }
                    _ => true,
                };
            }
            match instance {
                Instance::Scalar(Value::Null | Value::JsonNull(_)) => false,
                Instance::Repeated(items)
                    if items.is_empty()
                        && (child.repeating || child.name == XML_ELEMENTS_FIELD) =>
                {
                    false
                }
                Instance::MappedSequence(items)
                    if items.is_empty()
                        && !child.repeating
                        && matches!(child.kind, SchemaKind::Group { .. }) =>
                {
                    false
                }
                _ => true,
            }
        })
}

fn select_group_alternative<'a>(
    schema: &SchemaNode,
    alternatives: &'a [ir::GroupAlternative],
    fields: &[(String, Instance)],
) -> Result<Option<&'a ir::GroupAlternative>, XmlFormatError> {
    if alternatives.is_empty() {
        return Ok(None);
    }
    let marker_name = match schema.xml_alternative_kind {
        XmlAlternativeKind::XsiType => XML_TYPE_FIELD,
        XmlAlternativeKind::SubstitutionGroup => XML_SUBSTITUTION_FIELD,
    };
    if let Some((_, marker)) = fields.iter().find(|(name, _)| name == marker_name) {
        let Instance::Scalar(Value::String(type_name)) = marker else {
            return Err(match schema.xml_alternative_kind {
                XmlAlternativeKind::XsiType => XmlFormatError::InvalidXmlType {
                    name: schema.name.clone(),
                    value: "non-string internal marker".to_string(),
                },
                XmlAlternativeKind::SubstitutionGroup => {
                    XmlFormatError::InvalidSubstitutionMember {
                        name: schema.name.clone(),
                        value: "non-string internal marker".to_string(),
                    }
                }
            });
        };
        let selected = alternatives
            .iter()
            .find(|alternative| alternative.name == *type_name)
            .ok_or_else(|| match schema.xml_alternative_kind {
                XmlAlternativeKind::XsiType => XmlFormatError::UnknownXmlType {
                    name: schema.name.clone(),
                    value: type_name.clone(),
                },
                XmlAlternativeKind::SubstitutionGroup => {
                    XmlFormatError::UnknownSubstitutionMember {
                        name: schema.name.clone(),
                        value: type_name.clone(),
                    }
                }
            })?;
        validate_alternative_fields(schema, selected, fields)?;
        return Ok(Some(selected));
    }
    let populated: Vec<&str> = fields
        .iter()
        .filter(|(name, instance)| !is_xml_metadata_field(name) && instance_has_value(instance))
        .map(|(name, _)| name.as_str())
        .collect();
    if let Some(identity) = &schema.xml_default_type
        && let Some(default) = alternatives
            .iter()
            .find(|alternative| alternative.name == *identity)
        && populated
            .iter()
            .all(|field| default.members.iter().any(|member| member == field))
    {
        validate_alternative_fields(schema, default, fields)?;
        return Ok(Some(default));
    }
    let matches = alternatives
        .iter()
        .filter(|alternative| {
            populated
                .iter()
                .all(|field| alternative.members.iter().any(|member| member == field))
        })
        .collect::<Vec<_>>();
    let Some(member_count) = matches
        .iter()
        .map(|alternative| alternative.members.len())
        .min()
    else {
        return Err(XmlFormatError::NoMatchingAlternative {
            name: schema.name.clone(),
        });
    };
    let mut narrowest = matches
        .into_iter()
        .filter(|alternative| alternative.members.len() == member_count);
    let Some(selected) = narrowest.next() else {
        return Err(XmlFormatError::NoMatchingAlternative {
            name: schema.name.clone(),
        });
    };
    if narrowest.next().is_some() {
        return Err(XmlFormatError::AmbiguousAlternative {
            name: schema.name.clone(),
        });
    }
    Ok(Some(selected))
}

fn instance_has_value(instance: &Instance) -> bool {
    match instance {
        Instance::Scalar(Value::Null | Value::JsonNull(_)) => false,
        Instance::Scalar(Value::XmlNil(_)) => true,
        Instance::Scalar(_) => true,
        Instance::Group(fields) => fields.iter().any(|(_, value)| instance_has_value(value)),
        Instance::Repeated(items) => items.iter().any(instance_has_value),
        Instance::MappedSequence(items) => items.iter().any(instance_has_value),
        Instance::DocumentSet(documents) => documents
            .iter()
            .any(|document| instance_has_value(document.value())),
    }
}

fn split_expanded_name(name: &str) -> (Option<&str>, &str) {
    name.strip_prefix('{')
        .and_then(|name| name.split_once('}'))
        .map_or((None, name), |(namespace, local)| (Some(namespace), local))
}

fn validate_group_fields(
    schema: &SchemaNode,
    children: &[SchemaNode],
    alternatives: &[ir::GroupAlternative],
    fields: &[(String, Instance)],
) -> Result<(), XmlFormatError> {
    for (index, (name, _)) in fields.iter().enumerate() {
        let xml_alternative_marker = !alternatives.is_empty()
            && match schema.xml_alternative_kind {
                XmlAlternativeKind::XsiType => name == XML_TYPE_FIELD,
                XmlAlternativeKind::SubstitutionGroup => name == XML_SUBSTITUTION_FIELD,
            };
        let mixed_content_marker = name == XML_MIXED_CONTENT_FIELD
            && (children.iter().any(|child| child.text)
                || !schema.xml_repeating_sequences.is_empty()
                || !schema.xml_repeating_choices.is_empty()
                || children
                    .iter()
                    .any(|child| !child.xml_name_alternatives.is_empty()));
        if !xml_alternative_marker
            && !mixed_content_marker
            && !children.iter().any(|child| child.name == *name)
        {
            return Err(XmlFormatError::UnexpectedField {
                group: schema.name.clone(),
                field: name.clone(),
            });
        }
        if fields[..index].iter().any(|(previous, _)| previous == name) {
            return Err(XmlFormatError::DuplicateField {
                group: schema.name.clone(),
                field: name.clone(),
            });
        }
    }
    validate_singular_xml_choices(schema, fields)
}

fn validate_singular_xml_choices(
    schema: &SchemaNode,
    fields: &[(String, Instance)],
) -> Result<(), XmlFormatError> {
    for choice in schema
        .xml_repeating_choices
        .iter()
        .filter(|choice| !choice.repeating)
    {
        let selected = choice
            .members
            .iter()
            .filter(|member| {
                fields
                    .iter()
                    .find(|(name, _)| name == *member)
                    .is_some_and(|(_, instance)| instance_occurs(instance))
            })
            .count();
        if selected > 1 {
            return Err(XmlFormatError::InvalidXmlChoice {
                group: schema.name.clone(),
                reason: "more than one member is present".to_string(),
            });
        }
    }
    Ok(())
}

fn instance_occurs(instance: &Instance) -> bool {
    match instance {
        Instance::Scalar(Value::Null | Value::JsonNull(_)) => false,
        Instance::Scalar(_) | Instance::Group(_) => true,
        Instance::Repeated(items) | Instance::MappedSequence(items) => !items.is_empty(),
        Instance::DocumentSet(documents) => !documents.is_empty(),
    }
}

fn is_xml_metadata_field(name: &str) -> bool {
    matches!(
        name,
        XML_TYPE_FIELD | XML_SUBSTITUTION_FIELD | XML_MIXED_CONTENT_FIELD
    )
}

fn shape_error(schema: &SchemaNode, expected: &'static str, instance: &Instance) -> XmlFormatError {
    let got = match instance {
        Instance::Scalar(_) => "a scalar",
        Instance::Group(_) => "an element group",
        Instance::Repeated(_) => "repeating elements",
        Instance::MappedSequence(_) => "a mapped element sequence",
        Instance::DocumentSet(_) => "a document set",
    };
    XmlFormatError::Shape {
        name: schema.name.clone(),
        expected,
        got,
    }
}

fn xml_boolean(value: &str) -> Option<bool> {
    match value.trim() {
        "true" | "1" => Some(true),
        "false" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod effective_nil_tests;
#[cfg(test)]
mod strict_writer_tests;
#[cfg(test)]
mod tests;
