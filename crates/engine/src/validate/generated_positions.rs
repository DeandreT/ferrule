//! Exact generated-origin selectors do not inherit legacy value fallback.

use std::collections::BTreeSet;

use mapping::{FailureIteration, Graph, Node, NodeId, Project, Scope, ScopeRootRole};

use super::{ValidationIssue, ValidationOwner};

pub(super) fn validate(
    project: &Project,
    owners: &BTreeSet<NodeId>,
    issues: &mut Vec<ValidationIssue>,
) {
    if !project
        .graph
        .nodes
        .values()
        .any(|node| matches!(node, Node::GeneratedPosition { .. }))
    {
        return;
    }
    for (&node, value) in &project.graph.nodes {
        if let Node::GeneratedPosition { item } = value
            && !owners.contains(item)
        {
            issues.push(
                ValidationIssue::new(
                    format!("graph node {node}"),
                    format!("generated position references item {item} without a sequence owner"),
                )
                .with_owner(ValidationOwner::GraphNode {
                    function: None,
                    node,
                }),
            );
        }
    }
    let empty = BTreeSet::new();
    scope(project, &project.root, &empty, "primary target", issues);
    for target in &project.extra_targets {
        scope(
            project,
            &target.root,
            &empty,
            &format!("target `{}`", target.name),
            issues,
        );
    }
    for source in &project.extra_sources {
        if let Some(path) = &source.dynamic_path {
            expression(
                &project.graph,
                path.node,
                &empty,
                "dynamic source path",
                issues,
            );
        }
    }
    for (index, rule) in project.failure_rules.iter().enumerate() {
        let mut active = empty.clone();
        if let FailureIteration::Sequence { sequence } = &rule.iteration {
            for input in sequence.inputs() {
                expression(
                    &project.graph,
                    input,
                    &empty,
                    "failure sequence input",
                    issues,
                );
            }
            active.insert(sequence.item());
        }
        for root in rule.selection.predicate().into_iter().chain(rule.message) {
            expression(
                &project.graph,
                root,
                &active,
                &format!("failure rule {}", index + 1),
                issues,
            );
        }
    }
}

fn scope(
    project: &Project,
    scope: &Scope,
    parent: &BTreeSet<NodeId>,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            self::scope(project, segment, parent, location, issues);
        }
        return;
    }
    let mut candidate = parent.clone();
    if let Some(sequence) = scope.sequence() {
        candidate.insert(sequence.item());
    }
    // Group output has no unique member origin. Ancestors remain available;
    // candidate and post-group-member predicates still see the original item.
    let output = if scope.has_grouping() {
        parent
    } else {
        &candidate
    };
    scope.visit_roots(&mut |root| {
        let active = match root.role {
            ScopeRootRole::Parent => parent,
            ScopeRootRole::GeneratedItem => return,
            ScopeRootRole::Item | ScopeRootRole::GroupMember | ScopeRootRole::Lazy => &candidate,
            ScopeRootRole::Output | ScopeRootRole::RecursiveGroup => output,
        };
        expression(&project.graph, root.node, active, location, issues);
    });
    for child in &scope.children {
        self::scope(project, child, output, location, issues);
    }
    for child in &scope.dynamic_children {
        self::scope(project, &child.scope, output, location, issues);
    }
}

fn expression(
    graph: &Graph,
    root: NodeId,
    active: &BTreeSet<NodeId>,
    location: &str,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut pending = vec![(root, active.clone())];
    let mut visited = BTreeSet::new();
    while let Some((node, active)) = pending.pop() {
        if !visited.insert((node, active.clone())) {
            continue;
        }
        let Some(value) = graph.nodes.get(&node) else {
            continue;
        };
        match value {
            Node::GeneratedPosition { item } => {
                if !active.contains(item) {
                    issues.push(ValidationIssue::new(location,
                        format!("generated position node {node} requires active ungrouped sequence item {item}"))
                        .with_owner(ValidationOwner::GraphNode { function: None, node }));
                }
            }
            Node::SequenceExists {
                sequence,
                predicate,
            } => {
                let mut inner = active.clone();
                inner.insert(sequence.item());
                pending.push((*predicate, inner));
                pending.extend(
                    sequence
                        .inputs()
                        .into_iter()
                        .map(|input| (input, active.clone())),
                );
            }
            Node::SequenceAggregate {
                sequence,
                predicate,
                expression,
                arg,
                ..
            } => {
                let mut inner = active.clone();
                inner.insert(sequence.item());
                pending.extend(
                    predicate
                        .iter()
                        .chain(expression)
                        .map(|input| (*input, inner.clone())),
                );
                pending.extend(
                    sequence
                        .inputs()
                        .into_iter()
                        .chain(*arg)
                        .map(|input| (input, active.clone())),
                );
            }
            _ => pending.extend(
                value
                    .evaluated_inputs()
                    .into_iter()
                    .map(|input| (input, active.clone())),
            ),
        }
    }
}
