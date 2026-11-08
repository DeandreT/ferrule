use std::path::Path;

use ir::{ScalarType, SchemaNode, XmlNamespace};
use roxmltree::Node;

use crate::XmlFormatError;

use super::complex_types::{resolve_simple_type, simple_type_scalar};
use super::{
    ParseState, declaration_namespace, is_local_qname, local_name, map_xsd_type, qname_namespace,
    top_level,
};

pub(super) fn parse_attribute(
    declaration: &Node<'_, '_>,
    schema: &Node<'_, '_>,
    schema_path: &Path,
    state: &mut ParseState,
) -> SchemaNode {
    if declaration.attribute("name").is_none()
        && let Some(reference) = declaration.attribute("ref")
    {
        if let Some(mut attribute) = resolve_attribute(reference, schema, schema_path, state) {
            if attribute.fixed.is_some()
                && (declaration.attribute("use") == Some("required")
                    || declaration.attribute("fixed").is_some()
                    || declaration.attribute("default").is_some())
            {
                state.reject_attribute_use(XmlFormatError::UnsupportedXmlAttributeFixedReference {
                    name: attribute.name.clone(),
                });
            }
            if let Some(fixed) = declaration.attribute("fixed") {
                attribute.fixed = Some(fixed.to_string());
            }
            if let Some(default) = declaration.attribute("default") {
                attribute.default = Some(default.to_string());
            }
            apply_attribute_use(declaration, &mut attribute, state);
            return attribute;
        }
        let mut attribute =
            SchemaNode::scalar(local_name(reference), ScalarType::String).attribute();
        attribute.xml_namespace =
            qname_namespace(declaration, reference).or(Some(XmlNamespace::Unqualified));
        apply_attribute_use(declaration, &mut attribute, state);
        return attribute;
    }

    let name = declaration.attribute("name").unwrap_or_default();
    let ty = declaration
        .children()
        .find(|node| node.is_element() && node.tag_name().name() == "simpleType")
        .map(|simple| simple_type_scalar(&simple))
        .or_else(|| {
            declaration.attribute("type").map(|ty| {
                resolve_simple_type(ty, schema, schema_path, state)
                    .unwrap_or_else(|| map_xsd_type(ty))
            })
        })
        .unwrap_or(ScalarType::String);
    let mut attribute = SchemaNode::scalar(name, ty).attribute();
    attribute.xml_namespace = declaration_namespace(
        declaration,
        schema,
        state.substitutions.effective_namespace(schema_path),
        true,
    );
    if let Some(fixed) = declaration.attribute("fixed") {
        attribute.fixed = Some(fixed.to_string());
    }
    if let Some(default) = declaration.attribute("default") {
        attribute.default = Some(default.to_string());
    }
    apply_attribute_use(declaration, &mut attribute, state);
    attribute
}

fn apply_attribute_use(
    declaration: &Node<'_, '_>,
    attribute: &mut SchemaNode,
    state: &mut ParseState,
) {
    if declaration
        .parent()
        .is_some_and(|parent| parent.is_element() && parent.tag_name().name() == "schema")
        && let Some(value) = declaration.attribute("use")
    {
        state.reject_attribute_use(XmlFormatError::UnsupportedXmlAttributeUse {
            name: attribute.name.clone(),
            value: value.to_string(),
        });
    }
    attribute.xml_attribute_required = match declaration.attribute("use") {
        None | Some("optional" | "prohibited") => false,
        Some("required") => true,
        Some(value) => {
            state.reject_attribute_use(XmlFormatError::UnsupportedXmlAttributeUse {
                name: attribute.name.clone(),
                value: value.to_string(),
            });
            false
        }
    };
    // An attribute without an effective XML view is refused as required.
    if !matches!(crate::declaration::required_attribute(attribute), Ok(false))
        && attribute.default.is_some()
    {
        state.reject_attribute_use(XmlFormatError::UnsupportedXmlAttributeDefault {
            name: attribute.name.clone(),
        });
    }
}

fn resolve_attribute(
    qname: &str,
    schema: &Node<'_, '_>,
    schema_path: &Path,
    state: &mut ParseState,
) -> Option<SchemaNode> {
    let local = local_name(qname);
    if is_local_qname(schema, qname)
        && let Some(declaration) = top_level(schema, "attribute", local)
    {
        return Some(parse_attribute(&declaration, schema, schema_path, state));
    }
    let path = state.find_external_declaration(schema, schema_path, "attribute", qname)?;
    let text = state.reader.read(&path).ok()?;
    let document = roxmltree::Document::parse(&text).ok()?;
    let external_schema = document.root_element();
    let declaration = top_level(&external_schema, "attribute", local)?;
    Some(parse_attribute(
        &declaration,
        &external_schema,
        &path,
        state,
    ))
}
