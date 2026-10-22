use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode};
use mapping::{IterationOutput, ScopeConstruction, ScopeIteration};

use super::branch_owners::{self, BindingOrigins, DeclaredBranch, DeclaredBranches};
use super::{build_one, literal_constant_value, take_scope};
use crate::import::graph::GraphBuilder;
use crate::import::group_projection::TargetIteration;
use crate::import::schema::{ComponentFormat, SchemaComponent, schema_node_at};
use crate::import::scope::{BindingOccurrence, ScopeBuilder, TargetLeaf};

mod csv_x12;
mod edi_json;
mod json_implicit;
#[cfg(test)]
mod json_tests;
mod lowering;
mod parse_date;
#[cfg(test)]
mod parse_date_tests;
#[cfg(test)]
mod tests;

const MAX_BRANCHES: usize = 64;
const MAX_BINDINGS: usize = 256;
const MAX_DEPTH: usize = 64;

pub(super) fn csv_raw_authority(
    structure: &roxmltree::Node<'_, '_>,
    feeds: &BTreeMap<u32, u32>,
) -> bool {
    csv_x12::raw_authority(structure, feeds)
}

/// A declared occurrence either owns an already supported physical iteration or
/// constructs one item in its actual enclosing frame. Other driver contracts
/// must establish their own presence proof before joining this classification.
enum BranchDriver {
    Implicit,
    Source { feed: u32, anchor: Vec<String> },
}

struct BranchPlan {
    marker: u32,
    lineage: Vec<u32>,
    anchors: BTreeMap<Vec<String>, Vec<String>>,
    driver: BranchDriver,
    bindings: Vec<usize>,
    conditionals: BTreeMap<usize, StrictConditionalProof>,
    value_maps: BTreeMap<usize, TotalStringValueMapProof>,
    parse_dates: BTreeMap<usize, parse_date::ParseDateProof>,
    json_scalars: BTreeMap<usize, json_implicit::ScalarProof>,
    csv_scalars: BTreeMap<usize, csv_x12::ScalarProof>,
}

struct TotalStringValueMapProof {
    frame: Vec<String>,
    field: Vec<String>,
    table: Vec<(ir::Value, ir::Value)>,
    default: ir::Value,
}

impl TotalStringValueMapProof {
    fn matches_lowered(&self, node: mapping::NodeId, builder: &GraphBuilder<'_>) -> bool {
        let Some(mapping::Node::ValueMap {
            input,
            input_type,
            table,
            default,
        }) = builder.graph.nodes.get(&node)
        else {
            return false;
        };
        *input_type == Some(ir::ScalarType::String)
            && table == &self.table
            && default.as_ref() == Some(&self.default)
            && matches!(builder.graph.nodes.get(input), Some(mapping::Node::SourceField { path, frame: Some(frame) })
                if path == &self.field && frame == &self.frame)
    }
}

fn total_string_value_map(
    feed: u32,
    anchor: &[String],
    target: &SchemaNode,
    builder: &GraphBuilder<'_>,
) -> Option<TotalStringValueMapProof> {
    if !unique_producer(feed, builder)
        || !builder.framed.contains(anchor)
        || target.repeating
        || !matches!(
            target.kind,
            SchemaKind::Scalar {
                ty: ir::ScalarType::String
            }
        )
    {
        return None;
    }
    let component = builder
        .fn_components
        .get(*builder.fn_by_output.get(&feed)?)?;
    if component.library != "core"
        || component.name != "value-map"
        || component.kind != 23
        || !component.raw_single_output_valid
        || !component.bounded_scalar_pins_valid
        || component.output_pins.as_slice() != [Some(feed)]
        || component.outputs.as_slice() != [feed]
    {
        return None;
    }
    let [Some(input)] = component.inputs.as_slice() else {
        return None;
    };
    let source_feed = *builder.edge_from.get(input)?;
    if !unique_producer(source_feed, builder) {
        return None;
    }
    let primary = builder.sources.first()?;
    if primary.is_variable
        || primary.is_pass_through
        || primary.compute_when_key.is_some()
        || primary.dynamic_json.is_some()
    {
        return None;
    }
    let absolute = physical_scalar(source_feed, anchor, builder)?;
    let source = &primary.schema;
    if absolute.len() > MAX_DEPTH
        || anchor.len() > MAX_DEPTH
        || !guaranteed_scalar(absolute, anchor, source)
        || !matches!(
            schema_node_at(source, absolute)?.kind,
            SchemaKind::Scalar {
                ty: ir::ScalarType::String
            }
        )
        || (0..=absolute.len()).any(|length| {
            schema_node_at(source, &absolute[..length]).is_none_or(|node| {
                node.nullable
                    || node.container_nullable
                    || node.recursive_ref.is_some()
                    || !node.alternatives().is_empty()
                    || node.dynamic_fields().is_some()
                    || node.json_any
                    || nullable_object_item_frame(source, &absolute[..length])
            })
        })
    {
        return None;
    }
    let normalized = component.valuemap.as_ref()?;
    let raw = normalized.total_string.as_ref()?;
    let ownership = raw.ownership.as_ref()?;
    if ownership.output != feed || ownership.source != source_feed {
        return None;
    }
    let table = raw
        .table
        .iter()
        .map(|(from, to)| {
            (
                ir::Value::String(from.clone()),
                ir::Value::String(to.clone()),
            )
        })
        .collect::<Vec<_>>();
    let default = ir::Value::String(raw.default.clone());
    if normalized.input_type != Some(ir::ScalarType::String)
        || normalized.table != table
        || normalized.default.as_ref() != Some(&default)
    {
        return None;
    }
    Some(TotalStringValueMapProof {
        frame: anchor.to_vec(),
        field: absolute.strip_prefix(anchor)?.to_vec(),
        table,
        default,
    })
}

