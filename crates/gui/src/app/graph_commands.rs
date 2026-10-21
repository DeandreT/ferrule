//! Native graph edits recorded as typed commands instead of snapshots.
//!
//! Canvases apply node removals through a [`editor_ui::GraphPatch`] and post
//! them to a per-frame journal; the hosting document records each one with
//! its graph identity. Undo and redo invert or reapply the patch on the
//! observed project and rebuild canvases from the retained layout, which
//! stays outside the project.

use editor_ui::{GraphDocument, GraphEditError, GraphNodeRemoval, HistoryRecord, TargetId};

use super::{CanvasLayout, FerruleApp, MappingDocument};
use crate::graph_viewer::{PlannedNodeRemoval, removal_journal};
use crate::layout_store::project_fingerprint;

/// One command step with the canvas layout of the state it restores.
pub(super) struct GraphCommandRecord {
    removal: GraphNodeRemoval,
    layout: CanvasLayout,
}

impl GraphCommandRecord {
    #[cfg(test)]
    pub(super) fn removal(&self) -> &GraphNodeRemoval {
        &self.removal
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HistoryDirection {
    Undo,
    Redo,
}

impl FerruleApp {
    fn graph_document(&self, document: MappingDocument) -> Option<GraphDocument> {
        match document {
            MappingDocument::Main => Some(GraphDocument::Target(TargetId::Primary)),
            MappingDocument::Target(index) => self
                .project
                .extra_targets
                .get(index)
                .map(|target| GraphDocument::Target(TargetId::Named(target.name.clone()))),
            MappingDocument::Function(function) => Some(GraphDocument::Function(function)),
        }
    }

    /// Records the node removals one canvas applied during this frame.
    pub(super) fn record_canvas_removals(
        &mut self,
        document: MappingDocument,
        context: &egui::Context,
    ) {
        for planned in removal_journal::take(context) {
            self.record_node_removal(document, planned);
        }
    }

    /// Records an applied removal as one command step. The command must lead
    /// from the observed state to the current project; otherwise other
    /// unobserved edits share this frame and the snapshot observer records
    /// them together with the removal. Layout changes from the same frame
    /// belong to the command step.
    pub(super) fn record_node_removal(
        &mut self,
        document: MappingDocument,
        planned: PlannedNodeRemoval,
    ) -> bool {
        let Some(graph_document) = self.graph_document(document) else {
            return false;
        };
        let Ok(removal) = GraphNodeRemoval::new(graph_document, planned.requested, planned.patch)
        else {
            return false;
        };
        let mut before = self.project.clone();
        if removal.revert_on(&mut before).is_err()
            || crate::project_state::project_snapshot_key(&before)
                != self.observed_editor.state.serialized_project
        {
            return false;
        }
        self.commit_pending_history();
        let label = removal.label();
        let current = self.live_snapshot();
        let previous = self.adopt_observed(current);
        self.history.record(
            HistoryRecord::Command(GraphCommandRecord {
                removal,
                layout: previous.state.layout,
            }),
            label,
        );
        true
    }

    /// Inverts or reapplies one command on the observed project. On failure
    /// the record and editor state are unchanged.
    pub(super) fn exchange_graph_command(
        &mut self,
        record: &mut GraphCommandRecord,
        direction: HistoryDirection,
    ) -> Result<(), GraphEditError> {
        let mut project = self.observed_editor.project.clone();
        match direction {
            HistoryDirection::Undo => record.removal.revert_on(&mut project)?,
            HistoryDirection::Redo => record.removal.apply_to(&mut project)?,
        }
        let mut layout = std::mem::replace(
            &mut record.layout,
            self.observed_editor.state.layout.clone(),
        );
        // Positions do not depend on project content, but a save may have
        // rebased paths since the layout was captured.
        layout.project_fingerprint = Some(project_fingerprint(&project));
        self.restore_editor(project, &layout);
        Ok(())
    }
}

#[cfg(test)]
#[path = "graph_commands_tests.rs"]
mod tests;
