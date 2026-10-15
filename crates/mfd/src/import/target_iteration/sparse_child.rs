use std::collections::{BTreeMap, BTreeSet};

use ir::SchemaKind;
use mapping::{IterationOutput, Node, NodeId, Scope};

use super::super::function::produces_scalar;
use super::super::graph::GraphBuilder;
use super::super::group_projection::TargetIteration;
use super::super::schema::{ComponentFormat, SchemaComponent, schema_node_at};
use super::super::scope::{ScopeBuilder, TargetLeaf};
use super::branch_owners::{
    BindingOrigins, DeclaredBranch, DeclaredBranches, replaceable_placeholder,
};

#[cfg(test)]
mod tests;

const MAX_SCALAR_DEPTH: usize = 32;
const MAX_SCALAR_VISITS: usize = 256;

/// A single physical child can belong to only one declared concatenation
/// segment. Candidate graph/caches and ownership are deep copies until commit.
pub(super) fn build(
    iteration: &TargetIteration,
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32, u32)],
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    declared: &mut DeclaredBranches,
) -> bool {
    let Some(candidate) = prepare(iteration, target, bindings, builder, scopes, declared) else {
        return false;
    };
    *builder = candidate.0;
    *scopes = candidate.1;
    *declared = candidate.2;
    true
}

type Candidate<'a> = (GraphBuilder<'a>, ScopeBuilder, DeclaredBranches);

fn prepare<'a>(
    iteration: &TargetIteration,
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32, u32)],
    builder: &GraphBuilder<'a>,
    scopes: &ScopeBuilder,
    declared: &DeclaredBranches,
) -> Option<Candidate<'a>> {
    let path = &iteration.target_path;
    let input = iteration.target_port?;
    if !matches!(target.format, ComponentFormat::Xml | ComponentFormat::Edi)
        || target.has_target_node_controls
        || iteration.output != IterationOutput::Repeated
        || iteration.projects_whole_group
        || iteration.join.is_some()
        || !iteration.additional_feeds.is_empty()
        || !target.unambiguous_declared_input(input)
        || target.ambiguous_branch_inputs.contains(&input)
        || target
            .input_branch_paths
            .iter()
            .any(|(_, branch)| branch.starts_with(path))
        || target.ports.get(&input) != Some(path)
        || !schema_node_at(&target.schema, path)
            .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
    {
        return None;
    }
    let lineage = target
        .input_ancestors
        .get(&input)?
        .iter()
        .filter(|port| !target.input_keys.contains(port))
        .copied()
        .collect::<Vec<_>>();
    let (parent_path, parents) = declared
        .layouts
        .iter()
        .filter(|(parent, _)| parent.len() < path.len() && path.starts_with(parent))
        .max_by_key(|(parent, _)| parent.len())?;
    let mut owners = parents
        .iter()
        .enumerate()
        .filter(|(_, owner)| lineage.starts_with(&owner.lineage));
    let (owner_index, owner) = owners.next()?;
    if owners.next().is_some() || declared.layouts.keys().any(|child| child.starts_with(path)) {
        return None;
    }
    let parent = scopes.scope(parent_path)?;
    let segments = parent.concatenated()?;
    if segments.len() != parents.len() || scopes.scope(path).is_some() {
        return None;
    }
    let segment = segments.iter().nth(owner_index)?;
    let relative = path.strip_prefix(parent_path.as_slice())?;
    let placeholder = at(segment, relative)?;
    if !replaceable_placeholder(placeholder) {
        return None;
    }
    let anchor = owner.anchors.get(parent_path)?;
    let source = builder.source_abs_path(iteration.feed)?;
    if source.source != 0
        || builder.sources.first()?.format != ComponentFormat::Json
        || !source.path.starts_with(anchor)
        || source.path.len() <= anchor.len()
        || !plain_source_path(&source.path, builder)
        || !builder
            .schema_node(&source)
            .is_some_and(|node| node.repeating && matches!(node.kind, SchemaKind::Group { .. }))
        || !builder
            .sources
            .first()?
            .unambiguous_declared_output(iteration.feed)
        || !raw_scalar_owner(
            iteration.feed,
            anchor,
            &source.path,
            builder,
            true,
            &mut BTreeSet::new(),
        )
    {
        return None;
    }
    // The ordinary child is structurally owned, not selected by equal feeds.
    let indices = bindings
        .iter()
        .enumerate()
        .filter_map(|(index, (leaf, _, port))| {
            let field = leaf.path();
            (field.len() > path.len()
                && field.starts_with(path)
                && target.input_ancestors.get(port).is_some_and(|ancestors| {
                    let physical = ancestors
                        .iter()
                        .filter(|port| !target.input_keys.contains(port))
                        .copied()
                        .collect::<Vec<_>>();
                    physical == lineage
                }))
            .then_some(index)
        })
        .collect::<BTreeSet<_>>();
    if indices.is_empty() {
        return None;
    }
    let mut old_membership = BTreeMap::new();
    collect_membership(
        placeholder,
        &mut path.clone(),
        &owner.bindings,
        &mut old_membership,
    )?;
    let expected_membership = indices
        .iter()
        .map(|index| (bindings[*index].0.path(), *index))
        .collect::<BTreeSet<_>>();
    if old_membership.keys().cloned().collect::<BTreeSet<_>>() != expected_membership
        || old_membership.values().any(|count| *count != 1)
    {
        return None;
    }
    for index in &indices {
        let (leaf, feed, port) = &bindings[*index];
        if target.ambiguous_branch_inputs.contains(port)
            || !target.unambiguous_declared_input(*port)
            || target.ports.get(port) != Some(&leaf.path())
            || !raw_scalar_owner(
                *feed,
                anchor,
                &source.path,
                builder,
                false,
                &mut BTreeSet::new(),
            )
        {
            return None;
        }
    }
    let mut candidate_graph = (*builder).clone();
    let mut local = ScopeBuilder {
        root: Scope::default(),
        anchors: owner.anchors.clone(),
    };
    let connected = indices
        .iter()
        .map(|index| bindings[*index].0.path())
        .collect();
    let mut skipped = Vec::new();
    super::build_one(
        iteration.clone(),
        target,
        &connected,
        &mut candidate_graph,
        &mut local,
        &mut skipped,
    );
    if !skipped.is_empty() || candidate_graph.warnings != builder.warnings {
        return None;
    }
    let child_anchor = local.anchors.get(path)?.clone();
    if child_anchor != source.path {
        return None;
    }
    let mut origins = BindingOrigins::new();
    for index in &indices {
        let (leaf, feed, _) = &bindings[*index];
        let field = leaf.path();
        let active = local.enclosing_anchor(&field);
        let node = candidate_graph.binding_node_at_anchor(*feed, &field, &active)?;
        if !scalar_frames(node, &child_anchor, &candidate_graph, &mut BTreeSet::new()) {
            return None;
        }
        origins.entry((field, node)).or_default().insert(*index);
        local.add_binding(leaf.clone(), node);
    }
    let child = local.scope(path)?.clone();
    let mut actual_membership = BTreeMap::new();
    collect_membership(&child, &mut path.clone(), &origins, &mut actual_membership)?;
    if actual_membership != old_membership || candidate_graph.warnings != builder.warnings {
        return None;
    }
    let mut candidate_scopes = (*scopes).clone();
    let parent = candidate_scopes.ensure_scope(parent_path);
    let segment = parent.concatenated_mut()?.iter_mut().nth(owner_index)?;
    *at_mut(segment, relative)? = child;
    let mut candidate_declared = (*declared).clone();
    let parent_owner = &mut candidate_declared.layouts.get_mut(parent_path)?[owner_index];
    parent_owner
        .bindings
        .retain(|(field, _), _| !field.starts_with(path));
    parent_owner.bindings.extend(origins.clone());
    parent_owner
        .anchors
        .retain(|field, _| !field.starts_with(path));
    parent_owner.anchors.extend(
        local
            .anchors
            .iter()
            .filter(|(field, _)| field.starts_with(path))
            .map(|(field, source)| (field.clone(), source.clone())),
    );
    candidate_declared.record(
        path,
        vec![DeclaredBranch {
            lineage,
            anchors: local.anchors,
            bindings: origins,
        }],
    );
    Some((candidate_graph, candidate_scopes, candidate_declared))
}