/// One strict conditional whose successful result is a nonnull scalar. Its
/// predicate remains fallible at runtime; absence keeps the existing Bool error.
struct StrictConditionalProof {
    frame: Vec<String>,
    predicate: Vec<String>,
    when_true: ConditionalBranchProof,
    when_false: ConditionalBranchProof,
}

enum ConditionalBranchProof {
    Literal(ir::Value),
    PhysicalString {
        frame: Vec<String>,
        path: Vec<String>,
    },
}

impl ConditionalBranchProof {
    fn matches_lowered(&self, node: mapping::NodeId, builder: &GraphBuilder<'_>) -> bool {
        match self {
            Self::Literal(value) => exact_constant_node(node, value, builder),
            Self::PhysicalString { frame, path } => matches!(
                builder.graph.nodes.get(&node),
                Some(mapping::Node::SourceField { path: actual_path, frame: Some(actual_frame) })
                    if actual_path == path && actual_frame == frame
            ),
        }
    }
}

impl StrictConditionalProof {
    fn matches_lowered(&self, node: mapping::NodeId, builder: &GraphBuilder<'_>) -> bool {
        let Some(mapping::Node::If {
            condition,
            then,
            else_,
        }) = builder.graph.nodes.get(&node)
        else {
            return false;
        };
        matches!(
            builder.graph.nodes.get(condition),
            Some(mapping::Node::SourceField { path, frame: Some(frame) })
                if path == &self.predicate && frame == &self.frame
        ) && self.when_true.matches_lowered(*then, builder)
            && self.when_false.matches_lowered(*else_, builder)
    }
}

pub(super) fn exact_constant_node(
    node: mapping::NodeId,
    expected: &ir::Value,
    builder: &GraphBuilder<'_>,
) -> bool {
    let Some(mapping::Node::Const { value }) = builder.graph.nodes.get(&node) else {
        return false;
    };
    match (value, expected) {
        (ir::Value::Float(actual), ir::Value::Float(expected)) => {
            actual.to_bits() == expected.to_bits()
        }
        _ => value == expected,
    }
}

pub(super) fn exact_literal(feed: u32, builder: &GraphBuilder<'_>) -> Option<ir::Value> {
    if !unique_producer(feed, builder) {
        return None;
    }
    let component = builder
        .fn_components
        .get(*builder.fn_by_output.get(&feed)?)?;
    if !component.raw_single_output_valid
        || !component.bounded_scalar_pins_valid
        || component.output_pins.as_slice() != [Some(feed)]
    {
        return None;
    }
    literal_constant_value(feed, builder)
}

fn constant_has_type(value: &ir::Value, expected: ir::ScalarType) -> bool {
    matches!(
        (value, expected),
        (ir::Value::String(_), ir::ScalarType::String)
            | (ir::Value::Bool(_), ir::ScalarType::Bool)
            | (ir::Value::Int(_), ir::ScalarType::Int)
            | (ir::Value::Float(_), ir::ScalarType::Float)
    )
}

