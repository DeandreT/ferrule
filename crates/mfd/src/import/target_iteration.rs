use std::collections::BTreeSet;

use ir::{SchemaKind, Value, XML_TYPE_FIELD};
use mapping::{IterationOutput, Node};

use super::graph::GraphBuilder;
use super::group_projection::TargetIteration;
use super::iteration::split_at_innermost_repeating;
use super::schema::{SchemaComponent, collect_matching_scalar_paths, schema_node_at};
use super::scope::{IterationNodes, ScopeBuilder, TargetLeaf};

pub(super) fn build(
    iterations: Vec<TargetIteration>,
    target: &SchemaComponent,
    bindings: &mut Vec<(TargetLeaf, u32, u32)>,
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) -> Vec<Vec<String>> {
    let connected: BTreeSet<Vec<String>> = bindings
        .iter()
        .map(|(target, _, _)| target.path())
        .collect();
    let mut skipped = builder.rejected_join_paths.iter().cloned().collect();
    let mut claimed_bindings = BTreeSet::new();
    for iteration in iterations {
        if iteration.additional_feeds.is_empty() {
            build_one(iteration, target, &connected, builder, scopes, &mut skipped);
        } else {
            build_concatenated(
                iteration,
                target,
                bindings,
                builder,
                scopes,
                &mut skipped,
                &mut claimed_bindings,
            );
        }
    }
    let mut index = 0;
    bindings.retain(|_| {
        let keep = !claimed_bindings.contains(&index);
        index += 1;
        keep
    });
    skipped
}

fn build_concatenated(
    iteration: TargetIteration,
    target: &SchemaComponent,
    bindings: &[(TargetLeaf, u32, u32)],
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
    skipped: &mut Vec<Vec<String>>,
    claimed_bindings: &mut BTreeSet<usize>,
) {
    let target_path = iteration.target_path.clone();
    let feeds = std::iter::once((
        iteration.feed,
        iteration.target_port,
        iteration.projects_whole_group,
    ))
    .chain(
        iteration
            .additional_feeds
            .iter()
            .map(|(feed, target_port, copy_all)| (*feed, Some(*target_port), *copy_all)),
    )
    .collect::<Vec<_>>();
    let branch_ports = feeds
        .iter()
        .filter_map(|(_, target_port, _)| *target_port)
        .collect::<BTreeSet<_>>();
    let mut segments = Vec::with_capacity(feeds.len());
    let mut segment_binding_indices = BTreeSet::new();
    for (feed, target_port, projects_whole_group) in feeds {
        let branch_bindings = target_port.map_or_else(Vec::new, |target_port| {
            bindings
                .iter()
                .enumerate()
                .filter(|(_, (binding, _, input))| {
                    let path = binding.path();
                    if path.len() <= target_path.len() || !path.starts_with(&target_path) {
                        return false;
                    }
                    let owners = target
                        .input_ancestors
                        .get(input)
                        .into_iter()
                        .flatten()
                        .filter(|ancestor| branch_ports.contains(ancestor))
                        .copied()
                        .collect::<Vec<_>>();
                    owners.is_empty() || owners == [target_port]
                })
                .map(|(index, _)| index)
                .collect::<Vec<_>>()
        });
        let connected = branch_bindings
            .iter()
            .map(|index| bindings[*index].0.path())
            .collect::<BTreeSet<_>>();
        let mut segment_builder = ScopeBuilder {
            root: mapping::Scope::default(),
            anchors: scopes.anchors.clone(),
        };
        let mut segment_skipped = Vec::new();
        build_one(
            TargetIteration {
                target_path: target_path.clone(),
                feed,
                target_port,
                additional_feeds: Vec::new(),
                output: iteration.output,
                projects_whole_group,
                join: iteration.join,
            },
            target,
            &connected,
            builder,
            &mut segment_builder,
            &mut segment_skipped,
        );
        for index in &branch_bindings {
            let (binding, feed, _) = &bindings[*index];
            let path = binding.path();
            let active_anchor = segment_builder.enclosing_anchor(&path);
            if let Some(node) = builder.binding_node_at_anchor(*feed, &path, &active_anchor) {
                segment_builder.add_binding(binding.clone(), node);
            }
        }
        let Some(mut segment) = take_scope(&mut segment_builder.root, &target_path) else {
            if !segment_skipped.iter().any(|path| path == &target_path) {
                skipped.push(target_path.clone());
            }
            return;
        };
        if !segment_skipped.is_empty() {
            skipped.extend(segment_skipped);
            return;
        }
        segment.target_field.clear();
        segments.push(segment);
        segment_binding_indices.extend(branch_bindings);
    }
    let mut segments = segments.into_iter();
    let Some(first) = segments.next() else {
        return;
    };
    scopes.add_concatenated(&target_path, first, segments.collect(), iteration.output);
    claimed_bindings.extend(segment_binding_indices);
}

