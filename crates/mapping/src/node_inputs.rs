//! Graph inputs of each node, each with the context it is evaluated in.
//!
//! A node evaluates its inputs in several distinct contexts: once in its own
//! context, only when a branch is selected, once per item, joined tuple,
//! generated item or mixed-content occurrence, or never outside a generated
//! sequence's private item context. Analyses that need only reachability can
//! ignore the role; analyses that reason about available frames must not
//! merge roles, and none of them can recover evaluation order from this
//! visit order.

use crate::{Node, NodeId, OwnedXmlMixedContentContext, SequenceExpr};

/// Where and how often a node evaluates an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeInputRole {
    /// Once in the consumer's own context whenever the consumer is evaluated:
    /// call arguments, conditions, lookup needles, aggregate arguments,
    /// sequence inputs and indexes.
    Parent,
    /// In the consumer's own context, only when the consumer selects it: an
    /// `If` branch, or a runtime parameter's default.
    Lazy,
    /// Once per item of the consumer's own collection walk, with that item as
    /// the latest frame.
    Item,
    /// Once per joined tuple, with that join's members and position active.
    JoinTuple,
    /// Once per generated sequence item, in its private item context.
    GeneratedItem,
    /// The private item identity of a generated sequence. It is readable only
    /// in that sequence's item context and is never evaluated outside it.
    ItemIdentity,
    /// Once per matching XML mixed-content occurrence, with that occurrence
    /// as the latest frame.
    Occurrence,
    /// Once per matching XML mixed-content occurrence, in the consumer's own
    /// context.
    ParentOccurrence,
}

/// Which field of a node owns an input.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NodeInputSlot {
    /// A call argument at this index.
    Argument(usize),
    Default,
    Message,
    Condition,
    Then,
    Else,
    Input,
    Matches,
    /// A dynamic field's computed property name.
    PropertyName,
    Predicate,
    /// A collection search's per-item value.
    Value,
    /// An aggregate's computed value.
    Expression,
    /// An aggregate's extra scalar argument.
    AggregateArgument,
    Index,
    /// A mixed-content replacement at this index.
    Replacement(usize),
    /// A generated sequence's input at this index, in
    /// [`SequenceExpr::inputs`] order.
    SequenceInput(usize),
    /// A reserved source item that a composed sequence owns besides its item.
    SequenceReservedItem,
    /// The generated item identity returned by [`SequenceExpr::item`].
    SequenceItem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NodeInput {
    pub node: NodeId,
    pub slot: NodeInputSlot,
    pub role: NodeInputRole,
}

