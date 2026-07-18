use std::collections::{BTreeMap, BTreeSet};

use ir::{SchemaKind, SchemaNode, XML_TEXT_FIELD};
use mapping::{IterationOutput, JoinId, ScopeConstruction};

use super::function::{aggregate_op, produces_scalar};
use super::graph::GraphBuilder;
use super::schema::{
    ComponentFormat, SchemaComponent, collect_matching_scalar_paths, schema_node_at,
};
use super::scope::{ScopeBuilder, TargetLeaf};

pub(super) enum Projection {
    CopyCurrentSource,
    Group(Vec<String>, u32),
    Text(Vec<String>, u32),
}

#[derive(Clone)]
pub(super) struct TargetIteration {
    pub(super) target_path: Vec<String>,
    pub(super) feed: u32,
    pub(super) target_port: Option<u32>,
    pub(super) additional_feeds: Vec<(u32, u32, bool)>,
    pub(super) output: IterationOutput,
    pub(super) projects_whole_group: bool,
    pub(super) join: Option<JoinId>,
}

pub(super) struct TargetConnection<'a> {
    pub(super) target_path: &'a [String],
    pub(super) target_node: &'a SchemaNode,
    pub(super) input_key: u32,
    pub(super) feed: u32,
    pub(super) copy_all_targets: &'a BTreeSet<u32>,
}

