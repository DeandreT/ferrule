use std::collections::BTreeMap;

use ir::SchemaNode;
use mapping::SortFilterOrder;

use super::source::SourcePath;

pub(super) struct IterationFeed {
    /// Output key of the underlying source entry (or whatever else feeds
    /// the chain -- callers check it against the source ports).
    pub(super) source_key: u32,
    /// Physical collection inferred from the scalar expression at
    /// `source_key`. The expression remains the feed so it can be evaluated
    /// once per item in this collection's frame.
    pub(super) computed_source: Option<SourcePath>,
    /// A function that generates the sequence instead of reading a source
    /// collection directly.
    pub(super) sequence_component: Option<usize>,
    /// A database kind=21 where/order control crossed by the sequence.
    pub(super) db_where_component: Option<usize>,
    /// Path below `source_key` selected by transparent intermediate schema
    /// components crossed on the way to the target iteration.
    pub(super) source_suffix: Vec<String>,
    /// The filter's boolean expression key, if a filter was crossed.
    pub(super) filter_expr: Option<u32>,
    /// The sequence came from the filter's false output rather than its true output.
    pub(super) filter_inverted: bool,
    /// Scalar UDF outputs that are exact nullable pass-through filters. Each
    /// key resolves to the UDF's per-item keep predicate at materialization.
    pub(super) udf_filters: Vec<u32>,
    /// Whether a filter was crossed, including one with a missing condition.
    pub(super) has_filter: bool,
    /// The group-by's key expression key, if a group-by was crossed.
    pub(super) group_key: Option<u32>,
    /// Whether a key-based group operation was crossed.
    pub(super) has_key_grouping: bool,
    /// Boundary predicate for a contiguous group-starting-with operation.
    pub(super) group_starting_with: Option<u32>,
    /// Whether the chain contains group-starting-with, including a malformed
    /// component with a missing predicate.
    pub(super) has_start_grouping: bool,
    /// The block-size expression key, if group-into-blocks was crossed.
    pub(super) block_size: Option<u32>,
    /// Whether the chain contains group-into-blocks, including a component
    /// whose required size pin is absent or cannot be resolved.
    pub(super) has_block_grouping: bool,
    /// Scalar sequence feeding a distinct-values component. It becomes the
    /// grouping key while iteration retains the owning source item.
    pub(super) distinct_key: Option<u32>,
    /// First unsupported operator ordering found while unwrapping the
    /// sequence. The scope still imports using ferrule's canonical order.
    pub(super) order_issue: Option<&'static str>,
    /// Sort key expressions and directions crossed by the sequence, in
    /// lexicographic priority order. Missing pins remain explicit.
    pub(super) sort_keys: Vec<(Option<u32>, bool)>,
    /// Whether a sort was crossed, including one with a missing key.
    pub(super) has_sort: bool,
    pub(super) sort_filter_order: SortFilterOrder,
    /// A connected first-items count, or an absent count meaning the
    /// function's default of one item.
    pub(super) take_expr: Option<u32>,
    pub(super) take_default_one: bool,
    /// A transparent variable projects the connected source group as a
    /// constructed value, so matching scalar descendants must be copied.
    pub(super) projects_whole_group: bool,
    /// Scalar descendant inputs used to construct an intermediate group,
    /// keyed by their path relative to that group's output.
    pub(super) projections: BTreeMap<Vec<String>, u32>,
}

pub(super) fn note_iteration_control_order(
    upstream: u8,
    nearest_downstream: &mut Option<u8>,
    issue: &mut Option<&'static str>,
) {
    if let Some(downstream) = *nearest_downstream
        && upstream > downstream
    {
        issue.get_or_insert(match (upstream, downstream) {
            (1, 0) => "applies sort after filter, which cannot be represented exactly",
            (2, 0) => "applies sort after group-by, which cannot be represented exactly",
            (2, 1) => "applies filter after group-by, which cannot be represented exactly",
            (3, 0) => "applies sort after first-items, which cannot be represented exactly",
            (3, 1) => "applies filter after first-items, which cannot be represented exactly",
            (3, 2) => "applies group-by after first-items, which cannot be represented exactly",
            _ => "uses a sequence-control order that cannot be represented exactly",
        });
    }
    *nearest_downstream = Some(nearest_downstream.map_or(upstream, |rank| rank.min(upstream)));
}

pub(super) struct IntermediateFeed {
    pub(super) feed: u32,
    pub(super) suffix: Vec<String>,
    pub(super) control: Option<u32>,
    pub(super) projections: BTreeMap<Vec<String>, u32>,
}

/// Splits an absolute source path at its innermost repeating node: the
/// collection is everything up to and including it, the value the rest.
/// With no repeating node the collection is empty -- flat-rows sources
/// (csv/db) hold their repetition outside the schema.
pub(super) fn split_at_innermost_repeating(
    schema: &SchemaNode,
    abs: &[String],
) -> (Vec<String>, Vec<String>) {
    let mut node = schema;
    let mut cut = None;
    for (i, segment) in abs.iter().enumerate() {
        let Some(child) = node.child(segment) else {
            break;
        };
        if child.repeating {
            cut = Some(i);
        }
        node = child;
    }
    match cut {
        Some(i) => (abs[..=i].to_vec(), abs[i + 1..].to_vec()),
        None => (Vec::new(), abs.to_vec()),
    }
}

/// Picks the deepest repeated collection used by a computed expression,
/// provided every other dependency belongs to that collection or one of its
/// enclosing contexts. Empty collections represent flat row sources.
pub(super) fn compatible_collection(
    schema: &SchemaNode,
    paths: &[Vec<String>],
) -> Option<Vec<String>> {
    if paths.is_empty() {
        return None;
    }
    let collections: Vec<Vec<String>> = paths
        .iter()
        .map(|path| split_at_innermost_repeating(schema, path).0)
        .collect();
    let deepest = collections.iter().max_by_key(|path| path.len())?.clone();
    collections
        .iter()
        .all(|path| deepest.starts_with(path))
        .then_some(deepest)
}
