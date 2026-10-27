//! The node inputs the `.mfd` import and export passes read, taken from the
//! shared role-aware node input visitor and paired with the ports, frames or
//! node kinds the format gives them.

use mapping::{Node, NodeId, NodeInputRole, NodeInputSlot};

/// Whether a node is an explicitly owned read, which has no export
/// capability, so its inputs are neither wired nor walked.
fn owned_read(node: &Node) -> bool {
    matches!(
        node,
        Node::OwnedSourceField(_)
            | Node::OwnedPosition(_)
            | Node::OwnedRowDocumentPath(_)
            | Node::OwnedSourceDocumentPath(_)
            | Node::OwnedDynamicSourceField(_)
            | Node::OwnedLookup(_)
            | Node::OwnedCollectionFind(_)
            | Node::OwnedXmlSerialize(_)
            | Node::OwnedAggregate(_)
            | Node::OwnedJoinField(_)
            | Node::OwnedJoinSourcePosition(_)
            | Node::OwnedXmlMixedContent(_)
            | Node::OwnedJoinAggregate(_)
    )
}

/// The inputs wired by position onto an exported function's input ports, in
/// port order. A generated sequence's inputs are wired separately and its
/// per-item expressions belong to the generated item's own context; a raise
/// message and an owned read have no positional port. A search's value port
/// precedes its predicate port.
pub(crate) fn port_inputs(node: &Node) -> Vec<NodeId> {
    if owned_read(node) || matches!(node, Node::Raise { .. }) {
        return Vec::new();
    }
    let mut inputs = Vec::new();
    node.visit_inputs(&mut |input| {
        let wired = !matches!(input.slot, NodeInputSlot::SequenceInput(_))
            && !matches!(
                input.role,
                NodeInputRole::ItemIdentity
                    | NodeInputRole::ItemReference
                    | NodeInputRole::GeneratedItem
            );
        if wired {
            inputs.push(input);
        }
    });
    // Stable, so every other input keeps its visit order.
    inputs.sort_by_key(|input| input.slot == NodeInputSlot::Predicate);
    inputs.into_iter().map(|input| input.node).collect()
}

/// A node's inputs as the position-selector walk visits them.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SelectorInputs {
    /// Evaluated in the consumer's frames, eagerly or lazily.
    pub(crate) parent: Vec<NodeId>,
    /// Evaluated per item, joined tuple or generated item, in the frames the
    /// node itself adds.
    pub(crate) private: Vec<NodeId>,
    /// Mixed-content replacement expressions, each at its replacement index
    /// and walked in its own occurrence frames.
    pub(crate) replacements: Vec<(usize, NodeId)>,
}

pub(crate) fn selector_inputs(node: &Node) -> SelectorInputs {
    let mut inputs = SelectorInputs::default();
    if owned_read(node) {
        return inputs;
    }
    node.visit_inputs(&mut |input| match (input.role, input.slot) {
        (NodeInputRole::Parent | NodeInputRole::Lazy, _) => inputs.parent.push(input.node),
        (NodeInputRole::Item | NodeInputRole::JoinTuple | NodeInputRole::GeneratedItem, _) => {
            inputs.private.push(input.node)
        }
        (
            NodeInputRole::Occurrence | NodeInputRole::ParentOccurrence,
            NodeInputSlot::Replacement(index),
        ) => inputs.replacements.push((index, input.node)),
        _ => {}
    });
    inputs
}

/// The inputs an item-ordered exception expression is ordered by, or `None`
/// for a node kind with no known ordering depth: only constants, source
/// fields, positions, calls, branches and value maps have one.
pub(crate) fn ordering_inputs(node: &Node) -> Option<Vec<NodeId>> {
    matches!(
        node,
        Node::Const { .. }
            | Node::SourceField { .. }
            | Node::Position { .. }
            | Node::Call { .. }
            | Node::If { .. }
            | Node::ValueMap { .. }
    )
    .then(|| node.evaluated_inputs())
}

#[cfg(test)]
#[path = "tests/node_inputs.rs"]
mod tests;