fn take_scope(root: &mut mapping::Scope, target_path: &[String]) -> Option<mapping::Scope> {
    let (target, parents) = target_path.split_last()?;
    let mut parent = root;
    for field in parents {
        parent = parent
            .children
            .iter_mut()
            .find(|scope| scope.target_field == *field)?;
    }
    let index = parent
        .children
        .iter()
        .position(|scope| scope.target_field == *target)?;
    Some(parent.children.remove(index))
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
    let scope_source = source_path.as_ref().map(|source_path| {
        let structural_source = builder
            .schema_node(source_path)
            .is_some_and(|node| matches!(node.kind, SchemaKind::Group { .. }));
        if iteration.output == IterationOutput::MappedSequence || structural_source {
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
        }
    });
    let iteration_anchor = scope_source.as_ref().map_or_else(
        || scopes.enclosing_anchor(&target_path),
        |source| builder.context_path(source),
    );
    let xml_type_nodes = source_path.as_ref().and_then(|source_path| {
        xml_type_nodes(target, &target_path, source_path, feed.source_key, builder)
    });
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
    let mut existing_filter = feed
        .filter_expr
        .and_then(|key| builder.scalar_node_at_anchor(key, &iteration_anchor));
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
    if let Some((type_filter, _)) = xml_type_nodes {
        existing_filter = Some(match existing_filter {
            Some(existing) => builder.alloc(Node::Call {
                function: "and".into(),
                args: vec![existing, type_filter],
            }),
            None => type_filter,
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
    let distinct = feed
        .distinct_key
        .and_then(|key| builder.scalar_node_at_anchor(key, &iteration_anchor));
    let group = feed
        .group_key
        .and_then(|key| builder.scalar_node_at_anchor(key, &iteration_anchor))
        .or(distinct);
    let resolved_block = feed
        .block_size
        .and_then(|key| builder.scalar_node_at_anchor(key, &iteration_anchor));
    let start_group = feed
        .group_starting_with
        .and_then(|key| builder.scalar_node_at_anchor(key, &iteration_anchor));
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
    let ordinary_sort = feed
        .sort_expr
        .and_then(|key| builder.scalar_node_at_anchor(key, &iteration_anchor));
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
            .and_then(|key| builder.scalar_node_at_anchor(key, &iteration_anchor))
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
        sort_filter_order: feed.sort_filter_order,
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
    } else if let Some(scope_source) = &scope_source {
        scopes.add_iteration(
            &target_path,
            &builder.context_path(scope_source),
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
    if let Some((_, type_value)) = xml_type_nodes {
        let mut marker_path = target_path;
        marker_path.push(XML_TYPE_FIELD.to_string());
        if let Some(target) = TargetLeaf::from_path(&marker_path) {
            scopes.add_binding(target, type_value);
        }
    }
}

fn xml_type_nodes(
    target: &SchemaComponent,
    target_path: &[String],
    source_path: &super::source::SourcePath,
    source_port: u32,
    builder: &mut GraphBuilder<'_>,
) -> Option<(mapping::NodeId, mapping::NodeId)> {
    let type_name = builder.xml_type_conditions.get(&source_port)?.clone();
    let source_group = builder.schema_node(source_path)?;
    let target_group = schema_node_at(&target.schema, target_path)?;
    if !source_group
        .alternatives()
        .iter()
        .any(|alternative| alternative.name == type_name)
        || !target_group
            .alternatives()
            .iter()
            .any(|alternative| alternative.name == type_name)
    {
        return None;
    }
    let mut marker_path = source_path.clone();
    marker_path.path.push(XML_TYPE_FIELD.to_string());
    let marker = builder.source_field_at(&marker_path)?;
    let expected = builder.alloc(Node::Const {
        value: Value::String(type_name),
    });
    let filter = builder.alloc(Node::Call {
        function: "equal".into(),
        args: vec![marker, expected],
    });
    Some((filter, expected))
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