pub(super) fn ordinary_primary_json(builder: &GraphBuilder<'_>) -> bool {
    builder.sources.first().is_some_and(|primary| {
        primary.format == ComponentFormat::Json
            && primary.is_source
            && !primary.is_variable
            && !primary.is_pass_through
            && primary.compute_when_key.is_none()
            && primary.dynamic_json.is_none()
            && primary.db_queries.is_empty()
            && primary.db_xml_columns.is_empty()
            && primary.options
                == (mapping::FormatOptions {
                    json_document: true,
                    ..Default::default()
                })
            && !primary
                .input_keys
                .iter()
                .any(|key| builder.edge_from.contains_key(key))
            && !builder
                .source_node_functions
                .keys()
                .any(|key| primary.declares_output(*key))
    })
}

pub(super) fn ordinary_scalar_domain(
    path: &[String],
    anchor: &[String],
    source: &SchemaNode,
) -> bool {
    path.len() <= MAX_DEPTH
        && anchor.len() <= MAX_DEPTH
        && (0..=path.len()).all(|length| {
            schema_node_at(source, &path[..length]).is_some_and(|node| {
                !node.nullable
                    && !node.container_nullable
                    && node.recursive_ref.is_none()
                    && node.alternatives().is_empty()
                    && node.dynamic_fields().is_none()
                    && !node.json_any
                    && !node.json_array_wrapper
                    && !nullable_object_item_frame(source, &path[..length])
            })
        })
}

fn conditional_branch(
    feed: u32,
    anchor: &[String],
    ty: ir::ScalarType,
    builder: &GraphBuilder<'_>,
) -> Option<ConditionalBranchProof> {
    if let Some(value) = exact_literal(feed, builder) {
        return constant_has_type(&value, ty).then_some(ConditionalBranchProof::Literal(value));
    }
    if ty != ir::ScalarType::String
        || !unique_producer(feed, builder)
        || !ordinary_primary_json(builder)
    {
        return None;
    }
    let absolute = physical_scalar(feed, anchor, builder)?;
    let source = &builder.sources.first()?.schema;
    if !ordinary_scalar_domain(absolute, anchor, source)
        || !guaranteed_scalar(absolute, anchor, source)
        || !matches!(
            schema_node_at(source, absolute)?.kind,
            SchemaKind::Scalar {
                ty: ir::ScalarType::String
            }
        )
    {
        return None;
    }
    Some(ConditionalBranchProof::PhysicalString {
        frame: anchor.to_vec(),
        path: absolute.strip_prefix(anchor)?.to_vec(),
    })
}

fn strict_scalar_conditional(
    feed: u32,
    anchor: &[String],
    target: &SchemaNode,
    builder: &GraphBuilder<'_>,
) -> Option<StrictConditionalProof> {
    // These are raw operand and scalar-assignment checks, before lowering.
    // No UDF, alias, arbitrary scalar call or recursive conditional is admitted.
    if !unique_producer(feed, builder) || !builder.framed.contains(anchor) {
        return None;
    }
    let component = builder
        .fn_components
        .get(*builder.fn_by_output.get(&feed)?)?;
    if component.library != "core"
        || component.name != "if-else"
        || component.kind != 4
        || !component.raw_single_output_valid
        || !component.bounded_scalar_pins_valid
        || component.output_pins.as_slice() != [Some(feed)]
        || component.outputs.as_slice() != [feed]
    {
        return None;
    }
    let [Some(predicate), Some(when_true), Some(when_false)] = component.inputs.as_slice() else {
        return None;
    };
    let predicate = *builder.edge_from.get(predicate)?;
    if !unique_producer(predicate, builder) {
        return None;
    }
    let absolute = physical_scalar(predicate, anchor, builder)?;
    if !matches!(
        schema_node_at(&builder.sources.first()?.schema, absolute)?.kind,
        SchemaKind::Scalar {
            ty: ir::ScalarType::Bool
        }
    ) {
        return None;
    }
    let SchemaKind::Scalar { ty } = target.kind else {
        return None;
    };
    if target.repeating {
        return None;
    }
    let when_true_feed = *builder.edge_from.get(when_true)?;
    let when_false_feed = *builder.edge_from.get(when_false)?;
    let when_true = conditional_branch(when_true_feed, anchor, ty, builder)?;
    let when_false = conditional_branch(when_false_feed, anchor, ty, builder)?;
    if matches!(&when_true, ConditionalBranchProof::PhysicalString { .. })
        || matches!(&when_false, ConditionalBranchProof::PhysicalString { .. })
    {
        let primary = builder.sources.first()?;
        let ownership = component.conditional_ownership.as_ref()?;
        if ownership.output != feed
            || ownership.sources != [predicate, when_true_feed, when_false_feed]
            || ownership.primary_outputs != primary.output_keys
            || !ordinary_scalar_domain(absolute, anchor, &primary.schema)
        {
            return None;
        }
    }
    Some(StrictConditionalProof {
        frame: anchor.to_vec(),
        predicate: absolute.strip_prefix(anchor)?.to_vec(),
        when_true,
        when_false,
    })
}

