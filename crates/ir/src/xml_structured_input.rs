//! Pure admission proof and resource limits for ordinary structured XML input.

use std::collections::BTreeSet;

use crate::{
    GroupAlternativeMode, SchemaKind, SchemaNode, XML_TEXT_FIELD, XmlAlternativeKind, XmlNamespace,
    XmlWildcardProcessContents, primary_root_ncname_is_valid,
};

pub const MAX_STRUCTURED_XML_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_STRUCTURED_XML_DEPTH: usize = 64;
pub const MAX_STRUCTURED_XML_PHYSICAL_NODES: usize = 1_000_000;
pub const MAX_STRUCTURED_XML_SCHEMA_NODES: usize = 1_000_000;
pub const MAX_STRUCTURED_XML_MATERIALIZED_NODES: usize = 1_000_000;
pub const MAX_STRUCTURED_XML_MATERIALIZED_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_STRUCTURED_XML_PROJECTION_WORK: usize = 100_000_000;
pub const MAX_STRUCTURED_XML_PARSER_RESERVATION_SLOTS: usize = 1_000_000;
pub const MAX_STRUCTURED_XML_NAMESPACE_REFERENCES: usize = 1_000_000;
pub const MAX_STRUCTURED_XML_PARSER_WORK: usize = 100_000_000;
pub const MAX_STRUCTURED_XML_NAMESPACE_REGISTRY_BYTES: usize = 64 * 1024 * 1024;

/// A closed ordinary schema whose input projection can be budgeted before
/// allocating values. This does not change the general XML reader's support.
pub fn xml_structured_document_input_is_supported(schema: &SchemaNode) -> bool {
    let mut nodes = 0;
    !schema.repeating && supported(schema, true, 1, &mut nodes)
}

fn supported(schema: &SchemaNode, root: bool, depth: usize, nodes: &mut usize) -> bool {
    *nodes = nodes.saturating_add(1);
    if depth > MAX_STRUCTURED_XML_DEPTH || *nodes > MAX_STRUCTURED_XML_SCHEMA_NODES {
        return false;
    }
    let text = schema.text && schema.name == XML_TEXT_FIELD;
    if (!text
        && (schema.name.len() > 4096
            || schema.name.chars().any(|ch| ch as u32 > 0xFFFF)
            || !primary_root_ncname_is_valid(&schema.name)))
        || (schema.text && !text)
        || (root && (schema.attribute || schema.text || schema.xml_optional))
        || (schema.attribute && (schema.text || schema.repeating || schema.nillable))
        || (schema.text && (schema.repeating || schema.nillable || schema.xml_namespace.is_some()))
        || (schema.xml_optional && (schema.attribute || schema.text || schema.repeating))
        || (schema.xml_attribute_required && !schema.attribute)
        || !namespace_supported(schema)
        || !metadata_supported(schema)
    {
        return false;
    }
    match &schema.kind {
        SchemaKind::Scalar { .. } => true,
        SchemaKind::ScalarUnion { .. } => false,
        SchemaKind::Group {
            children,
            alternatives,
            required,
            xml_restricted_alternatives,
            dynamic,
        } => {
            if schema.attribute
                || schema.text
                || schema.nillable
                || !alternatives.is_empty()
                || !required.is_empty()
                || !xml_restricted_alternatives.is_empty()
                || dynamic.is_some()
                || (children.iter().any(|child| child.text)
                    && children.iter().any(|child| !child.attribute && !child.text))
            {
                return false;
            }
            let mut names = BTreeSet::new();
            children.iter().all(|child| {
                names.insert(child.name.as_str()) && supported(child, false, depth + 1, nodes)
            })
        }
    }
}

fn namespace_supported(schema: &SchemaNode) -> bool {
    let namespace = match &schema.xml_namespace {
        Some(XmlNamespace::Qualified(uri)) => Some(uri.as_str()),
        _ => None,
    };
    if schema.attribute && schema.name == "xmlns" {
        return false;
    }
    namespace.is_none_or(|uri| {
        !uri.is_empty()
            && uri.len() + schema.name.len() + 2 <= 4096
            && uri.chars().all(|character| {
                xml_character(character)
                    && !character.is_control()
                    && !character.is_whitespace()
                    && !matches!(character, '{' | '}')
            })
            && uri != "http://www.w3.org/2000/xmlns/"
            && (schema.attribute || uri != "http://www.w3.org/XML/1998/namespace")
            && !(schema.attribute
                && uri == "http://www.w3.org/2001/XMLSchema-instance"
                && matches!(schema.name.as_str(), "nil" | "type"))
    })
}

