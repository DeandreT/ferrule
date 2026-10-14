//! Optional ordinary String Data in flat JSON occurrences owned by one real
//! physical X12 frame. Failed proof leaves the legacy path and diagnostics exact.
use super::*;
use crate::import::schema::occurrence_provenance::RawOccurrenceOwner;

#[allow(clippy::too_many_arguments)]
pub(super) fn build(
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32, u32)],
    iterations: &[TargetIteration],
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    claimed: &mut BTreeSet<usize>,
    declared: &mut DeclaredBranches,
) {
    let Some(plans) = plans(target, bindings, iterations, builder, scopes, claimed, declared) else { return; };
    let (path, plans) = plans;
    let Some((graph, candidate_scopes, candidate_claimed, candidate_declared)) =
        lowering::prepare(&path, plans, target, bindings, builder, scopes, claimed, declared)
    else { return; };
    *builder = graph;
    *scopes = candidate_scopes;
    *claimed = candidate_claimed;
    *declared = candidate_declared;
}

fn plain_edi(node: &SchemaNode, depth: usize, remaining: &mut usize) -> bool {
    if depth > MAX_DEPTH || *remaining == 0 { return false; }
    *remaining -= 1;
    let mut expected = match &node.kind {
        SchemaKind::Scalar { ty: ir::ScalarType::String } if !node.repeating => {
            let mut scalar = SchemaNode::scalar(node.name.clone(), ir::ScalarType::String);
            scalar.fixed = node.fixed.clone();
            scalar
        }
        SchemaKind::Group { children, .. } if (depth == 0 || !segment_name(&node.name) || children.iter().all(SchemaNode::is_scalar))
            && children.iter().all(|child| plain_edi(child, depth + 1, remaining)) => {
            SchemaNode::group(node.name.clone(), children.clone())
        }
        _ => return false,
    };
    expected.repeating = node.repeating;
    *node == expected
}

fn segment_name(name: &str) -> bool {
    (2..=3).contains(&name.len()) && name.as_bytes()[0].is_ascii_uppercase()
        && name.bytes().all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
}

fn ordinary_source(source: &SchemaComponent) -> bool {
    let mut remaining = MAX_BINDINGS;
    source.format == ComponentFormat::Edi && source.is_source
        && !source.is_variable && !source.is_pass_through && source.compute_when_key.is_none()
        && !source.has_target_node_controls && source.dynamic_json.is_none()
        && source.db_queries.is_empty() && source.db_xml_columns.is_empty()
        && !source.schema.repeating
        && source.port_declarations.as_ref().is_some_and(|facts| facts.complete_x12_config())
        && source.options.edi_kind == Some(mapping::EdiBoundaryKind::X12)
        && source.options.x12_interchange_version.as_deref().is_none_or(|version| matches!(version, "00401" | "00501" | "00604"))
        && source.options == mapping::FormatOptions {
            lenient_segments: source.options.lenient_segments,
            edi_kind: Some(mapping::EdiBoundaryKind::X12),
            edi_value_constraints: source.options.edi_value_constraints.clone(),
            x12_separators: source.options.x12_separators,
            x12_interchange_version: source.options.x12_interchange_version.clone(),
            ..Default::default()
        }
        && plain_edi(&source.schema, 0, &mut remaining)
}

fn raw_pair<'a>(source: &'a SchemaComponent, target: &'a SchemaComponent, builder: &GraphBuilder<'_>)
    -> Option<(&'a RawOccurrenceOwner, &'a RawOccurrenceOwner)>
{
    let source_raw = source.port_declarations.as_ref()?.occurrence()?;
    let target_raw = target.port_declarations.as_ref()?.occurrence()?;
    if !source_raw.ordinary_x12 || !target_raw.ordinary_json
        || source_raw.component == target_raw.component
        || source_raw.selected_children != target_raw.selected_children
        || source_raw.inputs != target_raw.inputs || source_raw.outputs != target_raw.outputs
        || source_raw.feeds != target_raw.feeds || source_raw.components != target_raw.components
        || source_raw.feeds != *builder.edge_from
        || !source_raw.inputs.values().all(|owner| *owner == target_raw.component)
        || !source.output_keys.iter().all(|key| source_raw.outputs.get(key) == Some(&source_raw.component))
        || !target.input_keys.iter().all(|key| target_raw.inputs.get(key) == Some(&target_raw.component))
    { return None; }
    let mut owners = BTreeSet::from([source_raw.component, target_raw.component]);
    for function in builder.fn_components {
        let [Some(output)] = function.output_pins.as_slice() else { return None; };
        if !source_raw.literals.contains(output) || exact_literal(*output, builder).is_none() { return None; }
        owners.insert(*source_raw.outputs.get(output)?);
    }
    (owners == source_raw.components).then_some((source_raw, target_raw))
}

