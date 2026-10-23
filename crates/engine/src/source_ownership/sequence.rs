//! Owned capability admission of a generated sequence, one composition level
//! at a time.

use mapping::{FunctionId, NodeInputRole, SequenceExpr, SequenceLevelInput};

use super::{Admission, basic_item_at_producer};
use crate::OwnedCapabilityContext;

impl Admission<'_> {
    /// Admits `sequence` level by level, outermost first. Each level's
    /// context derives from the context of the level enclosing it. A level
    /// reads the inputs it and every deeper level declare in that context, so
    /// a composed source's inputs are admitted again at each enclosing level,
    /// and reads its own item identities in its generated-item context.
    pub(super) fn sequence(
        &mut self,
        sequence: &SequenceExpr,
        function: Option<FunctionId>,
        inherited: OwnedCapabilityContext,
        basic_consumer: bool,
    ) {
        let mut declared = Vec::new();
        sequence.visit_level_inputs(&mut |input| declared.push(input));
        let mut context = inherited;
        for (level, composed) in sequence.levels().into_iter().enumerate() {
            // Only the outermost level can be read by a Basic consumer.
            context = level_context(composed, context, basic_consumer && level == 0);
            let read = |admit: &dyn Fn(&SequenceLevelInput) -> bool| {
                declared
                    .iter()
                    .filter(|input| admit(input))
                    .map(|input| input.input.node)
                    .collect::<Vec<_>>()
            };
            self.push(
                function,
                context,
                false,
                read(&|input| input.level >= level && input.input.role == NodeInputRole::Parent),
            );
            // Producer arguments are parent scalars; the lexical declaration
            // is still an ordinary generated item, never an endpoint-bearing
            // input.
            let item_context = if context == OwnedCapabilityContext::Basic {
                OwnedCapabilityContext::GeneratedItem
            } else {
                context
            };
            self.push(
                function,
                item_context,
                false,
                read(&|input| {
                    input.level == level && input.input.role == NodeInputRole::ItemIdentity
                }),
            );
            match composed {
                SequenceExpr::RecursiveCollect { collection, .. } => self.path(collection),
                SequenceExpr::FilterMapV1(composition) => {
                    self.function(composition.predicate);
                    self.function(composition.mapper);
                }
                _ => {}
            }
        }
    }
}

/// The context of one composition level, from the context of the level
/// enclosing it.
fn level_context(
    sequence: &SequenceExpr,
    inherited: OwnedCapabilityContext,
    basic_consumer: bool,
) -> OwnedCapabilityContext {
    if basic_consumer
        && inherited == OwnedCapabilityContext::Basic
        && basic_item_at_producer(sequence)
    {
        OwnedCapabilityContext::Basic
    } else if matches!(sequence, SequenceExpr::RecursiveCollect { .. })
        && matches!(
            inherited,
            OwnedCapabilityContext::Basic | OwnedCapabilityContext::GeneratedItem
        )
    {
        OwnedCapabilityContext::TreeOperator
    } else if inherited == OwnedCapabilityContext::Basic {
        OwnedCapabilityContext::GeneratedItem
    } else {
        inherited
    }
}

#[cfg(test)]
#[path = "../tests/sequence_admission.rs"]
mod tests;
