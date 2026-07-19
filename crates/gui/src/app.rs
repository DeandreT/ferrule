//! The ferrule-gui app: schema panes, scope editor, and mapping canvas.
//!
//! The canvas carries the visual-mapper interaction: the Source and Target
//! schemas are endpoint nodes whose pins are their scalar leaves, so a
//! mapping is wired leaf-to-function-to-leaf. Scope iteration (which
//! repeating path each scope loops over) is still edited in the side
//! panel -- connecting wires never changes iteration, only values.

use std::path::PathBuf;

use editor_ui::{DocumentOrigin, SnapshotHistory};
use egui_snarl::{InPinId, OutPinId, Snarl};
use mapping::{Graph, Node, NodeId, Project, Scope};
use serde::{Deserialize, Serialize};

use crate::appearance::EditorAppearance;
use crate::appearance_editor::AppearanceTab;
use crate::canvas::{CanvasNode, SourceLeaf, TargetLeaf, source_leaves, target_leaves};
use crate::canvas_layout::arrange_snarl;
use crate::diagnostics::{Diagnostic, DiagnosticLevel, Diagnostics};
use crate::document::DocumentLocation;
use crate::extra_sources::{ExtraSourceDraft, remove_extra_source};
use crate::graph_viewer::GraphViewer;
use crate::layout_store::{project_fingerprint, read_layout, write_layout};
use crate::new_mapping::{NewMappingSetup, SchemaSide, blank_project};
use crate::path_picker::SourcePathCatalog;
use crate::schema_tree::{SchemaExplorerState, schema_field_count, show_schema_tree};
use crate::scope_editor::{
    ScopePath, available_static_child_scopes, binding_target_fields, create_static_child_scope,
    remove_child_scope, scope_at_mut, scope_target_chain, show_scope_editor, show_scope_tree,
};
use crate::theme::{Palette, ThemeState};
use crate::workspace_layout::{LayoutClass, SideDock, WorkspacePane, WorkspaceVisibility};

#[path = "app_extra_sources.rs"]
mod extra_source_ui;
#[path = "app_new_mapping.rs"]
mod new_mapping_ui;
#[path = "app_scopes.rs"]
mod scope_ui;
#[path = "app_workspace.rs"]
mod workspace_ui;

const HISTORY_COALESCE_DELAY: std::time::Duration = std::time::Duration::from_millis(400);
pub(super) const LAYOUT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct CanvasLayout {
    pub(super) version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    project_fingerprint: Option<String>,
    nodes: Vec<CanvasNodeLayout>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CanvasNodeLayout {
    node: PersistedCanvasNode,
    x: f32,
    y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PersistedCanvasNode {
    Source,
    Target,
    Graph { id: NodeId },
    Placeholder { id: NodeId },
}

impl From<CanvasNode> for PersistedCanvasNode {
    fn from(node: CanvasNode) -> Self {
        match node {
            CanvasNode::Source => Self::Source,
            CanvasNode::Target => Self::Target,
            CanvasNode::Graph(id) => Self::Graph { id },
            CanvasNode::Placeholder(id) => Self::Placeholder { id },
        }
    }
}

#[derive(Clone)]
struct EditorSnapshot {
    project: Project,
    state: EditorState,
}

#[derive(Clone, PartialEq)]
struct EditorState {
    serialized_project: String,
    layout: CanvasLayout,
}

impl PartialEq for EditorSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.state == other.state
    }
}

pub struct FerruleApp {
    project: Project,
    snarl: Snarl<CanvasNode>,
    canvas_node_sizes: std::collections::BTreeMap<CanvasNode, egui::Vec2>,
    canvas_view_generation: u64,
    show_source_panel: bool,
    show_inspector_panel: bool,
    compact_dock_open: bool,
    compact_dock: SideDock,
    narrow_pane: WorkspacePane,
    last_layout_class: Option<LayoutClass>,
    show_run_setup: bool,
    show_appearance_editor: bool,
    appearance_tab: AppearanceTab,
    theme: ThemeState,
    appearance: EditorAppearance,
    palette: Palette,
    document: DocumentLocation,
    input_path: String,
    output_path: String,
    source_schema_explorer: SchemaExplorerState,
    target_schema_explorer: SchemaExplorerState,
    selected_scope: ScopePath,
    status: String,
    diagnostics: Diagnostics,
    new_mapping_setup: Option<NewMappingSetup>,
    extra_source_draft: Option<ExtraSourceDraft>,
    pending_extra_source_removal: Option<usize>,
    /// Native file dialog receiver; the dialog runs outside the UI thread.
    pending_dialog: Option<(DialogKind, std::sync::mpsc::Receiver<Option<String>>)>,
    pending_destructive_action: Option<DestructiveAction>,
    pending_save_continuation: Option<SaveContinuation>,
    allow_close: bool,
    observed_editor: EditorSnapshot,
    history: SnapshotHistory<EditorSnapshot>,
    pending_history: Option<PendingHistory>,
}