fn frame_driver(target_path: &[String], source_path: &[String], target: &SchemaComponent,
                source: &SchemaComponent, raw: &RawOccurrenceOwner, target_raw: &RawOccurrenceOwner,
                builder: &GraphBuilder<'_>, scopes: &ScopeBuilder) -> bool {
    let enclosing = scopes.enclosing_anchor(target_path);
    let stored_path = if source_path.starts_with(&enclosing) {
        &source_path[enclosing.len()..]
    } else {
        source_path
    };
    schema_node_at(&target.schema, target_path).is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
        && scopes.scope(target_path).is_some_and(|scope| matches!(&scope.iteration, ScopeIteration::Source(path) if path.as_slice() == stored_path))
        && scopes.anchors.get(target_path).is_some_and(|path| path == source_path)
        && target.ports.iter().filter(|(_, path)| *path == target_path).any(|(input, _)| {
            target.unambiguous_declared_input(*input) && target.json_array_object_frame_port(*input)
                && target_raw.inputs.get(input) == Some(&target_raw.component)
                && raw.feeds.get(input).is_some_and(|feed| {
                    source.ports.get(feed).is_some_and(|path| path == source_path)
                        && source.unambiguous_declared_output(*feed) && raw.group_outputs.contains(feed)
                        && raw.outputs.get(feed) == Some(&raw.component) && unique_producer(*feed, builder)
                        && builder.resolve_iteration_feed(*feed).source_key == *feed
                })
        })
}

fn actual_frame(path: &[String], anchor: &[String], source: &SchemaComponent,
                target: &SchemaComponent, raw: &RawOccurrenceOwner, target_raw: &RawOccurrenceOwner,
                builder: &GraphBuilder<'_>, scopes: &ScopeBuilder) -> bool {
    !anchor.is_empty()
        && schema_node_at(&source.schema, anchor).is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
        && (1..=anchor.len()).all(|length| {
            schema_node_at(&source.schema, &anchor[..length]).is_some_and(|node| {
                !node.repeating || (1..path.len()).any(|parent| {
                    frame_driver(&path[..parent], &anchor[..length], target, source, raw, target_raw, builder, scopes)
                })
            })
        })
}

