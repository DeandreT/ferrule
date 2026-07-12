use ir::{SchemaKind, SchemaNode};

use super::graph::GraphBuilder;
use super::schema::{ComponentFormat, SchemaComponent, schema_node_at};
use super::scope::{ScopeBuilder, TargetLeaf};

pub(super) fn classify_target_connection(
    target: &SchemaComponent,
    target_path: &[String],
    target_node: &SchemaNode,
    feed: u32,
    builder: &mut GraphBuilder<'_>,
    iterations: &mut Vec<(Vec<String>, u32)>,
    projections: &mut Vec<(Vec<String>, u32)>,
) {
    let resolved = builder.resolve_iteration_feed(feed);
    let plain_feed = resolved.sequence_component.is_none()
        && resolved.db_where_component.is_none()
        && !resolved.has_filter
        && !resolved.has_key_grouping
        && !resolved.has_block_grouping
        && resolved.distinct_key.is_none()
        && resolved.order_issue.is_none()
        && !resolved.has_sort
        && resolved.take_expr.is_none()
        && !resolved.take_default_one
        && resolved.projections.is_empty();
    let exact_group_source = plain_feed
        .then(|| builder.iteration_source_path(&resolved))
        .flatten()
        .and_then(|source| builder.schema_node(&source))
        .is_some_and(|source| !source.repeating && matches!(source.kind, SchemaKind::Group { .. }));
    if target_path.is_empty() {
        // Document-root connectors normally carry structural context only.
        // Treat one as a copy request only for an exact plain group feed.
        let row_shaped = matches!(target.format, ComponentFormat::Csv | ComponentFormat::Db)
            || (target.format == ComponentFormat::Json && target_node.repeating);
        if row_shaped {
            iterations.push((target_path.to_vec(), feed));
        } else if exact_group_source && !has_connected_descendant(target, target_path, builder) {
            projections.push((target_path.to_vec(), feed));
        }
        return;
    }
    if target_node.repeating {
        iterations.push((target_path.to_vec(), feed));
    } else if !has_connected_descendant(target, target_path, builder) {
        if exact_group_source {
            projections.push((target_path.to_vec(), feed));
        } else {
            builder.warnings.push(format!(
                "connection into non-repeating group `{}` ignored",
                target_path.join("/")
            ));
        }
    }
}

fn has_connected_descendant(
    target: &SchemaComponent,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
) -> bool {
    target.ports.iter().any(|(key, path)| {
        path.len() > target_path.len()
            && path.starts_with(target_path)
            && builder.edge_from.contains_key(key)
    })
}

pub(super) fn build(
    projections: Vec<(Vec<String>, u32)>,
    target: &SchemaComponent,
    skipped_iterations: &[Vec<String>],
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    for (target_path, feed) in projections {
        if skipped_iterations
            .iter()
            .any(|skipped| target_path.starts_with(skipped))
        {
            continue;
        }
        let Some(source_path) = builder.sequence_source_path(feed) else {
            warn(builder, &target_path, "its source group cannot be resolved");
            continue;
        };
        let Some(source_group) = builder.schema_node(&source_path) else {
            warn(
                builder,
                &target_path,
                "its source schema path does not exist",
            );
            continue;
        };
        let Some(target_group) = schema_node_at(&target.schema, &target_path) else {
            warn(
                builder,
                &target_path,
                "its target schema path does not exist",
            );
            continue;
        };
        if source_group.repeating
            || target_group.repeating
            || !matches!(source_group.kind, SchemaKind::Group { .. })
            || !matches!(target_group.kind, SchemaKind::Group { .. })
        {
            warn(
                builder,
                &target_path,
                "both endpoints must be non-repeating groups",
            );
            continue;
        }
        let mut relative = Vec::new();
        let mut skipped_repeating = false;
        let active_anchor = scopes.enclosing_anchor(&target_path);
        collect_paths(
            source_group,
            target_group,
            &mut Vec::new(),
            &mut relative,
            &mut skipped_repeating,
        );
        let compatible = relative.len();
        for path in relative {
            let mut target_leaf = target_path.clone();
            target_leaf.extend(path.iter().cloned());
            let Some(target_leaf) = TargetLeaf::from_path(&target_leaf) else {
                continue;
            };
            let mut source_leaf = source_path.clone();
            source_leaf.path.extend(path);
            if let Some(node) = builder.source_field_at_anchor(&source_leaf, &active_anchor) {
                scopes.add_binding(target_leaf, node);
            }
        }
        if compatible == 0 {
            warn(
                builder,
                &target_path,
                if skipped_repeating {
                    "it contains only repeating compatible descendants, which require explicit iteration connections"
                } else {
                    "it has no compatible same-name scalar leaves"
                },
            );
        } else if skipped_repeating {
            warn(
                builder,
                &target_path,
                "matching repeating descendants were not copied; connect them to explicit iterations",
            );
        }
    }
}

fn collect_paths(
    source: &SchemaNode,
    target: &SchemaNode,
    path: &mut Vec<String>,
    paths: &mut Vec<Vec<String>>,
    skipped_repeating: &mut bool,
) {
    match (&source.kind, &target.kind) {
        (SchemaKind::Scalar { .. }, SchemaKind::Scalar { .. })
            if !source.repeating && !target.repeating =>
        {
            // This follows the same adapter-guided coercion as an explicit
            // scalar connection; structural copies do not impose stricter types.
            paths.push(path.clone());
        }
        (
            SchemaKind::Group {
                children: source_children,
                dynamic: source_dynamic,
                ..
            },
            SchemaKind::Group {
                children: target_children,
                dynamic: target_dynamic,
                ..
            },
        ) if source_dynamic.is_none() && target_dynamic.is_none() => {
            for target_child in target_children {
                let Some(source_child) = source_children
                    .iter()
                    .find(|source_child| source_child.name == target_child.name)
                else {
                    continue;
                };
                if source_child.repeating || target_child.repeating {
                    *skipped_repeating = true;
                    continue;
                }
                path.push(target_child.name.clone());
                collect_paths(source_child, target_child, path, paths, skipped_repeating);
                path.pop();
            }
        }
        _ => {}
    }
}

fn warn(builder: &mut GraphBuilder<'_>, target_path: &[String], reason: &str) {
    builder.warnings.push(format!(
        "non-repeating group connection into `{}` is unsupported: {reason}",
        target_path.join("/")
    ));
}
