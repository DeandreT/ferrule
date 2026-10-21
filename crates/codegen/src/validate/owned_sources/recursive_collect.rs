//! Explicitly owned recursive collection producers. Only a Basic parent-context
//! ItemAt consumes one, under the `sequences.owned_recursive_item_at`
//! capability. Scope iteration, failure iteration and private-item reducers
//! stay closed, so a generated item never becomes an endpoint-bearing frame.

use super::*;

/// The owned root collection of a generated scope iteration.
pub(super) fn scope_sequence(iteration: &crate::IterationPlan) -> Option<&mapping::OwnedSourceRef> {
    match iteration.input() {
        IterationSource::Generated(sequence) => sequence.owned_source(),
        _ => None,
    }
}

/// The owned root collection of a generated failure-rule iteration.
pub(super) fn failure_sequence(rule: &crate::FailureRule) -> Option<&mapping::OwnedSourceRef> {
    match &rule.iteration {
        FailureIteration::Generated(sequence) => sequence.owned_source(),
        _ => None,
    }
}

/// The request of an owned producer whose consumer this policy does not admit.
pub(super) fn closed_consumer(
    expression: &Expression,
    consumers: crate::SequenceConsumers,
) -> Option<Request> {
    let sequence = match expression {
        Expression::SequenceExists { sequence, .. }
        | Expression::SequenceItemAt { sequence, .. }
        | Expression::SequenceAggregate { sequence, .. } => sequence,
        _ => return None,
    };
    let source = sequence.owned_source()?;
    (!consumers.permits(expression)).then(|| iteration_request(source))
}

/// Refuse an owned producer that drives a failure rule's generated items.
pub(super) fn admit_failure(
    index: usize,
    rule: &crate::FailureRule,
) -> Result<(), ProgramValidationError> {
    let Some(source) = failure_sequence(rule) else {
        return Ok(());
    };
    Err(failure(
        Location::FailureIteration { index },
        Some(iteration_request(source)),
        Closed {
            reason: Capability::FailureRule,
            origin: Origin::Failure { index },
        },
    ))
}

/// Defensive error for an owned producer reaching a renderer that has no
/// paired consumer for it. The location is the producer's private item.
pub fn owned_sequence_emission_error(
    item: NodeId,
    collection: &mapping::OwnedSourceRef,
) -> ProgramValidationError {
    failure(
        Location::Expression {
            namespace: Namespace::Main,
            node: item,
            scope: None,
        },
        Some(iteration_request(collection)),
        Closed {
            reason: Capability::UnsupportedBasicContext,
            origin: Origin::RetainedExpression {
                namespace: Namespace::Main,
                node: item,
            },
        },
    )
}