impl TargetIteration {
    fn repeated(target_path: &[String], feed: u32, target_port: u32) -> Self {
        Self {
            target_path: target_path.to_vec(),
            feed,
            target_port: Some(target_port),
            additional_feeds: Vec::new(),
            output: IterationOutput::Repeated,
            projects_whole_group: false,
            join: None,
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
    match builder.classify_join_iteration(feed, target_path) {
        super::join::IterationFeed::Join(join) => {
            if target.format.is_xml_like() && target_path.is_empty() && !target_node.repeating {
                builder.rejected_join_paths.insert(target_path.to_vec());
                if builder.warned_join_controls.insert(join) {
                    builder.warnings.push(
                        "join cannot iterate a non-repeating XML document root; iteration skipped"
                            .to_string(),
                    );
                }
                return;
            }
            let output = if target_path.is_empty() || target_node.repeating {
                IterationOutput::Repeated
            } else {
                IterationOutput::MappedSequence
            };
            iterations.push(TargetIteration {
                target_path: target_path.to_vec(),
                feed,
                target_port: Some(input_key),
                additional_feeds: Vec::new(),
                output,
                projects_whole_group: false,
                join: Some(join),
            });
            return;
        }
        super::join::IterationFeed::Rejected => return,
        super::join::IterationFeed::Ordinary => {}
    }
    let structural_feeds =
        connected_structural_feeds(target, target_path, builder, copy_all_targets);
    let Some(connection_role) = structural_feeds.get(&feed) else {
        return;
    };
    let copy_all = connection_role.copy_all;
    let mapped_xml_target =
        target.format.is_xml_like() && !target_path.is_empty() && !target_node.repeating;
    if mapped_xml_target && connection_role.driver_port != input_key {
        return;
    }
    let resolved = builder.resolve_iteration_feed(feed);
    let plain_feed = resolved.sequence_component.is_none()
        && resolved.db_where_component.is_none()
        && !resolved.has_filter
        && !resolved.has_key_grouping
        && !resolved.has_start_grouping
        && !resolved.has_block_grouping
        && resolved.distinct_key.is_none()
        && resolved.order_issue.is_none()
        && !resolved.has_sort
        && resolved.take_expr.is_none()
        && !resolved.take_default_one
        && resolved.projections.is_empty();
    // Iteration resolution intentionally stops at the repeated owner. A
    // structural copy can target a non-repeating descendant of that owner,
    // so retain the direct endpoint path for group compatibility checks.
    let plain_structural_source_path = plain_feed
        .then(|| builder.sequence_source_path(feed))
        .flatten();
    let exact_group_source = plain_structural_source_path
        .as_ref()
        .and_then(|source| builder.schema_node(source))
        .is_some_and(|source| !source.repeating && matches!(source.kind, SchemaKind::Group { .. }));
    let exact_whole_source_copy =
        plain_structural_source_path
            .as_ref()
            .is_some_and(|source_path| {
                source_path.source == 0
                    && source_path.path.is_empty()
                    && builder.schema_node(source_path) == Some(target_node)
            });
    let max_one_database_source = builder
        .iteration_source_path(&resolved)
        .is_some_and(|source| builder.db_query_is_at_most_one(&source));
    if target_path.is_empty() {
        // Document-root connectors normally carry structural context only.
        // Treat one as a copy request only for an exact plain group feed.
        let row_shaped = matches!(
            target.format,
            ComponentFormat::Csv | ComponentFormat::Xlsx | ComponentFormat::Db
        ) || (target.format == ComponentFormat::Json && target_node.repeating);
        if row_shaped {
            iterations.push(TargetIteration::repeated(target_path, feed, input_key));
        } else if copy_all && has_connected_descendant(target, target_path, builder) {
            builder.warnings.push(
                "copy-all document connection also has connected descendants; mapping skipped"
                    .to_string(),
            );
        } else if target.format.is_xml_like()
            && (max_one_database_source || resolved.take_default_one)
            && matches!(target_node.kind, SchemaKind::Group { .. })
            && has_connected_descendant(target, target_path, builder)
        {
            iterations.push(TargetIteration {
                target_path: target_path.to_vec(),
                feed,
                target_port: Some(input_key),
                additional_feeds: Vec::new(),
                output: IterationOutput::First,
                projects_whole_group: false,
                join: None,
            });
        } else if copy_all
            && exact_whole_source_copy
            && !has_connected_descendant(target, target_path, builder)
        {
            projections.push(Projection::CopyCurrentSource);
        } else if copy_all
            && exact_group_source
            && !has_connected_descendant(target, target_path, builder)
        {
            projections.push(Projection::Group(target_path.to_vec(), feed));
        }
        return;
    }
    if target_node.repeating {
        let mut repeated_feeds = structural_feeds.iter().collect::<Vec<_>>();
        if target.format.supports_cloned_target_branches() && repeated_feeds.len() > 1 {
            repeated_feeds.sort_by_key(|(_, role)| role.representative);
            let branch_coverage =
                cloned_branch_coverage(target, target_path, builder, &repeated_feeds);
            if branch_coverage == ClonedBranchCoverage::Complete {
                if repeated_feeds
                    .first()
                    .is_some_and(|(_, role)| role.driver_port == input_key)
                {
                    let mut feeds = repeated_feeds
                        .iter()
                        .map(|(feed, role)| (**feed, role.representative, role.copy_all));
                    if let Some((feed, target_port, projects_whole_group)) = feeds.next() {
                        iterations.push(TargetIteration {
                            target_path: target_path.to_vec(),
                            feed,
                            target_port: Some(target_port),
                            additional_feeds: feeds.collect(),
                            output: IterationOutput::Repeated,
                            projects_whole_group,
                            join: None,
                        });
                    }
                }
                return;
            }
            if branch_coverage == ClonedBranchCoverage::Incomplete {
                if repeated_feeds
                    .first()
                    .is_some_and(|(_, role)| role.driver_port == input_key)
                {
                    builder.warnings.push(format!(
                        "target group `{}` has multiple connected structural sequence feeds; iteration skipped",
                        target_path.join("/")
                    ));
                }
                return;
            }
        }
        let mut iteration = TargetIteration::repeated(target_path, feed, input_key);
        iteration.projects_whole_group = copy_all;
        iterations.push(iteration);
        if connection_role.driver_port == input_key
            && is_xml_text_group(target, target_node)
            && !text_is_connected(target, target_path, builder)
            && builder.fn_by_output.contains_key(&feed)
            && is_scalar_feed(builder, feed)
        {
            projections.push(Projection::Text(target_path.to_vec(), feed));
        }
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
        if mapped_feeds.len() > 1 {
            let mut mapped_feeds = mapped_feeds;
            mapped_feeds.sort_by_key(|(_, role)| role.representative);
            let all_copy_all = mapped_feeds.iter().all(|(_, role)| role.copy_all);
            let branches_are_exact =
                exact_mapped_branches(target, target_path, builder, &mapped_feeds);
            let branches_are_compatible = branches_are_exact
                || (all_copy_all && !has_connected_descendant(target, target_path, builder));
            if mapped_feeds.first().is_some_and(|(_, role)| {
                role.driver_port == input_key
                    && branches_are_compatible
                    && mapped_feeds.iter().all(|(feed, role)| {
                        matching_xml_type_conditions(builder, **feed, role.driver_port)
                    })
            }) {
                let mut feeds = mapped_feeds
                    .iter()
                    .map(|(feed, role)| (**feed, role.representative, role.copy_all));
                if let Some((feed, target_port, projects_whole_group)) = feeds.next() {
                    iterations.push(TargetIteration {
                        target_path: target_path.to_vec(),
                        feed,
                        target_port: Some(target_port),
                        additional_feeds: feeds.collect(),
                        output: IterationOutput::MappedSequence,
                        projects_whole_group,
                        join: None,
                    });
                }
            } else if mapped_feeds
                .first()
                .is_some_and(|(_, role)| role.driver_port == input_key)
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
            target_port: Some(input_key),
            additional_feeds: Vec::new(),
            output: IterationOutput::MappedSequence,
            projects_whole_group: copy_all,
            join: None,
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

#[derive(Clone, Copy, PartialEq, Eq)]
enum ClonedBranchCoverage {
    None,
    Incomplete,
    Complete,
}

fn cloned_branch_coverage(
    target: &SchemaComponent,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    feeds: &[(&u32, &StructuralFeedRole)],
) -> ClonedBranchCoverage {
    let representatives = feeds
        .iter()
        .map(|(_, role)| role.representative)
        .collect::<BTreeSet<_>>();
    let mut covered = BTreeSet::new();
    for (input, path) in &target.ports {
        if path.len() <= target_path.len()
            || !path.starts_with(target_path)
            || !builder.edge_from.contains_key(input)
        {
            continue;
        }
        let owners = target
            .input_ancestors
            .get(input)
            .into_iter()
            .flatten()
            .filter(|ancestor| representatives.contains(ancestor))
            .copied()
            .collect::<Vec<_>>();
        match owners.as_slice() {
            [] => {}
            [owner] => {
                covered.insert(*owner);
            }
            _ => return ClonedBranchCoverage::None,
        }
    }
    if covered.is_empty() {
        ClonedBranchCoverage::None
    } else if covered == representatives {
        ClonedBranchCoverage::Complete
    } else {
        ClonedBranchCoverage::Incomplete
    }
}

fn exact_mapped_branches(
    target: &SchemaComponent,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    feeds: &[(&u32, &StructuralFeedRole)],
) -> bool {
    let representatives = feeds
        .iter()
        .map(|(_, role)| role.representative)
        .collect::<BTreeSet<_>>();
    let mut branches_with_content = BTreeSet::new();
    for (input, path) in &target.ports {
        if path.len() <= target_path.len()
            || !path.starts_with(target_path)
            || !builder.edge_from.contains_key(input)
        {
            continue;
        }
        if !schema_node_at(&target.schema, path)
            .is_some_and(|node| matches!(node.kind, SchemaKind::Scalar { .. }))
        {
            return false;
        }
        let owners = target
            .input_ancestors
            .get(input)
            .into_iter()
            .flatten()
            .filter(|ancestor| representatives.contains(ancestor))
            .copied()
            .collect::<Vec<_>>();
        match owners.as_slice() {
            [] => {}
            [owner] => {
                branches_with_content.insert(*owner);
            }
            _ => return false,
        }
    }
    feeds
        .iter()
        .all(|(_, role)| role.copy_all || branches_with_content.contains(&role.representative))
}

fn matching_xml_type_conditions(builder: &GraphBuilder<'_>, feed: u32, target_port: u32) -> bool {
    let source_port = builder.resolve_iteration_feed(feed).source_key;
    builder.xml_type_conditions.get(&source_port) == builder.xml_type_conditions.get(&target_port)
}

struct StructuralFeedRole {
    representative: u32,
    driver_port: u32,
    copy_all: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum StructuralBranch {
    Marker(u32),
    SharedXbrl,
    Port(u32),
}

fn connected_structural_feeds(
    target: &SchemaComponent,
    target_path: &[String],
    builder: &GraphBuilder<'_>,
    copy_all_targets: &BTreeSet<u32>,
) -> BTreeMap<u32, StructuralFeedRole> {
    let mut branches = BTreeMap::<StructuralBranch, Vec<(u32, u32, bool)>>::new();
    for (key, path) in &target.ports {
        if path != target_path {
            continue;
        }
        let Some(feed) = builder.edge_from.get(key) else {
            continue;
        };
        let branch = target
            .input_ancestors
            .get(key)
            .into_iter()
            .flatten()
            .rev()
            .find(|ancestor| !target.input_keys.contains(ancestor))
            .copied();
        let branch = match branch {
            Some(marker) => StructuralBranch::Marker(marker),
            None if target.format == ComponentFormat::Xbrl => StructuralBranch::SharedXbrl,
            None => StructuralBranch::Port(*key),
        };
        branches
            .entry(branch)
            .or_default()
            .push((*key, *feed, copy_all_targets.contains(key)));
    }

    let mut feeds = BTreeMap::new();
    for (branch, candidates) in branches {
        let representative = match branch {
            StructuralBranch::Marker(marker) | StructuralBranch::Port(marker) => marker,
            StructuralBranch::SharedXbrl => {
                let Some(port) = candidates.iter().map(|(port, _, _)| *port).min() else {
                    continue;
                };
                port
            }
        };
        let copy_all = candidates.iter().any(|(_, _, copy_all)| *copy_all);
        let selected = candidates.iter().max_by_key(|(port, feed, _)| {
            let resolved = builder.resolve_iteration_feed(*feed);
            let controlled = resolved.sequence_component.is_some()
                || resolved.db_where_component.is_some()
                || resolved.has_filter
                || resolved.has_key_grouping
                || resolved.has_start_grouping
                || resolved.has_block_grouping
                || resolved.distinct_key.is_some()
                || resolved.has_sort
                || resolved.take_expr.is_some()
                || resolved.take_default_one;
            let depth = builder
                .iteration_source_path(&resolved)
                .map_or(0, |source| source.path.len());
            (controlled, depth, std::cmp::Reverse(*port))
        });
        if let Some((driver_port, feed, _)) = selected {
            feeds.insert(
                *feed,
                StructuralFeedRole {
                    representative,
                    driver_port: *driver_port,
                    copy_all,
                },
            );
        }
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
    if !target.format.is_xml_like()
        || target_path.is_empty()
        || !copy_all && !has_descendants
        || feed.sequence_component.is_some()
        || feed.order_issue.is_some()
        || feed.has_key_grouping
        || feed.has_block_grouping
        || feed.distinct_key.is_some()
        || feed.projects_whole_group
        || !feed.projections.is_empty()
        || feed.has_filter && feed.filter_expr.is_none() && feed.udf_filters.is_empty()
        || feed.has_sort && feed.sort_expr.is_none()
    {
        return false;
    }
    let Some(source_path) = builder.iteration_source_path(feed) else {
        return false;
    };
    if enclosing_iteration_owns_source(target, target_path, builder, &source_path)
        && !builder.xml_type_conditions.contains_key(&feed.source_key)
    {
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
    let mut repeats = node.repeating
        || source.format == ComponentFormat::Csv
        || source.format == ComponentFormat::Xlsx && source.options.xlsx_composite.is_none();
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
    target.format.is_xml_like()
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
                let externally_repeated =
                    matches!(source.format, ComponentFormat::Csv | ComponentFormat::Db)
                        || source.format == ComponentFormat::Xlsx
                            && source.options.xlsx_composite.is_none();
                !externally_repeated
                    && (scalar_schema_path(&source.schema, path)
                        || is_generic_xml_text_path(source, path))
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

fn is_generic_xml_text_path(source: &SchemaComponent, path: &[String]) -> bool {
    source.format.is_xml_like()
        && path
            .last()
            .is_some_and(|name| name == ir::XML_ELEMENTS_FIELD)
        && schema_node_at(&source.schema, path).is_some_and(|node| {
            node.repeating
                && matches!(node.kind, SchemaKind::Group { .. })
                && node.child(XML_TEXT_FIELD).is_some_and(|text| {
                    !text.repeating && matches!(text.kind, SchemaKind::Scalar { .. })
                })
        })
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
            Projection::CopyCurrentSource => {
                scopes.root.construction = ScopeConstruction::CopyCurrentSource;
                continue;
            }
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
        let active_anchor = scopes.enclosing_anchor(&target_path);
        let plan = GroupProjectionPlan::between(source_group, target_group);
        let coverage = plan.coverage();
        for step in plan.into_ordered_steps() {
            match step {
                GroupProjectionStep::CopyRepeatedGroup(path) => {
                    let mut source_collection = source_path.clone();
                    source_collection.path.extend(path.iter().cloned());
                    builder.note_framed_prefixes(&source_collection);
                    let mut target_collection = target_path.clone();
                    target_collection.extend(path);
                    scopes.add_copy_iteration(
                        &target_collection,
                        &builder.context_path(&source_collection),
                    );
                }
                GroupProjectionStep::BindScalar(path) => {
                    let mut target_leaf = target_path.clone();
                    target_leaf.extend(path.iter().cloned());
                    let Some(target_leaf) = TargetLeaf::from_path(&target_leaf) else {
                        continue;
                    };
                    let mut source_leaf = source_path.clone();
                    source_leaf.path.extend(path);
                    if let Some(node) = builder.source_field_at_anchor(&source_leaf, &active_anchor)
                    {
                        scopes.add_binding(target_leaf, node);
                    }
                }
                GroupProjectionStep::UnsupportedRepetition => {}
            }
        }
        match coverage {
            ProjectionCoverage::Complete => {}
            ProjectionCoverage::NoCompatibleFields => {
                warn(
                    builder,
                    &target_path,
                    "it has no compatible same-name scalar leaves",
                );
            }
            ProjectionCoverage::UnsupportedRepetitionOnly => {
                warn(
                    builder,
                    &target_path,
                    "it contains only repeating compatible descendants, which require explicit iteration connections",
                );
            }
            ProjectionCoverage::PartialUnsupportedRepetition => {
                warn(
                    builder,
                    &target_path,
                    "matching repeating descendants were not copied; connect them to explicit iterations",
                );
            }
        }
    }
}

enum GroupProjectionStep {
    BindScalar(Vec<String>),
    CopyRepeatedGroup(Vec<String>),
    UnsupportedRepetition,
}

enum ProjectionCoverage {
    Complete,
    NoCompatibleFields,
    UnsupportedRepetitionOnly,
    PartialUnsupportedRepetition,
}

#[derive(Default)]
struct GroupProjectionPlan {
    steps: Vec<GroupProjectionStep>,
}

impl GroupProjectionPlan {
    fn between(source: &SchemaNode, target: &SchemaNode) -> Self {
        let mut plan = Self::default();
        collect_steps(source, target, &mut Vec::new(), &mut plan);
        plan
    }

    fn coverage(&self) -> ProjectionCoverage {
        let has_compatible = self.steps.iter().any(|step| {
            matches!(
                step,
                GroupProjectionStep::BindScalar(_) | GroupProjectionStep::CopyRepeatedGroup(_)
            )
        });
        let has_unsupported = self
            .steps
            .iter()
            .any(|step| matches!(step, GroupProjectionStep::UnsupportedRepetition));
        match (has_compatible, has_unsupported) {
            (true, false) => ProjectionCoverage::Complete,
            (false, false) => ProjectionCoverage::NoCompatibleFields,
            (false, true) => ProjectionCoverage::UnsupportedRepetitionOnly,
            (true, true) => ProjectionCoverage::PartialUnsupportedRepetition,
        }
    }

    fn into_ordered_steps(mut self) -> impl Iterator<Item = GroupProjectionStep> {
        self.steps.sort_by_key(GroupProjectionStep::order_key);
        self.steps.into_iter()
    }
}

impl GroupProjectionStep {
    fn order_key(&self) -> (u8, usize) {
        match self {
            // Parent repeated groups must exist before any scalar binding can
            // select its enclosing target scope.
            Self::CopyRepeatedGroup(path) => (0, path.len()),
            Self::BindScalar(_) => (1, 0),
            Self::UnsupportedRepetition => (2, 0),
        }
    }
}

fn collect_steps(
    source: &SchemaNode,
    target: &SchemaNode,
    path: &mut Vec<String>,
    plan: &mut GroupProjectionPlan,
) {
    match (&source.kind, &target.kind) {
        (SchemaKind::Scalar { .. }, SchemaKind::Scalar { .. })
            if !source.repeating && !target.repeating =>
        {
            // This follows the same adapter-guided coercion as an explicit
            // scalar connection; structural copies do not impose stricter types.
            plan.steps
                .push(GroupProjectionStep::BindScalar(path.clone()));
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
                path.push(target_child.name.clone());
                if source_child.repeating || target_child.repeating {
                    if source_child.repeating
                        && target_child.repeating
                        && matches!(source_child.kind, SchemaKind::Group { .. })
                        && source_child.kind == target_child.kind
                    {
                        plan.steps
                            .push(GroupProjectionStep::CopyRepeatedGroup(path.clone()));
                    } else {
                        plan.steps.push(GroupProjectionStep::UnsupportedRepetition);
                    }
                } else {
                    collect_steps(source_child, target_child, path, plan);
                }
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
