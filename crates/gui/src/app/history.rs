//! Editor history, save points, and semantic and layout revisions.
//!
//! In-place edits are observed after each frame and retained as snapshots;
//! migrated edit families record typed commands in the same linear history.
//! Revisions are classified against the save point, so per-frame dirty
//! displays read them instead of keying the project.

use editor_ui::{ComponentChange, DocumentOrigin, HistoryRecord, RevisionChange, SemanticRevision};
use mapping::Project;

use super::{
    CanvasDocumentState, CanvasLayout, EditorSnapshot, EditorState, FerruleApp,
    HISTORY_COALESCE_DELAY, MappingWorkspace, PendingHistory, build_snarl_with_layout,
    graph_commands,
};
use crate::layout_store::fingerprint_of_key;

impl FerruleApp {
    /// Exact dirty state of the live project and layout. Decisions such as
    /// unsaved-change guards use this; it keys the project once.
    pub(super) fn is_dirty(&self) -> bool {
        self.saved_state.as_ref() != Some(&self.live_state())
    }

    /// Dirty state as of the latest observed transition, read from revisions
    /// without keying the project. Per-frame displays use this.
    pub(super) fn observed_dirty(&self) -> bool {
        self.revisions.is_dirty()
    }

    /// The live editor state. The project is keyed once; its fingerprint is
    /// reused from the observed semantic revision when the key is unchanged.
    pub(super) fn live_state(&self) -> EditorState {
        let serialized_project = crate::project_state::project_snapshot_key(&self.project);
        let fingerprint = self
            .fingerprints
            .get(self.revisions.current().semantic())
            .filter(|_| serialized_project == self.observed_editor.state.serialized_project)
            .cloned()
            .unwrap_or_else(|| fingerprint_of_key(&serialized_project));
        EditorState {
            layout: CanvasLayout::capture_with_fingerprint(
                &self.project,
                fingerprint,
                &self.main_canvas.snarl,
                &self.mapping_workspace,
            ),
            serialized_project,
        }
    }

