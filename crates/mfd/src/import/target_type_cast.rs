use std::path::Path;

use mapping::Node;

use super::graph::GraphBuilder;
use super::schema::{
    ComponentFormat, SchemaComponent, normalize_xml_entry_name, resolve_xml_schema_reference,
};
use super::scope::ScopeBuilder;

const MAX_SCHEMA_BYTES: u64 = 8 * 1024 * 1024;

pub(super) fn install(
    target: &SchemaComponent,
    structure: &roxmltree::Node<'_, '_>,
    mfd_path: &Path,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    if target.format != ComponentFormat::Xml {
        return;
    }
    let Some(component) = structure
        .descendants()
        .filter(|node| node.has_tag_name("component"))
        .find(|component| owns_target_port(*component, target))
    else {
        return;
    };
    if !component.descendants().any(|node| {
        node.has_tag_name("entry")
            && normalize_xml_entry_name(node.attribute("name").unwrap_or_default()).0 == "document"
            && node.attribute("casttotargettypemode") == Some("cast-in-subtree")
    }) {
        return;
    }
    let schema_reference = component
        .descendants()
        .find(|node| node.has_tag_name("document"))
        .and_then(|document| document.attribute("schema"));
    let Some(schema_path) = schema_reference
        .and_then(|reference| resolve_xml_schema_reference(mfd_path, reference).ok())
    else {
        return;
    };
    if std::fs::metadata(&schema_path)
        .ok()
        .is_none_or(|metadata| metadata.len() > MAX_SCHEMA_BYTES)
    {
        return;
    }
    let Some(text) = std::fs::read_to_string(schema_path).ok() else {
        return;
    };
    let Some(document) = roxmltree::Document::parse(&text).ok() else {
        return;
    };
    install_scope(
        &mut scopes.root,
        &[],
        document.root_element(),
        &target.schema.name,
        builder,
    );
}

fn owns_target_port(component: roxmltree::Node<'_, '_>, target: &SchemaComponent) -> bool {
    component
        .descendants()
        .filter(|node| node.has_tag_name("entry"))
        .filter_map(|entry| super::schema::parse_u32(entry.attribute("inpkey")))
        .any(|key| target.input_keys.contains(&key))
}

fn install_scope(
    scope: &mut mapping::Scope,
    parent: &[String],
    schema: roxmltree::Node<'_, '_>,
    root_name: &str,
    builder: &mut GraphBuilder<'_>,
) {
    for binding in &mut scope.bindings {
        let mut path = parent.to_vec();
        path.push(binding.target_field.clone());
        if super::target_node_function::path_requires_datetime(schema, root_name, &path) {
            binding.node = builder.alloc(Node::Call {
                function: "coerce_datetime".to_string(),
                args: vec![binding.node],
            });
        }
    }
    for child in &mut scope.children {
        let mut path = parent.to_vec();
        path.push(child.target_field.clone());
        install_scope(child, &path, schema, root_name, builder);
    }
}
