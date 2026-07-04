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

use crate::canvas::{CanvasNode, SourceLeaf, TargetLeaf, source_leaves, target_leaves};
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
        }
    }
}

fn blank_project() -> Project {
    Project {
        source: SchemaNode::group("root", vec![]),
        target: SchemaNode::group("root", vec![]),
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

    let mut snarl_ids = std::collections::BTreeMap::new();
    let shown: Vec<NodeId> = project
        .graph
        .nodes
        .keys()
        .copied()
        .filter(|id| !hidden.contains_key(id))
        .collect();
    for (i, &id) in shown.iter().enumerate() {
        let col = (i % 3) as f32;
        let row = (i / 3) as f32;
        let snarl_id =
            snarl.insert_node(egui::pos2(col * 360.0, row * 160.0), CanvasNode::Graph(id));
        snarl_ids.insert(id, snarl_id);
    }
    let max_row = shown.len().div_ceil(3);
    let target_node = snarl.insert_node(
        egui::pos2(1180.0, (max_row as f32) * 40.0),
        CanvasNode::Target,
    );

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

    // Binding wires: walk the scope tree with its target_field chain and
    // match each binding to a target leaf.
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
    let mut binding_wires = Vec::new();
    walk_scopes(
        &project.root,
        &mut Vec::new(),
        &target_pins,
        &mut binding_wires,
    );
    for (node_id, leaf) in binding_wires {
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
                    self.snarl = build_snarl(&self.project);
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
