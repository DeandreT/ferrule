//! Native graph edits recorded as typed commands instead of snapshots.
//!
//! Canvases apply node removals, wire connections and disconnections,
//! palette insertions, and node property edits through a
//! [`editor_ui::GraphPatch`]; the scope editor describes binding and setting
//! changes of its scope the same way. Inspector actions and canvas menus
//! that change a project in place (child scope construction, joins,
//! filter/map sequences, XML types, and sequence consumer duplication) name
//! their action and derive its patch from the project before the edit.
//! Hosts queue each edit with its graph
//! identity, and the frame observer records the queued commands as one step
//! when together they exactly explain the frame's project change. Repeated
//! edits of one property while typing or dragging extend that step. Undo and
//! redo invert or reapply the patches on the observed project and rebuild
//! canvases from the retained layout, which stays outside the project.

use editor_ui::{
    GraphAction, GraphCommand, GraphDocument, GraphEditError, GraphPatch, HistoryRecord,
    ProjectParts, ScopeFields, ScopeLocation, ScopeSetting, TargetBinding, TargetId,
};
use mapping::{Graph, Node, Project, Scope};

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
/// one per changed binding list or setting. A binding or computed property
/// change that is not a single addition, removal, or replacement forms no
/// command.
pub(super) fn scope_edit_commands(
    document: &GraphDocument,
    location: &ScopeLocation,
    before: &ScopeFields,
    after: &ScopeFields,
) -> Vec<GraphCommand> {
    GraphPatch::of_scope(location, before, after)
        .into_scope_parts()
        .into_iter()
        .filter_map(|patch| {
            let action = match patch.settings().next() {
                Some((scope, ScopeSetting::ComputedProperties, change)) => {
                    computed_property_action(scope, change.before(), change.after())?
                }
                Some((scope, setting, _)) => GraphAction::EditScope {
                    scope: scope.clone(),
                    setting,
                },
                None => binding_action(location, before.bindings(), after.bindings())?,
            };
            GraphCommand::new(document.clone(), action, patch).ok()
        })
        .collect()
}

/// One item appended to, removed from, or replaced in an ordered list.
enum ListChange<T> {
    Add(T),
    Remove(T),
    Replace(T, T),
}

/// Names a list change as the one addition, removal, or replacement an
/// editor row performs; the command validates the exact shape.
fn list_change<T: Clone + PartialEq>(before: &[T], after: &[T]) -> Option<ListChange<T>> {
    let first_difference = before
        .iter()
        .zip(after)
        .position(|(before, after)| before != after);
    Some(match after.len().cmp(&before.len()) {
        std::cmp::Ordering::Greater => ListChange::Add(after.last()?.clone()),
        std::cmp::Ordering::Less => {
            ListChange::Remove(before[first_difference.unwrap_or(after.len())].clone())
        }
        std::cmp::Ordering::Equal => {
            let index = first_difference?;
            ListChange::Replace(before[index].clone(), after[index].clone())
        }
    })
}

fn binding_action(
    scope: &ScopeLocation,
    before: &[mapping::Binding],
    after: &[mapping::Binding],
) -> Option<GraphAction> {
    let scope = scope.clone();
    let targets =
        |bindings: &[mapping::Binding]| bindings.iter().map(TargetBinding::of).collect::<Vec<_>>();
    Some(match list_change(&targets(before), &targets(after))? {
        ListChange::Add(binding) => GraphAction::AddBinding { scope, binding },
        ListChange::Remove(binding) => GraphAction::RemoveBinding { scope, binding },
        ListChange::Replace(from, to) => GraphAction::RetargetBinding { scope, from, to },
    })
}

fn computed_property_action(
    scope: &ScopeLocation,
    before: &editor_ui::ScopeSettingValue,
    after: &editor_ui::ScopeSettingValue,
) -> Option<GraphAction> {
    use editor_ui::ScopeSettingValue::ComputedProperties;
    let (ComputedProperties(before), ComputedProperties(after)) = (before, after) else {
        return None;
    };
    let scope = scope.clone();
    Some(match list_change(before, after)? {
        ListChange::Add(property) => GraphAction::AddComputedProperty { scope, property },
        ListChange::Remove(property) => GraphAction::RemoveComputedProperty { scope, property },
        ListChange::Replace(from, to) => GraphAction::EditComputedProperty { scope, from, to },
    })
}

