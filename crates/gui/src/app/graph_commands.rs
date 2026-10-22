//! Native graph edits recorded as typed commands instead of snapshots.
//!
//! Canvases apply node removals, wire connections and disconnections,
//! palette insertions, and node property edits through a
//! [`editor_ui::GraphPatch`]; the scope editor describes binding and setting
//! changes of its scope the same way. Hosts queue each edit with its graph
//! identity, and the frame observer records the queued commands as one step
//! when together they exactly explain the frame's project change. Repeated
//! edits of one property while typing or dragging extend that step. Undo and
//! redo invert or reapply the patches on the observed project and rebuild
//! canvases from the retained layout, which stays outside the project.

use editor_ui::{
    GraphCommand, GraphDocument, GraphEditError, HistoryRecord, ScopeFields, ScopeLocation,
    TargetBinding, TargetId,
};
use mapping::{Node, Project};

use super::{CanvasLayout, EditorSnapshot, FerruleApp, HISTORY_COALESCE_DELAY, MappingDocument};
use crate::graph_viewer::PlannedGraphEdit;
use crate::layout_store::project_fingerprint;
use crate::project_state::{model_key, project_snapshot_key};

/// One history step of one or more commands, applied in order, with the
/// canvas layout of the state it restores.
pub(super) struct GraphCommandRecord {
    commands: Vec<GraphCommand>,
    layout: CanvasLayout,
}

impl GraphCommandRecord {
    #[cfg(test)]
    pub(super) fn commands(&self) -> &[GraphCommand] {
        &self.commands
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

/// The history label of a step: the shared action label, or a count of
/// the step's distinct edits.
fn step_label(commands: &[GraphCommand]) -> String {
    match commands {
        [first, rest @ ..] if rest.iter().all(|command| command.label() == first.label()) => {
            first.label().to_owned()
        }
        commands => format!("{} edits", commands.len()),
    }
}

/// Typed commands for one scope editor pass over the scope at `location`:
/// one per changed binding list or setting. A binding change that is not a
/// single addition, removal, or retarget forms no command.
pub(super) fn scope_edit_commands(
    document: &GraphDocument,
    location: &ScopeLocation,
    before: &ScopeFields,
    after: &ScopeFields,
) -> Vec<GraphCommand> {
    editor_ui::GraphPatch::of_scope(location, before, after)
        .into_scope_parts()
        .into_iter()
        .filter_map(|patch| {
            let action = match patch.settings().next() {
                Some((scope, setting, _)) => editor_ui::GraphAction::EditScope {
                    scope: scope.clone(),
                    setting,
                },
                None => binding_action(location, before.bindings(), after.bindings())?,
            };
            GraphCommand::new(document.clone(), action, patch).ok()
        })
        .collect()
}

/// Names a binding list change as the one addition, removal, or retarget
/// the binding panel performs; the command validates the exact shape.
fn binding_action(
    scope: &ScopeLocation,
    before: &[mapping::Binding],
    after: &[mapping::Binding],
) -> Option<editor_ui::GraphAction> {
    let scope = scope.clone();
    let differs = |left: &mapping::Binding, right: &mapping::Binding| {
        TargetBinding::of(left) != TargetBinding::of(right)
    };
    let first_difference = before
        .iter()
        .zip(after)
        .position(|(before, after)| differs(before, after));
    Some(match after.len().cmp(&before.len()) {
        std::cmp::Ordering::Greater => editor_ui::GraphAction::AddBinding {
            scope,
            binding: TargetBinding::of(after.last()?),
        },
        std::cmp::Ordering::Less => editor_ui::GraphAction::RemoveBinding {
            scope,
            binding: TargetBinding::of(&before[first_difference.unwrap_or(after.len())]),
        },
        std::cmp::Ordering::Equal => {
            let index = first_difference?;
            editor_ui::GraphAction::RetargetBinding {
                scope,
                from: TargetBinding::of(&before[index]),
                to: TargetBinding::of(&after[index]),
            }
        }
    })
}

impl FerruleApp {
    pub(super) fn graph_document(&self, document: MappingDocument) -> Option<GraphDocument> {
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

    /// Takes the frame's queued commands and returns them when together, in
    /// order, they lead from the observed state to `current`. Otherwise other
    /// edits made in place share the frame.
    pub(super) fn take_frame_commands(&mut self, current: &Project) -> Option<Vec<GraphCommand>> {
        let commands = std::mem::take(&mut self.frame_commands);
        if commands.is_empty() {
            return None;
        }
        let mut before = current.clone();
        let reverted = commands
            .iter()
            .rev()
            .all(|command| command.revert_on_matching(&mut before, same_node).is_ok());
        (reverted && project_snapshot_key(&before) == self.observed_editor.state.serialized_project)
            .then_some(commands)
    }

    /// Records `commands`, which lead from the observed state to `current`,
    /// as one history step. Layout changes from the same frame belong to it.
    /// While `coalesce` input continues, one repeated coalescing edit extends
    /// the newest step instead.
    pub(super) fn record_graph_commands(
        &mut self,
        commands: Vec<GraphCommand>,
        current: EditorSnapshot,
        now: std::time::Instant,
        coalesce: bool,
    ) {
        self.commit_pending_history();
        let continues = coalesce
            && self
                .command_coalescing
                .is_some_and(|last| now.saturating_duration_since(last) < HISTORY_COALESCE_DELAY);
        if continues
            && let [command] = &commands[..]
            && self.extend_newest_command(command)
        {
            self.adopt_observed(current);
            self.command_coalescing = Some(now);
            return;
        }
        self.command_coalescing = (coalesce
            && matches!(&commands[..], [command] if command.action().coalesces()))
        .then_some(now);
        let label = step_label(&commands);
        let previous = self.adopt_observed(current);
        self.history.record(
            HistoryRecord::Command(GraphCommandRecord {
                commands,
                layout: previous.state.layout,
            }),
            label,
        );
    }

    /// Folds `next` into the newest step when that step is one command
    /// performing the same coalescing action, returning whether it did.
    fn extend_newest_command(&mut self, next: &GraphCommand) -> bool {
        let combined = match self.history.next_undo() {
            Some(HistoryRecord::Command(record)) => match &record.commands[..] {
                [previous] => previous.then(next, same_node),
                _ => None,
            },
            _ => None,
        };
        let Some(combined) = combined else {
            return false;
        };
        if let Some(HistoryRecord::Command(record)) = self.history.newest_undo_mut() {
            record.commands = vec![combined];
        }
        true
    }

    /// Drops queued commands that no longer describe the observed state.
    pub(super) fn discard_frame_commands(&mut self) {
        self.frame_commands.clear();
    }

    /// Inverts or reapplies one step's commands on the observed project. On
    /// failure the record and editor state are unchanged.
    pub(super) fn exchange_graph_command(
        &mut self,
        record: &mut GraphCommandRecord,
        direction: HistoryDirection,
    ) -> Result<(), GraphEditError> {
        let mut project = self.observed_editor.project.clone();
        match direction {
            HistoryDirection::Undo => record
                .commands
                .iter()
                .rev()
                .try_for_each(|command| command.revert_on_matching(&mut project, same_node))?,
            HistoryDirection::Redo => record
                .commands
                .iter()
                .try_for_each(|command| command.apply_to_matching(&mut project, same_node))?,
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
