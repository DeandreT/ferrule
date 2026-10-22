//! Context ownership of generated-sequence consumers: an item-dependent
//! node stays inside the predicate or expression that owns its item, and a
//! consumer's parent-context inputs never read that item.

use std::collections::BTreeSet;

use mapping::{Graph, Node, NodeId, NodeInput, NodeInputRole, Project};

use super::node_inputs::{
    context_node_inputs, labelled_inputs, role_inputs, sequence_parent_inputs,
};
use super::{ValidationIssue, ValidationOwner, own_issues};

pub(super) fn validate_sequence_exists_contexts(
    project: &Project,
    sequence_items: &BTreeSet<NodeId>,
    issues: &mut Vec<ValidationIssue>,
) {
    let mut scope_roots = BTreeSet::new();
    crate::scope_slots::shared_graph_roots(&project.root, &mut scope_roots);
    for target in &project.extra_targets {
        crate::scope_slots::shared_graph_roots(&target.root, &mut scope_roots);
    }
    for (&owner, node) in &project.graph.nodes {
        let ownership_start = issues.len();
        let Node::SequenceExists { sequence, .. } = node else {
            continue;
        };
        let item = sequence.item();
        let location = format!("graph node {owner}");
        let allowed = context_dependencies(
            &project.graph,
            role_inputs(node, NodeInputRole::GeneratedItem),
        );

        for foreign in allowed.intersection(sequence_items) {
            if *foreign != item {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "predicate references sequence item node {foreign} owned by another generated context"
                    ),
                ));
            }
        }
        for argument in role_inputs(node, NodeInputRole::Parent) {
            if context_dependencies(&project.graph, [argument]).contains(&item) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "sequence argument depends on its own item node {item} before that item exists"
                    ),
                ));
            }
        }

        let dependent: BTreeSet<_> = allowed
            .iter()
            .copied()
            .filter(|&id| context_dependencies(&project.graph, [id]).contains(&item))
            .collect();
        if dependent.is_empty() {
            own_issues(
                &mut issues[ownership_start..],
                ValidationOwner::GraphNode {
                    function: None,
                    node: owner,
                },
            );
            continue;
        }
        for (&consumer, consumer_node) in &project.graph.nodes {
            for input in context_node_inputs(consumer_node) {
                if dependent.contains(&input) && !allowed.contains(&consumer) {
                    issues.push(ValidationIssue::new(
                        &location,
                        format!(
                            "item-dependent node {input} is also consumed by graph node {consumer} outside this predicate"
                        ),
                    ));
                }
            }
            if let Node::SequenceExists {
                predicate: nested_predicate,
                ..
            } = consumer_node
                && dependent.contains(nested_predicate)
                && consumer != owner
            {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "item-dependent node {nested_predicate} is reused as graph node {consumer}'s predicate"
                    ),
                ));
            }
        }
        for root in scope_roots.intersection(&dependent) {
            issues.push(ValidationIssue::new(
                &location,
                format!(
                    "item-dependent node {root} is also referenced by a scope outside this predicate"
                ),
            ));
        }
        own_issues(
            &mut issues[ownership_start..],
            ValidationOwner::GraphNode {
                function: None,
                node: owner,
            },
        );
    }
}

pub(super) fn validate_sequence_item_at_contexts(
    project: &Project,
    sequence_items: &BTreeSet<NodeId>,
    issues: &mut Vec<ValidationIssue>,
) {
    for (&owner, node) in &project.graph.nodes {
        let ownership_start = issues.len();
        let Node::SequenceItemAt { sequence, .. } = node else {
            continue;
        };
        let item = sequence.item();
        let location = format!("graph node {owner}");
        for (label, input) in sequence_parent_inputs(node) {
            let dependencies = context_dependencies(&project.graph, [input]);
            if dependencies.contains(&item) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "{label} depends on its own sequence item node {item} before that item exists"
                    ),
                ));
            }
            for foreign in dependencies.intersection(sequence_items) {
                if *foreign != item {
                    issues.push(ValidationIssue::new(
                        &location,
                        format!(
                            "{label} references sequence item node {foreign} owned by another generated context"
                        ),
                    ));
                }
            }
        }
        own_issues(
            &mut issues[ownership_start..],
            ValidationOwner::GraphNode {
                function: None,
                node: owner,
            },
        );
    }
}

pub(super) fn validate_sequence_aggregate_contexts(
    project: &Project,
    sequence_items: &BTreeSet<NodeId>,
    issues: &mut Vec<ValidationIssue>,
) {
    for (&owner, node) in &project.graph.nodes {
        let ownership_start = issues.len();
        let Node::SequenceAggregate { sequence, .. } = node else {
            continue;
        };
        let item = sequence.item();
        let location = format!("graph node {owner}");
        for (label, input) in sequence_parent_inputs(node) {
            let dependencies = context_dependencies(&project.graph, [input]);
            if dependencies.contains(&item) {
                issues.push(ValidationIssue::new(
                    &location,
                    format!(
                        "{label} depends on its own sequence item node {item} before that item exists"
                    ),
                ));
            }
            // Foreign items are legal when active in the caller's parent
            // context; the site-specific walker checks that permission.
        }

        let generated = |input: &NodeInput| input.role == NodeInputRole::GeneratedItem;
        for (label, root) in labelled_inputs(node, generated) {
            let dependencies = context_dependencies(&project.graph, [root]);
            for foreign in dependencies.intersection(sequence_items) {
                if *foreign != item {
                    issues.push(ValidationIssue::new(
                        &location,
                        format!(
                            "{label} references sequence item node {foreign} owned by another generated context"
                        ),
                    ));
                }
            }
        }
        let allowed = context_dependencies(
            &project.graph,
            role_inputs(node, NodeInputRole::GeneratedItem),
        );
        let dependent: BTreeSet<_> = allowed
            .iter()
            .copied()
            .filter(|&id| context_dependencies(&project.graph, [id]).contains(&item))
            .collect();
        for (&consumer, consumer_node) in &project.graph.nodes {
            for input in context_node_inputs(consumer_node) {
                if dependent.contains(&input) && !allowed.contains(&consumer) {
                    issues.push(ValidationIssue::new(
                        &location,
                        format!(
                            "item-dependent node {input} is also consumed by graph node {consumer} outside this predicate"
                        ),
                    ));
                }
            }
        }
        own_issues(
            &mut issues[ownership_start..],
            ValidationOwner::GraphNode {
                function: None,
                node: owner,
            },
        );
    }
}

pub(super) fn context_dependencies(
    graph: &Graph,
    roots: impl IntoIterator<Item = NodeId>,
) -> BTreeSet<NodeId> {
    let mut pending: Vec<_> = roots.into_iter().collect();
    let mut visited = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        if let Some(node) = graph.nodes.get(&id) {
            pending.extend(context_node_inputs(node));
        }
    }
    visited
}
