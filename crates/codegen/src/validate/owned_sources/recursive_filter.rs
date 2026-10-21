//! Recursive filters over an explicitly owned scope selector. The selector is
//! the filter's only owner: every visited group and item keeps it, and the
//! predicate reads those Basic item frames. Only the explicit companion admits
//! the construction; legacy filters and other tree operators stay closed.

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

/// Validate the owned recursive-collect composition (owned adjacency trees,
/// path hierarchies, failure sources, EndingWith controls and ItemAt over owned
/// walks) plus recursive filtering of an owned selector. Every earlier
/// companion keeps refusing the owned filter.
pub fn validate_basic_owned_recursive_filter_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_constructions(
        program,
        BasicOwnedProfile::ByAndAdjacentByAndStartingWithAndEndingWithAndWindowsAndSortAndFilter,
        FailurePolicy::OwnedSources,
        SequenceItemAtPolicy::BasicScalarAndOwnedRecursiveProducers,
        PathHierarchyPolicy::Owned,
        AdjacencyTreePolicy::OwnedWithRecursiveFilter,
    )
}