fn at<'a>(mut scope: &'a Scope, path: &[String]) -> Option<&'a Scope> {
    for field in path {
        let mut matches = scope
            .children
            .iter()
            .filter(|child| &child.target_field == field);
        scope = matches.next()?;
        if matches.next().is_some() {
            return None;
        }
    }
    Some(scope)
}

fn plain_source_path(path: &[String], builder: &GraphBuilder<'_>) -> bool {
    (1..=path.len()).all(|length| {
        schema_node_at(&builder.sources[0].schema, &path[..length]).is_some_and(|node| {
            !node.container_nullable
                && !node.nullable
                && !node.json_single_container_nullable()
                && !node.json_any
                && !node.json_array_wrapper
                && matches!(node.kind, SchemaKind::Group { .. })
        })
    })
}

fn at_mut<'a>(scope: &'a mut Scope, path: &[String]) -> Option<&'a mut Scope> {
    let Some((field, rest)) = path.split_first() else {
        return Some(scope);
    };
    let index = scope
        .children
        .iter()
        .position(|child| &child.target_field == field)?;
    at_mut(&mut scope.children[index], rest)
}

fn collect_membership(
    scope: &Scope,
    path: &mut Vec<String>,
    origins: &BindingOrigins,
    result: &mut BTreeMap<(Vec<String>, usize), usize>,
) -> Option<()> {
    for binding in &scope.bindings {
        let mut field = path.clone();
        field.push(binding.target_field.clone());
        let original = origins.get(&(field.clone(), binding.node))?;
        if original.len() != 1 {
            return None;
        }
        *result.entry((field, *original.first()?)).or_default() += 1;
    }
    for child in &scope.children {
        path.push(child.target_field.clone());
        collect_membership(child, path, origins, result)?;
        path.pop();
    }
    if scope.concatenated().is_some() {
        return None;
    }
    Some(())
}

