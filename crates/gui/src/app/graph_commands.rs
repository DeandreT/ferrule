//! Native graph edits recorded as typed commands instead of snapshots.
//!
//! Canvases apply node removals, wire connections and disconnections, and
//! palette insertions through a [`editor_ui::GraphPatch`] and hand them to
//! the hosting document, which queues each one with its graph identity. The
//! frame observer records a queued command when it alone explains the
//! frame's project change. Undo and redo invert or reapply the patch on the
//! observed project and rebuild canvases from the retained layout, which
//! stays outside the project.

use editor_ui::{GraphCommand, GraphDocument, GraphEditError, HistoryRecord, TargetId};
use mapping::{Node, Project};

use super::{CanvasLayout, EditorSnapshot, FerruleApp, MappingDocument};
use crate::graph_viewer::PlannedGraphEdit;
use crate::layout_store::project_fingerprint;
use crate::project_state::{model_key, project_snapshot_key};

/// One command step with the canvas layout of the state it restores.
pub(super) struct GraphCommandRecord {
    command: GraphCommand,
    layout: CanvasLayout,
}

impl GraphCommandRecord {
    #[cfg(test)]
    pub(super) fn command(&self) -> &GraphCommand {
        &self.command
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum HistoryDirection {
    Undo,
    Redo,
}

fn same_node(left: &Node, right: &Node) -> bool {
    model_key(left) == model_key(right)
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

    /// Queues the typed edits one canvas applied during this frame for the
    /// frame observer. An edit whose action does not match its patch is
    /// dropped, so its change can only be observed as a snapshot.
    pub(super) fn queue_canvas_edits(
        &mut self,
        document: MappingDocument,
        edits: Vec<PlannedGraphEdit>,
    ) {
        let Some(graph) = self.graph_document(document) else {
            return;
        };
        self.frame_commands.extend(
            edits
                .into_iter()
                .filter_map(|edit| GraphCommand::new(graph.clone(), edit.action, edit.patch).ok()),
        );
    }

    /// Takes the frame's queued commands and returns the one command that
    /// alone leads from the observed state to `current`, if any. Otherwise
    /// several edits, or other edits made in place, share the frame.
    pub(super) fn take_frame_command(&mut self, current: &Project) -> Option<GraphCommand> {
        let commands = std::mem::take(&mut self.frame_commands);
        let [command] = <[GraphCommand; 1]>::try_from(commands).ok()?;
        let mut before = current.clone();
        (command.revert_on_matching(&mut before, same_node).is_ok()
            && project_snapshot_key(&before) == self.observed_editor.state.serialized_project)
            .then_some(command)
    }

    /// Records `command`, which leads from the observed state to `current`,
    /// as one history step. Layout changes from the same frame belong to it.
    pub(super) fn record_graph_command(&mut self, command: GraphCommand, current: EditorSnapshot) {
        self.commit_pending_history();
        let label = command.label();
        let previous = self.adopt_observed(current);
        self.history.record(
            HistoryRecord::Command(GraphCommandRecord {
                command,
                layout: previous.state.layout,
            }),
            label,
        );
    }

    /// Drops queued commands that no longer describe the observed state.
    pub(super) fn discard_frame_commands(&mut self) {
        self.frame_commands.clear();
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
            HistoryDirection::Undo => record.command.revert_on_matching(&mut project, same_node)?,
            HistoryDirection::Redo => record.command.apply_to_matching(&mut project, same_node)?,
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