struct PendingHistory {
    before: EditorSnapshot,
    last_change: std::time::Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DialogKind {
    OpenProject,
    SaveProjectAs,
    BrowseInput,
    BrowseOutput,
    ImportMfd,
    ExportMfd,
    BrowseSourceSchema,
    BrowseTargetSchema,
    BrowseExtraSourceSchema,
    BrowseExtraSourceInstance,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DestructiveAction {
    OpenProject,
    NewProject,
    ImportMfd,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SaveContinuation {
    Destructive(DestructiveAction),
    Run,
}

impl Default for FerruleApp {
    fn default() -> Self {
        let project = blank_project();
        let snarl = build_snarl(&project);
        let snapshot = editor_snapshot(&project, &snarl);
        let history = SnapshotHistory::new(snapshot.clone(), DocumentOrigin::Saved);
        Self {
            project,
            snarl,
            canvas_node_sizes: std::collections::BTreeMap::new(),
            canvas_view_generation: 0,
            show_source_panel: true,
            show_inspector_panel: true,
            compact_dock_open: true,
            compact_dock: SideDock::Inspector,
            narrow_pane: WorkspacePane::Canvas,
            last_layout_class: None,
            show_run_setup: false,
            show_appearance_editor: false,
            appearance_tab: AppearanceTab::default(),
            theme: ThemeState::default(),
            appearance: EditorAppearance::default(),
            palette: crate::theme::palette(crate::theme::ResolvedTheme::Dark),
            document: DocumentLocation::untitled("project.json"),
            input_path: String::new(),
            output_path: String::new(),
            source_schema_explorer: SchemaExplorerState::default(),
            target_schema_explorer: SchemaExplorerState::default(),
            selected_scope: Vec::new(),
            status: String::new(),
            diagnostics: Diagnostics::default(),
            new_mapping_setup: None,
            extra_source_draft: None,
            pending_extra_source_removal: None,
            pending_dialog: None,
            pending_destructive_action: None,
            pending_save_continuation: None,
            allow_close: false,
            observed_editor: snapshot,
            history,
            pending_history: None,
        }
    }
}

fn editor_snapshot(project: &Project, snarl: &Snarl<CanvasNode>) -> EditorSnapshot {
    EditorSnapshot {
        project: project.clone(),
        state: editor_state(project, snarl),
    }
}

fn editor_state(project: &Project, snarl: &Snarl<CanvasNode>) -> EditorState {
    EditorState {
        serialized_project: serde_json::to_string(project)
            .expect("Project serialization cannot fail"),
        layout: CanvasLayout::capture(project, snarl),
    }
}

impl CanvasLayout {
    fn capture(project: &Project, snarl: &Snarl<CanvasNode>) -> Self {
        let mut nodes: Vec<_> = snarl
            .nodes_pos()
            .map(|(pos, &node)| CanvasNodeLayout {
                node: node.into(),
                x: pos.x,
                y: pos.y,
            })
            .collect();
        nodes.sort_by_key(|entry| entry.node);
        Self {
            version: LAYOUT_VERSION,
            project_fingerprint: Some(project_fingerprint(project)),
            nodes,
        }
    }

    fn apply(&self, snarl: &mut Snarl<CanvasNode>) {
        if self.version != LAYOUT_VERSION {
            return;
        }
        let positions: std::collections::BTreeMap<_, _> = self
            .nodes
            .iter()
            .filter(|entry| entry.x.is_finite() && entry.y.is_finite())
            .map(|entry| (entry.node, egui::pos2(entry.x, entry.y)))
            .collect();
        let node_ids: Vec<_> = snarl
            .node_ids()
            .map(|(id, &node)| (id, PersistedCanvasNode::from(node)))
            .collect();
        for (id, node) in node_ids {
            if let Some(&pos) = positions.get(&node)
                && let Some(info) = snarl.get_node_info_mut(id)
            {
                info.pos = pos;
            }
        }
    }

    fn matches_project(&self, project: &Project) -> bool {
        self.version == LAYOUT_VERSION
            && self.project_fingerprint.as_deref() == Some(project_fingerprint(project).as_str())
    }
}

fn node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::SourceField { .. }
        | Node::SourceDocumentPath
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. } => vec![],
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } | Node::Lookup { matches: input, .. } => vec![*input],
        Node::DynamicSourceField { key, .. } => vec![*key],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| replacement.expression)
            .collect(),
        Node::CollectionFind {
            predicate, value, ..
        } => vec![*predicate, *value],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence.inputs().into_iter().chain([*predicate]).collect(),
        Node::SequenceItemAt { sequence, index } => {
            sequence.inputs().into_iter().chain([*index]).collect()
        }
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

