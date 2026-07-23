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
    if restricted_elements.is_empty() && !base_elements.is_empty() {
        return Err(unsupported(
            base_name,
            "a nonempty base particle requires an explicit restricted xs:sequence",
        ));
    }

    let mut next_base = 0;
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
        if !compatible_restriction(candidate, child) {
            return Err(unsupported(
                base_name,
                "a restricted particle changes an incompatible field shape or widens repetition",
            ));
        }
        next_base += offset + 1;
    }

    let prohibited = prohibited_attributes(declaration, schema, schema_path, state);
    let restricted_attributes = restricted
        .children
        .iter()
        .filter(|child| child.attribute)
        .cloned()
        .collect::<Vec<_>>();
    for attribute in &restricted_attributes {
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
        if !compatible_restriction(candidate, attribute) {
            return Err(unsupported(
                base_name,
                "a restricted attribute changes an incompatible field shape",
            ));
        }
    }

    let mut children = restricted
        .children
        .drain(..)
        .filter(|child| !child.attribute)
        .collect::<Vec<_>>();
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

fn compatible_restriction(base: &SchemaNode, restricted: &SchemaNode) -> bool {
    if base.name != restricted.name
        || base.attribute != restricted.attribute
        || base.text != restricted.text
        || base.xml_namespace != restricted.xml_namespace
        || (!base.repeating && restricted.repeating)
        || (!base.nillable && restricted.nillable)
    {
        return false;
    }
    match (&base.kind, &restricted.kind) {
        (SchemaKind::Scalar { ty: base }, SchemaKind::Scalar { ty: restricted }) => {
            base == restricted
        }
        (
            SchemaKind::Group { children: base, .. },
            SchemaKind::Group {
                children: restricted,
                ..
            },
        ) => {
            let mut next_base = 0;
            restricted.iter().all(|child| {
                let Some(offset) = base[next_base..]
                    .iter()
                    .position(|candidate| candidate.name == child.name)
                else {
                    return false;
                };
                let compatible = compatible_restriction(&base[next_base + offset], child);
                next_base += offset + 1;
                compatible
            })
        }
        _ => false,
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
