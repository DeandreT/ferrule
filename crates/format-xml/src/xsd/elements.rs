use std::path::Path;

use ir::{ScalarType, SchemaKind, SchemaNode};
use roxmltree::Node;

use crate::XmlFormatError;

use super::complex_types::{resolve_simple_type, simple_type_scalar};
use super::derived_types::{apply_exported_alternative_view, attach_type_alternatives};
use super::{
    ComplexTypeResolution, ParseState, attach_substitution_alternatives, declaration_namespace,
    is_local_qname, local_name, map_xsd_type, parse_complex_type, qname_namespace,
    resolve_complex_type, top_level,
};

pub(super) fn parse_element(
    el: &Node,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) -> SchemaNode {
    let fallback_name = el
        .attribute("name")
        .or_else(|| el.attribute("ref").map(local_name))
        .unwrap_or_default();
    state.nested(
        || SchemaNode::scalar(fallback_name, ScalarType::String),
        |state| parse_element_level(el, schema_el, schema_path, fallback_name, state),
    )
}

fn parse_element_level(
    el: &Node,
    schema_el: &Node,
    schema_path: &Path,
    fallback_name: &str,
    state: &mut ParseState,
) -> SchemaNode {
    if !state.reserve_element() {
        return SchemaNode::scalar(fallback_name, ScalarType::String);
    }
    if el.attribute("name").is_none()
        && let Some(r) = el.attribute("ref")
    {
        let local = r.rsplit(':').next().unwrap_or(r);
        if let Some(node) = resolve_element(r, schema_el, schema_path, state) {
            return node;
        }
        // An unresolved non-recursive reference still degrades leniently.
        let mut node = SchemaNode::scalar(local, ScalarType::String);
        node.xml_namespace = qname_namespace(el, r);
        return node;
    }
    let name = el.attribute("name").unwrap_or_default().to_string();
    let mut node = if let Some(complex_type) = el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "complexType")
    {
        parse_complex_type(&complex_type, schema_el, schema_path, state).into_schema(name)
    } else if let Some(simple_type) = el
        .children()
        .find(|n| n.is_element() && n.tag_name().name() == "simpleType")
    {
        SchemaNode::scalar(name, simple_type_scalar(&simple_type))
    } else if let Some(ty) = el.attribute("type") {
        if let Some(resolved) = resolve_complex_type(ty, schema_el, schema_path, state, Some(&name))
        {
            match resolved {
                ComplexTypeResolution::Group(group) => {
                    let mut node = group.into_schema(name);
                    attach_type_alternatives(&mut node, ty, schema_el, schema_path, state);
                    node
                }
                ComplexTypeResolution::Recursive(anchor) => {
                    SchemaNode::recursive_group(name, anchor)
                }
            }
        } else if let Some(ty) = resolve_simple_type(ty, schema_el, schema_path, state) {
            SchemaNode::scalar(name, ty)
        } else {
            SchemaNode::scalar(name, map_xsd_type(ty))
        }
    } else {
        SchemaNode::scalar(name, ScalarType::String)
    };
    node.xml_namespace = declaration_namespace(
        el,
        schema_el,
        state.substitutions.effective_namespace(schema_path),
        false,
    );
    record_ambiguous_namespace(&node, schema_el, schema_path, state);
    apply_exported_alternative_view(el, &mut node, schema_el, schema_path, state);
    apply_fixed_value(el, &mut node);
    apply_default_value(el, &mut node, state);
    if el
        .attribute("nillable")
        .is_some_and(|value| matches!(value, "true" | "1"))
    {
        node.nillable()
    } else {
        node
    }
}

/// Records an element declaration whose namespace is unknowable because its
/// chameleon document is reached through several namespace contexts.
fn record_ambiguous_namespace(
    node: &SchemaNode,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) {
    if node.xml_namespace.is_none()
        && schema_el.attribute("targetNamespace").is_none()
        && state.substitutions.namespace_contexts(schema_path) > 1
    {
        state
            .ambiguous_unqualified_elements
            .insert(node.name.clone());
    }
}

fn apply_fixed_value(declaration: &Node<'_, '_>, node: &mut SchemaNode) {
    let Some(fixed) = declaration.attribute("fixed") else {
        return;
    };
    match &mut node.kind {
        SchemaKind::Scalar { .. } => node.fixed = Some(fixed.to_string()),
        SchemaKind::ScalarUnion { .. } => {}
        SchemaKind::Group { children, .. } => {
            if let Some(text) = children.iter_mut().find(|child| child.text) {
                text.fixed = Some(fixed.to_string());
            }
        }
    }
}

fn apply_default_value(declaration: &Node<'_, '_>, node: &mut SchemaNode, state: &mut ParseState) {
    let Some(default) = declaration.attribute("default") else {
        return;
    };
    match &mut node.kind {
        SchemaKind::Scalar { .. } => node.default = Some(default.to_string()),
        SchemaKind::ScalarUnion { .. } => {}
        SchemaKind::Group { children, .. } => {
            if let Some(text) = children.iter_mut().find(|child| child.text) {
                text.default = Some(default.to_string());
            } else {
                state.reject_default(XmlFormatError::UnsupportedSchemaDefault {
                    name: node.name.clone(),
                    reason: "only scalar elements and simple-content elements support defaults",
                });
            }
        }
    }
}

fn resolve_element(
    qname: &str,
    schema_el: &Node,
    schema_path: &Path,
    state: &mut ParseState,
) -> Option<SchemaNode> {
    let local = local_name(qname);
    if is_local_qname(schema_el, qname)
        && let Some(declaration) = top_level(schema_el, "element", local)
    {
        return parse_element_declaration(&declaration, schema_el, schema_path, local, state);
    }

    let path = state.find_external_declaration(schema_el, schema_path, "element", qname)?;
    let text = state.reader.read(&path).ok()?;
    let doc = roxmltree::Document::parse(&text).ok()?;
    let external_schema = doc.root_element();
    let declaration = top_level(&external_schema, "element", local)?;
    parse_element_declaration(&declaration, &external_schema, &path, local, state)
}

pub(super) fn parse_element_declaration(
    declaration: &Node,
    schema_el: &Node,
    schema_path: &Path,
    name: &str,
    state: &mut ParseState,
) -> Option<SchemaNode> {
    parse_element_declaration_inner(declaration, schema_el, schema_path, name, state, true)
}

pub(super) fn parse_element_declaration_inner(
    declaration: &Node,
    schema_el: &Node,
    schema_path: &Path,
    name: &str,
    state: &mut ParseState,
    attach_substitutions: bool,
) -> Option<SchemaNode> {
    if !state.enter(schema_path, "element", name) {
        let mut node = SchemaNode::recursive_group(name, name);
        node.xml_namespace = declaration_namespace(
            declaration,
            schema_el,
            state.substitutions.effective_namespace(schema_path),
            false,
        );
        record_ambiguous_namespace(&node, schema_el, schema_path, state);
        return Some(node);
    }
    let mut node = parse_element(declaration, schema_el, schema_path, state);
    if attach_substitutions
        && let Err(error) = attach_substitution_alternatives(&mut node, declaration, state)
    {
        state.reject_substitution(error);
    }
    state.leave();
    Some(node)
}
