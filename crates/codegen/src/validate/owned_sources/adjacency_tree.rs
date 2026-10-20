//! Explicit owned adjacency-tree row collections. The rows are an exact owned
//! read in the receiving context; the optional root expression keeps that
//! parent context. Only the explicit companion admits the construction.

use super::*;

/// Construction permission for an owned adjacency-tree row collection. It is
/// orthogonal to source controls, consumers and failure policies.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::validate) enum AdjacencyTreePolicy {
    Closed,
    Owned,
}

impl AdjacencyTreePolicy {
    pub(super) fn admits(self, scope: &TargetScope) -> bool {
        self == Self::Owned && owned_rows(scope).is_some()
    }
}

/// The owned row collection of an owned adjacency-tree scope.
pub(super) fn owned_rows(scope: &TargetScope) -> Option<&mapping::OwnedSourceRef> {
    match &scope.construction {
        TargetConstruction::OwnedAdjacencyTree { collection, .. } => Some(collection),
        _ => None,
    }
}

/// The parent-context root expression of an owned adjacency-tree scope.
pub(super) fn owned_root(scope: &TargetScope) -> Option<NodeId> {
    match &scope.construction {
        TargetConstruction::OwnedAdjacencyTree { root, .. } => *root,
        _ => None,
    }
}

/// Validate the owned path-hierarchy composition (owned failure sources,
/// EndingWith, scalar-producer ItemAt and primary-row failures) plus owned
/// adjacency-tree rows. Legacy trees and other tree operators stay closed.
pub fn validate_basic_owned_adjacency_tree_emission(
    program: &Program,
) -> Result<(), ProgramValidationError> {
    validate_basic_owned_with_constructions(
        program,
        BasicOwnedProfile::ByAndAdjacentByAndStartingWithAndEndingWithAndWindowsAndSortAndFilter,
        FailurePolicy::OwnedSources,
        SequenceItemAtPolicy::BasicScalarProducers,
        PathHierarchyPolicy::Owned,
        AdjacencyTreePolicy::Owned,
    )
}