/// The target scope tree a mapping document edits; function bodies have
/// none.
fn document_root(project: &Project, document: MappingDocument) -> Option<&Scope> {
    match document {
        MappingDocument::Main => Some(&project.root),
        MappingDocument::Target(index) => {
            project.extra_targets.get(index).map(|target| &target.root)
        }
        MappingDocument::Function(_) => None,
    }
}

/// The graph a mapping document edits.
fn document_graph(project: &Project, document: MappingDocument) -> Option<&Graph> {
    match document {
        MappingDocument::Main | MappingDocument::Target(_) => Some(&project.graph),
        MappingDocument::Function(function) => project
            .user_functions
            .get(&function)
            .map(|function| &function.body),
    }
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
        for edit in &edits {
            self.graph_drafts.record_patch(&graph, &edit.patch, true);
        }
        self.frame_commands.extend(
            edits
                .into_iter()
                .filter_map(|edit| GraphCommand::new(graph.clone(), edit.action, edit.patch).ok()),
        );
    }

    /// Queues `action` for an edit made in place since `before` to the
    /// target document `document`. The patch holds the node, binding, and
    /// child-scope differences of the document, with the setting differences
    /// of the scope at `settings_at`. The frame observer still checks that
    /// the queued commands explain the frame, so an edit the patch misses
    /// leaves the frame a snapshot.
    pub(super) fn queue_document_edit(
        &mut self,
        document: MappingDocument,
        before: &Project,
        action: GraphAction,
        settings_at: Option<&[usize]>,
    ) {
        let Some(graph) = self.graph_document(document) else {
            return;
        };
        let (Some(before_root), Some(after_root)) = (
            document_root(before, document),
            document_root(&self.project, document),
        ) else {
            return;
        };
        let Ok(mut patch) = GraphPatch::between_reshaped(
            (&before.graph, before_root),
            (&self.project.graph, after_root),
            same_node,
        ) else {
            return;
        };
        if let Some(path) = settings_at {
            let scopes = (
                crate::auto_connect::scope_at(before_root, path),
                crate::auto_connect::scope_at(after_root, path),
            );
            let (Some(before), Some(after)) = scopes else {
                return;
            };
            let location = ScopeLocation::of_children(path);
            let Ok(with_settings) = patch.with_scope_settings(
                &location,
                &ScopeFields::capture(before),
                &ScopeFields::capture(after),
            ) else {
                return;
            };
            patch = with_settings;
        }
        if let Ok(command) = GraphCommand::new(graph, action, patch) {
            self.graph_drafts
                .record_patch(command.document(), command.patch(), false);
            self.frame_commands.push(command);
        }
    }

    /// Queues `action` for an edit made in place since `before` that changes
    /// only the nodes of `document`'s graph.
    pub(super) fn queue_node_edit(
        &mut self,
        document: MappingDocument,
        before: &Graph,
        action: GraphAction,
    ) {
        let (Some(graph), Some(after)) = (
            self.graph_document(document),
            document_graph(&self.project, document),
        ) else {
            return;
        };
        let patch = GraphPatch::of_nodes(before, after, same_node);
        if let Ok(command) = GraphCommand::new(graph, action, patch) {
            self.graph_drafts
                .record_patch(command.document(), command.patch(), false);
            self.frame_commands.push(command);
        }
    }

    /// Queues `action` for an edit made in place since `before` to the
    /// project parts outside any target scope and to the shared graph.
    pub(super) fn queue_project_edit(&mut self, before: &Project, action: GraphAction) {
        let patch = GraphPatch::of_nodes(&before.graph, &self.project.graph, same_node)
            .with_project_parts(ProjectParts::between(before, &self.project, same_node));
        if let Ok(command) = GraphCommand::new(GraphDocument::Project, action, patch) {
            self.graph_drafts
                .record_patch(command.document(), command.patch(), false);
            self.frame_commands.push(command);
        }
    }

    /// Takes the frame's queued commands and returns them when together, in
    /// order, they lead from the observed state to `current`. Otherwise other
    /// edits made in place share the frame.
    pub(super) fn take_frame_commands(&mut self, current: &Project) -> Option<Vec<GraphCommand>> {
        let commands = std::mem::take(&mut self.frame_commands);
        #[cfg(test)]
        if self.snapshot_oracle {
            return None;
        }
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
        for command in &record.commands {
            self.graph_drafts
                .record_patch(command.document(), command.patch(), false);
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
