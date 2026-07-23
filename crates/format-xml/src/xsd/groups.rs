use std::path::Path;

use ir::SchemaNode;
use roxmltree::Node;

use crate::XmlFormatError;

use super::{
    ParseState, collect_sequence, is_local_qname, is_repeating, local_name, parse_attribute,
    read_xml_text, top_level,
};

pub(super) fn resolve_model_group(
    occurrence: &Node<'_, '_>,
    schema: &Node<'_, '_>,
    schema_path: &Path,
    state: &mut ParseState,
) -> Result<Vec<SchemaNode>, XmlFormatError> {
    let reference = occurrence.attribute("ref").ok_or_else(|| {
        unsupported(
            "group",
            occurrence.attribute("name").unwrap_or("anonymous"),
            "a model-group particle must use ref",
        )
    })?;
    if is_repeating(occurrence) {
        return Err(unsupported(
            "group",
            reference,
            "repeating xs:group references are not supported",
        ));
    }
    let local = local_name(reference);
    if is_local_qname(schema, reference)
        && let Some(declaration) = top_level(schema, "group", local)
    {
        return parse_model_group(&declaration, schema, schema_path, local, state);
    }
    let path = state
        .find_external_declaration(schema, schema_path, "group", reference)
        .ok_or_else(|| XmlFormatError::MissingElement(format!("named xs:group `{reference}`")))?;
    let text = read_xml_text(&path)?;
    let document = roxmltree::Document::parse(&text)?;
    let external_schema = document.root_element();
    let declaration = top_level(&external_schema, "group", local)
        .ok_or_else(|| XmlFormatError::MissingElement(format!("named xs:group `{reference}`")))?;
    parse_model_group(&declaration, &external_schema, &path, local, state)
}

pub(super) fn resolve_attribute_group(
    occurrence: &Node<'_, '_>,
    schema: &Node<'_, '_>,
    schema_path: &Path,
    state: &mut ParseState,
) -> Result<Vec<SchemaNode>, XmlFormatError> {
    let reference = occurrence.attribute("ref").ok_or_else(|| {
        unsupported(
            "attributeGroup",
            occurrence.attribute("name").unwrap_or("anonymous"),
            "an attribute-group use must use ref",
        )
    })?;
    let local = local_name(reference);
    if is_local_qname(schema, reference)
        && let Some(declaration) = top_level(schema, "attributeGroup", local)
    {
        return parse_attribute_group(&declaration, schema, schema_path, local, state);
    }
    let path = state
        .find_external_declaration(schema, schema_path, "attributeGroup", reference)
        .ok_or_else(|| {
            XmlFormatError::MissingElement(format!("named xs:attributeGroup `{reference}`"))
        })?;
    let text = read_xml_text(&path)?;
    let document = roxmltree::Document::parse(&text)?;
    let external_schema = document.root_element();
    let declaration = top_level(&external_schema, "attributeGroup", local).ok_or_else(|| {
        XmlFormatError::MissingElement(format!("named xs:attributeGroup `{reference}`"))
    })?;
    parse_attribute_group(&declaration, &external_schema, &path, local, state)
}

fn parse_model_group(
    declaration: &Node<'_, '_>,
    schema: &Node<'_, '_>,
    schema_path: &Path,
    name: &str,
    state: &mut ParseState,
) -> Result<Vec<SchemaNode>, XmlFormatError> {
    if !state.enter(schema_path, "group", name) {
        return Err(XmlFormatError::SchemaGroupCycle {
            kind: "group",
            name: name.to_string(),
        });
    }
    let result = (|| {
        let particles = declaration
            .children()
            .filter(|node| {
                node.is_element() && matches!(node.tag_name().name(), "sequence" | "choice" | "all")
            })
            .collect::<Vec<_>>();
        let [sequence] = particles.as_slice() else {
            return Err(unsupported(
                "group",
                name,
                "exactly one xs:sequence compositor is required",
            ));
        };
        if sequence.tag_name().name() != "sequence" {
            return Err(unsupported(
                "group",
                name,
                "xs:choice and xs:all model groups are not supported",
            ));
        }
        if is_repeating(sequence) {
            return Err(unsupported(
                "group",
                name,
                "a named group's declaration sequence cannot repeat",
            ));
        }
        if let Some(reason) = unsupported_sequence_member(sequence) {
            return Err(unsupported("group", name, reason));
        }
        let mut children = Vec::new();
        collect_sequence(sequence, false, schema, schema_path, state, &mut children);
        Ok(children)
    })();
    state.leave();
    result
}

fn parse_attribute_group(
    declaration: &Node<'_, '_>,
    schema: &Node<'_, '_>,
    schema_path: &Path,
    name: &str,
    state: &mut ParseState,
) -> Result<Vec<SchemaNode>, XmlFormatError> {
    if !state.enter(schema_path, "attributeGroup", name) {
        return Err(XmlFormatError::SchemaGroupCycle {
            kind: "attributeGroup",
            name: name.to_string(),
        });
    }
    let result = (|| {
        let mut attributes = Vec::new();
        for child in declaration.children().filter(|node| node.is_element()) {
            match child.tag_name().name() {
                "annotation" => {}
                "attribute" if child.attribute("use") == Some("prohibited") => {
                    return Err(unsupported(
                        "attributeGroup",
                        name,
                        "prohibited attributes are not supported",
                    ));
                }
                "attribute" => {
                    if !state.reserve_element() {
                        break;
                    }
                    let attribute = parse_attribute(&child, schema, schema_path, state);
                    attributes.push(attribute);
                }
                "attributeGroup" => {
                    let nested = resolve_attribute_group(&child, schema, schema_path, state)?;
                    attributes.extend(nested);
                }
                "anyAttribute" => {
                    return Err(unsupported(
                        "attributeGroup",
                        name,
                        "xs:anyAttribute is not supported",
                    ));
                }
                _ => {
                    return Err(unsupported(
                        "attributeGroup",
                        name,
                        "only ordinary attributes and attribute-group references are supported",
                    ));
                }
            }
        }
        Ok(attributes)
    })();
    state.leave();
    result
}

fn unsupported_sequence_member(sequence: &Node<'_, '_>) -> Option<&'static str> {
    for child in sequence.children().filter(|node| node.is_element()) {
        match child.tag_name().name() {
            "choice" | "all" => {
                return Some("nested xs:choice and xs:all compositors are not supported");
            }
            "any" => return Some("xs:any particles are not supported"),
            "sequence" => {
                if is_repeating(&child) {
                    return Some(
                        "repeating nested sequences require tuple metadata and are not supported",
                    );
                }
                if let Some(reason) = unsupported_sequence_member(&child) {
                    return Some(reason);
                }
            }
            // Element-local complex types own a separate particle tree.
            "element" | "group" | "annotation" => {}
            _ => {
                return Some(
                    "only elements, sequences, and nonrepeating group references are supported",
                );
            }
        }
    }
    None
}

fn unsupported(kind: &'static str, name: &str, reason: &'static str) -> XmlFormatError {
    XmlFormatError::UnsupportedSchemaGroup {
        kind,
        name: name.to_string(),
        reason,
    }
}
