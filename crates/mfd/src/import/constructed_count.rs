use std::collections::BTreeSet;

use ir::{SchemaKind, SchemaNode};

use super::constructed_variable_driver::{
    active_parent_frame, declared_producer_count, plain_parent_chain,
};
use super::function::FnComponent;
use super::graph::GraphBuilder;
use super::iteration::split_at_innermost_repeating;
use super::schema::{ComponentFormat, SchemaComponent, schema_node_at};
use super::scope::ScopeBuilder;
use super::source::MAX_SEQUENCE_HOPS;

/// Ordinary sequence resolution visits each forward and then the source.
const MAX_FORWARD_HOPS: usize = MAX_SEQUENCE_HOPS - 1;
const MAX_COLLECTION_DEPTH: usize = 64;

/// A constructed record field may count one physical primary JSON collection,
/// reached directly or through plain structural JSON variable forwards.
/// Ordinary lowering emits `Aggregate::Count` over that collection relative to
/// the actual enclosing parent frame: null-valued items count and an empty or
/// absent collection yields zero. A count never witnesses record presence.
pub(super) fn assignment(
    feed: u32,
    function: &FnComponent,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> bool {
    let [None, Some(sequence)] = function.inputs.as_slice() else {
        return false;
    };
    if function.library != "core"
        || function.name != "count"
        || function.kind != 5
        || !function.raw_count_signature_valid
        || !function.raw_single_output_valid
        || function.outputs.as_slice() != [feed]
        || function.output_pins.as_slice() != [Some(feed)]
        || !plain_parent_chain(target_path, scopes)
    {
        return false;
    }
    let Some(sequence) = builder.edge_from.get(sequence).copied() else {
        return false;
    };
    let (Some(collection), Some(primary)) = (
        physical_collection(sequence, builder),
        builder.sources.first(),
    ) else {
        return false;
    };
    // Every outer repetition must be an actual plain parent frame; the counted
    // collection itself must not be one, or the count would reduce one item.
    collection.len() <= MAX_COLLECTION_DEPTH
        && (1..=collection.len()).all(|length| {
            schema_node_at(&primary.schema, &collection[..length]).is_some_and(|node| {
                !node.repeating
                    || (length < collection.len())
                        == active_parent_frame(&collection[..length], target_path, builder, scopes)
            })
        })
        // Ordinary aggregate lowering must reduce the same physical collection.
        && builder.sequence_source_path(sequence).is_some_and(|resolved| {
            resolved.source == 0
                && resolved.path == collection
                && split_at_innermost_repeating(&primary.schema, &resolved.path)
                    == (collection.clone(), Vec::new())
        })
}

/// Follow a bounded acyclic chain of plain JSON variable forwards to one
/// primary physical JSON repeating group, comparing every complete selected
/// schema with the collection it forwards.
fn physical_collection(mut feed: u32, builder: &GraphBuilder<'_>) -> Option<Vec<String>> {
    let mut seen = BTreeSet::new();
    let mut forwarded: Option<&SchemaNode> = None;
    for _ in 0..=MAX_FORWARD_HOPS {
        if !seen.insert(feed) || declared_producer_count(feed, builder) != 1 {
            return None;
        }
        if let Some(path) = primary_collection(feed, builder) {
            let node = schema_node_at(&builder.sources.first()?.schema, path)?;
            return forwarded
                .is_none_or(|forwarded| same_collection(forwarded, node))
                .then(|| path.clone());
        }
        let (node, input) = forward(feed, builder)?;
        if forwarded.is_some_and(|forwarded| !same_collection(forwarded, node)) {
            return None;
        }
        forwarded = Some(node);
        feed = input;
    }
    None
}

fn primary_collection<'a>(feed: u32, builder: &'a GraphBuilder<'_>) -> Option<&'a Vec<String>> {
    let primary = builder.sources.first()?;
    let path = primary.ports.get(&feed)?;
    let node = schema_node_at(&primary.schema, path)?;
    (ordinary_json_source(primary)
        && primary.unambiguous_declared_output(feed)
        && node.repeating
        && matches!(node.kind, SchemaKind::Group { .. })
        && !builder.has_source_node_functions(feed)
        && !builder.udf_by_output.contains_key(&feed)
        && !builder
            .sources
            .iter()
            .skip(1)
            .any(|source| source.declares_output(feed) || source.ports.contains_key(&feed))
        && !path.first().is_some_and(|first| {
            builder
                .source_names
                .iter()
                .skip(1)
                .any(|name| name == first)
        }))
    .then_some(path)
}

fn ordinary_json_source(source: &SchemaComponent) -> bool {
    source.format == ComponentFormat::Json
        && source.is_source
        && !source.is_variable
        && !source.is_pass_through
        && source.compute_when_key.is_none()
        && source.dynamic_json.is_none()
        && source.input_keys.is_empty()
        && source.db_queries.is_empty()
        && !source.has_target_node_controls
        && !source.schema.repeating
}

/// One plain JSON variable forward: a selected repeating group whose only
/// connected input is the exact structural input at that same group.
fn forward<'a>(feed: u32, builder: &'a GraphBuilder<'_>) -> Option<(&'a SchemaNode, u32)> {
    let mut owners = builder
        .intermediates
        .iter()
        .filter(|owner| owner.declares_output(feed));
    let variable = owners.next()?;
    if owners.next().is_some()
        || !variable.is_variable
        || variable.is_pass_through
        || variable.format != ComponentFormat::Json
        || variable.compute_when_key.is_some()
        || variable.dynamic_json.is_some()
        || variable.has_target_node_controls
        || !variable.unambiguous_declared_output(feed)
        || builder.udf_by_output.contains_key(&feed)
        || builder.has_source_node_functions(feed)
    {
        return None;
    }
    let path = variable.ports.get(&feed)?;
    let node = schema_node_at(&variable.schema, path)?;
    if !node.repeating
        || !matches!(node.kind, SchemaKind::Group { .. })
        || (0..path.len()).any(|length| {
            schema_node_at(&variable.schema, &path[..length]).is_none_or(|ancestor| {
                ancestor.repeating
                    || ancestor.dynamic_fields().is_some()
                    || !ancestor.alternatives().is_empty()
            })
        })
        || variable
            .input_keys
            .iter()
            .any(|key| variable.ambiguous_input_affects(*key, path))
    {
        return None;
    }
    let mut connected = variable
        .input_keys
        .iter()
        .filter(|key| builder.edge_from.contains_key(key));
    let input = *connected.next()?;
    (connected.next().is_none()
        && variable.ports.get(&input) == Some(path)
        && variable.unambiguous_declared_input(input))
    .then(|| (node, builder.edge_from[&input]))
}

/// Forwarding preserves the complete collection schema, including item
/// cardinality and uniqueness; only the local name may differ.
fn same_collection(left: &SchemaNode, right: &SchemaNode) -> bool {
    let mut left = left.clone();
    let mut right = right.clone();
    left.name.clear();
    right.name.clear();
    left == right
}
