//! The ferrule-gui app: source/target schema panes, a scope editor, and
//! the mapping canvas.
//!
//! The canvas carries the visual-mapper interaction: the Source and Target
//! schemas are endpoint nodes whose pins are their scalar leaves, so a
//! mapping is wired leaf-to-function-to-leaf. Scope iteration (which
//! repeating path each scope loops over) is still edited in the side
//! panel -- connecting wires never changes iteration, only values.

use std::path::PathBuf;

use egui_snarl::{InPinId, OutPinId, Snarl};
use ir::SchemaNode;
use mapping::{Graph, Node, NodeId, Project, Scope};
use serde::{Deserialize, Serialize};

use crate::canvas::{
    CanvasNode, SourceLeaf, TargetLeaf, layered_layout, source_leaves, target_leaves,
};
use crate::graph_viewer::GraphViewer;
use crate::path_picker::SourcePathCatalog;
use crate::schema_tree::show_schema_tree;
use crate::scope_editor::{ScopePath, scope_at_mut, show_scope_editor, show_scope_tree};

const HISTORY_LIMIT: usize = 100;
const HISTORY_COALESCE_DELAY: std::time::Duration = std::time::Duration::from_millis(400);
const LAYOUT_VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
struct CanvasLayout {
    version: u32,
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

#[derive(Serialize)]
struct EditorSnapshotRef<'a> {
    project: &'a Project,
    layout: CanvasLayout,
}

#[derive(Deserialize)]
struct EditorSnapshot {
    project: Project,
    layout: CanvasLayout,
}

pub struct FerruleApp {
    project: Project,
    /// Serialized editor state at the last successful load/save. `None` marks
    /// imported projects that have never been saved as ferrule JSON.
    saved_editor: Option<String>,
    snarl: Snarl<CanvasNode>,
    project_path: String,
    input_path: String,
    output_path: String,
    selected_scope: ScopePath,
    status: String,
    /// An in-flight native file dialog, running on its own thread so a
    /// missing portal backend can never freeze the UI.
    pending_dialog: Option<(DialogKind, std::sync::mpsc::Receiver<Option<String>>)>,
    pending_destructive_action: Option<DestructiveAction>,
    allow_close: bool,
    history_editor: String,
    undo_history: Vec<String>,
    redo_history: Vec<String>,
    pending_history: Option<PendingHistory>,
}

struct PendingHistory {
    before: String,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DestructiveAction {
    OpenProject,
    LoadProject,
    NewProject,
    ImportMfd,
    Close,
}

impl Default for FerruleApp {
    fn default() -> Self {
        let project = blank_project();
        let snarl = build_snarl(&project);
        let snapshot = editor_snapshot(&project, &snarl);
        Self {
            project,
            saved_editor: Some(snapshot.clone()),
            snarl,
            project_path: "project.json".to_string(),
            input_path: String::new(),
            output_path: String::new(),
            selected_scope: Vec::new(),
            status: String::new(),
            pending_dialog: None,
            pending_destructive_action: None,
            allow_close: false,
            history_editor: snapshot,
            undo_history: Vec::new(),
            redo_history: Vec::new(),
            pending_history: None,
        }
    }
}

fn editor_snapshot(project: &Project, snarl: &Snarl<CanvasNode>) -> String {
    serde_json::to_string(&EditorSnapshotRef {
        project,
        layout: CanvasLayout::capture(project, snarl),
    })
    .expect("Editor state serialization cannot fail")
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
}

fn project_fingerprint(project: &Project) -> String {
    let json = serde_json::to_vec(project).expect("Project serialization cannot fail");
    let hash = json.into_iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
    });
    format!("{hash:016x}")
}

fn layout_path(project_path: &str) -> PathBuf {
    let mut path = PathBuf::from(project_path);
    path.set_extension("layout.json");
    path
}

fn read_layout(project_path: &str) -> anyhow::Result<Option<CanvasLayout>> {
    let path = layout_path(project_path);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    let layout: CanvasLayout = serde_json::from_str(&text)?;
    anyhow::ensure!(
        layout.version == LAYOUT_VERSION,
        "unsupported canvas layout version {}",
        layout.version
    );
    Ok(Some(layout))
}

