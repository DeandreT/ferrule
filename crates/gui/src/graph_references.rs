use mapping::{Graph, NamedTarget, Node, NodeId, Scope, ScopeConstruction};

pub(super) fn node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::SourceField { .. }
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. } => Vec::new(),
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence.inputs().into_iter().chain([*predicate]).collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

pub(super) fn references_to(
    graph: &Graph,
    root_scope: &Scope,
    extra_targets: &[NamedTarget],
    needle: NodeId,
) -> Vec<String> {
    fn scope_references(
        scope: &Scope,
        path: &mut Vec<String>,
        needle: NodeId,
        found: &mut std::collections::BTreeSet<String>,
    ) {
        let label = if path.is_empty() {
            "root scope".to_string()
        } else {
            format!("scope {}", path.join("/"))
        };
        for (reference, description) in [
            (scope.filter, "filter"),
            (scope.group_by, "group-by key"),
            (scope.group_starting_with, "group-starting predicate"),
            (scope.group_into_blocks, "group block size"),
            (scope.sort_by, "sort key"),
            (scope.take, "take count"),
        ] {
            if reference == Some(needle) {
                found.insert(format!("{label} {description}"));
            }
        }
        if let Some(sequence) = scope.sequence() {
            if sequence.inputs().contains(&needle) {
                found.insert(format!("{label} sequence input"));
            }
            if sequence.item() == needle {
                found.insert(format!("{label} sequence item"));
            }
        }
        if let ScopeConstruction::AdjacencyTree { plan } = &scope.construction
            && plan.root() == Some(needle)
        {
            found.insert(format!("{label} adjacency-tree root"));
        }
        for binding in &scope.bindings {
            if binding.node == needle {
                found.insert(format!("{label} binding {}", binding.target_field));
            }
        }
        for (index, binding) in scope.dynamic_bindings.iter().enumerate() {
            if binding.key == needle {
                found.insert(format!("{label} dynamic binding {} key", index + 1));
            }
            if binding.value == needle {
                found.insert(format!("{label} dynamic binding {} value", index + 1));
            }
        }
        if let Some(segments) = scope.concatenated() {
            for (index, segment) in segments.iter().enumerate() {
                path.push(format!("<segment {}>", index + 1));
                scope_references(segment, path, needle, found);
                path.pop();
            }
        }
        for child in &scope.children {
            path.push(child.target_field.clone());
            scope_references(child, path, needle, found);
            path.pop();
        }
        for (index, child) in scope.dynamic_children.iter().enumerate() {
            if child.key == needle {
                found.insert(format!("{label} dynamic child {} key", index + 1));
            }
            path.push(format!("<dynamic child {}>", index + 1));
            scope_references(&child.scope, path, needle, found);
            path.pop();
        }
    }

    let mut found = std::collections::BTreeSet::new();
    for (&owner, node) in &graph.nodes {
        if owner != needle && node_inputs(node).contains(&needle) {
            found.insert(format!("graph node {owner}"));
        }
        if owner != needle
            && matches!(node, Node::SequenceExists { sequence, .. } if sequence.item() == needle)
        {
            found.insert(format!("graph node {owner} sequence item"));
        }
    }
    scope_references(root_scope, &mut Vec::new(), needle, &mut found);
    for target in extra_targets {
        let mut path = vec![format!("<target {}>", target.name)];
        scope_references(&target.root, &mut path, needle, &mut found);
    }
    found.into_iter().collect()
}
