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

use crate::canvas::{
    CanvasNode, SourceLeaf, TargetLeaf, layered_layout, source_leaves, target_leaves,
};
use crate::graph_viewer::GraphViewer;
use crate::schema_tree::show_schema_tree;
use crate::scope_editor::{ScopePath, scope_at_mut, show_scope_editor, show_scope_tree};

pub struct FerruleApp {
    project: Project,
    snarl: Snarl<CanvasNode>,
    project_path: String,
    input_path: String,
    output_path: String,
    selected_scope: ScopePath,
    status: String,
    /// An in-flight native file dialog, running on its own thread so a
    /// missing portal backend can never freeze the UI.
    pending_dialog: Option<(DialogKind, std::sync::mpsc::Receiver<Option<String>>)>,
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

impl Default for FerruleApp {
    fn default() -> Self {
        let project = blank_project();
        let snarl = build_snarl(&project);
        Self {
            project,
            snarl,
            project_path: "project.json".to_string(),
            input_path: String::new(),
            output_path: String::new(),
            selected_scope: Vec::new(),
            status: String::new(),
            pending_dialog: None,
        }
    }
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
        Node::SourceField { .. } | Node::Const { .. } => vec![],
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } | Node::Lookup { matches: input, .. } => vec![*input],
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
    let source_pins = source_leaves(&project.source);
    let target_pins = target_leaves(&project.target);

    let mut snarl = Snarl::new();
    let source_node = snarl.insert_node(egui::pos2(0.0, 0.0), CanvasNode::Source);

