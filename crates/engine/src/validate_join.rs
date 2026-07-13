use std::collections::BTreeSet;

use mapping::{Graph, JoinId, JoinPlan, Node, NodeId, Project, Scope};

use super::validate::{
    ValidationIssue, display_path, node_inputs, source_path_matches, validate_collection_value,
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
        if !collection.is_empty()
            && !source_path_matches(project, collection, |node| node.repeating)
        {
            issues.push(ValidationIssue::new(
                location,
                format!(
                    "join {} collection `{}` is missing or not repeating",
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

pub(super) fn validate_scope_nodes(
    graph: &Graph,
    scope: &Scope,
    active_joins: &[(JoinId, Vec<Vec<String>>)],
    location: &str,
    project: &Project,
    issues: &mut Vec<ValidationIssue>,
) {
    let roots = [
        scope.filter,
        scope.group_by,
        scope.group_starting_with,
        scope.group_into_blocks,
        scope.sort_by,
    ]
    .into_iter()
    .flatten()
    .chain(scope.bindings.iter().map(|binding| binding.node))
    .chain(
        scope
            .dynamic_bindings
            .iter()
            .flat_map(|binding| [binding.key, binding.value]),
    )
    .chain(scope.dynamic_children.iter().map(|child| child.key));
    validate_roots(graph, roots, active_joins, location, project, issues);
}

pub(super) fn validate_roots(
    graph: &Graph,
    roots: impl IntoIterator<Item = NodeId>,
    active_joins: &[(JoinId, Vec<Vec<String>>)],
    location: &str,
    project: &Project,
    issues: &mut Vec<ValidationIssue>,
) {
    validate_roots_inner(
        graph,
        roots,
        active_joins,
        location,
        project,
        issues,
        &mut BTreeSet::new(),
    );
}

fn validate_roots_inner(
    graph: &Graph,
    roots: impl IntoIterator<Item = NodeId>,
    active_joins: &[(JoinId, Vec<Vec<String>>)],
    location: &str,
    project: &Project,
    issues: &mut Vec<ValidationIssue>,
    ancestors: &mut BTreeSet<NodeId>,
) {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if ancestors.contains(&id) || !visited.insert(id) {
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
                pending.extend(arg);
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
                        [*expression],
                        &local_joins,
                        location,
                        project,
                        issues,
                        ancestors,
                    );
                    ancestors.remove(&id);
                }
            }
            _ => pending.extend(node_inputs(node).into_iter().map(|(_, input)| input)),
        }
    }
}
