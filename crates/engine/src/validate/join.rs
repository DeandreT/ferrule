use std::collections::BTreeSet;

use ir::SchemaNode;
use mapping::{Graph, JoinId, JoinPlan, JoinSourceCardinality, Node, NodeId, Project, Scope};

use super::ValidationIssue;
use super::graph::{node_inputs, validate_collection_value};
use super::schema::{
    display_path, root_source_path_direct, source_path_matches, source_path_matches_direct,
};

pub(super) fn validate_plan(
    project: &Project,
    join: JoinId,
    plan: &JoinPlan,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    for source in plan.sources() {
        let collection = source.collection();
        let singleton = |node: &SchemaNode| !node.repeating && node.is_scalar();
        let valid = match source.cardinality() {
            JoinSourceCardinality::Repeating => {
                collection.is_empty()
                    || source_path_matches(project, collection, |node| node.repeating)
            }
            JoinSourceCardinality::Singleton => source_path_matches(project, collection, singleton),
        };
        if valid
            && source.cardinality() == JoinSourceCardinality::Singleton
            && !source_path_matches_direct(project, collection, singleton)
        {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "join {} singleton collection `{}` crosses a repeating source level; join the repeating collection with a key path instead",
                    join.get(),
                    display_path(collection)
                ),
            ));
        }
        if !valid {
            let expected = match source.cardinality() {
                JoinSourceCardinality::Repeating => "missing or not repeating",
                JoinSourceCardinality::Singleton => "missing or not a singleton scalar",
            };
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "join {} collection `{}` is {expected}",
                    join.get(),
                    display_path(collection)
                ),
            ));
        }
    }
    for (right, conditions) in plan.stages() {
        for key in conditions.iter() {
            validate_collection_value(
                project,
                location,
                key.left_collection(),
                key.left_path(),
                "join left key",
                issues,
            );
            validate_collection_value(
                project,
                location,
                right.collection(),
                key.right_path(),
                "join right key",
                issues,
            );
        }
    }
}

/// Whether `source` is the primary root schema, where expressions see no
/// active item frame. Identity iterations reuse that schema and resolve
/// paths identically.
pub(super) fn is_root_context(project: &Project, source: Option<&SchemaNode>) -> bool {
    source.is_some_and(|source| std::ptr::eq(source, &project.source))
}

/// A root-context join has no active item frame, so each singleton must
/// resolve from the primary root or a named source rather than from a
/// descendant that exists only beneath some other collection.
pub(super) fn validate_root_singletons(
    project: &Project,
    join: JoinId,
    plan: &JoinPlan,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let singleton = |node: &SchemaNode| !node.repeating && node.is_scalar();
    for source in plan.sources() {
        let collection = source.collection();
        // Missing or repetition-crossing paths already have their own issue.
        if source.cardinality() == JoinSourceCardinality::Singleton
            && source_path_matches_direct(project, collection, singleton)
            && !root_source_path_direct(project, collection).is_some_and(singleton)
        {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "join {} singleton collection `{}` does not resolve from the root source context; use a path from the source root or a named source",
                    join.get(),
                    display_path(collection)
                ),
            ));
        }
    }
}

pub(super) fn validate_scope_nodes(
    graph: &Graph,
    scope: &Scope,
    active_joins: &[(JoinId, Vec<Vec<String>>)],
    root_context: bool,
    location: &str,
    project: &Project,
    issues: &mut Vec<ValidationIssue>,
) {
    let roots = scope
        .filter
        .into_iter()
        .chain(scope.post_group_filter)
        .chain(scope.grouping_nodes())
        .chain(scope.sort_by)
        .chain(scope.output_path())
        .chain(scope.sort_then_by.iter().map(|key| key.node))
        .chain(scope.bindings.iter().map(|binding| binding.node))
        .chain(
            scope
                .dynamic_bindings
                .iter()
                .flat_map(|binding| [binding.key, binding.value]),
        )
        .chain(scope.dynamic_children.iter().map(|child| child.key));
    validate_roots(
        graph,
        roots,
        active_joins,
        root_context,
        location,
        project,
        issues,
    );
}