#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32, u32)],
    iterations: &[TargetIteration],
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    claimed: &mut BTreeSet<usize>,
    declared: &mut DeclaredBranches,
    presence_paths: &BTreeSet<Vec<String>>,
) {
    csv_x12::build(
        target, bindings, iterations, builder, scopes, claimed, declared,
    );
    edi_json::build(
        target, bindings, iterations, builder, scopes, claimed, declared,
    );
    if !matches!(
        target.format,
        ComponentFormat::Xml | ComponentFormat::Edi | ComponentFormat::Json
    ) || builder
        .sources
        .first()
        .is_none_or(|source| source.format != ComponentFormat::Json)
    {
        return;
    }
    let mut paths = BTreeMap::<Vec<String>, Vec<u32>>::new();
    for (marker, path) in &target.input_branch_paths {
        if schema_node_at(&target.schema, path)
            .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
        {
            paths.entry(path.clone()).or_default().push(*marker);
        }
    }
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort_by_key(|(path, _)| path.len());
    for (path, markers) in paths {
        // Conditional presence drivers own their declared clones, lowered or refused.
        if presence_paths.contains(&path) {
            continue;
        }
        // Explicit multi-driver concatenations already carry the qualified
        // declaration ownership contract. Leave them on that existing path.
        if markers.len() < 2
            || !target.has_target_node_controls
                && !target.ports.iter().any(|(input, candidate)| {
                    candidate.starts_with(&path) && builder.edge_from.contains_key(input)
                })
            || iterations.iter().any(|iteration| {
                iteration.target_path == path && !iteration.additional_feeds.is_empty()
            })
        {
            continue;
        }
        if target.has_target_node_controls
            || markers.len() > MAX_BRANCHES
            || path.len() > MAX_DEPTH
            || iterations.iter().any(|iteration| {
                iteration.target_path.len() > path.len() && iteration.target_path.starts_with(&path)
            })
            || target.input_branch_paths.iter().any(|(_, descendant)| {
                descendant.len() > path.len() && descendant.starts_with(&path)
            })
            || !plain_parents(&path, scopes)
        {
            warn_unproved(&path, builder);
            continue;
        }
        let Some(plans) = plan(
            &path, &markers, target, bindings, builder, scopes, declared, claimed,
        ) else {
            warn_unproved(&path, builder);
            continue;
        };
        let Some((candidate_graph, candidate_scopes, candidate_claimed, candidate_declared)) =
            lowering::prepare(
                &path, plans, target, bindings, builder, scopes, claimed, declared,
            )
        else {
            builder.warnings.push(format!(
                "declared target clones in `{}` could not preserve every scalar binding; occurrence lowering skipped",
                path.join("/")
            ));
            continue;
        };
        *builder = candidate_graph;
        *scopes = candidate_scopes;
        *claimed = candidate_claimed;
        *declared = candidate_declared;
    }
}

fn warn_unproved(path: &[String], builder: &mut GraphBuilder<'_>) {
    builder.warnings.push(format!(
        "declared target clones in `{}` cannot prove complete enclosing-frame scalar occurrences; lowering skipped",
        path.join("/")
    ));
}

