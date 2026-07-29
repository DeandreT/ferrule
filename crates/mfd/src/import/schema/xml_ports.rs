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
pub(super) fn refine_copied_fallback_source_shapes(
    components: &mut [SchemaComponent],
    edge_from: &BTreeMap<u32, u32>,
    copy_all_targets: &BTreeSet<u32>,
    fallback_source_outputs: &BTreeSet<u32>,
    fallback_target_inputs: &BTreeSet<u32>,
    warnings: &mut Vec<String>,
) {
    let mut inferred = BTreeMap::<(usize, Vec<String>), Option<SchemaNode>>::new();
    let mut rejected = BTreeSet::<(usize, Vec<String>)>::new();
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
            let Some(source_node) = schema_node_at(&source.schema, source_path) else {
                continue;
            };
            if source_node.name != target_group.name || source_path.last() != target_path.last() {
                continue;
            }
            let key = (source_index, source_path.clone());
            let recoverable = plain_entry_tree_leaf(source_node)
                || exact_fallback_alternative_subset(source_node, target_group);
            if !recoverable {
                if !source_node.alternatives().is_empty()
                    && target_group.alternatives().len() > source_node.alternatives().len()
                {
                    rejected.insert(key);
                }
                continue;
            }
            if source.output_keys.iter().any(|output| {
                source.ports.get(output).is_some_and(|path| {
                    path.len() > source_path.len() && path.starts_with(source_path)
                })
            }) {
                continue;
            }
            let mut replacement = target_group.clone();
            replacement.xml_namespace = source_node.xml_namespace.clone();
            inferred
                .entry(key.clone())
                .and_modify(|candidate| {
                    if candidate.as_ref() != Some(&replacement) {
                        *candidate = None;
                        rejected.insert(key.clone());
                    }
                })
                .or_insert(Some(replacement));
        }
    }
    for ((source_index, source_path), replacement) in inferred {
        if rejected.contains(&(source_index, source_path.clone())) {
            continue;
        }
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
    for (source_index, source_path) in rejected {
        let component = components
            .get(source_index)
            .map(|component| component.name.as_str())
            .unwrap_or("<unknown>");
        warnings.push(format!(
            "fallback XML type alternatives at `{component}/{}` could not be recovered exactly from connected target schemas; conditioned subset retained",
            source_path.join("/")
        ));
    }
}

/// A failed target XML Schema import can likewise leave a structural input as
/// a plain string. An exact copy-all edge from one typed source group supplies
/// that target shape when the target has no independently connected content.
pub(super) fn refine_copied_fallback_target_groups(
    components: &mut [SchemaComponent],
    edge_from: &BTreeMap<u32, u32>,
    copy_all_targets: &BTreeSet<u32>,
    fallback_source_outputs: &BTreeSet<u32>,
    fallback_target_inputs: &BTreeSet<u32>,
    warnings: &mut Vec<String>,
) {
    let mut inferred = BTreeMap::<(usize, Vec<String>), Option<SchemaNode>>::new();
    let mut rejected = BTreeSet::<(usize, Vec<String>)>::new();
    for (target_index, target) in components
        .iter()
        .enumerate()
        .filter(|(_, component)| component.format == ComponentFormat::Xml && component.is_target())
    {
        for (input, target_path) in &target.ports {
            if target_path.is_empty()
                || !copy_all_targets.contains(input)
                || !fallback_target_inputs.contains(input)
            {
                continue;
            }
            let Some(feed) = edge_from
                .get(input)
                .filter(|feed| !fallback_source_outputs.contains(feed))
            else {
                continue;
            };
            let Some(target_node) = schema_node_at(&target.schema, target_path)
                .filter(|node| plain_entry_tree_leaf(node))
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
            let key = (target_index, target_path.clone());
            let mut owners = components.iter().filter(|source| {
                source.format == ComponentFormat::Xml
                    && source.is_source
                    && source.output_keys.contains(feed)
            });
            let Some(source) = owners.next() else {
                rejected.insert(key);
                continue;
            };
            if owners.next().is_some() {
                rejected.insert(key);
                continue;
            }
            let Some(source_path) = source.ports.get(feed) else {
                rejected.insert(key);
                continue;
            };
            let Some(source_group) = schema_node_at(&source.schema, source_path)
                .filter(|node| !node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
            else {
                rejected.insert(key);
                continue;
            };
            if source_group.name != target_node.name
                || source_path.last() != target_path.last()
                    && !(source_path.is_empty() && source_group.name == target_node.name)
            {
                rejected.insert(key);
                continue;
            }
            let mut replacement = source_group.clone();
            replacement.xml_namespace = target_node.xml_namespace.clone();
            inferred
                .entry(key.clone())
                .and_modify(|candidate| {
                    if candidate.as_ref() != Some(&replacement) {
                        *candidate = None;
                        rejected.insert(key.clone());
                    }
                })
                .or_insert(Some(replacement));
        }
    }
    for ((target_index, target_path), replacement) in inferred {
        if rejected.contains(&(target_index, target_path.clone())) {
            continue;
        }
        let Some(replacement) = replacement else {
            continue;
        };
        if let Some(target) = components
            .get_mut(target_index)
            .and_then(|component| schema_node_at_mut(&mut component.schema, &target_path))
        {
            *target = replacement;
        }
    }
    for (target_index, target_path) in rejected {
        let component = components
            .get(target_index)
            .map(|component| component.name.as_str())
            .unwrap_or("<unknown>");
        warnings.push(format!(
            "fallback XML target group at `{component}/{}` could not be recovered exactly from its copy-all source; scalar placeholder retained",
            target_path.join("/")
        ));
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

fn exact_fallback_alternative_subset(source: &SchemaNode, target: &SchemaNode) -> bool {
    if source.name != target.name
        || source.repeating != target.repeating
        || source.recursive_ref != target.recursive_ref
        || source.attribute != target.attribute
        || source.text != target.text
        || source.nillable != target.nillable
        || source.nullable != target.nullable
        || source.container_nullable != target.container_nullable
        || source.json_any != target.json_any
        || source.fixed != target.fixed
        || source.default != target.default
        || source.value_generation != target.value_generation
        || source.alternative_mode != target.alternative_mode
        || source.xml_alternative_kind != target.xml_alternative_kind
        || source.xml_repeating_sequences != target.xml_repeating_sequences
        || source.database_relation != target.database_relation
        || source
            .xml_namespace
            .as_ref()
            .is_some_and(|namespace| target.xml_namespace.as_ref() != Some(namespace))
    {
        return false;
    }
    let (
        SchemaKind::Group {
            children: source_children,
            alternatives: source_alternatives,
            xml_restricted_alternatives: source_restricted,
            dynamic: source_dynamic,
        },
        SchemaKind::Group {
            children: target_children,
            alternatives: target_alternatives,
            xml_restricted_alternatives: target_restricted,
            dynamic: target_dynamic,
        },
    ) = (&source.kind, &target.kind)
    else {
        return false;
    };
    !source_alternatives.is_empty()
        && target_alternatives.len() > source_alternatives.len()
        && source_restricted == target_restricted
        && source_dynamic == target_dynamic
        && source_children
            .iter()
            .all(|child| target_children.iter().any(|candidate| candidate == child))
        && source_alternatives.iter().all(|alternative| {
            target_alternatives
                .iter()
                .any(|candidate| candidate == alternative)
        })
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