    // A SourceField is hidden when some source leaf carries its path.
    let leaf_for_path = |path: &[String]| source_pins.iter().position(|l| l.path == path);
    let hidden: std::collections::BTreeMap<NodeId, usize> = project
        .graph
        .nodes
        .iter()
        .filter_map(|(&id, node)| match node {
            Node::SourceField { path } => leaf_for_path(path).map(|leaf| (id, leaf)),
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

    let mut snarl_ids = std::collections::BTreeMap::new();
    let mut max_col = 0usize;
    for (&id, &(col, row)) in &layout {
        max_col = max_col.max(col);
        let snarl_id = snarl.insert_node(
            egui::pos2(340.0 + col as f32 * 420.0, row as f32 * 190.0),
            CanvasNode::Graph(id),
        );
        snarl_ids.insert(id, snarl_id);
    }
    let target_x = if layout.is_empty() {
        420.0
    } else {
        340.0 + (max_col as f32 + 1.0) * 420.0
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
    fn load_project(&mut self) {
        match std::fs::read_to_string(&self.project_path).and_then(|text| {
            serde_json::from_str::<Project>(&text).map_err(|e| std::io::Error::other(e.to_string()))
        }) {
            Ok(project) => {
                self.snarl = build_snarl(&project);
                self.project = project;
                self.selected_scope.clear();
                self.status = format!("loaded {}", self.project_path);
            }
            Err(e) => self.status = format!("failed to load: {e}"),
        }
    }

    fn save_project(&mut self) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(&self.project)?;
        std::fs::write(&self.project_path, json)?;
        Ok(())
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
                    Ok(()) => self.status = format!("saved {}", self.project_path),
                    Err(e) => self.status = format!("failed to save: {e}"),
                }
            }
            DialogKind::BrowseInput => self.input_path = path,
            DialogKind::BrowseOutput => self.output_path = path,
            DialogKind::ImportMfd => match mfd::import(std::path::Path::new(&path)) {
                Ok(imported) => {
                    self.snarl = build_snarl(&imported.project);
                    self.project = imported.project;
                    self.selected_scope.clear();
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
        if let Err(e) = self.save_project() {
            self.status = format!("failed to save before running: {e}");
            return;
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
        if self.pending_dialog.is_some() {
            // Keep polling even without input events.
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(100));
        }
        egui::Panel::top("top_panel").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("project:");
                ui.text_edit_singleline(&mut self.project_path);
                if ui.button("Open\u{2026}").clicked() && self.pending_dialog.is_none() {
                    self.pending_dialog = Some((
                        DialogKind::OpenProject,
                        pick_file("ferrule project", &["json"]),
                    ));
                }
                if ui.button("Load").clicked() {
                    self.load_project();
                }
                if ui.button("Save").clicked() {
                    match self.save_project() {
                        Ok(()) => self.status = format!("saved {}", self.project_path),
                        Err(e) => self.status = format!("failed to save: {e}"),
                    }
                }
                if ui.button("Save As\u{2026}").clicked() && self.pending_dialog.is_none() {
                    self.pending_dialog = Some((
                        DialogKind::SaveProjectAs,
                        save_file("ferrule project", &["json"], &self.project_path),
                    ));
                }
                if ui.button("New").clicked() {
                    self.project = blank_project();
                    self.snarl = build_snarl(&self.project);
                    self.selected_scope.clear();
                }
                if ui.button("Import MFD\u{2026}").clicked() && self.pending_dialog.is_none() {
                    self.pending_dialog = Some((
                        DialogKind::ImportMfd,
                        pick_file("MapForce design", &["mfd"]),
                    ));
                }
                if ui.button("Export MFD\u{2026}").clicked() && self.pending_dialog.is_none() {
                    self.pending_dialog = Some((
                        DialogKind::ExportMfd,
                        save_file("MapForce design", &["mfd"], &self.project_path),
                    ));
                }
            });
            ui.horizontal(|ui| {
                ui.label("input:");
                ui.text_edit_singleline(&mut self.input_path);
                if ui.button("Browse\u{2026}").clicked() && self.pending_dialog.is_none() {
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
                if ui.button("Browse\u{2026}").clicked() && self.pending_dialog.is_none() {
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
                    self.snarl = build_snarl(&self.project);
                    self.status = "canvas re-arranged".to_string();
                }
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
                    let scope = scope_at_mut(&mut self.project.root, &self.selected_scope);
                    show_scope_editor(ui, scope, &self.project.graph);
                });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let source_pins: Vec<SourceLeaf> = source_leaves(&self.project.source);
            let target_pins: Vec<TargetLeaf> = target_leaves(&self.project.target);
            let mut viewer = GraphViewer {
                graph: &mut self.project.graph,
                root_scope: &mut self.project.root,
                source_leaves: &source_pins,
                target_leaves: &target_pins,
                error: None,
            };
            egui_snarl::ui::SnarlWidget::new().show(&mut self.snarl, &mut viewer, ui);
            if let Some(error) = viewer.error {
                self.status = error;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::ScalarType;
    use mapping::Binding;

    /// Loading the orders-style project must recreate the whole picture:
    /// hidden SourceFields become wires from the Source endpoint, function
    /// inputs become node-to-node wires, and bindings become wires into
    /// the Target endpoint's leaf pins.
    #[test]
    fn build_snarl_recreates_endpoint_and_binding_wires() {
        let mut graph = Graph::default();
        // 0: hidden SourceField (matches leaf "name"), 1: upper(0)
        graph.nodes.insert(
            0,
            Node::SourceField {
                path: vec!["name".into()],
            },
        );
        graph.nodes.insert(
            1,
            Node::Call {
                function: "upper".into(),
                args: vec![0],
            },
        );
        let project = Project {
            source: SchemaNode::group(
                "row",
                vec![
                    SchemaNode::scalar("name", ScalarType::String),
                    SchemaNode::scalar("age", ScalarType::Int),
                ],
            ),
            target: SchemaNode::group(
                "row",
                vec![
                    SchemaNode::scalar("loud_name", ScalarType::String),
                    SchemaNode::scalar("age", ScalarType::Int),
                ],
            ),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: Some(vec![]),
                filter: None,
                bindings: vec![
                    Binding {
                        target_field: "loud_name".into(),
                        node: 1,
                    },
                    // Bound straight from the hidden SourceField? Use a
                    // second field to prove Source->Target wires too.
                    Binding {
                        target_field: "age".into(),
                        node: 2,
                    },
                ],
                children: vec![],
            },
        };
        // 2: hidden SourceField for "age", bound directly to the target.
        let mut project = project;
        project.graph.nodes.insert(
            2,
            Node::SourceField {
                path: vec!["age".into()],
            },
        );

        let snarl = build_snarl(&project);

        // Only Source, Target, and the Call node should be on the canvas.
        let kinds: Vec<CanvasNode> = snarl.nodes().copied().collect();
        assert_eq!(kinds.len(), 3);
        assert!(kinds.contains(&CanvasNode::Source));
        assert!(kinds.contains(&CanvasNode::Target));
        assert!(kinds.contains(&CanvasNode::Graph(1)));

        // Wires: Source(name)->Call arg0, Call->Target(loud_name),
        // Source(age)->Target(age).
        let mut wires: Vec<(CanvasNode, usize, CanvasNode, usize)> = snarl
            .wires()
            .map(|(o, i)| (snarl[o.node], o.output, snarl[i.node], i.input))
            .collect();
        // Wire iteration order is not deterministic; compare as a set.
        wires.sort_by_key(|w| format!("{w:?}"));
        let mut expected = vec![
            (CanvasNode::Source, 0, CanvasNode::Graph(1), 0),
            (CanvasNode::Graph(1), 0, CanvasNode::Target, 0),
            (CanvasNode::Source, 1, CanvasNode::Target, 1),
        ];
        expected.sort_by_key(|w| format!("{w:?}"));
        assert_eq!(wires, expected);
    }
}
