use std::collections::BTreeSet;

use ir::{SchemaKind, SchemaNode};
use mapping::{ScopeConstruction, ScopeIteration};

use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, schema_node_at};
use super::scope::ScopeBuilder;
use super::target_iteration::literal_constant_value;

/// A flat variable constructs one present record without introducing a frame.
/// This is a presence driver; scalar consumers retain their ordinary bindings.
pub(super) fn recognize(
    output: u32,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> bool {
    let mut owners = builder
        .intermediates
        .iter()
        .filter(|component| component.output_keys.contains(&output));
    let Some(variable) = owners.next() else {
        return false;
    };
    if owners.next().is_some()
        || !variable.is_variable
        || variable.is_pass_through
        || !matches!(
            variable.format,
            ComponentFormat::Json | ComponentFormat::Edi
        )
        || variable.compute_when_key.is_some()
    {
        return false;
    }
    let Some(output_path) = variable.ports.get(&output) else {
        return false;
    };
    let Some(record) = schema_node_at(&variable.schema, output_path) else {
        return false;
    };
    let SchemaKind::Group { children, .. } = &record.kind else {
        return false;
    };
    if record.repeating
        || (0..output_path.len()).any(|length| {
            schema_node_at(&variable.schema, &output_path[..length])
                .is_none_or(|node| node.repeating)
        })
        || record.dynamic_fields().is_some()
        || !record.alternatives().is_empty()
        || children
            .iter()
            .any(|child| child.repeating || !child.is_scalar() || !child.alternatives().is_empty())
    {
        return false;
    }
    let mut assigned = BTreeSet::new();
    let mut present = false;
    for (key, path) in &variable.ports {
        if !variable.input_keys.contains(key) {
            continue;
        }
        let Some(feed) = builder.edge_from.get(key).copied() else {
            continue;
        };
        if path.len() < output_path.len() && output_path.starts_with(path) {
            return false;
        }
        let Some(relative) = path.strip_prefix(output_path.as_slice()) else {
            continue;
        };
        if relative.len() != 1
            || !schema_node_at(record, relative).is_some_and(|node| node.is_scalar())
            || !assigned.insert(relative.to_vec())
        {
            return false;
        }
        if literal_constant_value(feed, builder).is_some()
            && builder
                .fn_components
                .iter()
                .filter(|component| component.outputs.contains(&feed))
                .count()
                == 1
            && !builder
                .sources
                .iter()
                .any(|source| source.ports.contains_key(&feed))
            && !builder
                .intermediates
                .iter()
                .any(|component| component.output_keys.contains(&feed))
        {
            present = true;
        } else if !parent_scalar(feed, target_path, builder, scopes) {
            return false;
        }
    }
    present
}

const MAX_SCALAR_ALIAS_HOPS: usize = 64;

fn parent_scalar(
    mut feed: u32,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> bool {
    let mut seen = BTreeSet::new();
    let mut expected: Option<&SchemaNode> = None;
    for hop in 0..=MAX_SCALAR_ALIAS_HOPS {
        if !seen.insert(feed) {
            return false;
        }
        if direct_parent_scalar(feed, target_path, builder, scopes) {
            let source = builder.sources[0];
            let node = schema_node_at(&source.schema, &source.ports[&feed]);
            return expected.is_none_or(|expected| {
                node.is_some_and(|node| same_scalar_schema(expected, node))
            });
        }
        if hop == MAX_SCALAR_ALIAS_HOPS
            || builder
                .sources
                .iter()
                .any(|source| source.ports.contains_key(&feed))
            || builder.fn_by_output.contains_key(&feed)
            || builder.udf_by_output.contains_key(&feed)
            || builder.has_source_node_functions(feed)
        {
            return false;
        }
        let mut owners = builder
            .intermediates
            .iter()
            .filter(|owner| owner.output_keys.contains(&feed));
        let Some(alias) = owners.next() else {
            return false;
        };
        if owners.next().is_some()
            || !alias.is_variable
            || alias.is_pass_through
            || alias.compute_when_key.is_some()
            || !matches!(alias.format, ComponentFormat::Json | ComponentFormat::Edi)
        {
            return false;
        }
        let Some(path) = alias.ports.get(&feed) else {
            return false;
        };
        let Some(node) = schema_node_at(&alias.schema, path) else {
            return false;
        };
        if !node.is_scalar()
            || node.repeating
            || !node.alternatives().is_empty()
            || expected.is_some_and(|expected| !same_scalar_schema(expected, node))
            || (0..path.len()).any(|length| {
                schema_node_at(&alias.schema, &path[..length]).is_none_or(|ancestor| {
                    ancestor.repeating
                        || ancestor.dynamic_fields().is_some()
                        || !ancestor.alternatives().is_empty()
                })
            })
        {
            return false;
        }
        let mut inputs = alias.ports.iter().filter(|(key, candidate)| {
            alias.input_keys.contains(key)
                && builder.edge_from.contains_key(key)
                && (path.starts_with(candidate) || candidate.starts_with(path))
        });
        let Some((input, input_path)) = inputs.next() else {
            return false;
        };
        // Only exact scalar pass-throughs qualify. A structural ancestor or
        // descendant override needs a separate complete structural proof.
        if inputs.next().is_some() || input_path != path {
            return false;
        }
        expected = Some(node);
        feed = builder.edge_from[input];
    }
    false
}

fn same_scalar_schema(left: &SchemaNode, right: &SchemaNode) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.name.clear();
    right.name.clear();
    left == right
}

fn direct_parent_scalar(
    feed: u32,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> bool {
    // Inspect physical ports directly: computed/variable aliases do not prove ownership.
    let Some(owner) = builder.sources.first() else {
        return false;
    };
    let Some(path) = owner.ports.get(&feed) else {
        return false;
    };
    if !owner.output_keys.contains(&feed)
        || !matches!(owner.format, ComponentFormat::Json | ComponentFormat::Xml)
        || owner.options.local_xml_file_set
        || owner.schema.repeating
        || !schema_node_at(&owner.schema, path)
            .is_some_and(|node| node.is_scalar() && !node.repeating)
        || builder
            .sources
            .iter()
            .skip(1)
            .any(|source| source.ports.contains_key(&feed))
        || builder.fn_by_output.contains_key(&feed)
        || builder.has_source_node_functions(feed)
        || builder
            .intermediates
            .iter()
            .any(|variable| variable.output_keys.contains(&feed))
        || path.first().is_some_and(|first| {
            builder
                .source_names
                .iter()
                .skip(1)
                .any(|name| name == first)
        })
        || !plain_parent_chain(target_path, scopes)
    {
        return false;
    }
    for length in 1..=path.len() {
        let Some(node) = schema_node_at(&owner.schema, &path[..length]) else {
            return false;
        };
        if node.repeating && !active_parent_frame(&path[..length], target_path, builder, scopes) {
            return false;
        }
    }
    true
}

fn active_parent_frame(
    source_path: &[String],
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> bool {
    if !builder.framed.contains(source_path) {
        return false;
    }
    // Global framed metadata alone is insufficient: a sibling may own the frame.
    // Require a plain source-backed ancestor with this exact physical anchor.
    plain_parent_chain(target_path, scopes)
        && (0..target_path.len()).any(|length| {
            scopes.scope(&target_path[..length]).is_some_and(|scope| {
                matches!(scope.iteration, ScopeIteration::Source(_))
                    && scopes
                        .anchors
                        .get(&target_path[..length])
                        .is_some_and(|anchor| anchor == source_path)
            })
        })
}

fn plain_parent_chain(target_path: &[String], scopes: &ScopeBuilder) -> bool {
    for length in 0..target_path.len() {
        let Some(scope) = scopes.scope(&target_path[..length]) else {
            continue;
        };
        if !matches!(
            scope.iteration,
            ScopeIteration::None | ScopeIteration::Source(_)
        ) || !matches!(scope.construction, ScopeConstruction::Constructed)
            || scope.filter.is_some()
            || scope.post_group_filter.is_some()
            || scope.group_by.is_some()
            || scope.group_adjacent_by.is_some()
            || scope.group_starting_with.is_some()
            || scope.group_ending_with.is_some()
            || scope.group_into_blocks.is_some()
            || scope.sort_by.is_some()
            || !scope.sort_then_by.is_empty()
            || !scope.windows.is_empty()
        {
            return false;
        }
    }
    true
}