fn xml_character(character: char) -> bool {
    matches!(character as u32,
        0x9 | 0xA | 0xD | 0x20..=0xD7FF | 0xE000..=0xFFFD | 0x10000..=0x10FFFF)
}

fn metadata_supported(schema: &SchemaNode) -> bool {
    schema.xml_name_alternatives.is_empty()
        && schema.xml_wildcard_namespace.is_none()
        && schema.xml_wildcard_process_contents == XmlWildcardProcessContents::Skip
        && schema.recursive_ref.is_none()
        && !schema.nullable
        && !schema.container_nullable
        && !schema.json_any
        && schema.fixed.is_none()
        && schema.default.is_none()
        && schema.json_allowed_values.is_none()
        && schema.numeric_range.is_none()
        && schema.json_multiple_of.is_none()
        && schema.item_count_range.is_none()
        && schema.json_contains.is_none()
        && schema.json_dependent_schemas.is_none()
        && schema.property_count_range.is_none()
        && schema.json_property_dependencies.is_none()
        && schema.json_pattern_property_names.is_none()
        && schema.json_property_names.is_none()
        && !schema.json_unique_items
        && schema.string_length_range.is_none()
        && schema.json_patterns.is_none()
        && schema.json_formats.is_empty()
        && schema.value_generation.is_none()
        && schema.alternative_mode == GroupAlternativeMode::Exclusive
        && schema.xml_alternative_kind == XmlAlternativeKind::XsiType
        && !schema.xml_type_alternatives
        && schema.xml_default_type.is_none()
        && schema.xml_repeating_sequences.is_empty()
        && schema.xml_repeating_choices.is_empty()
        && schema.database_relation.is_none()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ScalarType;

    #[test]
    fn namespace_declaration_local_name_is_never_an_ordinary_input_attribute() {
        for namespace in [
            None,
            Some(XmlNamespace::Unqualified),
            XmlNamespace::qualified("urn:data"),
        ] {
            let mut attribute = SchemaNode::scalar("xmlns", ScalarType::String);
            attribute.attribute = true;
            attribute.xml_namespace = namespace;
            assert!(!xml_structured_document_input_is_supported(
                &SchemaNode::group("Record", vec![attribute])
            ));
        }
        let mut attribute = SchemaNode::scalar("lang", ScalarType::String);
        attribute.attribute = true;
        attribute.xml_namespace = XmlNamespace::qualified("http://www.w3.org/XML/1998/namespace");
        assert!(xml_structured_document_input_is_supported(
            &SchemaNode::group("Record", vec![attribute])
        ));
    }

    #[test]
    fn ordinary_repeated_groups_and_scalar_nil_are_supported_but_metadata_is_closed() {
        let mut item =
            SchemaNode::group("Item", vec![SchemaNode::scalar("Price", ScalarType::Float)]);
        item.repeating = true;
        let mut schema = SchemaNode::group("Items", vec![item]);
        assert!(xml_structured_document_input_is_supported(&schema));
        let SchemaKind::Group { children, .. } = &mut schema.kind else {
            unreachable!()
        };
        let SchemaKind::Group { children, .. } = &mut children[0].kind else {
            unreachable!()
        };
        children[0].nillable = true;
        assert!(xml_structured_document_input_is_supported(&schema));
        schema.default = Some("default".into());
        assert!(!xml_structured_document_input_is_supported(&schema));
    }

    #[test]
    fn simple_content_is_closed_against_mixed_fields_and_duplicate_local_names() {
        let mut text = SchemaNode::scalar(XML_TEXT_FIELD, ScalarType::String);
        text.text = true;
        let mut attribute = SchemaNode::scalar("Code", ScalarType::String);
        attribute.attribute = true;
        let mut schema = SchemaNode::group("Record", vec![text, attribute]);
        assert!(xml_structured_document_input_is_supported(&schema));
        let SchemaKind::Group { children, .. } = &mut schema.kind else {
            unreachable!()
        };
        children.push(SchemaNode::scalar("Child", ScalarType::String));
        assert!(!xml_structured_document_input_is_supported(&schema));
        schema = SchemaNode::group(
            "Record",
            vec![SchemaNode::scalar("Code", ScalarType::String); 2],
        );
        assert!(!xml_structured_document_input_is_supported(&schema));
    }
}