#[allow(clippy::too_many_arguments)]
fn plan(
    path: &[String],
    markers: &[u32],
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32, u32)],
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
    declared: &DeclaredBranches,
    claimed: &BTreeSet<usize>,
) -> Option<Vec<BranchPlan>> {
    let target_node = schema_node_at(&target.schema, path)?;
    if !plain_shape(target_node, 0)
        || target.format == ComponentFormat::Json
            && (!json_implicit::admit(target, builder)
                || !json_implicit::plain_item(target_node)
                || scopes
                    .scope(path)
                    .is_some_and(|scope| !json_implicit::replaceable(scope)))
    {
        return None;
    }
    if target.input_keys.iter().any(|input| {
        builder.edge_from.contains_key(input) && target.ambiguous_input_affects(*input, path)
    }) {
        return None;
    }
    for (input, input_path) in &target.ports {
        if !input_path.starts_with(path) || !builder.edge_from.contains_key(input) {
            continue;
        }
        let mut owners = target
            .input_ancestors
            .get(input)?
            .iter()
            .filter(|ancestor| markers.contains(ancestor));
        if owners.next().is_none()
            || owners.next().is_some()
            || !target.unambiguous_declared_input(*input)
            || target.ambiguous_branch_inputs.contains(input)
            || controlled_input(target, *input, builder)
        {
            return None;
        }
    }
    let mut owned = BTreeMap::<u32, Vec<usize>>::new();
    let mut count = 0;
    for (index, (binding, _, input)) in bindings.iter().enumerate() {
        let binding_path = binding.path();
        if binding_path.len() <= path.len() || !binding_path.starts_with(path) {
            continue;
        }
        count += 1;
        if count > MAX_BINDINGS || claimed.contains(&index) {
            return None;
        }
        let mut owners = target
            .input_ancestors
            .get(input)?
            .iter()
            .filter(|ancestor| markers.contains(ancestor));
        let owner = *owners.next()?;
        if owners.next().is_some()
            || !target.unambiguous_declared_input(*input)
            || target.ambiguous_branch_inputs.contains(input)
            || controlled_input(target, *input, builder)
        {
            return None;
        }
        owned.entry(owner).or_default().push(index);
    }
    let mut output = Vec::with_capacity(markers.len());
    for marker in markers {
        // A declaration with neither a driver nor a connected descendant has
        // no executable occurrence. Preserve its schema without adding a scope.
        if !target.ports.iter().any(|(input, input_path)| {
            input_path.starts_with(path)
                && builder.edge_from.contains_key(input)
                && target
                    .input_ancestors
                    .get(input)
                    .is_some_and(|ancestors| ancestors.contains(marker))
        }) {
            continue;
        }
        let lineage = branch_owners::lineage(target, *marker)?;
        if target.format == ComponentFormat::Json && lineage.len() != 1 {
            return None;
        }
        let anchors = declared.enclosing_anchors(path, &lineage, &scopes.anchors)?;
        let enclosing = enclosing_anchor(path, &anchors);
        if !actual_json_frame(path, &enclosing, target, builder, scopes)
            || target.format == ComponentFormat::Json
                && (!guaranteed_scalar(&enclosing, &[], &builder.sources.first()?.schema)
                    || !json_implicit::actual_array_frame(
                        path, &enclosing, target, builder, scopes,
                    ))
        {
            return None;
        }
        let driver = classify_driver(path, *marker, target, builder, &enclosing)?;
        if target.format == ComponentFormat::Json && !matches!(driver, BranchDriver::Implicit) {
            return None;
        }
        let anchor = match &driver {
            BranchDriver::Implicit => &enclosing,
            BranchDriver::Source { anchor, .. } => anchor,
        };
        let indices = owned.remove(marker).unwrap_or_default();
        if indices.is_empty() && matches!(driver, BranchDriver::Implicit) {
            return None;
        }
        let mut conditionals = BTreeMap::new();
        let mut value_maps = BTreeMap::new();
        let mut parse_dates = BTreeMap::new();
        let mut json_scalars = BTreeMap::new();
        let mut assigned = BTreeSet::new();
        let mut groups = BTreeSet::new();
        let mut present_groups = BTreeSet::new();
        if matches!(driver, BranchDriver::Source { .. }) {
            present_groups.insert(path.to_vec());
        }
        for index in &indices {
            let (binding, feed, input) = &bindings[*index];
            let binding_path = binding.path();
            if !assigned.insert(binding_path.clone()) || !unique_producer(*feed, builder) {
                return None;
            }
            let present = if target.format == ComponentFormat::Json {
                let SchemaKind::Scalar { ty } = schema_node_at(&target.schema, &binding_path)?.kind
                else {
                    return None;
                };
                if target.ports.get(input) != Some(&binding_path)
                    || builder.edge_from.get(input) != Some(feed)
                    || !target.json_scalar_port(*input, ty)
                {
                    return None;
                }
                let proof = json_implicit::scalar_proof(
                    *feed,
                    anchor,
                    schema_node_at(&target.schema, &binding_path)?,
                    builder,
                )?;
                json_scalars.insert(*index, proof);
                true
            } else if literal_constant_value(*feed, builder).is_some()
                && builder.fn_by_output.get(feed).is_some_and(|index| {
                    builder.fn_components[*index].output_pins.as_slice() == [Some(*feed)]
                        && builder.fn_components[*index].bounded_scalar_pins_valid
                })
            {
                true
            } else if matches!(driver, BranchDriver::Implicit)
                && let Some(proof) = strict_scalar_conditional(
                    *feed,
                    anchor,
                    schema_node_at(&target.schema, &binding_path)?,
                    builder,
                )
            {
                conditionals.insert(*index, proof);
                true
            } else if matches!(driver, BranchDriver::Implicit)
                && let Some(proof) = total_string_value_map(
                    *feed,
                    anchor,
                    schema_node_at(&target.schema, &binding_path)?,
                    builder,
                )
            {
                value_maps.insert(*index, proof);
                true
            } else if matches!(driver, BranchDriver::Implicit)
                && let Some(proof) = parse_date::prove(
                    *feed,
                    anchor,
                    schema_node_at(&target.schema, &binding_path)?,
                    builder,
                )
            {
                parse_dates.insert(*index, proof);
                true
            } else {
                let source = physical_scalar(*feed, anchor, builder)?;
                guaranteed_scalar(source, anchor, &builder.sources.first()?.schema)
            };
            for length in path.len()..binding_path.len() {
                let group = binding_path[..length].to_vec();
                groups.insert(group.clone());
                if present {
                    present_groups.insert(group);
                }
            }
        }
        if !groups.is_subset(&present_groups)
            || target.format == ComponentFormat::Json
                && !json_implicit::required_assignments(
                    target_node,
                    path,
                    &assigned,
                    &present_groups,
                )
        {
            return None;
        }
        output.push(BranchPlan {
            marker: *marker,
            lineage,
            anchors,
            driver,
            bindings: indices,
            conditionals,
            value_maps,
            parse_dates,
            json_scalars,
            csv_scalars: BTreeMap::new(),
        });
    }
    owned.is_empty().then_some(output)
}

