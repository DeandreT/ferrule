use std::path::Path;

use ir::{SchemaKind, SchemaNode};
use roxmltree::Node;

use crate::XmlFormatError;

use super::{ParseState, ParsedComplexType, parse_attribute, parse_complex_type};

pub(super) fn apply(
    base_name: &str,
    base: ParsedComplexType,
    declaration: &Node<'_, '_>,
    schema: &Node<'_, '_>,
    schema_path: &Path,
    state: &mut ParseState,
) -> Result<ParsedComplexType, XmlFormatError> {
    validate_children(base_name, declaration)?;
    let mut restricted = parse_complex_type(declaration, schema, schema_path, state);
    let base_elements = base
        .children
        .iter()
        .filter(|child| !child.attribute)
        .collect::<Vec<_>>();
    let restricted_elements = restricted
        .children
        .iter()
        .filter(|child| !child.attribute)
        .collect::<Vec<_>>();
    let mut next_base = 0;
    let mut normalized_elements = Vec::with_capacity(restricted_elements.len());
    for child in restricted_elements {
        let Some(offset) = base_elements[next_base..]
            .iter()
            .position(|candidate| candidate.name == child.name)
        else {
            return Err(unsupported(
                base_name,
                "restricted particles must be an ordered subset of the base particle",
            ));
        };
        let candidate = base_elements[next_base + offset];
        let Some(normalized) = normalize_restriction(candidate, child) else {
            return Err(unsupported(
                base_name,
                "a restricted particle changes an incompatible field shape or widens repetition",
            ));
        };
        normalized_elements.push(normalized);
        next_base += offset + 1;
    }

    let prohibited = prohibited_attributes(declaration, schema, schema_path, state);
    let mut restricted_attributes = restricted
        .children
        .iter()
        .filter(|child| child.attribute)
        .cloned()
        .collect::<Vec<_>>();
    for attribute in &mut restricted_attributes {
        let Some(candidate) = base
            .children
            .iter()
            .find(|candidate| candidate.attribute && candidate.name == attribute.name)
        else {
            return Err(unsupported(
                base_name,
                "a restriction cannot introduce an attribute absent from its base",
            ));
        };
        let Some(normalized) = normalize_restriction(candidate, attribute) else {
            return Err(unsupported(
                base_name,
                "a restricted attribute changes an incompatible field shape",
            ));
        };
        *attribute = normalized;
    }

    let mut children = normalized_elements;
    for base_attribute in base.children.into_iter().filter(|child| child.attribute) {
        if prohibited
            .iter()
            .any(|prohibited| same_xml_name(prohibited, &base_attribute))
        {
            continue;
        }
        if let Some(replacement) = restricted_attributes
            .iter()
            .find(|attribute| same_xml_name(attribute, &base_attribute))
        {
            children.push(replacement.clone());
        } else {
            children.push(base_attribute);
        }
    }
    restricted.children = children;
    Ok(restricted)
}

pub(super) fn apply_simple_content(
    base_name: &str,
    base: ParsedComplexType,
    declaration: &Node<'_, '_>,
    schema: &Node<'_, '_>,
    schema_path: &Path,
    state: &mut ParseState,
) -> Result<ParsedComplexType, XmlFormatError> {
    validate_simple_content_children(base_name, declaration)?;
    if !base.repeating_sequences.is_empty() {
        return Err(unsupported_simple(
            base_name,
            "the base cannot contain repeating sequence metadata",
        ));
    }
    let text = base
        .children
        .iter()
        .filter(|child| !child.attribute)
        .collect::<Vec<_>>();
    if text.len() != 1 || !text[0].text || !matches!(text[0].kind, SchemaKind::Scalar { .. }) {
        return Err(unsupported_simple(
            base_name,
            "the base must contain exactly one scalar text field",
        ));
    }

    let parsed = parse_complex_type(declaration, schema, schema_path, state);
    if parsed.children.iter().any(|child| !child.attribute)
        || !parsed.repeating_sequences.is_empty()
    {
        return Err(unsupported_simple(
            base_name,
            "a restriction may contain only attribute declarations",
        ));
    }
    let prohibited = prohibited_attributes(declaration, schema, schema_path, state);
    let mut restricted_attributes = parsed.children;
    for attribute in &mut restricted_attributes {
        let Some(candidate) = base
            .children
            .iter()
            .find(|candidate| candidate.attribute && candidate.name == attribute.name)
        else {
            return Err(unsupported_simple(
                base_name,
                "a restriction cannot introduce an attribute absent from its base",
            ));
        };
        let Some(normalized) = normalize_restriction(candidate, attribute) else {
            return Err(unsupported_simple(
                base_name,
                "a restricted attribute changes an incompatible field shape",
            ));
        };
        *attribute = normalized;
    }

    let mut children = vec![text[0].clone()];
    for base_attribute in base.children.into_iter().filter(|child| child.attribute) {
        if prohibited
            .iter()
            .any(|prohibited| same_xml_name(prohibited, &base_attribute))
        {
            continue;
        }
        if let Some(replacement) = restricted_attributes
            .iter()
            .find(|attribute| same_xml_name(attribute, &base_attribute))
        {
            children.push(replacement.clone());
        } else {
            children.push(base_attribute);
        }
    }
    Ok(ParsedComplexType {
        children,
        repeating_sequences: Vec::new(),
    })
}