impl Node {
    /// Visits this node's graph inputs in a fixed slot order, which follows
    /// expression evaluation where that order is meaningful.
    pub fn visit_inputs(&self, visit: &mut impl FnMut(NodeInput)) {
        let mut input = |node, slot, role| visit(NodeInput { node, slot, role });
        use NodeInputRole::{
            GeneratedItem, Item, JoinTuple, Lazy, Occurrence, Parent, ParentOccurrence,
        };
        use NodeInputSlot as Slot;
        match self {
            Self::OwnedSourceField(_)
            | Self::OwnedPosition(_)
            | Self::OwnedRowDocumentPath(_)
            | Self::OwnedXmlSerialize(_)
            | Self::OwnedJoinField(_)
            | Self::OwnedJoinSourcePosition(_)
            | Self::SourceField { .. }
            | Self::SourceRootXmlTypeEquals { .. }
            | Self::SourceRootField { .. }
            | Self::SourceDocumentPath
            | Self::Position { .. }
            | Self::JoinField { .. }
            | Self::JoinPosition { .. }
            | Self::Unconnected
            | Self::Const { .. }
            | Self::FunctionParameter { .. }
            | Self::RuntimeValue { .. }
            | Self::RuntimeParameter { .. }
            | Self::XmlSerialize { .. } => {}
            Self::RuntimeParameterDefault { default, .. } => input(*default, Slot::Default, Lazy),
            Self::Raise { message } => {
                if let Some(message) = message {
                    input(*message, Slot::Message, Parent);
                }
            }
            Self::Call { args, .. } | Self::UserFunctionCall { args, .. } => {
                for (index, arg) in args.iter().enumerate() {
                    input(*arg, Slot::Argument(index), Parent);
                }
            }
            Self::If {
                condition,
                then,
                else_,
            } => {
                input(*condition, Slot::Condition, Parent);
                input(*then, Slot::Then, Lazy);
                input(*else_, Slot::Else, Lazy);
            }
            Self::ValueMap { input: value, .. } => input(*value, Slot::Input, Parent),
            // A lookup's needle is evaluated once before the walk.
            Self::Lookup { matches, .. } => input(*matches, Slot::Matches, Parent),
            Self::OwnedLookup(lookup) => input(lookup.matches, Slot::Matches, Parent),
            Self::OwnedCollectionFind(find) => {
                input(find.predicate, Slot::Predicate, Item);
                input(find.value, Slot::Value, Item);
            }
            Self::OwnedAggregate(aggregate) => {
                if let Some(expression) = aggregate.expression() {
                    input(expression, Slot::Expression, Item);
                }
                if let Some(arg) = aggregate.arg {
                    input(arg, Slot::AggregateArgument, Parent);
                }
            }
            Self::OwnedJoinAggregate(aggregate) => {
                if let Some(expression) = aggregate.expression {
                    input(expression, Slot::Expression, JoinTuple);
                }
                if let Some(arg) = aggregate.arg {
                    input(arg, Slot::AggregateArgument, Parent);
                }
            }
            Self::DynamicSourceField { key, .. } => input(*key, Slot::PropertyName, Parent),
            Self::OwnedDynamicSourceField(field) => input(field.key, Slot::PropertyName, Parent),
            // An empty collection keeps the expression in the group's frame.
            Self::XmlMixedContent { replacements, .. } => {
                for (index, replacement) in replacements.iter().enumerate() {
                    let role = if replacement.collection.is_empty() {
                        ParentOccurrence
                    } else {
                        Occurrence
                    };
                    input(replacement.expression, Slot::Replacement(index), role);
                }
            }
            Self::OwnedXmlMixedContent(mixed) => {
                for (index, replacement) in mixed.replacements.iter().enumerate() {
                    let role = match replacement.context {
                        OwnedXmlMixedContentContext::Parent => ParentOccurrence,
                        OwnedXmlMixedContentContext::Occurrence => Occurrence,
                    };
                    input(replacement.expression, Slot::Replacement(index), role);
                }
            }
            Self::CollectionFind {
                predicate, value, ..
            } => {
                input(*predicate, Slot::Predicate, Item);
                input(*value, Slot::Value, Item);
            }
            Self::SequenceExists {
                sequence,
                predicate,
            } => {
                visit_sequence_inputs(sequence, &mut input);
                input(*predicate, Slot::Predicate, GeneratedItem);
            }
            // The index is evaluated once in the enclosing context.
            Self::SequenceItemAt { sequence, index } => {
                visit_sequence_inputs(sequence, &mut input);
                input(*index, Slot::Index, Parent);
            }
            Self::SequenceAggregate {
                sequence,
                predicate,
                expression,
                arg,
                ..
            } => {
                visit_sequence_inputs(sequence, &mut input);
                if let Some(predicate) = predicate {
                    input(*predicate, Slot::Predicate, GeneratedItem);
                }
                if let Some(expression) = expression {
                    input(*expression, Slot::Expression, GeneratedItem);
                }
                if let Some(arg) = arg {
                    input(*arg, Slot::AggregateArgument, Parent);
                }
            }
            Self::Aggregate {
                expression, arg, ..
            } => {
                if let Some(expression) = expression {
                    input(*expression, Slot::Expression, Item);
                }
                if let Some(arg) = arg {
                    input(*arg, Slot::AggregateArgument, Parent);
                }
            }
            Self::JoinAggregate {
                expression, arg, ..
            } => {
                if let Some(expression) = expression {
                    input(*expression, Slot::Expression, JoinTuple);
                }
                if let Some(arg) = arg {
                    input(*arg, Slot::AggregateArgument, Parent);
                }
            }
        }
    }

    /// Graph nodes whose outputs must be available to evaluate this node,
    /// in [`Node::visit_inputs`] order and whatever their role.
    pub fn dependencies(&self) -> Vec<NodeId> {
        let mut dependencies = Vec::new();
        self.visit_inputs(&mut |input| dependencies.push(input.node));
        dependencies
    }
}

/// A sequence's inputs at every composition level, then the item
/// identities its outermost level owns, as in [`SequenceExpr::owned_items`].
fn visit_sequence_inputs(
    sequence: &SequenceExpr,
    input: &mut impl FnMut(NodeId, NodeInputSlot, NodeInputRole),
) {
    sequence.visit_level_inputs(&mut |declared| {
        if declared.level == 0 || declared.input.role == NodeInputRole::Parent {
            input(
                declared.input.node,
                declared.input.slot,
                declared.input.role,
            );
        }
    });
}

#[cfg(test)]
#[path = "tests/node_inputs.rs"]
mod tests;
