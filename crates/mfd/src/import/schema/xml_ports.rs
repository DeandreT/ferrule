use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode, XML_TEXT_FIELD};

use super::{ComponentFormat, SchemaComponent, normalize_xml_entry_name, schema_node_at};

/// Parent ports on simple-content XML elements normally carry their scalar
/// text value. When such a port is connected from a structural source entry,
/// however, it carries group context for its connected attribute descendants.
/// Restore that parent path after edges are known so the connection is lowered
/// as a group projection instead of an invalid group-to-`#text` binding.
pub(super) fn restore_connected_structural_ports(
    components: &mut [SchemaComponent],
    edge_from: &BTreeMap<u32, u32>,
) {
    let structural_outputs = components
        .iter()
        .flat_map(|component| {
            component.output_keys.iter().filter(|key| {
                component
                    .ports
                    .get(key)
                    .and_then(|path| schema_node_at(&component.schema, path))
                    .is_some_and(|node| matches!(node.kind, SchemaKind::Group { .. }))
            })
        })
        .copied()
        .collect::<BTreeSet<_>>();

    for component in components
        .iter_mut()
        .filter(|component| component.format == ComponentFormat::Xml && !component.is_source)
    {
        let structural_inputs = component
            .input_ancestors
            .values()
            .flatten()
            .copied()
            .collect::<BTreeSet<_>>();
        for (key, path) in &mut component.ports {
            if !structural_inputs.contains(key)
                || !edge_from
                    .get(key)
                    .is_some_and(|feed| structural_outputs.contains(feed))
                || path.last().is_none_or(|field| field != XML_TEXT_FIELD)
            {
                continue;
            }
            let parent = &path[..path.len() - 1];
            if schema_node_at(&component.schema, parent)
                .is_some_and(|node| matches!(node.kind, SchemaKind::Group { .. }))
            {
                path.pop();
            }
        }
    }
}

/// An untyped XSD element imports as a scalar, but MapForce can expose an
/// explicit `#text` child below that element. Preserve the visible structural
/// parent port by promoting the scalar to ferrule's simple-content shape.
pub(super) fn reconcile_explicit_text_entries(
    entry: &roxmltree::Node<'_, '_>,
    schema: &mut SchemaNode,
) {
    reconcile_children(entry, schema, &mut Vec::new());
}

fn reconcile_children(
    entry: &roxmltree::Node<'_, '_>,
    schema: &mut SchemaNode,
    path: &mut Vec<String>,
) {
    for child in entry.children().filter(|node| node.has_tag_name("entry")) {
        let (name, _) = normalize_xml_entry_name(child.attribute("name").unwrap_or_default());
        if child.attribute("type") == Some("xml-type") && name != XML_TEXT_FIELD {
            reconcile_children(&child, schema, path);
            continue;
        }
        if name == XML_TEXT_FIELD {
            if let Some(parent) = schema_node_at_mut(schema, path)
                && let SchemaKind::Scalar { ty } = parent.kind
            {
                let mut text = SchemaNode::scalar(XML_TEXT_FIELD, ty).text();
                text.fixed = parent.fixed.take();
                parent.kind = SchemaKind::Group {
                    children: vec![text],
                    alternatives: Vec::new(),
                    dynamic: None,
                };
            }
            continue;
        }
        path.push(name.to_string());
        reconcile_children(&child, schema, path);
        path.pop();
    }
}

fn schema_node_at_mut<'a>(
    mut schema: &'a mut SchemaNode,
    path: &[String],
) -> Option<&'a mut SchemaNode> {
    for segment in path {
        let SchemaKind::Group { children, .. } = &mut schema.kind else {
            return None;
        };
        schema = children.iter_mut().find(|child| child.name == *segment)?;
    }
    Some(schema)
}

/// MapForce puts a non-repeating simple-content value on its parent element's
/// port. Ferrule stores that value under `#text`; repeating and mixed-content
/// parent ports stay structural because their port carries the node sequence.
pub(super) fn normalize_xml_text_ports(
    schema: &SchemaNode,
    ports: &mut BTreeMap<u32, Vec<String>>,
) {
    let explicit_text_parents = ports
        .values()
        .filter(|path| path.last().is_some_and(|segment| segment == XML_TEXT_FIELD))
        .map(|path| path[..path.len() - 1].to_vec())
        .collect::<BTreeSet<_>>();
    for path in ports.values_mut() {
        if explicit_text_parents.contains(path) {
            continue;
        }
        let node = schema_node_at(schema, path);
        if let Some(text) = node.and_then(SchemaNode::text_child).filter(|_| {
            node.is_some_and(|node| !node.repeating && has_only_text_and_attributes(node))
        }) {
            path.push(text.name.clone());
        }
    }
}

fn has_only_text_and_attributes(node: &SchemaNode) -> bool {
    matches!(&node.kind, SchemaKind::Group { children, .. } if children
        .iter()
        .all(|child| child.attribute || child.text))
}
