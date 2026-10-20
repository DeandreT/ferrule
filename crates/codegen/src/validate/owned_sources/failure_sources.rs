//! Explicit owned failure-rule iteration. The walk selects exactly one declared
//! static endpoint; predicate and message keep a closed scalar subset. This is
//! generation-time classification; both runtimes walk only the selected owner.

use super::*;

/// Failure permissions are orthogonal to target controls and consumer policies.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::validate) enum FailurePolicy {
    Closed,
    PrimaryRows,
    /// Primary-row legacy rules plus owned endpoint iteration.
    OwnedSources,
}

impl FailurePolicy {
    pub(in crate::validate) fn from_primary_rows(primary_rows: bool) -> Self {
        if primary_rows {
            Self::PrimaryRows
        } else {
            Self::Closed
        }
    }

    /// Whether this rule's owned reads are admitted by this policy.
    pub(super) fn admits_owned_rule(self, program: &Program, rule: &crate::FailureRule) -> bool {
        match self {
            Self::Closed => false,
            Self::PrimaryRows => is_basic_owned_primary_row_failure(program, rule),
            Self::OwnedSources => {
                is_basic_owned_primary_row_failure(program, rule)
                    || is_owned_failure_source(program, rule)
            }
        }
    }
}

/// Validate the EndingWith target profile, scalar-producer ItemAt and both
/// owned failure permissions. Earlier companions keep refusing owned walks.
pub fn validate_basic_owned_failure_source_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_failures(
        program,
        BasicOwnedProfile::ByAndAdjacentByAndStartingWithAndEndingWithAndWindowsAndSortAndFilter,
        FailurePolicy::OwnedSources,
        SequenceItemAtPolicy::BasicScalarProducers,
    )
}

/// Refuse an owned walk before any of its expressions when the policy, the
/// owner or the rule's scalar subset is closed.
pub(super) fn admit_iteration(
    program: &Program,
    index: usize,
    rule: &crate::FailureRule,
    failures: FailurePolicy,
) -> Result<(), ProgramValidationError> {
    let FailureIteration::OwnedSource(source) = &rule.iteration else {
        return Ok(());
    };
    let query = iteration_request(source.source());
    let location = Location::FailureIteration { index };
    if let Some(closed) = dynamic_owner(program, &query) {
        return Err(failure(location, Some(query), closed));
    }
    if failures != FailurePolicy::OwnedSources || !is_owned_failure_source(program, rule) {
        return Err(failure(
            location,
            Some(query),
            Closed {
                reason: Capability::FailureRule,
                origin: Origin::Failure { index },
            },
        ));
    }
    Ok(())
}

/// An owned walk whose predicate and message use only static scalar reads.
/// Owned fields and positions may name any static endpoint and pin; legacy
/// reads resolve through the selected row like an owned scope's item.
fn is_owned_failure_source(program: &Program, rule: &crate::FailureRule) -> bool {
    let FailureIteration::OwnedSource(source) = &rule.iteration else {
        return false;
    };
    if dynamic_owner(program, &iteration_request(source.source())).is_some() {
        return false;
    }
    let mut pending = rule
        .selection
        .predicate()
        .into_iter()
        .chain(rule.message)
        .collect::<Vec<_>>();
    let mut seen = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if !seen.insert(id) {
            continue;
        }
        let Some(node) = program.expressions.iter().find(|node| node.id == id) else {
            return false;
        };
        match &node.expression {
            Expression::OwnedSourceField(_) | Expression::OwnedPosition(_) => {
                if request(&node.expression)
                    .is_some_and(|query| dynamic_owner(program, &query).is_some())
                {
                    return false;
                }
            }
            Expression::SourceField { path, frame } => {
                if basic_dynamic_path(program, path)
                    || frame
                        .as_ref()
                        .is_some_and(|frame| basic_dynamic_path(program, frame))
                {
                    return false;
                }
            }
            Expression::Position { collection } => {
                if basic_dynamic_path(program, collection) {
                    return false;
                }
            }
            Expression::Const { .. }
            | Expression::Call { .. }
            | Expression::If { .. }
            | Expression::RuntimeValue { .. }
            | Expression::RuntimeParameter { .. }
            | Expression::RuntimeParameterDefault { .. }
            | Expression::Raise { .. }
            | Expression::ValueMap { .. } => {}
            _ => return false,
        }
        pending.extend(graph_dependencies::of(&node.expression));
    }
    true
}