fn raw_scalar_owner(
    feed: u32,
    anchor: &[String],
    active: &[String],
    builder: &GraphBuilder<'_>,
    collection: bool,
    visiting: &mut BTreeSet<u32>,
) -> bool {
    let mut remaining = MAX_SCALAR_VISITS;
    raw_scalar_owner_bounded(
        feed,
        anchor,
        active,
        builder,
        collection,
        visiting,
        &mut remaining,
    )
}

fn raw_scalar_owner_bounded(
    feed: u32,
    anchor: &[String],
    active: &[String],
    builder: &GraphBuilder<'_>,
    collection: bool,
    visiting: &mut BTreeSet<u32>,
    remaining: &mut usize,
) -> bool {
    if visiting.len() >= MAX_SCALAR_DEPTH || *remaining == 0 || !visiting.insert(feed) {
        return false;
    }
    *remaining -= 1;
    let sources = builder
        .sources
        .iter()
        .filter(|source| source.declares_output(feed))
        .count();
    let functions = builder
        .fn_components
        .iter()
        .filter(|component| component.declared_output_keys.contains(&feed))
        .count();
    if sources + functions != 1 || builder.udf_by_output.contains_key(&feed) {
        return false;
    }
    if let Some(source) = builder.source_abs_path(feed) {
        return source.source == 0
            && source.path.starts_with(anchor)
            && source_path_in_frame(&source.path, active, builder)
            && builder.sources[0].unambiguous_declared_output(feed)
            && builder.schema_node(&source).is_some_and(|node| {
                if collection {
                    node.repeating && matches!(node.kind, SchemaKind::Group { .. })
                } else {
                    !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
                }
            });
    }
    if collection {
        return false;
    }
    let Some(index) = builder.fn_by_output.get(&feed) else {
        return false;
    };
    let component = &builder.fn_components[*index];
    component.library == "core"
        && produces_scalar(component)
        && component.raw_single_output_valid
        && component.bounded_scalar_pins_valid
        && component.output_pins.as_slice() == [Some(feed)]
        && component.inputs.iter().all(|input| {
            input
                .and_then(|input| builder.edge_from.get(&input).copied())
                .is_some_and(|feed| {
                    raw_scalar_owner_bounded(
                        feed,
                        anchor,
                        active,
                        builder,
                        false,
                        &mut visiting.clone(),
                        remaining,
                    )
                })
        })
}

fn source_path_in_frame(path: &[String], active: &[String], builder: &GraphBuilder<'_>) -> bool {
    (1..=path.len()).all(|length| {
        schema_node_at(&builder.sources[0].schema, &path[..length]).is_some_and(|node| {
            (!node.repeating || active.starts_with(&path[..length]))
                && !node.json_array_wrapper
                && !node.json_any
                && (!matches!(node.kind, SchemaKind::Group { .. })
                    || !node.nullable
                        && !node.container_nullable
                        && !node.json_single_container_nullable())
        })
    })
}

fn scalar_frames(
    node: NodeId,
    anchor: &[String],
    builder: &GraphBuilder<'_>,
    visiting: &mut BTreeSet<NodeId>,
) -> bool {
    let mut remaining = MAX_SCALAR_VISITS;
    scalar_frames_bounded(node, anchor, builder, visiting, &mut remaining)
}

fn scalar_frames_bounded(
    node: NodeId,
    anchor: &[String],
    builder: &GraphBuilder<'_>,
    visiting: &mut BTreeSet<NodeId>,
    remaining: &mut usize,
) -> bool {
    if visiting.len() >= MAX_SCALAR_DEPTH || *remaining == 0 || !visiting.insert(node) {
        return false;
    }
    *remaining -= 1;
    match builder.graph.nodes.get(&node) {
        Some(Node::Const { .. }) => true,
        Some(Node::SourceField {
            path,
            frame: Some(frame),
        }) => {
            let absolute = [frame.as_slice(), path.as_slice()].concat();
            anchor.starts_with(frame)
                && source_path_in_frame(&absolute, anchor, builder)
                && schema_node_at(&builder.sources[0].schema, &absolute).is_some_and(|node| {
                    !node.repeating && matches!(node.kind, SchemaKind::Scalar { .. })
                })
        }
        Some(Node::If {
            condition,
            then,
            else_,
        }) => [*condition, *then, *else_].iter().all(|node| {
            scalar_frames_bounded(*node, anchor, builder, &mut visiting.clone(), remaining)
        }),
        Some(Node::Call { args, .. }) => args.iter().all(|node| {
            scalar_frames_bounded(*node, anchor, builder, &mut visiting.clone(), remaining)
        }),
        _ => false,
    }
}
