use super::*;

type Candidate<'a> = (
    GraphBuilder<'a>,
    ScopeBuilder,
    BTreeSet<usize>,
    DeclaredBranches,
);

/// Trial lowering owns every mutable graph/cache and occurrence membership.
/// Refusal discards the trial; the caller commits the four completed states.
#[allow(clippy::too_many_arguments)]
pub(super) fn prepare<'a>(
    path: &[String],
    plans: Vec<BranchPlan>,
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32, u32)],
    builder: &GraphBuilder<'a>,
    scopes: &ScopeBuilder,
    claimed: &BTreeSet<usize>,
    declared: &DeclaredBranches,
) -> Option<Candidate<'a>> {
    let mut graph = builder.clone();
    let mut segments = Vec::with_capacity(plans.len());
    let mut owners = Vec::with_capacity(plans.len());
    let mut indices = BTreeSet::new();
    for plan in plans {
        let mut anchors = plan.anchors;
        anchors.retain(|candidate, _| !candidate.starts_with(path));
        let mut local = ScopeBuilder::new(anchors);
        let connected = plan
            .bindings
            .iter()
            .map(|index| bindings[*index].0.path())
            .collect();
        match plan.driver {
            BranchDriver::Implicit => {
                local.ensure_scope(path);
            }
            BranchDriver::Source { feed, .. } => {
                let mut skipped = Vec::new();
                build_one(
                    TargetIteration {
                        target_path: path.to_vec(),
                        feed,
                        target_port: Some(plan.marker),
                        additional_feeds: Vec::new(),
                        output: IterationOutput::Repeated,
                        projects_whole_group: false,
                        join: None,
                    },
                    target,
                    &connected,
                    &mut graph,
                    &mut local,
                    &mut skipped,
                );
                if !skipped.is_empty() {
                    return None;
                }
            }
        }
        let mut origins = BindingOrigins::new();
        for index in &plan.bindings {
            let (binding, feed, input) = &bindings[*index];
            let binding_path = binding.path();
            let anchor = local.enclosing_anchor(&binding_path);
            let node = graph.binding_node_at_anchor(*feed, &binding_path, &anchor)?;
            if plan
                .conditionals
                .get(index)
                .is_some_and(|proof| !proof.matches_lowered(node, &graph))
                || plan
                    .value_maps
                    .get(index)
                    .is_some_and(|proof| !proof.matches_lowered(node, &graph))
                || plan
                    .parse_dates
                    .get(index)
                    .is_some_and(|proof| !proof.matches_lowered(node, &graph))
                || plan
                    .json_scalars
                    .get(index)
                    .is_some_and(|proof| !proof.matches_lowered(node, &graph))
                || plan
                    .csv_scalars
                    .get(index)
                    .is_some_and(|proof| !proof.matches_lowered(node, &graph))
            {
                return None;
            }
            origins
                .entry((binding_path, node))
                .or_default()
                .insert(*index);
            local.add_binding(
                binding.clone(),
                node,
                BindingOccurrence::authored(target, *input),
            );
        }
        let mut segment = take_scope(&mut local.root, path)?;
        if (target.format == ComponentFormat::Json || !plan.csv_scalars.is_empty())
            && !json_implicit::bindings_preserved(&segment, &origins, path, &plan.bindings)
        {
            return None;
        }
        segment.target_field.clear();
        owners.push(DeclaredBranch {
            lineage: plan.lineage,
            anchors: local.anchors,
            bindings: origins,
        });
        indices.extend(plan.bindings);
        segments.push(segment);
    }
    let mut segments = segments.into_iter();
    let first = segments.next()?;
    let mut scopes = scopes.clone();
    let mut claimed = claimed.clone();
    let mut declared = declared.clone();
    let _ = take_scope(&mut scopes.root, path);
    scopes
        .anchors
        .retain(|candidate, _| !candidate.starts_with(path));
    scopes.add_concatenated(path, first, segments.collect(), IterationOutput::Repeated);
    declared.record(path, owners);
    claimed.extend(indices);
    Some((graph, scopes, claimed, declared))
}
