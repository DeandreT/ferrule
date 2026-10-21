//! Explicit owned adjacency-tree row collections. The rows are an exact owned
//! read in the receiving context; the optional root expression keeps that
//! parent context. Only the `constructions.adjacency_tree` capability admits the
//! construction.

use super::*;

impl OwnedCapabilities {
    /// Whether an owned tree construction of `scope` is admitted: owned
    /// adjacency-tree rows, or recursive filtering of the scope's own owned
    /// selector. Both are orthogonal to source controls and failure policies.
    pub(super) fn admits_owned_tree_construction(self, scope: &TargetScope) -> bool {
        self.constructions.adjacency_tree && owned_rows(scope).is_some()
            || self.constructions.recursive_filter
                && super::recursive_filter::owned_predicate(scope).is_some()
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
