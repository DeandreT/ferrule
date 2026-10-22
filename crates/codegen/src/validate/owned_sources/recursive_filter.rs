//! Recursive filters over an explicitly owned scope selector. The selector is
//! the filter's only owner: every visited group and item keeps it, and the
//! predicate reads those Basic item frames. Only the
//! `constructions.recursive_filter` capability admits the construction; legacy
//! filters and other tree operators stay closed.

use super::*;

/// The predicate of a recursive filter whose scope iterates an owned source.
pub(super) fn owned_predicate(scope: &TargetScope) -> Option<NodeId> {
    match (&scope.construction, &scope.iteration) {
        (TargetConstruction::RecursiveFilter { predicate, .. }, Some(iteration))
            if iteration.owned_source_iteration().is_some() =>
        {
            Some(*predicate)
        }
        _ => None,
    }
}
