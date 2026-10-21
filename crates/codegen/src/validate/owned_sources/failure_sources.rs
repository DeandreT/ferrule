//! Explicit owned failure-rule iteration. The walk selects exactly one declared
//! static endpoint; predicate and message keep a closed scalar subset. This is
//! generation-time classification; both runtimes walk only the selected owner.
//! Only the `failures.owned_sources` capability admits the walk.

use super::*;

impl OwnedCapabilities {
    /// Whether this rule's owned reads are admitted. Failure permissions are
    /// orthogonal to target controls and consumer policies.
    pub(super) fn admits_owned_failure_rule(
        self,
        program: &Program,
        rule: &crate::FailureRule,
    ) -> bool {
        self.failures.primary_rows && is_basic_owned_primary_row_failure(program, rule)
            || self.failures.owned_sources && is_owned_failure_source(program, rule)
    }
}

/// Refuse an owned walk before any of its expressions when the policy, the
/// owner or the rule's scalar subset is closed.
pub(super) fn admit_iteration(
    program: &Program,
    index: usize,
    rule: &crate::FailureRule,
    capabilities: OwnedCapabilities,
) -> Result<(), ProgramValidationError> {
    let FailureIteration::OwnedSource(source) = &rule.iteration else {
        return Ok(());
    };
    let query = iteration_request(source.source());
    let location = Location::FailureIteration { index };
    if let Some(closed) = dynamic_owner(program, &query) {
        return Err(failure(location, Some(query), closed));
    }
    if !capabilities.failures.owned_sources || !is_owned_failure_source(program, rule) {
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