fn classify_driver(
    path: &[String],
    marker: u32,
    target: &SchemaComponent,
    builder: &GraphBuilder<'_>,
    enclosing: &[String],
) -> Option<BranchDriver> {
    let mut feeds = Vec::new();
    for (input, input_path) in &target.ports {
        if !input_path.starts_with(path) || !builder.edge_from.contains_key(input) {
            continue;
        }
        if !target
            .input_ancestors
            .get(input)
            .is_some_and(|ancestors| ancestors.contains(&marker))
        {
            continue;
        }
        if !target.unambiguous_declared_input(*input)
            || target.ambiguous_branch_inputs.contains(input)
            || controlled_input(target, *input, builder)
        {
            return None;
        }
        if schema_node_at(&target.schema, input_path)?.is_scalar() {
            continue;
        }
        if input_path != path {
            return None;
        }
        feeds.push(builder.edge_from[input]);
    }
    match feeds.as_slice() {
        [] => Some(BranchDriver::Implicit),
        [feed] if unique_producer(*feed, builder) && !builder.has_source_node_functions(*feed) => {
            let primary = builder.sources.first()?;
            let anchor = primary.ports.get(feed)?;
            if !primary.unambiguous_declared_output(*feed)
                || primary.format != ComponentFormat::Json
                || !anchor.starts_with(enclosing)
                || nullable_object_item_frame(&primary.schema, anchor)
                || !schema_node_at(&primary.schema, anchor).is_some_and(|node| {
                    node.repeating && matches!(node.kind, SchemaKind::Group { .. })
                })
                || (enclosing.len() + 1..anchor.len()).any(|length| {
                    schema_node_at(&primary.schema, &anchor[..length])
                        .is_none_or(|node| node.repeating)
                })
            {
                return None;
            }
            Some(BranchDriver::Source {
                feed: *feed,
                anchor: anchor.clone(),
            })
        }
        _ => None,
    }
}

