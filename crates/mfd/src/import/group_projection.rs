use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode, XML_TEXT_FIELD};
use mapping::IterationOutput;

use super::function::{aggregate_op, produces_scalar};
use super::graph::GraphBuilder;
use super::schema::{
    ComponentFormat, SchemaComponent, collect_matching_scalar_paths, schema_node_at,
};
use super::scope::{ScopeBuilder, TargetLeaf};

pub(super) enum Projection {
    Group(Vec<String>, u32),
    Text(Vec<String>, u32),
}

pub(super) struct TargetIteration {
    pub(super) target_path: Vec<String>,
    pub(super) feed: u32,
    pub(super) output: IterationOutput,
    pub(super) projects_whole_group: bool,
}

pub(super) struct TargetConnection<'a> {
    pub(super) target_path: &'a [String],
    pub(super) target_node: &'a SchemaNode,
    pub(super) input_key: u32,
    pub(super) feed: u32,
    pub(super) copy_all_targets: &'a BTreeSet<u32>,
}

impl TargetIteration {
    fn repeated(target_path: &[String], feed: u32) -> Self {
        Self {
            target_path: target_path.to_vec(),
            feed,
            output: IterationOutput::Repeated,
            projects_whole_group: false,
        }
    }
}

pub(super) fn classify_target_connection(
    target: &SchemaComponent,
    connection: TargetConnection<'_>,
    builder: &mut GraphBuilder<'_>,
    iterations: &mut Vec<TargetIteration>,
    projections: &mut Vec<Projection>,
) {
    let TargetConnection {
        target_path,
        target_node,
        input_key,
        feed,
        copy_all_targets,
    } = connection;
    let structural_feeds =
        connected_structural_feeds(target, target_path, builder, copy_all_targets);
    let Some(connection_role) = structural_feeds.get(&feed) else {
        return;
    };
    let copy_all = connection_role.copy_all;
    let mapped_xml_target =
        target.format == ComponentFormat::Xml && !target_path.is_empty() && !target_node.repeating;
    if mapped_xml_target && connection_role.representative != input_key {
        return;
    }
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
            iterations.push(TargetIteration::repeated(target_path, feed));
        } else if copy_all && has_connected_descendant(target, target_path, builder) {
            builder.warnings.push(
                "copy-all document connection also has connected descendants; mapping skipped"
                    .to_string(),
            );
        } else if copy_all
            && exact_group_source
            && !has_connected_descendant(target, target_path, builder)
        {
            projections.push(Projection::Group(target_path.to_vec(), feed));
        }
        return;
    }
    if target_node.repeating {
        iterations.push(TargetIteration::repeated(target_path, feed));
    } else if is_xml_text_group(target, target_node)
        && !text_is_connected(target, target_path, builder)
        && is_scalar_feed(builder, feed)
    {
        projections.push(Projection::Text(target_path.to_vec(), feed));
    } else if copy_all && has_connected_descendant(target, target_path, builder) {
        builder.warnings.push(format!(
            "copy-all group connection into `{}` also has connected descendants; mapping skipped",
            target_path.join("/")
        ));
    } else if mapped_group_sequence(target, target_path, builder, &resolved, copy_all) {
        let mapped_feeds = structural_feeds
            .iter()
            .filter(|(structural_feed, role)| {
                let candidate = builder.resolve_iteration_feed(**structural_feed);
                mapped_group_sequence(target, target_path, builder, &candidate, role.copy_all)
            })
            .collect::<Vec<_>>();
        if mapped_feeds.len() != 1 {
            if mapped_feeds
                .first()
                .is_some_and(|(_, role)| role.representative == input_key)
            {
                builder.warnings.push(format!(
                    "target group `{}` has multiple connected structural sequence feeds; iteration skipped",
                    target_path.join("/")
                ));
            }
            return;
        }
        iterations.push(TargetIteration {
            target_path: target_path.to_vec(),
            feed,
            output: IterationOutput::MappedSequence,
            projects_whole_group: copy_all,
        });
    } else if !has_connected_descendant(target, target_path, builder) {
        if copy_all && exact_group_source {
            projections.push(Projection::Group(target_path.to_vec(), feed));
        } else {
            builder.warnings.push(format!(
                "connection into non-repeating group `{}` ignored",
                target_path.join("/")
            ));
        }
    }
}

struct StructuralFeedRole {
    representative: u32,
    copy_all: bool,
}

fn connected_structural_feeds(
    target: &SchemaComponent,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    copy_all_targets: &BTreeSet<u32>,
) -> BTreeMap<u32, StructuralFeedRole> {
    let mut feeds = BTreeMap::new();
    for (key, path) in &target.ports {
        if path != target_path {
            continue;
        }
        let Some(feed) = builder.edge_from.get(key) else {
            continue;
        };
        let role = feeds.entry(*feed).or_insert(StructuralFeedRole {
            representative: *key,
            copy_all: false,
        });
        role.copy_all |= copy_all_targets.contains(key);
    }
    feeds
}