/// Collects `(node, target-leaf-index)` for every binding, walking the
/// scope tree with its target_field chain.
fn walk_scopes(
    scope: &Scope,
    chain: &mut Vec<String>,
    target_pins: &[TargetLeaf],
    out: &mut Vec<(NodeId, usize)>,
) {
    for binding in &scope.bindings {
        if let Some(leaf) = target_pins
            .iter()
            .position(|l| l.chain == *chain && l.field == binding.target_field)
        {
            out.push((binding.node, leaf));
        }
    }
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            walk_scopes(segment, chain, target_pins, out);
        }
    }
    for child in &scope.children {
        chain.push(child.target_field.clone());
        walk_scopes(child, chain, target_pins, out);
        chain.pop();
    }
}

/// Rebuilds the canvas from a project: Source/Target endpoints at the
/// edges, the graph's nodes in a grid between them, and wires recreated
/// from node inputs and scope bindings. `SourceField` nodes whose path
/// matches a source leaf are hidden -- wires leave the Source endpoint's
/// pin directly.
fn build_snarl(project: &Project) -> Snarl<CanvasNode> {
    build_snarl_with_layout(project, None)
}

fn build_snarl_with_layout(
    project: &Project,
    saved_layout: Option<&CanvasLayout>,
) -> Snarl<CanvasNode> {
    let saved_layout = saved_layout.filter(|layout| layout.matches_project(project));
    let source_pins = source_leaves(&project.source);
    let target_pins = target_leaves(&project.target);

    let mut snarl = Snarl::new();
    let source_node = snarl.insert_node(egui::pos2(0.0, 0.0), CanvasNode::Source);

    // A SourceField is hidden when a source leaf carries its exact frame and
    // relative path. The frame distinguishes equal leaf paths in siblings.
    let leaf_for_field = |frame: &Option<Vec<String>>, path: &[String]| {
        let exact = source_pins
            .iter()
            .position(|leaf| &leaf.frame == frame && leaf.path == path);
        if exact.is_some() || frame.is_some() {
            return exact;
        }
        // Older GUI projects stored repeating source fields without a
        // frame. Preserve their endpoint wire only when the suffix is
        // unique; ambiguous fields stay visible instead of being miswired.
        let mut legacy_matches = source_pins
            .iter()
            .enumerate()
            .filter(|(_, leaf)| leaf.path == path)
            .map(|(index, _)| index);
        let first = legacy_matches.next()?;
        legacy_matches.next().is_none().then_some(first)
    };
    let hidden: std::collections::BTreeMap<NodeId, usize> = project
        .graph
        .nodes
        .iter()
        .filter_map(|(&id, node)| match node {
            Node::SourceField { path, frame } => leaf_for_field(frame, path).map(|leaf| (id, leaf)),
            _ => None,
        })
        .collect();

    // Binding order drives row placement: nodes sit near the target pins
    // they feed. Collected up front; also reused for the binding wires.
    let mut binding_order = Vec::new();
    walk_scopes(
        &project.root,
        &mut Vec::new(),
        &target_pins,
        &mut binding_order,
    );
    let placeholders: std::collections::BTreeSet<NodeId> = saved_layout
        .into_iter()
        .flat_map(|layout| &layout.nodes)
        .filter_map(|entry| match entry.node {
            PersistedCanvasNode::Placeholder { id }
                if matches!(
                    project.graph.nodes.get(&id),
                    Some(Node::Const {
                        value: ir::Value::Null
                    })
                ) =>
            {
                Some(id)
            }
            _ => None,
        })
        .collect();
    let mut snarl_ids = std::collections::BTreeMap::new();
    for &id in project
        .graph
        .nodes
        .keys()
        .filter(|id| !hidden.contains_key(id))
    {
        let snarl_id = snarl.insert_node(
            egui::Pos2::ZERO,
            if placeholders.contains(&id) {
                CanvasNode::Placeholder(id)
            } else {
                CanvasNode::Graph(id)
            },
        );
        snarl_ids.insert(id, snarl_id);
    }
    let target_node = snarl.insert_node(egui::Pos2::ZERO, CanvasNode::Target);

    // The producing pin for a mapping node: the Source endpoint's leaf pin
    // for hidden SourceFields, the node's own output otherwise.
    let out_pin_for = |id: NodeId| -> Option<OutPinId> {
        if let Some(&leaf) = hidden.get(&id) {
            Some(OutPinId {
                node: source_node,
                output: leaf,
            })
        } else {
            snarl_ids.get(&id).map(|&node| OutPinId { node, output: 0 })
        }
    };

    for (&id, node) in &project.graph.nodes {
        let Some(&to_node) = snarl_ids.get(&id) else {
            continue;
        };
        for (input, arg) in node_inputs(node).iter().enumerate() {
            if let Some(from) = out_pin_for(*arg) {
                snarl.connect(
                    from,
                    InPinId {
                        node: to_node,
                        input,
                    },
                );
            }
        }
    }

    for &(node_id, leaf) in &binding_order {
        if let Some(from) = out_pin_for(node_id) {
            snarl.connect(
                from,
                InPinId {
                    node: target_node,
                    input: leaf,
                },
            );
        }
    }

    if let Some(layout) = saved_layout {
        layout.apply(&mut snarl);
    } else {
        let source_width = source_pins
            .iter()
            .map(|leaf| leaf.label.chars().count())
            .max()
            .map_or(320.0, |length| (length as f32 * 9.0 + 180.0).max(320.0));
        let initial_sizes = std::collections::BTreeMap::from([(
            CanvasNode::Source,
            egui::vec2(source_width, 160.0),
        )]);
        arrange_snarl(
            &mut snarl,
            &initial_sizes,
            crate::appearance::WireAppearance::default(),
        );
    }
    snarl
}