fn validate_children(base: &str, declaration: &Node<'_, '_>) -> Result<(), XmlFormatError> {
    let mut compositor_count = 0;
    for child in declaration.children().filter(Node::is_element) {
        match child.tag_name().name() {
            "annotation" | "attribute" | "attributeGroup" => {}
            "sequence" => compositor_count += 1,
            "choice" | "all" => {
                return Err(unsupported(
                    base,
                    "xs:choice and xs:all restriction particles are not supported",
                ));
            }
            "any" | "anyAttribute" => {
                return Err(unsupported(
                    base,
                    "wildcard restriction particles and attributes are not supported",
                ));
            }
            _ => {
                return Err(unsupported(
                    base,
                    "only a sequence and ordinary attribute restrictions are supported",
                ));
            }
        }
    }
    if compositor_count > 1 {
        return Err(unsupported(
            base,
            "at most one restricted xs:sequence is supported",
        ));
    }
    Ok(())
}

fn validate_simple_content_children(
    base: &str,
    declaration: &Node<'_, '_>,
) -> Result<(), XmlFormatError> {
    for child in declaration.children().filter(Node::is_element) {
        if !matches!(
            child.tag_name().name(),
            "annotation" | "attribute" | "attributeGroup"
        ) {
            return Err(unsupported_simple(
                base,
                "only ordinary attribute restrictions are supported",
            ));
        }
    }
    Ok(())
}

fn prohibited_attributes(
    declaration: &Node<'_, '_>,
    schema: &Node<'_, '_>,
    schema_path: &Path,
    state: &mut ParseState,
) -> Vec<SchemaNode> {
    declaration
        .children()
        .filter(|child| {
            child.is_element()
                && child.tag_name().name() == "attribute"
                && child.attribute("use") == Some("prohibited")
        })
        .map(|child| parse_attribute(&child, schema, schema_path, state))
        .collect()
}

fn normalize_restriction(base: &SchemaNode, restricted: &SchemaNode) -> Option<SchemaNode> {
    if base.name != restricted.name
        || base.attribute != restricted.attribute
        || base.text != restricted.text
        || base.xml_namespace != restricted.xml_namespace
        || (!base.repeating && restricted.repeating)
        || (!base.nillable && restricted.nillable)
    {
        return None;
    }
    match (&base.kind, &restricted.kind) {
        (SchemaKind::Scalar { ty: base_ty }, SchemaKind::Scalar { ty: restricted_ty }) => {
            (base_ty == restricted_ty).then(|| restricted.clone())
        }
        (
            SchemaKind::Group {
                children: base_children,
                ..
            },
            SchemaKind::Group {
                children: restricted_children,
                ..
            },
        ) => {
            if base.recursive_ref.is_some() {
                let mut normalized = base.clone();
                normalized.repeating = restricted.repeating;
                normalized.nillable = restricted.nillable;
                return Some(normalized);
            }
            let mut next_base = 0;
            let mut normalized_children = Vec::with_capacity(restricted_children.len());
            for child in restricted_children {
                let offset = base_children[next_base..]
                    .iter()
                    .position(|candidate| candidate.name == child.name)?;
                let normalized = normalize_restriction(&base_children[next_base + offset], child)?;
                normalized_children.push(normalized);
                next_base += offset + 1;
            }
            let mut normalized = restricted.clone();
            if let SchemaKind::Group { children, .. } = &mut normalized.kind {
                *children = normalized_children;
            }
            Some(normalized)
        }
        _ => None,
    }
}

fn same_xml_name(left: &SchemaNode, right: &SchemaNode) -> bool {
    left.name == right.name && left.xml_namespace == right.xml_namespace
}

fn unsupported(base: &str, reason: &'static str) -> XmlFormatError {
    XmlFormatError::UnsupportedComplexContentRestriction {
        base: base.to_string(),
        reason,
    }
}

fn unsupported_simple(base: &str, reason: &'static str) -> XmlFormatError {
    XmlFormatError::UnsupportedSimpleContentRestriction {
        base: base.to_string(),
        reason,
    }
}
