use std::collections::BTreeSet;

use mapping::NodeId;

/// Identifies one input boundary without relying on its list position.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceId {
    Primary,
    Named(String),
}

/// Identifies one output boundary without relying on its list position.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TargetId {
    Primary,
    Named(String),
}

/// Identifies either side of a schema-oriented workspace selection.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BoundaryId {
    Source(SourceId),
    Target(TargetId),
}

/// A non-empty graph selection with one stable primary node.
///
/// The primary node is the inspector owner. Additional nodes are kept sorted
/// for deterministic batch commands and cannot duplicate the primary node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GraphNodeSelection {
    primary: NodeId,
    additional: BTreeSet<NodeId>,
}

impl GraphNodeSelection {
    pub fn new(primary: NodeId) -> Self {
        Self {
            primary,
            additional: BTreeSet::new(),
        }
    }

    pub fn with_additional(primary: NodeId, additional: impl IntoIterator<Item = NodeId>) -> Self {
        Self {
            primary,
            additional: additional
                .into_iter()
                .filter(|node| *node != primary)
                .collect(),
        }
    }

    pub fn primary(&self) -> NodeId {
        self.primary
    }

    pub fn additional(&self) -> impl Iterator<Item = NodeId> + '_ {
        self.additional.iter().copied()
    }

    pub fn nodes(&self) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::once(self.primary).chain(self.additional())
    }

    pub fn node_count(&self) -> usize {
        1 + self.additional.len()
    }
}

/// The semantic object currently owned by the workspace inspector.
///
/// Selection is intentionally outside the persisted mapping project and undo
/// history. Indexes are used only where the mapping IR has no stable identity.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum WorkspaceSelection {
    #[default]
    None,
    Project,
    Boundary(BoundaryId),
    SchemaNode {
        boundary: BoundaryId,
        path: Vec<String>,
    },
    Scope {
        target: TargetId,
        path: Vec<usize>,
    },
    Binding {
        target: TargetId,
        scope_path: Vec<usize>,
        index: usize,
    },
    FailureRule {
        index: usize,
    },
    GraphNodes(GraphNodeSelection),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graph_selection_is_non_empty_deduplicated_and_deterministic() {
        let selection = GraphNodeSelection::with_additional(5, [8, 3, 5, 8]);

        assert_eq!(selection.primary(), 5);
        assert_eq!(selection.nodes().collect::<Vec<_>>(), vec![5, 3, 8]);
        assert_eq!(selection.node_count(), 3);
    }
}