fn mapped_group_sequence(
    target: &SchemaComponent,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    feed: &super::iteration::IterationFeed,
    copy_all: bool,
) -> bool {
    let has_descendants = has_connected_descendant(target, target_path, builder);
    if target.format != ComponentFormat::Xml
        || target_path.is_empty()
        || !copy_all && !has_descendants
        || feed.sequence_component.is_some()
        || feed.order_issue.is_some()
        || feed.has_key_grouping
        || feed.has_block_grouping
        || feed.distinct_key.is_some()
        || feed.projects_whole_group
        || !feed.projections.is_empty()
        || feed.has_filter && feed.filter_expr.is_none()
        || feed.has_sort && feed.sort_expr.is_none()
    {
        return false;
    }
    let Some(source_path) = builder.iteration_source_path(feed) else {
        return false;
    };
    if enclosing_iteration_owns_source(target, target_path, builder, &source_path) {
        return false;
    }
    let Some(source_group) = builder.schema_node(&source_path) else {
        return false;
    };
    let Some(target_group) = schema_node_at(&target.schema, target_path) else {
        return false;
    };
    let mut compatible = Vec::new();
    collect_matching_scalar_paths(source_group, target_group, &mut Vec::new(), &mut compatible);
    (!copy_all || !compatible.is_empty()) && is_group_sequence_path(builder, &source_path)
}

fn is_group_sequence_path(
    builder: &GraphBuilder<'_>,
    source_path: &super::source::SourcePath,
) -> bool {
    let Some(source) = builder.sources.get(source_path.source) else {
        return false;
    };
    let mut node = &source.schema;
    let mut repeats = node.repeating || source.format == ComponentFormat::Csv;
    for segment in &source_path.path {
        let Some(child) = node.child(segment) else {
            return false;
        };
        repeats |= child.repeating;
        node = child;
    }
    repeats && matches!(node.kind, SchemaKind::Group { .. })
}

fn enclosing_iteration_owns_source(
    target: &SchemaComponent,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    source_path: &super::source::SourcePath,
) -> bool {
    target.ports.iter().any(|(key, path)| {
        path.len() < target_path.len()
            && target_path.starts_with(path)
            && builder.edge_from.get(key).is_some_and(|feed| {
                let enclosing = builder.resolve_iteration_feed(*feed);
                builder
                    .iteration_source_path(&enclosing)
                    .is_some_and(|source| {
                        source.source == source_path.source
                            && source_path.path.starts_with(&source.path)
                            && schema_node_at(&target.schema, path)
                                .is_some_and(|node| node.repeating)
                    })
            })
    })
}

fn is_xml_text_group(target: &SchemaComponent, node: &SchemaNode) -> bool {
    target.format == ComponentFormat::Xml
        && node
            .child(XML_TEXT_FIELD)
            .is_some_and(|text| !text.repeating && matches!(text.kind, SchemaKind::Scalar { .. }))
}

fn text_is_connected(
    target: &SchemaComponent,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
) -> bool {
    let mut text_path = target_path.to_vec();
    text_path.push(XML_TEXT_FIELD.to_string());
    target
        .ports
        .iter()
        .any(|(key, path)| *path == text_path && builder.edge_from.contains_key(key))
}

fn is_scalar_feed(builder: &GraphBuilder<'_>, feed: u32) -> bool {
    fn visit(builder: &GraphBuilder<'_>, feed: u32, visiting: &mut BTreeSet<u32>) -> bool {
        if !visiting.insert(feed) {
            return false;
        }
        let scalar = builder.sources.iter().any(|source| {
            source.ports.get(&feed).is_some_and(|path| {
                !matches!(source.format, ComponentFormat::Csv | ComponentFormat::Db)
                    && scalar_schema_path(&source.schema, path)
            })
        }) || builder.fn_by_output.get(&feed).is_some_and(|index| {
            let component = &builder.fn_components[*index];
            if !produces_scalar(component) {
                return false;
            }
            if component.name == "constant"
                || component.name == "position"
                || component.kind == 5 && aggregate_op(&component.name).is_some()
            {
                return true;
            }
            component.inputs.iter().flatten().all(|input| {
                builder
                    .edge_from
                    .get(input)
                    .is_none_or(|upstream| visit(builder, *upstream, visiting))
            })
        });
        visiting.remove(&feed);
        scalar
    }
    visit(builder, feed, &mut BTreeSet::new())
}

fn scalar_schema_path(schema: &SchemaNode, path: &[String]) -> bool {
    if schema.repeating {
        return false;
    }
    let mut node = schema;
    for segment in path {
        let Some(child) = node.child(segment) else {
            return false;
        };
        if child.repeating {
            return false;
        }
        node = child;
    }
    matches!(node.kind, SchemaKind::Scalar { .. })
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
    projections: Vec<Projection>,
    target: &SchemaComponent,
    skipped_iterations: &[Vec<String>],
    builder: &mut GraphBuilder<'_>,
    scopes: &mut ScopeBuilder,
) {
    for projection in projections {
        let (target_path, feed) = match projection {
            Projection::Group(target_path, feed) => (target_path, feed),
            Projection::Text(target_path, feed) => {
                if skipped_iterations
                    .iter()
                    .any(|skipped| target_path.starts_with(skipped))
                {
                    continue;
                }
                let mut text_path = target_path.clone();
                text_path.push(XML_TEXT_FIELD.to_string());
                if let Some(target) = TargetLeaf::from_path(&text_path)
                    && let Some(node) = builder.binding_node(feed, &text_path)
                {
                    scopes.add_binding(target, node);
                }
                continue;
            }
        };
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
