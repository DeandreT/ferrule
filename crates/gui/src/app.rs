//! The ferrule-gui app: source/target schema panes, a scope editor, and a
//! node-graph canvas for the mapping's function graph.
//!
//! Scope note for this first GUI pass: a visual mapper's signature
//! interaction is dragging a wire from a schema-tree leaf to another leaf or
//! function pin, with the trees living in the same canvas as the functions.
//! Rebuilding that exact interaction is its own large custom-canvas project.
//! Here the function graph (Const/Call/If/ValueMap/SourceField) is a real,
//! interactive egui-snarl canvas, while wiring a graph node to a specific
//! target field (a `Scope`'s `bindings`) is done through a simpler picker
//! panel instead of drag-and-drop. Extending the canvas to cover schema
//! leaves directly is the natural next step once this is in use.

use std::path::PathBuf;

use egui_snarl::{InPinId, OutPinId, Snarl};
use ir::SchemaNode;
use mapping::{Graph, Node, NodeId, Project, Scope};

use crate::graph_viewer::GraphViewer;
use crate::schema_tree::show_schema_tree;
use crate::scope_editor::{ScopePath, scope_at_mut, show_scope_editor, show_scope_tree};

pub struct FerruleApp {
    project: Project,
    snarl: Snarl<NodeId>,
    project_path: String,
    input_path: String,
    output_path: String,
    selected_scope: ScopePath,
    status: String,
}

impl Default for FerruleApp {
    fn default() -> Self {
        let project = blank_project();
        let snarl = build_snarl(&project.graph);
        Self {
            project,
            snarl,
            project_path: "project.json".to_string(),
            input_path: String::new(),
            output_path: String::new(),
            selected_scope: Vec::new(),
            status: String::new(),
        }
    }
}

fn blank_project() -> Project {
    Project {
        source: SchemaNode::group("root", vec![]),
        target: SchemaNode::group("root", vec![]),
        graph: Graph::default(),
        root: Scope::default(),
    }
}

fn build_snarl(graph: &Graph) -> Snarl<NodeId> {
    let mut snarl = Snarl::new();
    let mut snarl_ids = std::collections::BTreeMap::new();
    for (i, &id) in graph.nodes.keys().enumerate() {
        let col = (i % 4) as f32;
        let row = (i / 4) as f32;
        let snarl_id = snarl.insert_node(egui::pos2(col * 360.0, row * 160.0), id);
        snarl_ids.insert(id, snarl_id);
    }
    for (&id, node) in &graph.nodes {
        let inputs: Vec<NodeId> = match node {
            Node::SourceField { .. } | Node::Const { .. } => vec![],
            Node::Call { args, .. } => args.clone(),
            Node::If {
                condition,
                then,
                else_,
            } => vec![*condition, *then, *else_],
            Node::ValueMap { input, .. } => vec![*input],
        };
        for (input_idx, arg) in inputs.iter().enumerate() {
            if let (Some(&from), Some(&to)) = (snarl_ids.get(arg), snarl_ids.get(&id)) {
                snarl.connect(
                    OutPinId {
                        node: from,
                        output: 0,
                    },
                    InPinId {
                        node: to,
                        input: input_idx,
                    },
                );
            }
        }
    }
    snarl
}

impl FerruleApp {
    fn load_project(&mut self) {
        match std::fs::read_to_string(&self.project_path).and_then(|text| {
            serde_json::from_str::<Project>(&text).map_err(|e| std::io::Error::other(e.to_string()))
        }) {
            Ok(project) => {
                self.snarl = build_snarl(&project.graph);
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
        egui::Panel::top("top_panel").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label("project:");
                ui.text_edit_singleline(&mut self.project_path);
                if ui.button("Load").clicked() {
                    self.load_project();
                }
                if ui.button("Save").clicked() {
                    match self.save_project() {
                        Ok(()) => self.status = format!("saved {}", self.project_path),
                        Err(e) => self.status = format!("failed to save: {e}"),
                    }
                }
                if ui.button("New").clicked() {
                    self.project = blank_project();
                    self.snarl = build_snarl(&self.project.graph);
                    self.selected_scope.clear();
                }
            });
            ui.horizontal(|ui| {
                ui.label("input:");
                ui.text_edit_singleline(&mut self.input_path);
                ui.label("output:");
                ui.text_edit_singleline(&mut self.output_path);
                if ui.button("Run").clicked() {
                    self.run();
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
            let mut viewer = GraphViewer {
                graph: &mut self.project.graph,
            };
            egui_snarl::ui::SnarlWidget::new().show(&mut self.snarl, &mut viewer, ui);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::Value;

    /// Loading a project must recreate the canvas wires from the mapping
    /// graph's node references -- one wire per Call arg, If branch, and
    /// ValueMap input.
    #[test]
    fn build_snarl_recreates_wires_for_every_node_input() {
        let mut graph = Graph::default();
        graph.nodes.insert(
            0,
            Node::SourceField {
                path: vec!["a".into()],
            },
        );
        graph.nodes.insert(
            1,
            Node::Const {
                value: Value::Int(1),
            },
        );
        graph.nodes.insert(
            2,
            Node::Call {
                function: "add".to_string(),
                args: vec![0, 1],
            },
        );
        graph.nodes.insert(
            3,
            Node::If {
                condition: 0,
                then: 1,
                else_: 2,
            },
        );
        graph.nodes.insert(
            4,
            Node::ValueMap {
                input: 2,
                table: vec![],
                default: None,
            },
        );

        let snarl = build_snarl(&graph);

        let mut wires: Vec<(NodeId, NodeId, usize)> = snarl
            .wires()
            .map(|(from, to)| (snarl[from.node], snarl[to.node], to.input))
            .collect();
        wires.sort_unstable();
        assert_eq!(
            wires,
            vec![
                (0, 2, 0), // add arg 0
                (0, 3, 0), // if condition
                (1, 2, 1), // add arg 1
                (1, 3, 1), // if then
                (2, 3, 2), // if else
                (2, 4, 0), // value-map input
            ]
        );
    }
}