/// Runs a native open dialog on its own thread; the result arrives through
/// the returned channel (never blocking the UI, even with no dialog
/// backend available).
fn pick_file(description: &str, extensions: &[&str]) -> std::sync::mpsc::Receiver<Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let description = description.to_string();
    let extensions: Vec<String> = extensions.iter().map(|e| e.to_string()).collect();
    std::thread::spawn(move || {
        let result = rfd::FileDialog::new()
            .add_filter(description, &extensions)
            .pick_file()
            .map(|p| p.display().to_string());
        let _ = tx.send(result);
    });
    rx
}

/// Threaded native save dialog, pre-filled from `current`.
fn save_file(
    description: &str,
    extensions: &[&str],
    current: &str,
) -> std::sync::mpsc::Receiver<Option<String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    let description = description.to_string();
    let extensions: Vec<String> = extensions.iter().map(|e| e.to_string()).collect();
    let current = std::path::PathBuf::from(current);
    std::thread::spawn(move || {
        let mut dialog = rfd::FileDialog::new().add_filter(description, &extensions);
        if let Some(dir) = current.parent().filter(|d| d.is_dir()) {
            dialog = dialog.set_directory(dir);
        }
        if let Some(name) = current.file_name().and_then(|n| n.to_str()) {
            dialog = dialog.set_file_name(name);
        }
        let _ = tx.send(dialog.save_file().map(|p| p.display().to_string()));
    });
    rx
}

impl FerruleApp {
    pub fn from_storage(storage: Option<&dyn eframe::Storage>) -> Self {
        let preferences = crate::preferences::load(storage);
        Self {
            theme: preferences.theme,
            appearance: preferences.appearance,
            ..Self::default()
        }
    }

    fn is_dirty(&self) -> bool {
        let state = editor_state(&self.project, &self.snarl);
        self.history.is_dirty_by(&state, |snapshot| &snapshot.state)
    }

    fn mark_clean(&mut self) {
        self.history
            .mark_saved(&editor_snapshot(&self.project, &self.snarl));
    }

    fn rebase_history(&mut self) {
        let snapshot = editor_snapshot(&self.project, &self.snarl);
        let origin = if self.history.is_dirty(&snapshot) {
            DocumentOrigin::Unsaved
        } else {
            DocumentOrigin::Saved
        };
        self.history.rebase(snapshot.clone(), origin);
        self.observed_editor = snapshot;
        self.pending_history = None;
    }

    fn commit_pending_history(&mut self) {
        if let Some(pending) = self.pending_history.take()
            && pending.before != self.observed_editor
        {
            self.history.record(pending.before, "Edit mapping");
        }
    }