#[allow(clippy::too_many_arguments)]
fn plans(target: &SchemaComponent, bindings: &[(TargetLeaf, u32, u32)], iterations: &[TargetIteration],
         builder: &GraphBuilder<'_>, scopes: &ScopeBuilder, claimed: &BTreeSet<usize>,
         declared: &DeclaredBranches) -> Option<(Vec<String>, Vec<BranchPlan>)> {
    let [source] = builder.sources else { return None; };
    let mut remaining = MAX_BINDINGS;
    if !ordinary_source(source) || !json_implicit::ordinary(target, false)
        || !builder.intermediates.is_empty() || !builder.udf_calls.is_empty()
        || !builder.json_parsers.is_empty() || !builder.flextext_parsers.is_empty()
        || !builder.json_serializers.is_empty() || !builder.xml_serializers.is_empty()
        || !builder.external_scalar_recipes.is_empty() || !builder.external_xslt_aggregates.is_empty()
        || !builder.source_node_functions.is_empty()
        || !json_implicit::plain_schema(&target.schema, 0, &mut remaining)
    { return None; }
    let (raw, target_raw) = raw_pair(source, target, builder)?;
    let paths = target.input_branch_paths.iter().map(|(_, path)| path.clone()).collect::<BTreeSet<_>>();
    // One independent cloned child layout is the entire first cohort. An outer
    // duplicate or another candidate needs its own complete occurrence contract.
    if paths.len() != 1 { return None; }
    let path = paths.iter().next()?.clone();
    if bindings.iter().any(|(binding, _, _)| !binding.path().starts_with(&path)) { return None; }
    let item = schema_node_at(&target.schema, &path)?;
    let SchemaKind::Group { children, required, .. } = &item.kind else { return None; };
    if !item.repeating || children.iter().any(|child| !child.is_scalar() || child.repeating)
        || path.len() > MAX_DEPTH || !plain_parents(&path, scopes)
        || scopes.scope(&path).is_some_and(|scope| !json_implicit::replaceable(scope))
        || iterations.iter().any(|iteration| iteration.target_path.starts_with(&path))
    { return None; }
    let markers = target.input_branch_paths.iter().map(|(marker, _)| *marker).collect::<Vec<_>>();
    if !(2..=MAX_BRANCHES).contains(&markers.len()) { return None; }
    let mut owned = BTreeMap::<u32, Vec<usize>>::new();
    for (index, (binding, feed, input)) in bindings.iter().enumerate() {
        if !binding.path().starts_with(&path) { continue; }
        if claimed.contains(&index) || !target.unambiguous_declared_input(*input)
            || target.ambiguous_branch_inputs.contains(input) || controlled_input(target, *input, builder)
            || target_raw.feeds.get(input) != Some(feed)
        { return None; }
        let ancestors = target.input_ancestors.get(input)?;
        let owners = ancestors.iter().filter(|marker| markers.contains(marker)).collect::<Vec<_>>();
        let [owner] = owners.as_slice() else { return None; };
        owned.entry(**owner).or_default().push(index);
    }
    if owned.values().map(Vec::len).sum::<usize>() > MAX_BINDINGS { return None; }
    let mut plans = Vec::new();
    for marker in markers {
        let indices = owned.remove(&marker).unwrap_or_default();
        if indices.is_empty() {
            // No connected descendant or driver means no executable occurrence.
            if target.input_ancestors.iter().any(|(input, ancestors)| ancestors.contains(&marker) && builder.edge_from.contains_key(input)) { return None; }
            continue;
        }
        let lineage = branch_owners::lineage(target, marker)?;
        if lineage.len() != 1 { return None; }
        let anchors = declared.enclosing_anchors(&path, &lineage, &scopes.anchors)?;
        let anchor = enclosing_anchor(&path, &anchors);
        if !actual_frame(&path, &anchor, source, target, raw, target_raw, builder, scopes)
            || target.input_ancestors.iter().any(|(input, ancestors)| {
                ancestors.contains(&marker) && builder.edge_from.contains_key(input)
                    && target.ports.get(input).is_none_or(|field| field.len() != path.len() + 1)
            })
        { return None; }
        let mut assigned = BTreeSet::new();
        let mut present_literals = BTreeSet::new();
        let mut string_presence = BTreeSet::new();
        let mut scalars = BTreeMap::new();
        for index in &indices {
            let (binding, feed, input) = &bindings[*index];
            let field_path = binding.path();
            let field = schema_node_at(&target.schema, &field_path)?;
            let SchemaKind::Scalar { ty } = field.kind else { return None; };
            if !assigned.insert(field.name.clone()) || !target.json_scalar_port(*input, ty) || !unique_producer(*feed, builder) { return None; }
            let proof = if let Some(value) = exact_literal(*feed, builder) {
                let function = builder.fn_components.get(*builder.fn_by_output.get(feed)?)?;
                if !raw.literals.contains(feed) || !function.raw_literal_valid || !constant_has_type(&value, ty) { return None; }
                present_literals.insert(field.name.clone());
                if ty == ir::ScalarType::String { string_presence.insert(field.name.clone()); }
                json_implicit::ScalarProof::Literal(value)
            } else {
                let source_path = source.ports.get(feed)?;
                let source_field = schema_node_at(&source.schema, source_path)?;
                if ty != ir::ScalarType::String || required.contains(&field.name)
                    || !raw.scalar_outputs.contains(feed) || raw.outputs.get(feed) != Some(&raw.component)
                    || !source.unambiguous_declared_output(*feed) || !source_path.starts_with(&anchor)
                    || source_path.len() != anchor.len() + 2
                    || !schema_node_at(&source.schema, &source_path[..source_path.len() - 1]).is_some_and(|segment| {
                        segment_name(&segment.name)
                            && matches!(&segment.kind, SchemaKind::Group { children, .. } if children.iter().all(SchemaNode::is_scalar))
                    })
                    || *source_field != SchemaNode::scalar(source_field.name.clone(), ir::ScalarType::String)
                    || (anchor.len() + 1..source_path.len()).any(|length| schema_node_at(&source.schema, &source_path[..length]).is_none_or(|node| node.repeating))
                    || source.options.edi_value_constraints.iter().any(|constraint| constraint.path() == source_path.as_slice())
                { return None; }
                json_implicit::ScalarProof::Field { frame: anchor.clone(), path: source_path.strip_prefix(anchor.as_slice())?.to_vec() }
            };
            scalars.insert(*index, proof);
        }
        if !string_presence.iter().any(|field| required.contains(field))
            || !required.iter().all(|field| present_literals.contains(field)) { return None; }
        plans.push(BranchPlan {
            marker, lineage, anchors, driver: BranchDriver::Implicit, bindings: indices,
            conditionals: BTreeMap::new(), value_maps: BTreeMap::new(), parse_dates: BTreeMap::new(), json_scalars: scalars,
            csv_scalars: BTreeMap::new(),
        });
    }
    (owned.is_empty() && plans.len() >= 2).then_some((path, plans))
}

#[cfg(test)]
mod tests;
