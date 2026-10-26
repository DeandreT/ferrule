//! The graph inputs validation reads, taken from the shared role-aware node
//! input visitor.

use mapping::{Node, NodeId, NodeInput, NodeInputRole, NodeInputSlot as Slot};

/// Every input except a generated sequence's private item identities, each
/// labelled for issues, in visit order.
pub(super) fn node_inputs(node: &Node) -> Vec<(String, NodeId)> {
    labelled_inputs(node, |_| true)
}

/// The inputs admitted by `admit`, other than item identities, each labelled
/// for issues, in visit order.
pub(super) fn labelled_inputs(
    node: &Node,
    admit: impl Fn(&NodeInput) -> bool,
) -> Vec<(String, NodeId)> {
    slotted_inputs(node, admit)
        .into_iter()
        .map(|(slot, input)| (label(node, slot), input))
        .collect()
}

/// The inputs admitted by `admit`, other than item identities, with their
/// slots, in visit order.
fn slotted_inputs(node: &Node, admit: impl Fn(&NodeInput) -> bool) -> Vec<(Slot, NodeId)> {
    let mut inputs = Vec::new();
    node.visit_inputs(&mut |input| {
        if input.role != NodeInputRole::ItemIdentity && admit(&input) {
            inputs.push((input.slot, input.node));
        }
    });
    inputs
}

/// The inputs a generated-sequence consumer evaluates in its enclosing
/// context, labelled for its sequence-context issues, which name an
/// aggregate's argument as the aggregate's.
pub(super) fn sequence_parent_inputs(node: &Node) -> Vec<(String, NodeId)> {
    slotted_inputs(node, |input| input.role == NodeInputRole::Parent)
        .into_iter()
        .map(|(slot, input)| match slot {
            Slot::AggregateArgument => ("aggregate argument".to_string(), input),
            slot => (label(node, slot), input),
        })
        .collect()
}

/// The inputs a node evaluates in `role`, in visit order.
pub(super) fn role_inputs(node: &Node, role: NodeInputRole) -> Vec<NodeId> {
    let mut inputs = Vec::new();
    node.visit_inputs(&mut |input| {
        if input.role == role {
            inputs.push(input.node);
        }
    });
    inputs
}

/// The inputs a node evaluates outside a generated item's private context:
/// every input except item identities and generated-item expressions.
pub(super) fn context_node_inputs(node: &Node) -> Vec<NodeId> {
    let mut inputs = Vec::new();
    node.visit_inputs(&mut |input| {
        if !matches!(
            input.role,
            NodeInputRole::ItemIdentity | NodeInputRole::GeneratedItem
        ) {
            inputs.push(input.node);
        }
    });
    inputs
}

/// How issues name the input in `slot` of `node`.
fn label(node: &Node, slot: Slot) -> String {
    match slot {
        Slot::Argument(index) => format!("argument {index}"),
        Slot::Default => "default".into(),
        Slot::Start => "start".into(),
        Slot::Step => "step".into(),
        Slot::Restart => "restart".into(),
        Slot::Message => "message".into(),
        Slot::Condition => "condition".into(),
        Slot::Then => "then branch".into(),
        Slot::Else => "else branch".into(),
        Slot::Input => "input".into(),
        Slot::Matches => "matches".into(),
        Slot::PropertyName => "property name".into(),
        Slot::Predicate => "predicate".into(),
        Slot::Value => "value expression".into(),
        Slot::Expression if matches!(node, Node::SequenceAggregate { .. }) => {
            "aggregate expression".into()
        }
        Slot::Expression => "value expression".into(),
        Slot::AggregateArgument => "argument".into(),
        Slot::Index => "index".into(),
        Slot::Replacement(index) => {
            let element = match node {
                Node::XmlMixedContent { replacements, .. } => replacements
                    .get(index)
                    .map(|replacement| &replacement.element),
                Node::OwnedXmlMixedContent(mixed) => mixed
                    .replacements
                    .get(index)
                    .map(|replacement| &replacement.element),
                _ => None,
            };
            format!(
                "replacement for `{}`",
                element.map_or("", |element| element.as_str())
            )
        }
        Slot::SequenceInput(index) => format!("sequence argument {index}"),
        Slot::SequenceReservedItem | Slot::SequenceItem => "sequence item".into(),
    }
}

#[cfg(test)]
#[path = "../tests/validate_node_inputs.rs"]
mod tests;
