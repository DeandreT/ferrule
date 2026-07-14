use std::collections::BTreeSet;

use ir::{SchemaKind, Value};
use mapping::{IterationOutput, Node};

use super::graph::GraphBuilder;
use super::group_projection::TargetIteration;
use super::iteration::split_at_innermost_repeating;
use super::schema::{SchemaComponent, collect_matching_scalar_paths, schema_node_at};
use super::scope::{IterationNodes, ScopeBuilder, TargetLeaf};

pub(super) fn build(
    iterations: Vec<TargetIteration>,
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32)],
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) -> Vec<Vec<String>> {
    let connected: BTreeSet<Vec<String>> =
        bindings.iter().map(|(target, _)| target.path()).collect();
    let mut skipped = builder.rejected_join_paths.iter().cloned().collect();
    for iteration in iterations {
        build_one(iteration, target, &connected, builder, scopes, &mut skipped);
    }
    skipped
}

fn build_one(
    iteration: TargetIteration,
    target: &SchemaComponent,
    connected: &BTreeSet<Vec<String>>,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    skipped: &mut Vec<Vec<String>>,
) {
    let target_path = iteration.target_path;
    let join = iteration.join;
    let feed = builder.resolve_iteration_feed(iteration.feed);
    if let Some(id) = join {
        match builder.prepare_join_iteration(
            id,
            &target_path,
            iteration.output == IterationOutput::MappedSequence,
            scopes,
        ) {
            super::join::PreparedIteration::Owner => {}
            super::join::PreparedIteration::Projection => {
                let has_connected_descendant = connected
                    .iter()
                    .any(|path| path.len() > target_path.len() && path.starts_with(&target_path));
                if !has_connected_descendant {
                    builder.rejected_join_paths.insert(target_path.clone());
                    builder.warnings.push(format!(
                        "join projection into `{}` has no connected scalar descendants; structural copy is unsupported and was skipped",
                        target_path.join("/")
                    ));
                    skipped.push(target_path);
                } else if feed.has_filter
                    || feed.has_sort
                    || feed.take_expr.is_some()
                    || feed.take_default_one
                {
                    builder.rejected_join_paths.insert(target_path.clone());
                    if builder.warned_join_controls.insert(id) {
                        builder.warnings.push(format!(
                            "join projection into `{}` has independent filter, sort, or item-limit controls; projection skipped",
                            target_path.join("/")
                        ));
                    }
                    skipped.push(target_path);
                }
                return;
            }
            super::join::PreparedIteration::Rejected => {
                skipped.push(target_path);
                return;
            }
        }
    }
    if let Some(issue) = feed.order_issue {
        builder.warnings.push(format!(
            "sequence into `{}` {issue}; imported using ferrule's sequence order",
            target_path.join("/")
        ));
    }
    let source_path = builder.iteration_source_path(&feed);
    let sequence = feed
        .sequence_component
        .and_then(|index| builder.sequence_expr(index));
    if source_path.is_none() && sequence.is_none() && join.is_none() {
        builder.warnings.push(format!(
            "iteration into `{}` comes from an unsupported feed; skipped",
            target_path.join("/")
        ));
        return;
    }
    let mut existing_filter = feed.filter_expr.and_then(|key| builder.value_node(key));
    for output in &feed.udf_filters {
        let Some(udf_filter) = builder.udf_iteration_filter_node(*output) else {
            continue;
        };
        existing_filter = Some(match existing_filter {
            Some(existing) => builder.alloc(Node::Call {
                function: "and".into(),
                args: vec![existing, udf_filter],
            }),
            None => udf_filter,
        });
    }
    if let Some(id) = join
        && feed.has_filter
        && existing_filter.is_none()
    {
        reject_join_control(
            builder,
            skipped,
            id,
            target_path,
            "has a missing or unsupported filter predicate",
        );
        return;
    }
    let (mut filter, database_sort, database_descending, query_at_most_one) = match builder
        .apply_db_controls(
            feed.db_where_component,
            source_path.as_ref(),
            existing_filter,
        ) {
        Ok(nodes) => nodes,
        Err(error) => {
            builder.warnings.push(error.warning(&target_path));
            skipped.push(target_path);
            return;
        }
    };
    if query_at_most_one
        && (feed.db_where_component.is_some()
            || feed.has_filter
            || feed.has_key_grouping
            || feed.has_start_grouping
            || feed.has_block_grouping
            || feed.distinct_key.is_some()
            || feed.has_sort
            || feed.take_expr.is_some()
            || feed.take_default_one
            || feed.order_issue.is_some())
    {
        builder.warnings.push(format!(
            "database LIMIT 1 feeding `{}` is followed by sequence controls whose order cannot be represented exactly; iteration skipped",
            target_path.join("/")
        ));
        skipped.push(target_path);
        return;
    }
    let distinct = feed.distinct_key.and_then(|key| builder.value_node(key));
    let group = feed
        .group_key
        .and_then(|key| builder.value_node(key))
        .or(distinct);
    let resolved_block = feed.block_size.and_then(|key| builder.value_node(key));
    let start_group = feed
        .group_starting_with
        .and_then(|key| builder.value_node(key));
    if feed.has_start_grouping && start_group.is_none() {
        builder.warnings.push(format!(
            "group-starting-with feeding `{}` has a missing or unsupported predicate; iteration skipped",
            target_path.join("/")
        ));
        skipped.push(target_path);
        return;
    }
    if feed.has_block_grouping && resolved_block.is_none() {
        builder.warnings.push(format!(
            "group-into-blocks feeding `{}` has a missing or unsupported block-size; iteration skipped",
            target_path.join("/")
        ));
        skipped.push(target_path);
        return;
    }
    let block = resolved_block.filter(|_| group.is_none() && start_group.is_none());
    if let Some(distinct) = distinct {
        let exists = builder.alloc(Node::Call {
            function: "exists".into(),
            args: vec![distinct],
        });
        filter = Some(match filter {
            Some(filter) => builder.alloc(Node::Call {
                function: "and".into(),
                args: vec![filter, exists],
            }),
            None => exists,
        });
    }
    let ordinary_sort = feed.sort_expr.and_then(|key| builder.value_node(key));
    if let Some(id) = join
        && feed.has_sort
        && ordinary_sort.is_none()
    {
        reject_join_control(
            builder,
            skipped,
            id,
            target_path,
            "has a missing or unsupported sort key",
        );
        return;
    }
    if ordinary_sort.is_some() && database_sort.is_some() {
        builder.warn_conflicting_db_sort(&target_path);
        skipped.push(target_path);
        return;
    }
    let sort = ordinary_sort.or(database_sort);
    let take = if query_at_most_one {
        Some(builder.alloc(Node::Const {
            value: Value::Int(1),
        }))
    } else {
        feed.take_expr
            .and_then(|key| builder.value_node(key))
            .or_else(|| {
                feed.take_default_one.then(|| {
                    builder.alloc(Node::Const {
                        value: Value::Int(1),
                    })
                })
            })
    };
    if let Some(id) = join
        && feed.take_expr.is_some()
        && take.is_none()
    {
        reject_join_control(
            builder,
            skipped,
            id,
            target_path,
            "has an unsupported item-limit count",
        );
        return;
    }
    let nodes = IterationNodes {
        filter,
        group_by: group,
        group_starting_with: start_group,
        group_into_blocks: block,
        sort_by: sort,
        sort_descending: ordinary_sort
            .map(|_| feed.sort_descending)
            .unwrap_or(database_descending),
        take,
    };
    if let Some(id) = join {
        let Some(plan) = builder.join_plan(id) else {
            skipped.push(target_path);
            return;
        };
        scopes.add_join(&target_path, id, plan, nodes, iteration.output);
    } else if let Some(sequence) = sequence {
        scopes.add_sequence(&target_path, sequence, nodes, iteration.output);
    } else if let Some(source_path) = &source_path {
        let scope_source = if iteration.output == IterationOutput::MappedSequence {
            builder
                .sources
                .get(source_path.source)
                .map(|source| super::source::SourcePath {
                    source: source_path.source,
                    path: split_at_innermost_repeating(&source.schema, &source_path.path).0,
                })
                .unwrap_or_else(|| source_path.clone())
        } else {
            source_path.clone()
        };
        scopes.add_iteration(
            &target_path,
            &builder.context_path(&scope_source),
            nodes,
            iteration.output,
        );
    }
    if feed.projects_whole_group || iteration.projects_whole_group {
        project_whole_group(
            target,
            &target_path,
            source_path.as_ref(),
            &feed.projections,
            connected,
            builder,
            scopes,
        );
    }
    project_connected_fields(
        target,
        &target_path,
        &feed.projections,
        connected,
        builder,
        scopes,
    );
}