    /// Observes project and canvas-layout mutations after the frame has rendered. Changes
    /// close together form one user-level undo transaction instead of one
    /// entry per keystroke/frame.
    fn observe_editor_history(
        &mut self,
        now: std::time::Instant,
        coalesce_change: bool,
    ) -> Option<std::time::Duration> {
        let current_state = editor_state(&self.project, &self.snarl);
        if current_state != self.observed_editor.state {
            let current = EditorSnapshot {
                project: self.project.clone(),
                state: current_state,
            };
            if coalesce_change {
                if self.pending_history.as_ref().is_some_and(|pending| {
                    now.saturating_duration_since(pending.last_change) >= HISTORY_COALESCE_DELAY
                }) {
                    self.commit_pending_history();
                }
                match &mut self.pending_history {
                    Some(pending) => pending.last_change = now,
                    None => {
                        self.pending_history = Some(PendingHistory {
                            before: self.observed_editor.clone(),
                            last_change: now,
                        });
                        self.history.clear_redo();
                    }
                }
                self.observed_editor = current;
            } else {
                self.commit_pending_history();
                self.history
                    .record(self.observed_editor.clone(), "Edit mapping");
                self.observed_editor = current;
                return None;
            }
        }

        let pending = self.pending_history.as_ref()?;
        let elapsed = now.saturating_duration_since(pending.last_change);
        if elapsed >= HISTORY_COALESCE_DELAY {
            self.commit_pending_history();
            None
        } else {
            Some(HISTORY_COALESCE_DELAY - elapsed)
        }
    }

    fn can_undo(&self) -> bool {
        self.pending_history
            .as_ref()
            .is_some_and(|pending| pending.before != self.observed_editor)
            || self.history.can_undo()
    }

