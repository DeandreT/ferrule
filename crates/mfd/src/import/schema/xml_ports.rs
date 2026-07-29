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

/// A failed XML Schema import leaves visible entry-tree leaves as strings.
/// A whole-structure connection to a same-named, typed target group supplies
/// the missing shape when no descendant connection or competing target shape
/// makes that inference ambiguous.
pub(super) fn refine_copied_fallback_source_groups(
    components: &mut [SchemaComponent],
    edge_from: &BTreeMap<u32, u32>,
    copy_all_targets: &BTreeSet<u32>,
    fallback_source_outputs: &BTreeSet<u32>,
    fallback_target_inputs: &BTreeSet<u32>,
) {
    let mut inferred = BTreeMap::<(usize, Vec<String>), Option<SchemaNode>>::new();
    for target in components
        .iter()
        .filter(|component| component.format == ComponentFormat::Xml && component.is_target())
    {
        for (input, target_path) in &target.ports {
            if target_path.is_empty()
                || !copy_all_targets.contains(input)
                || fallback_target_inputs.contains(input)
            {
                continue;
            }
            let Some(feed) = edge_from
                .get(input)
                .filter(|feed| fallback_source_outputs.contains(feed))
            else {
                continue;
            };
            let Some(target_group) = schema_node_at(&target.schema, target_path)
                .filter(|node| !node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
            else {
                continue;
            };
            if target.ports.iter().any(|(descendant, path)| {
                path.len() > target_path.len()
                    && path.starts_with(target_path)
                    && edge_from.contains_key(descendant)
            }) {
                continue;
            }
            let mut owners = components.iter().enumerate().filter_map(|(index, source)| {
                (source.format == ComponentFormat::Xml
                    && source.is_source
                    && source.output_keys.contains(feed))
                .then_some((index, source))
            });
            let Some((source_index, source)) = owners.next() else {
                continue;
            };
            if owners.next().is_some() {
                continue;
            }
            let Some(source_path) = source.ports.get(feed) else {
                continue;
            };
            if !schema_node_at(&source.schema, source_path).is_some_and(|node| {
                plain_entry_tree_leaf(node)
                    && node.name == target_group.name
                    && source_path.last() == target_path.last()
            }) {
                continue;
            }
            if source.output_keys.iter().any(|output| {
                source.ports.get(output).is_some_and(|path| {
                    path.len() > source_path.len() && path.starts_with(source_path)
                })
            }) {
                continue;
            }
            let key = (source_index, source_path.clone());
            inferred
                .entry(key)
                .and_modify(|candidate| {
                    if candidate.as_ref() != Some(target_group) {
                        *candidate = None;
                    }
                })
                .or_insert_with(|| Some(target_group.clone()));
        }
    }
    for ((source_index, source_path), replacement) in inferred {
        let Some(replacement) = replacement else {
            continue;
        };
        if let Some(source) = components
            .get_mut(source_index)
            .and_then(|component| schema_node_at_mut(&mut component.schema, &source_path))
        {
            *source = replacement;
        }
    }
}

fn plain_entry_tree_leaf(node: &SchemaNode) -> bool {
    node.xml_namespace.is_none()
        && !node.repeating
        && node.recursive_ref.is_none()
        && !node.attribute
        && !node.text
        && !node.nillable
        && !node.nullable
        && !node.container_nullable
        && !node.json_any
        && node.fixed.is_none()
        && node.default.is_none()
        && node.value_generation.is_none()
        && matches!(node.alternative_mode, ir::GroupAlternativeMode::Exclusive)
        && matches!(node.xml_alternative_kind, ir::XmlAlternativeKind::XsiType)
        && node.xml_repeating_sequences.is_empty()
        && node.database_relation.is_none()
        && matches!(
            node.kind,
            SchemaKind::Scalar {
                ty: ir::ScalarType::String
            }
        )
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
                    xml_restricted_alternatives: Vec::new(),
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