fn write_layout(
    project_path: &str,
    project: &Project,
    snarl: &Snarl<CanvasNode>,
) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(&CanvasLayout::capture(project, snarl))?;
    std::fs::write(layout_path(project_path), json)?;
    Ok(())
}

fn blank_project() -> Project {
    Project {
        source: SchemaNode::group("root", vec![]),
        target: SchemaNode::group("root", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}

fn node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::SourceField { .. } | Node::Position { .. } | Node::Const { .. } => vec![],
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } | Node::Lookup { matches: input, .. } => vec![*input],
        Node::Aggregate {
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

fn arrange_snarl(project: &Project, current: &Snarl<CanvasNode>) -> Snarl<CanvasNode> {
    let placeholders: std::collections::BTreeSet<_> = current
        .nodes()
        .filter_map(|node| match node {
            CanvasNode::Placeholder(id) => Some(*id),
            _ => None,
        })
        .collect();
    let mut arranged = build_snarl(project);
    for node in arranged.nodes_mut() {
        if let CanvasNode::Graph(id) = *node
            && placeholders.contains(&id)
        {
            *node = CanvasNode::Placeholder(id);
        }
    }
    arranged
}

fn build_snarl_with_layout(
    project: &Project,
    saved_layout: Option<&CanvasLayout>,
) -> Snarl<CanvasNode> {
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
    let hidden_set: std::collections::BTreeSet<NodeId> = hidden.keys().copied().collect();
    let layout = layered_layout(&project.graph, &hidden_set, &binding_order);
    let fingerprint = project_fingerprint(project);
    let placeholders: std::collections::BTreeSet<NodeId> = saved_layout
        .filter(|layout| layout.project_fingerprint.as_deref() == Some(fingerprint.as_str()))
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
    let graph_start = source_pins
        .iter()
        .map(|leaf| leaf.label.chars().count())
        .max()
        .map_or(420.0, |length| (length as f32 * 9.0 + 180.0).max(420.0));

    let mut snarl_ids = std::collections::BTreeMap::new();
    let mut max_col = 0usize;
    for (&id, &(col, row)) in &layout {
        max_col = max_col.max(col);
        let snarl_id = snarl.insert_node(
            egui::pos2(graph_start + col as f32 * 420.0, row as f32 * 190.0),
            if placeholders.contains(&id) {
                CanvasNode::Placeholder(id)
            } else {
                CanvasNode::Graph(id)
            },
        );
        snarl_ids.insert(id, snarl_id);
    }
    let target_x = if layout.is_empty() {
        graph_start
    } else {
        graph_start + (max_col as f32 + 1.0) * 420.0
    };
    let target_node = snarl.insert_node(egui::pos2(target_x, 0.0), CanvasNode::Target);

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
    fn is_dirty(&self) -> bool {
        self.saved_editor
            .as_ref()
            .is_none_or(|saved| *saved != editor_snapshot(&self.project, &self.snarl))
    }

    fn mark_clean(&mut self) {
        self.saved_editor = Some(editor_snapshot(&self.project, &self.snarl));
    }

    fn rebase_history(&mut self) {
        self.history_editor = editor_snapshot(&self.project, &self.snarl);
        self.undo_history.clear();
        self.redo_history.clear();
        self.pending_history = None;
    }

    fn push_undo(&mut self, snapshot: String) {
        if self.undo_history.len() == HISTORY_LIMIT {
            self.undo_history.remove(0);
        }
        self.undo_history.push(snapshot);
    }

    fn commit_pending_history(&mut self) {
        if let Some(pending) = self.pending_history.take()
            && pending.before != self.history_editor
        {
            self.push_undo(pending.before);
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
        let current = editor_snapshot(&self.project, &self.snarl);
        if current != self.history_editor {
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
                            before: self.history_editor.clone(),
                            last_change: now,
                        });
                        self.redo_history.clear();
                    }
                }
                self.history_editor = current;
            } else {
                self.commit_pending_history();
                self.redo_history.clear();
                self.push_undo(self.history_editor.clone());
                self.history_editor = current;
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
            .is_some_and(|pending| pending.before != self.history_editor)
            || !self.undo_history.is_empty()
    }

    fn restore_history_snapshot(&mut self, snapshot: String) {
        let state: EditorSnapshot =
            serde_json::from_str(&snapshot).expect("history contains valid editor JSON");
        self.project = state.project;
        self.snarl = build_snarl_with_layout(&self.project, Some(&state.layout));
        self.selected_scope.clear();
        self.history_editor = snapshot;
        self.pending_history = None;
    }

    fn undo_project(&mut self) {
        self.commit_pending_history();
        let Some(previous) = self.undo_history.pop() else {
            return;
        };
        self.redo_history.push(self.history_editor.clone());
        self.restore_history_snapshot(previous);
        self.status = "undid edit".to_string();
    }

    fn redo_project(&mut self) {
        let Some(next) = self.redo_history.pop() else {
            return;
        };
        self.push_undo(self.history_editor.clone());
        self.restore_history_snapshot(next);
        self.status = "redid edit".to_string();
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
            DestructiveAction::LoadProject => self.load_project(),
            DestructiveAction::NewProject => {
                self.project = blank_project();
                self.snarl = build_snarl(&self.project);
                self.project_path = "project.json".to_string();
                self.selected_scope.clear();
                self.mark_clean();
                self.rebase_history();
                self.status = "new project".to_string();
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
                    self.project_path
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
            Some(true) => match self.save_project() {
                Ok(issues) => {
                    self.status = Self::saved_status(&self.project_path, &issues);
                    self.pending_destructive_action = None;
                    self.perform_destructive_action(action, ctx);
                }
                Err(error) => self.status = format!("failed to save: {error}"),
            },
            Some(false) => {
                self.pending_destructive_action = None;
                self.perform_destructive_action(action, ctx);
            }
            None => {}
        }
    }

    fn load_project(&mut self) {
        match std::fs::read_to_string(&self.project_path).and_then(|text| {
            serde_json::from_str::<Project>(&text).map_err(|e| std::io::Error::other(e.to_string()))
        }) {
            Ok(project) => {
                let (layout, layout_warning) = match read_layout(&self.project_path) {
                    Ok(layout) => (layout, None),
                    Err(error) => (None, Some(error.to_string())),
                };
                self.snarl = build_snarl_with_layout(&project, layout.as_ref());
                self.project = project;
                self.selected_scope.clear();
                self.mark_clean();
                self.rebase_history();
                self.status = layout_warning.map_or_else(
                    || format!("loaded {}", self.project_path),
                    |warning| {
                        format!(
                            "loaded {} with default canvas layout: {warning}",
                            self.project_path
                        )
                    },
                );
            }
            Err(e) => self.status = format!("failed to load: {e}"),
        }
    }

    fn save_project(&mut self) -> anyhow::Result<Vec<String>> {
        let json = serde_json::to_string_pretty(&self.project)?;
        std::fs::write(&self.project_path, json)?;
        write_layout(&self.project_path, &self.project, &self.snarl)?;
        self.mark_clean();
        Ok(cli::validate(&self.project)
            .into_iter()
            .map(|issue| issue.to_string())
            .collect())
    }

    fn saved_status(path: &str, issues: &[String]) -> String {
        if issues.is_empty() {
            format!("saved {path}")
        } else {
            format!(
                "saved {path} with {} validation issue(s): {}",
                issues.len(),
                issues[0]
            )
        }
    }

    /// Applies the result of a finished file dialog, if any.
    fn poll_dialog(&mut self) {
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
            return; // cancelled or no dialog backend
        };
        match kind {
            DialogKind::OpenProject => {
                self.project_path = path;
                self.load_project();
            }
            DialogKind::SaveProjectAs => {
                self.project_path = path;
                match self.save_project() {
                    Ok(issues) => {
                        self.status = Self::saved_status(&self.project_path, &issues);
                    }
                    Err(e) => self.status = format!("failed to save: {e}"),
                }
            }
            DialogKind::BrowseInput => self.input_path = path,
            DialogKind::BrowseOutput => self.output_path = path,
            DialogKind::ImportMfd => match mfd::import(std::path::Path::new(&path)) {
                Ok(imported) => {
                    self.snarl = build_snarl(&imported.project);
                    self.project = imported.project;
                    self.saved_editor = None;
                    self.selected_scope.clear();
                    self.rebase_history();
                    self.project_path = std::path::Path::new(&path)
                        .with_extension("json")
                        .display()
                        .to_string();
                    self.status = if imported.warnings.is_empty() {
                        format!("imported {path}")
                    } else {
                        format!(
                            "imported {path} with {} warning(s): {}",
                            imported.warnings.len(),
                            imported.warnings.join(" | ")
                        )
                    };
                }
                Err(e) => self.status = format!("import failed: {e}"),
            },
            DialogKind::ExportMfd => {
                match mfd::export(&self.project, std::path::Path::new(&path)) {
                    Ok(warnings) if warnings.is_empty() => {
                        self.status = format!("exported {path}");
                    }
                    Ok(warnings) => {
                        self.status = format!(
                            "exported {path} with {} warning(s): {}",
                            warnings.len(),
                            warnings.join(" | ")
                        );
                    }
                    Err(e) => self.status = format!("export failed: {e}"),
                }
            }
        }
    }

    fn run(&mut self) {
        match self.save_project() {
            Ok(issues) if issues.is_empty() => {}
            Ok(issues) => {
                self.status = format!(
                    "run blocked by {} validation issue(s): {}",
                    issues.len(),
                    issues[0]
                );
                return;
            }
            Err(e) => {
                self.status = format!("failed to save before running: {e}");
                return;
            }
        }
        match cli::run_project(
            std::path::Path::new(&self.project_path),
            &PathBuf::from(&self.input_path),
            &PathBuf::from(&self.output_path),
        ) {
            Ok(rows) => self.status = format!("wrote {rows} record(s) to {}", self.output_path),
            Err(e) => self.status = format!("run failed: {e}"),
        }
    }
}

