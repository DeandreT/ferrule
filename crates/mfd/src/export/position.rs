use std::collections::{BTreeMap, BTreeSet};

use mapping::{Graph, Node, NodeId};

fn graph_node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        // The reducer's predicate has its own generated-item context and is
        // connected explicitly when the filter/exists chain is emitted.
        // Its sequence arguments still execute in the enclosing scope.
        Node::SequenceExists { sequence, .. } => sequence.inputs(),
        Node::Aggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
        Node::SourceField { .. }
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. } => Vec::new(),
    }
}

fn position_nodes_for_roots(
    roots: impl IntoIterator<Item = NodeId>,
    graph: &Graph,
) -> BTreeSet<NodeId> {
    let mut pending: Vec<NodeId> = roots.into_iter().collect();
    let mut visited = BTreeSet::new();
    let mut positions = BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        match graph.nodes.get(&id) {
            Some(Node::Position { .. }) => {
                positions.insert(id);
            }
            Some(node) => pending.extend(graph_node_inputs(node)),
            None => {}
        }
    }
    positions
}

#[allow(clippy::too_many_arguments)]
pub(super) fn connect_position_roots(
    roots: impl IntoIterator<Item = NodeId>,
    source_collection: Option<&[String]>,
    allow_empty: bool,
    from: u32,
    graph: &Graph,
    position_inputs: &BTreeMap<NodeId, u32>,
    position_contexts: &mut BTreeMap<NodeId, Option<u32>>,
    edges: &mut Vec<(u32, u32)>,
    warnings: &mut Vec<String>,
) {
    let referenced = position_nodes_for_roots(roots, graph);
    for id in referenced {
        let Some(&input) = position_inputs.get(&id) else {
            continue;
        };
        let Some(Node::Position { collection }) = graph.nodes.get(&id) else {
            continue;
        };
        let matches_scope = if collection.is_empty() {
            allow_empty
        } else {
            source_collection.is_some_and(|source| source.ends_with(collection))
        };
        if !matches_scope {
            continue;
        }
        match position_contexts.get(&id).copied() {
            None => {
                position_contexts.insert(id, Some(from));
                edges.push((from, input));
            }
            Some(Some(existing)) if existing != from => {
                warnings.push(format!(
                    "position node {id} is used in multiple iteration stages or scopes; \
                     its first context connection was kept"
                ));
                position_contexts.insert(id, None);
            }
            Some(_) => {}
        }
    }
}