    pub(super) fn live_snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            project: self.project.clone(),
            state: self.live_state(),
        }
    }

    /// Replaces the observed state, advancing revisions by content, and
    /// returns the previous observation.
    pub(super) fn adopt_observed(&mut self, next: EditorSnapshot) -> EditorSnapshot {
        let previous = std::mem::replace(&mut self.observed_editor, next);
        self.record_observed_revision(&previous.state);
        previous
    }

    /// Classifies the observed state against `previous` and the save point.
    /// The layout component ignores the fingerprint, which follows content.
    pub(super) fn record_observed_revision(&mut self, previous: &EditorState) {
        let current = &self.observed_editor.state;
        let saved = self.saved_state.as_ref();
        let layout = if current.layout.same_view(&previous.layout) {
            ComponentChange::Unchanged
        } else if saved.is_some_and(|saved| current.layout.same_view(&saved.layout)) {
            ComponentChange::MatchesSaved
        } else {
            ComponentChange::Changed
        };
        let revision = self.revisions.record(RevisionChange {
            semantic: ComponentChange::classify(
                current.serialized_project.as_str(),
                previous.serialized_project.as_str(),
                saved.map(|saved| saved.serialized_project.as_str()),
            ),
            layout,
        });
        self.cache_observed_fingerprint(revision.semantic());
    }

    pub(super) fn cache_observed_fingerprint(&mut self, revision: SemanticRevision) {
        let state = &self.observed_editor.state;
        self.fingerprints.get_or_compute(revision, || {
            state
                .layout
                .project_fingerprint
                .clone()
                .unwrap_or_else(|| fingerprint_of_key(&state.serialized_project))
        });
    }

    pub(super) fn mark_clean(&mut self) {
        let live = self.live_state();
        let observed = &self.observed_editor.state;
        let semantic_differs = live.serialized_project != observed.serialized_project;
        let layout_differs = !live.layout.same_view(&observed.layout);
        if semantic_differs || layout_differs {
            self.revisions
                .mark_saved_ahead(semantic_differs, layout_differs);
        } else {
            self.revisions.mark_saved();
        }
        self.saved_state = Some(live);
    }

    /// Makes `snapshot` the clean, history-free baseline.
    #[cfg(test)]
    pub(super) fn reset_history_to(&mut self, snapshot: EditorSnapshot) {
        self.history = super::EditorHistory::default();
        self.saved_state = Some(snapshot.state.clone());
        self.revisions.rebase(DocumentOrigin::Saved);
        self.fingerprints.clear();
        self.observed_editor = snapshot;
        self.cache_observed_fingerprint(self.revisions.current().semantic());
        self.pending_history = None;
    }

    pub(super) fn mark_unsaved(&mut self) {
        self.saved_state = None;
        self.revisions.mark_unsaved();
    }

    pub(super) fn rebase_history(&mut self) {
        self.clear_diagnostic_navigation();
        let snapshot = self.live_snapshot();
        let origin = if self.saved_state.as_ref() == Some(&snapshot.state) {
            DocumentOrigin::Saved
        } else {
            DocumentOrigin::Unsaved
        };
        self.history.clear();
        self.discard_frame_commands();
        self.command_coalescing = None;
        self.saved_state = (origin == DocumentOrigin::Saved).then(|| snapshot.state.clone());
        self.revisions.rebase(origin);
        self.fingerprints.clear();
        self.observed_editor = snapshot;
        self.cache_observed_fingerprint(self.revisions.current().semantic());
        self.pending_history = None;
    }

    pub(super) fn commit_pending_history(&mut self) {
        if let Some(pending) = self.pending_history.take()
            && pending.before != self.observed_editor
        {
            let label = snapshot_label(&pending.before.state, &self.observed_editor.state);
            self.history
                .record(HistoryRecord::Snapshot(pending.before), label);
        }
    }

    /// Observes project and canvas-layout mutations after the frame has rendered. Changes
    /// close together form one user-level undo transaction instead of one
    /// entry per keystroke/frame. Edits that still mutate the project in place
    /// are detected here, which keys the project once per observation; a
    /// frame's queued command is recorded here as well.
    pub(super) fn observe_editor_history(
        &mut self,
        now: std::time::Instant,
        coalesce_change: bool,
    ) -> Option<std::time::Duration> {
        self.reconcile_graph_drafts();
        let current_state = self.live_state();
        if current_state != self.observed_editor.state {
            let current = EditorSnapshot {
                project: self.project.clone(),
                state: current_state,
            };
            // One more project key proves that the queued commands alone
            // explain this frame's change.
            if let Some(commands) = self.take_frame_commands(&current.project) {
                self.record_graph_commands(commands, current, now, coalesce_change);
                return None;
            }
            self.command_coalescing = None;
            if coalesce_change {
                if self.pending_history.as_ref().is_some_and(|pending| {
                    now.saturating_duration_since(pending.last_change) >= HISTORY_COALESCE_DELAY
                }) {
                    self.commit_pending_history();
                }
                let previous = self.adopt_observed(current);
                match &mut self.pending_history {
                    Some(pending) => pending.last_change = now,
                    None => {
                        self.pending_history = Some(PendingHistory {
                            before: previous,
                            last_change: now,
                        });
                        self.history.clear_redo();
                    }
                }
            } else {
                self.commit_pending_history();
                let previous = self.adopt_observed(current);
                let label = snapshot_label(&previous.state, &self.observed_editor.state);
                self.history
                    .record(HistoryRecord::Snapshot(previous), label);
                return None;
            }
        } else {
            // Keep the current navigation for the next edit's undo context,
            // without treating a tab visit as a document mutation.
            self.observed_editor.state.layout = current_state.layout;
        }
        self.discard_frame_commands();

        let pending = self.pending_history.as_ref()?;
        let elapsed = now.saturating_duration_since(pending.last_change);
        if elapsed >= HISTORY_COALESCE_DELAY {
            self.commit_pending_history();
            None
        } else {
            Some(HISTORY_COALESCE_DELAY - elapsed)
        }
    }

    pub(super) fn can_undo(&self) -> bool {
        self.pending_history
            .as_ref()
            .is_some_and(|pending| pending.before != self.observed_editor)
            || self.history.can_undo()
    }

    pub(super) fn restore_history_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.restore_editor(snapshot.project, &snapshot.state.layout);
    }

    pub(super) fn restore_editor(&mut self, project: Project, layout: &CanvasLayout) {
        self.clear_diagnostic_navigation();
        self.project = project;
        self.reconcile_graph_drafts();
        self.mapping_workspace = MappingWorkspace::from_layout(&self.project, Some(layout));
        self.main_canvas =
            CanvasDocumentState::with_snarl(build_snarl_with_layout(&self.project, Some(layout)));
        self.selected_scope.clear();
        self.discard_frame_commands();
        self.command_coalescing = None;
        // Rebuilding can materialize graph nodes in previously unvisited canvases.
        // That layout change belongs to this restore, not a new undo transaction.
        let restored = self.live_snapshot();
        self.adopt_observed(restored);
        self.pending_history = None;
    }

    pub(super) fn exchange_history_record(
        &mut self,
        record: &mut HistoryRecord<EditorSnapshot, graph_commands::GraphCommandRecord>,
        direction: graph_commands::HistoryDirection,
    ) -> Result<(), editor_ui::GraphEditError> {
        match record {
            HistoryRecord::Snapshot(snapshot) => {
                let target = std::mem::replace(snapshot, self.observed_editor.clone());
                self.restore_history_snapshot(target);
                Ok(())
            }
            HistoryRecord::Command(command) => self.exchange_graph_command(command, direction),
        }
    }

    pub(super) fn undo_project(&mut self) {
        self.commit_pending_history();
        let mut history = std::mem::take(&mut self.history);
        let result = history.undo(|record| {
            self.exchange_history_record(record, graph_commands::HistoryDirection::Undo)
        });
        self.history = history;
        self.finish_history_step(result, "undid", "Undo");
    }

    pub(super) fn redo_project(&mut self) {
        let mut history = std::mem::take(&mut self.history);
        let result = history.redo(|record| {
            self.exchange_history_record(record, graph_commands::HistoryDirection::Redo)
        });
        self.history = history;
        self.finish_history_step(result, "redid", "Redo");
    }

    pub(super) fn finish_history_step(
        &mut self,
        result: Result<Option<String>, editor_ui::GraphEditError>,
        verb: &str,
        action: &str,
    ) {
        match result {
            Ok(Some(label)) => self.status = format!("{verb} {label}"),
            Ok(None) => {}
            Err(error) => {
                self.status = format!("{action} failed");
                self.diagnostics
                    .error(format!("{action} failed"), error.to_string());
            }
        }
    }
}

/// Undo label of a snapshot step: canvas-only changes read as layout edits.
fn snapshot_label(before: &EditorState, after: &EditorState) -> &'static str {
    if before.serialized_project == after.serialized_project {
        "Edit layout"
    } else {
        "Edit mapping"
    }
}