fn reject_join_control(
    builder: &mut GraphBuilder<'_>,
    skipped: &mut Vec<Vec<String>>,
    join: mapping::JoinId,
    target_path: Vec<String>,
    reason: &str,
) {
    builder.rejected_join_paths.insert(target_path.clone());
    if builder.warned_join_controls.insert(join) {
        builder.warnings.push(format!(
            "join feeding `{}` {reason}; iteration skipped",
            target_path.join("/")
        ));
    }
    skipped.push(target_path);
}

fn project_whole_group(
    target: &SchemaComponent,
    target_path: &[String],
    source_path: Option<&super::source::SourcePath>,
    projections: &std::collections::BTreeMap<Vec<String>, u32>,
    connected: &BTreeSet<Vec<String>>,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    let Some(source_path) = source_path else {
        return;
    };
    let (Some(source_group), Some(target_group)) = (
        builder.schema_node(source_path),
        schema_node_at(&target.schema, target_path),
    ) else {
        return;
    };
    let mut relative_paths = Vec::new();
    collect_matching_scalar_paths(
        source_group,
        target_group,
        &mut Vec::new(),
        &mut relative_paths,
    );
    for relative in relative_paths {
        let mut target_leaf = target_path.to_vec();
        target_leaf.extend(relative.iter().cloned());
        if connected.contains(&target_leaf) || projections.contains_key(&relative) {
            continue;
        }
        let mut source_leaf = source_path.clone();
        source_leaf.path.extend(relative);
        if let (Some(target), Some(node)) = (
            TargetLeaf::from_path(&target_leaf),
            builder.source_field_at(&source_leaf),
        ) {
            scopes.add_binding(target, node);
        }
    }
}

fn project_connected_fields(
    target: &SchemaComponent,
    target_path: &[String],
    projections: &std::collections::BTreeMap<Vec<String>, u32>,
    connected: &BTreeSet<Vec<String>>,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    let mut paths = Vec::new();
    if let Some(target_group) = schema_node_at(&target.schema, target_path) {
        collect_matching_scalar_paths(target_group, target_group, &mut Vec::new(), &mut paths);
    }
    for relative in paths {
        let Some(feed) = projections.get(&relative) else {
            continue;
        };
        let mut target_leaf = target_path.to_vec();
        target_leaf.extend(relative);
        if connected.contains(&target_leaf)
            || !schema_node_at(&target.schema, &target_leaf)
                .is_some_and(|node| matches!(node.kind, SchemaKind::Scalar { .. }))
        {
            continue;
        }
        if let Some(node) = builder.value_node(*feed)
            && let Some(target) = TargetLeaf::from_path(&target_leaf)
        {
            scopes.add_binding(target, node);
        }
    }
}