    fn restore_history_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.project = snapshot.project.clone();
        self.snarl = build_snarl_with_layout(&self.project, Some(&snapshot.state.layout));
        self.selected_scope.clear();
        self.observed_editor = snapshot;
        self.pending_history = None;
    }

    fn undo_project(&mut self) {
        self.commit_pending_history();
        let Some(previous) = self.history.undo(self.observed_editor.clone()) else {
            return;
        };
        let label = previous.label().to_string();
        self.restore_history_snapshot(previous.into_snapshot());
        self.status = format!("undid {label}");
    }

    fn redo_project(&mut self) {
        let Some(next) = self.history.redo(self.observed_editor.clone()) else {
            return;
        };
        let label = next.label().to_string();
        self.restore_history_snapshot(next.into_snapshot());
        self.status = format!("redid {label}");
    }

    /// Returns the action when it can run immediately. Dirty projects queue
    /// the action for the shared save/discard/cancel confirmation instead.
    fn request_destructive_action(
        &mut self,
        action: DestructiveAction,
    ) -> Option<DestructiveAction> {
        if self.is_dirty() {
            self.pending_destructive_action = Some(action);
            None
        } else {
            Some(action)
        }
    }

    fn perform_destructive_action(&mut self, action: DestructiveAction, ctx: &egui::Context) {
        match action {
            DestructiveAction::OpenProject => {
                self.pending_dialog = Some((
                    DialogKind::OpenProject,
                    pick_file("ferrule project", &["json"]),
                ));
            }
            DestructiveAction::NewProject => {
                self.project = blank_project();
                self.snarl = build_snarl(&self.project);
                self.reset_canvas_view();
                self.document = DocumentLocation::untitled("project.json");
                self.selected_scope.clear();
                self.mark_clean();
                self.rebase_history();
                self.diagnostics.clear();
                self.status = "new project".to_string();
                self.begin_new_mapping();
            }
            DestructiveAction::ImportMfd => {
                self.pending_dialog = Some((
                    DialogKind::ImportMfd,
                    pick_file("MapForce design", &["mfd"]),
                ));
            }
            DestructiveAction::Close => {
                self.allow_close = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn show_unsaved_confirmation(&mut self, ctx: &egui::Context) {
        let Some(action) = self.pending_destructive_action else {
            return;
        };
        let mut choice = None;
        egui::Window::new("Unsaved changes")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Save changes to {} before continuing?",
                    self.document.display_path()
                ));
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        choice = Some(true);
                    }
                    if ui.button("Discard").clicked() {
                        choice = Some(false);
                    }
                    if ui.button("Cancel").clicked() {
                        self.pending_destructive_action = None;
                    }
                });
            });

        match choice {
            Some(true) => {
                self.save_with_continuation(Some(SaveContinuation::Destructive(action)), ctx)
            }
            Some(false) => {
                self.pending_destructive_action = None;
                self.perform_destructive_action(action, ctx);
            }
            None => {}
        }
    }

    fn load_project_from(&mut self, path: &std::path::Path) {
        match std::fs::read_to_string(path).and_then(|text| {
            serde_json::from_str::<Project>(&text).map_err(|e| std::io::Error::other(e.to_string()))
        }) {
            Ok(project) => {
                let (layout, layout_warning) = match read_layout(path) {
                    Ok(layout) => (layout, None),
                    Err(error) => (None, Some(error.to_string())),
                };
                self.snarl = build_snarl_with_layout(&project, layout.as_ref());
                self.reset_canvas_view();
                self.project = project;
                self.document = DocumentLocation::saved(path);
                self.selected_scope.clear();
                self.mark_clean();
                self.rebase_history();
                let validation = cli::validate(&self.project);
                let mut diagnostics = validation
                    .into_iter()
                    .map(|issue| Diagnostic {
                        level: DiagnosticLevel::Error,
                        message: issue.to_string(),
                    })
                    .collect::<Vec<_>>();
                diagnostics.extend(layout_warning.map(|warning| Diagnostic {
                    level: DiagnosticLevel::Warning,
                    message: format!("using default canvas layout: {warning}"),
                }));
                if diagnostics.is_empty() {
                    self.diagnostics.clear();
                } else {
                    self.diagnostics.replace("Loaded project", diagnostics);
                }
                self.status = format!("loaded {}", path.display());
            }
            Err(error) => {
                self.status = format!("failed to load {}", path.display());
                self.diagnostics.error("Open failed", error.to_string());
            }
        }
    }

    fn save_document_to(&mut self, path: &std::path::Path) -> anyhow::Result<Vec<String>> {
        let json = serde_json::to_string_pretty(&self.project)?;
        std::fs::write(path, json)?;
        write_layout(path, &CanvasLayout::capture(&self.project, &self.snarl))?;
        self.document = DocumentLocation::saved(path);
        self.mark_clean();
        Ok(cli::validate(&self.project)
            .into_iter()
            .map(|issue| issue.to_string())
            .collect())
    }

    fn saved_status(path: &std::path::Path, issues: &[String]) -> String {
        if issues.is_empty() {
            format!("saved {}", path.display())
        } else {
            format!(
                "saved {} with {} validation issue(s)",
                path.display(),
                issues.len()
            )
        }
    }

    fn start_save_as(&mut self, continuation: Option<SaveContinuation>) {
        self.pending_save_continuation = continuation;
        self.pending_dialog = Some((
            DialogKind::SaveProjectAs,
            save_file("ferrule project", &["json"], &self.document.display_path()),
        ));
    }

    fn save_with_continuation(
        &mut self,
        continuation: Option<SaveContinuation>,
        ctx: &egui::Context,
    ) {
        let Some(path) = self.document.saved_path().map(std::path::Path::to_path_buf) else {
            self.pending_destructive_action = None;
            self.start_save_as(continuation);
            return;
        };
        match self.save_document_to(&path) {
            Ok(issues) => {
                self.status = Self::saved_status(&path, &issues);
                if issues.is_empty() {
                    self.diagnostics.clear();
                } else {
                    self.diagnostics.validation(issues);
                }
                self.pending_destructive_action = None;
                self.complete_save_continuation(continuation, ctx);
            }
            Err(error) => {
                self.status = format!("failed to save {}", path.display());
                self.diagnostics.error("Save failed", error.to_string());
            }
        }
    }

    fn complete_save_continuation(
        &mut self,
        continuation: Option<SaveContinuation>,
        ctx: &egui::Context,
    ) {
        match continuation {
            Some(SaveContinuation::Destructive(action)) => {
                self.perform_destructive_action(action, ctx)
            }
            Some(SaveContinuation::Run) => self.run_saved(),
            None => {}
        }
    }

    /// Applies the result of a finished file dialog, if any.
    fn poll_dialog(&mut self, ctx: &egui::Context) {
        let Some((kind, rx)) = &self.pending_dialog else {
            return;
        };
        let kind = *kind;
        let result = match rx.try_recv() {
            Ok(result) => result,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
        };
        self.pending_dialog = None;
        let Some(path) = result else {
            if kind == DialogKind::SaveProjectAs {
                self.pending_save_continuation = None;
            }
            return; // cancelled or no dialog backend
        };
        match kind {
            DialogKind::OpenProject => {
                self.load_project_from(std::path::Path::new(&path));
            }
            DialogKind::SaveProjectAs => {
                let continuation = self.pending_save_continuation.take();
                let path = PathBuf::from(path);
                match self.save_document_to(&path) {
                    Ok(issues) => {
                        self.status = Self::saved_status(&path, &issues);
                        if issues.is_empty() {
                            self.diagnostics.clear();
                        } else {
                            self.diagnostics.validation(issues);
                        }
                        self.complete_save_continuation(continuation, ctx);
                    }
                    Err(error) => {
                        self.status = format!("failed to save {}", path.display());
                        self.diagnostics.error("Save failed", error.to_string());
                    }
                }
            }
            DialogKind::BrowseInput => self.input_path = path,
            DialogKind::BrowseOutput => self.output_path = path,
            DialogKind::BrowseSourceSchema => {
                self.stage_mapping_schema(SchemaSide::Source, PathBuf::from(path));
            }
            DialogKind::BrowseTargetSchema => {
                self.stage_mapping_schema(SchemaSide::Target, PathBuf::from(path));
            }
            DialogKind::BrowseExtraSourceSchema => {
                self.stage_extra_source_schema(PathBuf::from(path));
            }
            DialogKind::BrowseExtraSourceInstance => {
                if let Some(draft) = &mut self.extra_source_draft {
                    draft.instance_path = path;
                }
            }
            DialogKind::ImportMfd => match mfd::import(std::path::Path::new(&path)) {
                Ok(imported) => {
                    self.snarl = build_snarl(&imported.project);
                    self.reset_canvas_view();
                    self.project = imported.project;
                    self.history.mark_unsaved();
                    self.selected_scope.clear();
                    self.rebase_history();
                    self.document = DocumentLocation::untitled(
                        std::path::Path::new(&path).with_extension("json"),
                    );
                    let validation = cli::validate(&self.project);
                    let mut diagnostics = imported
                        .warnings
                        .iter()
                        .cloned()
                        .map(|message| Diagnostic {
                            level: DiagnosticLevel::Warning,
                            message,
                        })
                        .collect::<Vec<_>>();
                    diagnostics.extend(validation.into_iter().map(|issue| Diagnostic {
                        level: DiagnosticLevel::Error,
                        message: issue.to_string(),
                    }));
                    if diagnostics.is_empty() {
                        self.diagnostics.clear();
                    } else {
                        self.diagnostics.replace("MFD import", diagnostics);
                    }
                    self.status = if imported.warnings.is_empty() {
                        format!("imported {path}")
                    } else {
                        format!(
                            "imported {path} with {} warning(s)",
                            imported.warnings.len()
                        )
                    };
                }
                Err(error) => {
                    self.status = format!("failed to import {path}");
                    self.diagnostics
                        .error("MFD import failed", error.to_string());
                }
            },
            DialogKind::ExportMfd => {
                match mfd::export(&self.project, std::path::Path::new(&path)) {
                    Ok(warnings) if warnings.is_empty() => {
                        self.status = format!("exported {path}");
                        self.diagnostics.clear();
                    }
                    Ok(warnings) => {
                        self.status = format!("exported {path} with {} warning(s)", warnings.len());
                        self.diagnostics.warnings("MFD export", warnings);
                    }
                    Err(error) => {
                        self.status = format!("failed to export {path}");
                        self.diagnostics
                            .error("MFD export failed", error.to_string());
                    }
                }
            }
        }
    }

    fn run(&mut self, ctx: &egui::Context) {
        let issues = cli::validate(&self.project);
        if !issues.is_empty() {
            self.status = format!("run blocked by {} validation issue(s)", issues.len());
            self.diagnostics.validation(issues);
            return;
        }
        self.diagnostics.clear();
        self.save_with_continuation(Some(SaveContinuation::Run), ctx);
    }

    fn run_saved(&mut self) {
        let Some(project_path) = self.document.saved_path() else {
            self.diagnostics
                .error("Run failed", "project has no saved file");
            return;
        };
        let input_path = nonempty_path(&self.input_path);
        let output_path = nonempty_path(&self.output_path);
        match cli::run_project_with_paths(
            project_path,
            input_path.as_deref(),
            output_path.as_deref(),
        ) {
            Ok(outcome) => {
                self.status = format!(
                    "wrote {} record(s) to {}",
                    outcome.records_written,
                    outcome.output_path.display()
                );
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = "run failed".to_string();
                self.diagnostics.error("Run failed", error.to_string());
            }
        }
    }
}

