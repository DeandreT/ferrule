//! Cardinality proof for aggregates whose native inputs have no context port.

use std::collections::{BTreeMap, BTreeSet};

use mapping::{Graph, Node, NodeId};

use super::schema::PortMatch;
use super::source::SourceExports;

/// Without a parent-context pin, native item-at must receive its cardinality
/// through the expression's own connection to the selected source collection.
pub(super) fn expression_reads_collection(
    graph: &Graph,
    root: NodeId,
    sources: &SourceExports<'_>,
    collection: u32,
) -> bool {
    // The importer infers computed collections only from primary-source paths.
    // A named source needs owner information this two-input form cannot carry.
    if !sources
        .iter()
        .next()
        .is_some_and(|source| source.ports.path_for_key(collection).is_some())
    {
        return false;
    }
    let mut pending = vec![(root, false)];
    let mut active = BTreeSet::new();
    // Some(false) is scalar context, Some(true) is the exact collection, and
    // None means the exported expression cannot prove the same cardinality.
    let mut proofs = BTreeMap::new();
    while let Some((id, ready)) = pending.pop() {
        if proofs.contains_key(&id) {
            continue;
        }
        let Some(node) = graph.nodes.get(&id) else {
            proofs.insert(id, None);
            continue;
        };
        if !ready {
            if !active.insert(id) {
                proofs.insert(id, None);
                continue;
            }
            pending.push((id, true));
            pending.extend(node.dependencies().into_iter().map(|input| (input, false)));
            continue;
        }
        active.remove(&id);
        let proof = |input| proofs.get(&input).copied().flatten();
        let result = match node {
            Node::SourceField { path, frame } => {
                let mut absolute = frame.clone().unwrap_or_default();
                absolute.extend(path.iter().cloned());
                match sources.match_field(&absolute, frame.is_some()) {
                    PortMatch::Unique(port) => match sources.repetition_owner_of_port(port) {
                        Some(owner) if owner == collection => Some(true),
                        None => Some(false),
                        Some(_) => None,
                    },
                    _ => None,
                }
            }
            Node::Const { .. } | Node::RuntimeValue { .. } | Node::RuntimeParameter { .. } => {
                Some(false)
            }
            Node::Call { args, .. } => args.iter().try_fold(false, |has_collection, input| {
                proof(*input).map(|value| has_collection || value)
            }),
            Node::If {
                condition,
                then,
                else_,
            } => proof(*condition).and_then(|condition| {
                proof(*then)
                    .and_then(|then| proof(*else_).map(|else_| condition || (then && else_)))
            }),
            Node::ValueMap { input, .. } => proof(*input),
            // Reducers consume their input cardinality. Unknown expression
            // forms, including user functions, need a separate native proof.
            _ => None,
        };
        proofs.insert(id, result);
    }
    proofs.get(&root).copied().flatten() == Some(true)
}