impl eframe::App for FerruleApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_dialog();
        let close_requested = ui.ctx().input(|input| input.viewport().close_requested());
        if close_requested && !self.allow_close && self.is_dirty() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.pending_destructive_action
                .get_or_insert(DestructiveAction::Close);
        }
        let project_editing_enabled =
            self.pending_dialog.is_none() && self.pending_destructive_action.is_none();
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
        let source_paths =
            SourcePathCatalog::new(&self.project.source, &self.project.extra_sources);
        if self.pending_dialog.is_some() {
            // Keep polling even without input events.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        egui::Panel::top("top_panel").show(ui, |ui| {
            ui.add_enabled_ui(project_editing_enabled, |ui| {
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.can_undo(),
                            egui::Button::new("<").min_size(egui::vec2(28.0, 28.0)),
                        )
                        .on_hover_text(format!(
                            "Undo ({})",
                            ui.ctx().format_shortcut(&undo_shortcut)
                        ))
                        .clicked()
                    {
                        self.undo_project();
                    }
                    if ui
                        .add_enabled(
                            !self.redo_history.is_empty(),
                            egui::Button::new(">").min_size(egui::vec2(28.0, 28.0)),
                        )
                        .on_hover_text(format!(
                            "Redo ({})",
                            ui.ctx().format_shortcut(&redo_shortcut)
                        ))
                        .clicked()
                    {
                        self.redo_project();
                    }
                    ui.separator();
                    ui.label("project:");
                    ui.text_edit_singleline(&mut self.project_path);
                    if ui.button("Open\u{2026}").clicked()
                        && let Some(action) =
                            self.request_destructive_action(DestructiveAction::OpenProject)
                    {
                        self.perform_destructive_action(action, ui.ctx());
                    }
                    if ui.button("Load").clicked()
                        && let Some(action) =
                            self.request_destructive_action(DestructiveAction::LoadProject)
                    {
                        self.perform_destructive_action(action, ui.ctx());
                    }
                    if ui.button("Save").clicked() {
                        match self.save_project() {
                            Ok(issues) => {
                                self.status = Self::saved_status(&self.project_path, &issues);
                            }
                            Err(e) => self.status = format!("failed to save: {e}"),
                        }
                    }
                    if ui.button("Save As\u{2026}").clicked() {
                        self.pending_dialog = Some((
                            DialogKind::SaveProjectAs,
                            save_file("ferrule project", &["json"], &self.project_path),
                        ));
                    }
                    if ui.button("New").clicked()
                        && let Some(action) =
                            self.request_destructive_action(DestructiveAction::NewProject)
                    {
                        self.perform_destructive_action(action, ui.ctx());
                    }
                    if ui.button("Import MFD\u{2026}").clicked()
                        && let Some(action) =
                            self.request_destructive_action(DestructiveAction::ImportMfd)
                    {
                        self.perform_destructive_action(action, ui.ctx());
                    }
                    if ui.button("Export MFD\u{2026}").clicked() {
                        self.pending_dialog = Some((
                            DialogKind::ExportMfd,
                            save_file("MapForce design", &["mfd"], &self.project_path),
                        ));
                    }
                });
            });
            ui.add_enabled_ui(project_editing_enabled, |ui| {
                ui.horizontal(|ui| {
                    ui.label("input:");
                    ui.text_edit_singleline(&mut self.input_path);
                    if ui.button("Browse\u{2026}").clicked() {
                        self.pending_dialog = Some((
                            DialogKind::BrowseInput,
                            pick_file(
                                "input data",
                                &[
                                    "csv", "xml", "json", "db", "sqlite", "edi", "x12", "edifact",
                                ],
                            ),
                        ));
                    }
                    ui.label("output:");
                    ui.text_edit_singleline(&mut self.output_path);
                    if ui.button("Browse\u{2026}").clicked() {
                        self.pending_dialog = Some((
                            DialogKind::BrowseOutput,
                            save_file(
                                "output data",
                                &[
                                    "csv", "xml", "json", "db", "sqlite", "edi", "x12", "edifact",
                                ],
                                &self.output_path,
                            ),
                        ));
                    }
                    if ui.button("Run").clicked() {
                        self.run();
                    }
                    if ui.button("Arrange").clicked() {
                        self.snarl = arrange_snarl(&self.project, &self.snarl);
                        self.status = "canvas re-arranged".to_string();
                    }
                });
            });
            if !self.status.is_empty() {
                ui.label(&self.status);
            }
        });

        egui::Panel::left("source_schema").show(ui, |ui| {
            ui.strong("Source schema");
            egui::ScrollArea::vertical().show(ui, |ui| {
                show_schema_tree(ui, &self.project.source);
                for extra in &self.project.extra_sources {
                    ui.separator();
                    ui.strong(format!("Extra: {}", extra.name));
                    show_schema_tree(ui, &extra.schema);
                }
            });
        });

        egui::Panel::right("target_schema_and_scopes").show(ui, |ui| {
            ui.add_enabled_ui(project_editing_enabled, |ui| {
                ui.strong("Target schema");
                egui::ScrollArea::vertical()
                    .max_height(200.0)
                    .show(ui, |ui| {
                        show_schema_tree(ui, &self.project.target);
                    });

                ui.separator();
                ui.strong("Scopes");
                egui::ScrollArea::vertical()
                    .id_salt("scope_tree_scroll")
                    .max_height(200.0)
                    .show(ui, |ui| {
                        if let Some(new_selection) =
                            show_scope_tree(ui, &self.project.root, &self.selected_scope)
                        {
                            self.selected_scope = new_selection;
                        }
                    });

                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt("scope_editor_scroll")
                    .show(ui, |ui| {
                        let nested = !self.selected_scope.is_empty();
                        let scope = scope_at_mut(&mut self.project.root, &self.selected_scope);
                        show_scope_editor(ui, scope, &self.project.graph, &source_paths, nested);
                    });
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_enabled_ui(project_editing_enabled, |ui| {
                let source_pins: Vec<SourceLeaf> = source_leaves(&self.project.source);
                let target_pins: Vec<TargetLeaf> = target_leaves(&self.project.target);
                let mut viewer = GraphViewer {
                    graph: &mut self.project.graph,
                    root_scope: &mut self.project.root,
                    source_leaves: &source_pins,
                    target_leaves: &target_pins,
                    source_paths: &source_paths,
                    error: None,
                };
                egui_snarl::ui::SnarlWidget::new().show(&mut self.snarl, &mut viewer, ui);
                if let Some(error) = viewer.error {
                    self.status = error;
                }
            });
        });

        self.show_unsaved_confirmation(ui.ctx());
        if let Some(repaint_after) =
            self.observe_editor_history(std::time::Instant::now(), coalesce_history_change)
        {
            ui.ctx().request_repaint_after(repaint_after);
        }
        let marker = if self.is_dirty() { " *" } else { "" };
        let name = std::path::Path::new(&self.project_path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(&self.project_path);
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title(format!(
                "{name}{marker} - ferrule"
            )));
    }
}

#[cfg(test)]
#[path = "app_tests.rs"]
mod tests;