fn nonempty_path(value: &str) -> Option<PathBuf> {
    (!value.trim().is_empty()).then(|| PathBuf::from(value.trim()))
}

impl eframe::App for FerruleApp {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        crate::preferences::store(
            storage,
            crate::preferences::EditorPreferences::new(self.theme, self.appearance),
        );
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.palette = self.theme.apply(ui.ctx());
        let layout_class = LayoutClass::from_width(ui.available_width());
        if self.last_layout_class != Some(layout_class) {
            self.last_layout_class = Some(layout_class);
            self.reset_canvas_view();
        }
        self.poll_dialog(ui.ctx());
        let close_requested = ui.ctx().input(|input| input.viewport().close_requested());
        if close_requested && !self.allow_close && self.is_dirty() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending_destructive_action
                .get_or_insert(DestructiveAction::Close);
        }
        let project_editing_enabled = self.pending_dialog.is_none()
            && self.pending_destructive_action.is_none()
            && self.new_mapping_setup.is_none()
            && self.extra_source_draft.is_none()
            && self.pending_extra_source_removal.is_none();
        let undo_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
        let redo_shortcut = egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::Z,
        );
        let redo_secondary = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Y);
        let coalesce_history_change = ui.ctx().input(|input| {
            input.pointer.primary_down()
                || input.events.iter().any(|event| {
                    matches!(
                        event,
                        egui::Event::Cut
                            | egui::Event::Paste(_)
                            | egui::Event::Text(_)
                            | egui::Event::Key { .. }
                            | egui::Event::Ime(_)
                    )
                })
        });
        if project_editing_enabled
            && ui
                .ctx()
                .input_mut(|input| input.consume_shortcut(&undo_shortcut))
        {
            self.undo_project();
        } else if project_editing_enabled
            && (ui
                .ctx()
                .input_mut(|input| input.consume_shortcut(&redo_shortcut))
                || ui
                    .ctx()
                    .input_mut(|input| input.consume_shortcut(&redo_secondary)))
        {
            self.redo_project();
        }
        if self.pending_dialog.is_some() {
            // Keep polling even without input events.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        egui::Panel::top("top_panel").show(ui, |ui| {
            self.show_command_bar(
                ui,
                project_editing_enabled,
                layout_class,
                &undo_shortcut,
                &redo_shortcut,
            );
        });

        egui::Panel::bottom("status_bar")
            .exact_size(28.0)
            .show(ui, |ui| self.show_status_bar(ui));
        if !self.diagnostics.is_empty() {
            egui::Panel::bottom("diagnostics_panel")
                .resizable(true)
                .default_size(120.0)
                .show(ui, |ui| self.diagnostics.show(ui));
        }

        let visibility = WorkspaceVisibility::resolve(
            layout_class,
            self.show_source_panel,
            self.show_inspector_panel,
            self.compact_dock_open,
            self.compact_dock,
            self.narrow_pane,
        );
        if visibility.source_dock {
            egui::Panel::left("source_schema")
                .default_size(220.0)
                .min_size(150.0)
                .max_size(420.0)
                .show(ui, |ui| {
                    self.show_source_explorer(ui, project_editing_enabled)
                });
        }

        if visibility.inspector_dock {
            egui::Panel::right("target_schema_and_scopes")
                .default_size(300.0)
                .min_size(220.0)
                .max_size(480.0)
                .show(ui, |ui| self.show_inspector(ui, project_editing_enabled));
        }

        egui::CentralPanel::default().show(ui, |ui| match visibility.center {
            WorkspacePane::Source => {
                self.show_source_explorer(ui, project_editing_enabled);
            }
            WorkspacePane::Canvas => self.show_canvas(ui, project_editing_enabled),
            WorkspacePane::Inspector => self.show_inspector(ui, project_editing_enabled),
        });

        crate::appearance_editor::show(
            ui.ctx(),
            &mut self.show_appearance_editor,
            &mut self.appearance_tab,
            &mut self.theme,
            &mut self.appearance,
            self.palette,
        );

        self.show_unsaved_confirmation(ui.ctx());
        self.show_new_mapping_setup(ui.ctx());
        self.show_extra_source_setup(ui.ctx());
        self.show_extra_source_removal_confirmation(ui.ctx());
        if let Some(repaint_after) =
            self.observe_editor_history(std::time::Instant::now(), coalesce_history_change)
        {
            ui.ctx().request_repaint_after(repaint_after);
        }
        let marker = if self.is_dirty() { " *" } else { "" };
        let name = self.document.display_name();
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title(format!(
                "{name}{marker} - ferrule"
            )));
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