pub(super) fn unique_producer(feed: u32, builder: &GraphBuilder<'_>) -> bool {
    let count = builder
        .sources
        .iter()
        .filter(|owner| owner.declares_output(feed) || owner.ports.contains_key(&feed))
        .count()
        + builder
            .intermediates
            .iter()
            .filter(|owner| owner.declares_output(feed))
            .count()
        + builder
            .fn_components
            .iter()
            .filter(|owner| owner.declared_output_keys.contains(&feed))
            .count()
        + builder
            .udf_calls
            .iter()
            .filter(|owner| owner.outputs.contains_key(&feed))
            .count();
    count == 1
        && builder
            .sources
            .iter()
            .filter(|owner| owner.declares_output(feed) || owner.ports.contains_key(&feed))
            .all(|owner| owner.unambiguous_declared_output(feed))
        && builder
            .intermediates
            .iter()
            .filter(|owner| owner.declares_output(feed))
            .all(|owner| owner.unambiguous_declared_output(feed))
        && builder
            .fn_components
            .iter()
            .filter(|owner| owner.declared_output_keys.contains(&feed))
            .all(|owner| owner.raw_single_output_valid)
        && !builder.has_source_node_functions(feed)
        && !builder
            .json_parsers
            .iter()
            .any(|recipe| recipe.outputs.contains_key(&feed))
        && !builder
            .flextext_parsers
            .iter()
            .any(|recipe| recipe.outputs.contains_key(&feed))
        && !builder
            .json_serializers
            .iter()
            .any(|recipe| recipe.output == feed)
        && !builder
            .xml_serializers
            .iter()
            .any(|recipe| recipe.output == feed)
        && !builder
            .external_scalar_recipes
            .iter()
            .any(|recipe| recipe.output == feed)
        && !builder
            .external_xslt_aggregates
            .iter()
            .any(|recipe| recipe.output == feed)
        && builder.joins.hierarchical_source(feed).is_none()
}

pub(super) fn physical_scalar<'a>(
    feed: u32,
    anchor: &[String],
    builder: &'a GraphBuilder<'_>,
) -> Option<&'a [String]> {
    let primary = builder.sources.first()?;
    let path = primary.ports.get(&feed)?;
    if primary.format != ComponentFormat::Json
        || !primary.unambiguous_declared_output(feed)
        || !path.starts_with(anchor)
        || nullable_object_item_frame(&primary.schema, path)
        || path.first().is_some_and(|first| {
            builder
                .source_names
                .iter()
                .skip(1)
                .any(|name| name == first)
        })
        || builder.has_source_node_functions(feed)
        || !schema_node_at(&primary.schema, path).is_some_and(|node| {
            node.is_scalar() && !node.repeating && !node.nullable && !node.container_nullable
        })
        || (anchor.len() + 1..=path.len()).any(|length| {
            schema_node_at(&primary.schema, &path[..length]).is_none_or(|node| {
                node.repeating || node.dynamic_fields().is_some() || !node.alternatives().is_empty()
            })
        })
    {
        return None;
    }
    Some(path)
}

fn guaranteed_scalar(path: &[String], anchor: &[String], source: &SchemaNode) -> bool {
    (anchor.len() + 1..=path.len()).all(|length| {
        let parent = schema_node_at(source, &path[..length - 1]);
        let node = schema_node_at(source, &path[..length]);
        parent.is_some_and(|parent| parent.required_fields().contains(&path[length - 1]))
            && node.is_some_and(|node| !node.nullable && !node.container_nullable)
    })
}

pub(super) fn actual_json_frame(
    path: &[String],
    anchor: &[String],
    target: &SchemaComponent,
    builder: &GraphBuilder<'_>,
    scopes: &ScopeBuilder,
) -> bool {
    let Some(primary) = builder.sources.first() else {
        return false;
    };
    primary.format == ComponentFormat::Json
        && !primary.schema.repeating
        && !anchor.is_empty()
        && !nullable_object_item_frame(&primary.schema, anchor)
        && !anchor.first().is_some_and(|first| {
            builder
                .source_names
                .iter()
                .skip(1)
                .any(|name| name == first)
        })
        && schema_node_at(&primary.schema, anchor).is_some_and(|node| {
            matches!(node.kind, SchemaKind::Group { .. })
                && !node.container_nullable
                && node.alternatives().is_empty()
                && node.dynamic_fields().is_none()
        })
        && (0..path.len()).any(|length| {
            scopes
                .scope(&path[..length])
                .is_some_and(|scope| matches!(scope.iteration, ScopeIteration::Source(_)))
                && scopes
                    .anchors
                    .get(&path[..length])
                    .is_some_and(|actual| actual == anchor)
                && physical_frame_driver(&path[..length], anchor, target, builder)
        })
        && (1..anchor.len()).all(|length| {
            schema_node_at(&primary.schema, &anchor[..length]).is_some_and(|node| {
                !node.repeating
                    || (0..path.len()).any(|length_target| {
                        scopes.scope(&path[..length_target]).is_some_and(|scope| {
                            matches!(scope.iteration, ScopeIteration::Source(_))
                        }) && scopes
                            .anchors
                            .get(&path[..length_target])
                            .is_some_and(|actual| actual == &anchor[..length])
                            && physical_frame_driver(
                                &path[..length_target],
                                &anchor[..length],
                                target,
                                builder,
                            )
                    })
            })
        })
}