/// `root_context` says whether `roots` evaluate without an active item frame;
/// per-item expression inputs never do.
pub(super) fn validate_roots(
    graph: &Graph,
    roots: impl IntoIterator<Item = NodeId>,
    active_joins: &[(JoinId, Vec<Vec<String>>)],
    root_context: bool,
    location: &str,
    project: &Project,
    issues: &mut Vec<ValidationIssue>,
) {
    validate_roots_inner(
        graph,
        roots.into_iter().map(|root| (root, root_context)),
        active_joins,
        location,
        project,
        issues,
        &mut BTreeSet::new(),
    );
}

/// Each pending root carries whether it evaluates in the root context.
fn validate_roots_inner(
    graph: &Graph,
    roots: impl IntoIterator<Item = (NodeId, bool)>,
    active_joins: &[(JoinId, Vec<Vec<String>>)],
    location: &str,
    project: &Project,
    issues: &mut Vec<ValidationIssue>,
    ancestors: &mut BTreeSet<NodeId>,
) {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut visited = BTreeSet::new();
    while let Some((id, root_context)) = pending.pop() {
        if ancestors.contains(&id) || !visited.insert((id, root_context)) {
            continue;
        }
        let Some(node) = graph.nodes.get(&id) else {
            continue;
        };
        match node {
            Node::JoinField {
                join,
                collection,
                path,
            } => match active_joins.iter().rev().find(|(active, _)| active == join) {
                None => issues.push(ValidationIssue::new(
                    location,
                    format!(
                        "join field node {id} references inactive join {}",
                        join.get()
                    ),
                )),
                Some((_, collections)) if !collections.contains(collection) => {
                    issues.push(ValidationIssue::new(
                        location,
                        format!(
                            "join field node {id} collection `{}` does not belong to join {}",
                            display_path(collection),
                            join.get()
                        ),
                    ));
                }
                Some(_) => validate_collection_value(
                    project,
                    location,
                    collection,
                    path,
                    "join field",
                    issues,
                ),
            },
            Node::JoinPosition { join }
                if !active_joins.iter().any(|(active, _)| active == join) =>
            {
                issues.push(ValidationIssue::new(
                    location,
                    format!(
                        "join position node {id} references inactive join {}",
                        join.get()
                    ),
                ));
            }
            Node::JoinAggregate {
                join,
                plan,
                expression,
                arg,
                ..
            } => {
                if root_context {
                    validate_root_singletons(project, *join, plan, location, issues);
                }
                pending.extend(arg.map(|arg| (arg, root_context)));
                if let Some(expression) = expression {
                    let mut local_joins = active_joins.to_vec();
                    local_joins.push((
                        *join,
                        plan.sources()
                            .map(|source| source.collection().to_vec())
                            .collect(),
                    ));
                    ancestors.insert(id);
                    validate_roots_inner(
                        graph,
                        [(*expression, false)],
                        &local_joins,
                        location,
                        project,
                        issues,
                        ancestors,
                    );
                    ancestors.remove(&id);
                }
            }
            _ => {
                let item_inputs = item_context_inputs(node);
                pending.extend(
                    node_inputs(node)
                        .into_iter()
                        .map(|(_, input)| (input, root_context && !item_inputs.contains(&input))),
                );
            }
        }
    }
}

/// Inputs evaluated in a collection item, generated item, or mixed-content
/// occurrence context rather than in their consumer's context.
fn item_context_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Aggregate { expression, .. } => expression.iter().copied().collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists { predicate, .. } => vec![*predicate],
        Node::SequenceAggregate {
            predicate,
            expression,
            ..
        } => predicate.iter().chain(expression).copied().collect(),
        Node::Lookup { matches, .. } => vec![*matches],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        _ => Vec::new(),
    }
}