// Item domains are distinct from array-container nullability. Omitted
// item metadata retains the legacy inherited domain through the model helper.
fn nullable_object_item_frame(source: &SchemaNode, path: &[String]) -> bool {
    (0..=path.len()).any(|length| {
        schema_node_at(source, &path[..length]).is_some_and(|node| {
            node.repeating
                && matches!(node.kind, SchemaKind::Group { .. })
                && !node.json_array_wrapper
                && !node.json_any
                && node.json_single_container_nullable()
        })
    })
}

fn physical_frame_driver(
    target_path: &[String],
    anchor: &[String],
    target: &SchemaComponent,
    builder: &GraphBuilder<'_>,
) -> bool {
    let Some(primary) = builder.sources.first() else {
        return false;
    };
    target.ports.iter().any(|(input, path)| {
        if path != target_path
            || !target.unambiguous_declared_input(*input)
            || target.format == ComponentFormat::Json
                && !target.json_array_object_frame_port(*input)
            || target.ambiguous_branch_inputs.contains(input)
            || target.ambiguous_input_affects(*input, target_path)
        {
            return false;
        }
        let Some(feed) = builder.edge_from.get(input) else {
            return false;
        };
        let resolved = builder.resolve_iteration_feed(*feed);
        // This contract owns direct physical parent drivers. Transparent or
        // computed parent aliases need their own complete raw lineage proof.
        *feed == resolved.source_key
            && resolved.computed_source.is_none()
            && resolved.source_suffix.is_empty()
            && primary.unambiguous_declared_output(resolved.source_key)
            && (target.format != ComponentFormat::Json
                || primary.json_array_object_frame_port(resolved.source_key))
            && primary
                .ports
                .get(&resolved.source_key)
                .is_some_and(|path| path == anchor)
            && unique_producer(resolved.source_key, builder)
            && !builder.has_source_node_functions(resolved.source_key)
    })
}

pub(super) fn enclosing_anchor(
    path: &[String],
    anchors: &BTreeMap<Vec<String>, Vec<String>>,
) -> Vec<String> {
    (0..path.len())
        .rev()
        .find_map(|length| anchors.get(&path[..length]).cloned())
        .unwrap_or_default()
}

pub(super) fn controlled_input(
    target: &SchemaComponent,
    input: u32,
    builder: &GraphBuilder<'_>,
) -> bool {
    builder.xml_condition_ports.contains(&input)
        || target.input_ancestors.get(&input).is_some_and(|ancestors| {
            ancestors
                .iter()
                .any(|ancestor| builder.xml_condition_ports.contains(ancestor))
        })
}

pub(super) fn plain_parents(path: &[String], scopes: &ScopeBuilder) -> bool {
    (0..path.len()).all(|length| {
        scopes.scope(&path[..length]).is_none_or(|scope| {
            matches!(
                scope.iteration,
                ScopeIteration::None | ScopeIteration::Source(_)
            ) && scope.construction == ScopeConstruction::Constructed
                && scope.iteration_output == IterationOutput::Repeated
                && scope.output_path().is_none()
                && scope.filter.is_none()
                && scope.post_group_filter.is_none()
                && !scope.has_grouping()
                && !scope.has_sort()
                && !scope.sort_descending
                && scope.windows.is_empty()
                && scope.dynamic_bindings.is_empty()
                && scope.dynamic_children.is_empty()
                && !scope.merge_dynamic_fields
        })
    })
}

pub(super) fn plain_shape(node: &SchemaNode, depth: usize) -> bool {
    depth <= MAX_DEPTH
        && node.recursive_ref.is_none()
        && node.alternatives().is_empty()
        && node.dynamic_fields().is_none()
        && match &node.kind {
            SchemaKind::Group { children, .. } => {
                children.iter().all(|child| plain_shape(child, depth + 1))
            }
            _ => node.is_scalar() && !node.repeating,
        }
}
