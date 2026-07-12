//! Browser playground for ferrule: a small eframe app around the real
//! `mapping` + `engine` crates, compiled to WebAssembly for the website
//! (and runnable natively for local testing). It ships one built-in
//! project -- the orders/aggregates example -- with an editable source
//! document, a read-only node canvas, editable constants, and live output.

use eframe::egui;
use egui_snarl::ui::{PinInfo, SnarlViewer, SnarlWidget};
use egui_snarl::{InPin, InPinId, OutPin, OutPinId, Snarl};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{AggregateOp, Binding, Graph, Node, NodeId, Project, Scope};

const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Orders>
  <Order>
    <Id>A-1</Id>
    <Item><Price>1.5</Price></Item>
    <Item><Price>2.5</Price></Item>
  </Order>
  <Order>
    <Id>B-2</Id>
    <Item><Price>10.0</Price></Item>
  </Order>
</Orders>
"#;

/// The built-in demo mapping: per-order aggregates plus a joined id list.
fn demo_project() -> Project {
    let source = SchemaNode::group(
        "Orders",
        vec![
            SchemaNode::group(
                "Order",
                vec![
                    SchemaNode::scalar("Id", ScalarType::String),
                    SchemaNode::group("Item", vec![SchemaNode::scalar("Price", ScalarType::Float)])
                        .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let target = SchemaNode::group(
        "Summary",
        vec![
            SchemaNode::scalar("AllIds", ScalarType::String),
            SchemaNode::group(
                "Order",
                vec![
                    SchemaNode::scalar("Id", ScalarType::String),
                    SchemaNode::scalar("ItemCount", ScalarType::Int),
                    SchemaNode::scalar("Total", ScalarType::Float),
                ],
            )
            .repeating(),
        ],
    );

    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Id".into()],
            frame: None,
        },
    );
    graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String(", ".into()),
        },
    );
    graph.nodes.insert(
        2,
        Node::Aggregate {
            function: AggregateOp::Join,
            collection: vec!["Order".into()],
            value: vec!["Id".into()],
            expression: None,
            arg: Some(1),
        },
    );
    graph.nodes.insert(
        3,
        Node::Aggregate {
            function: AggregateOp::Count,
            collection: vec!["Item".into()],
            value: vec![],
            expression: None,
            arg: None,
        },
    );
    graph.nodes.insert(
        4,
        Node::Aggregate {
            function: AggregateOp::Sum,
            collection: vec!["Item".into()],
            value: vec!["Price".into()],
            expression: None,
            arg: None,
        },
    );

    Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            source: None,
            sequence: None,
            filter: None,
            group_by: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            take: None,
            bindings: vec![Binding {
                target_field: "AllIds".into(),
                node: 2,
            }],
            children: vec![Scope {
                target_field: "Order".into(),
                source: Some(vec!["Order".into()]),
                sequence: None,
                filter: None,
                group_by: None,
                group_into_blocks: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![
                    Binding {
                        target_field: "Id".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "ItemCount".into(),
                        node: 3,
                    },
                    Binding {
                        target_field: "Total".into(),
                        node: 4,
                    },
                ],
                children: vec![],
            }],
        },
    }
}

/// What a snarl node on the demo canvas stands for.
enum CanvasNode {
    /// One mapping-graph node (indexes into the project graph).
    Graph(NodeId),
    /// The target document: one input pin per binding.
    Target,
}

/// `(label, node)` for every binding, outer scopes first.
fn flat_bindings(scope: &Scope, prefix: &str, out: &mut Vec<(String, NodeId)>) {
    for binding in &scope.bindings {
        out.push((format!("{prefix}{}", binding.target_field), binding.node));
    }
    for child in &scope.children {
        let child_prefix = format!("{prefix}{}/", child.target_field);
        flat_bindings(child, &child_prefix, out);
    }
}

/// The wired inputs a graph node has (pin order).
fn node_inputs(node: &Node) -> Vec<Option<NodeId>> {
    match node {
        Node::SourceField { .. } | Node::Const { .. } | Node::Position { .. } => vec![],
        Node::Call { args, .. } => args.iter().copied().map(Some).collect(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![Some(*condition), Some(*then), Some(*else_)],
        Node::ValueMap { input, .. } | Node::Lookup { matches: input, .. } => vec![Some(*input)],
        Node::Aggregate {
            expression, arg, ..
        } => vec![*expression, *arg],
    }
}

fn node_title(node: &Node) -> String {
    match node {
        Node::SourceField { path, .. } => format!("field · {}", path.join("/")),
        Node::Position { collection } => format!("position · {}", collection.join("/")),
        Node::Const { .. } => "const".to_string(),
        Node::Call { function, .. } => function.clone(),
        Node::If { .. } => "if".to_string(),
        Node::ValueMap { .. } => "value-map".to_string(),
        Node::Lookup { collection, .. } => format!("lookup · {}", collection.join("/")),
        Node::Aggregate {
            function,
            collection,
            value,
            ..
        } => {
            let mut path = collection.clone();
            path.extend(value.iter().cloned());
            let op = format!("{function:?}").to_lowercase();
            format!("{op} · {}", path.join("/"))
        }
    }
}

/// Builds the canvas: hand-placed nodes plus wires for function arguments
/// and target bindings.
fn build_snarl(project: &Project, bindings: &[(String, NodeId)]) -> Snarl<CanvasNode> {
    let mut snarl = Snarl::new();
    let mut positions: std::collections::BTreeMap<NodeId, egui::Pos2> = Default::default();
    positions.insert(0, egui::pos2(20.0, 30.0));
    positions.insert(1, egui::pos2(20.0, 120.0));
    positions.insert(2, egui::pos2(180.0, 80.0));
    positions.insert(3, egui::pos2(180.0, 175.0));
    positions.insert(4, egui::pos2(180.0, 250.0));

    let mut snarl_ids = std::collections::BTreeMap::new();
    for &id in project.graph.nodes.keys() {
        let pos = positions
            .get(&id)
            .copied()
            .unwrap_or(egui::pos2(120.0, 60.0 + 90.0 * id as f32));
        snarl_ids.insert(id, snarl.insert_node(pos, CanvasNode::Graph(id)));
    }
    let target = snarl.insert_node(egui::pos2(360.0, 60.0), CanvasNode::Target);

    for (&id, node) in &project.graph.nodes {
        for (input, feed) in node_inputs(node).into_iter().enumerate() {
            if let Some(feed) = feed {
                snarl.connect(
                    OutPinId {
                        node: snarl_ids[&feed],
                        output: 0,
                    },
                    InPinId {
                        node: snarl_ids[&id],
                        input,
                    },
                );
            }
        }
    }
    for (i, (_, node)) in bindings.iter().enumerate() {
        snarl.connect(
            OutPinId {
                node: snarl_ids[node],
                output: 0,
            },
            InPinId {
                node: target,
                input: i,
            },
        );
    }
    snarl
}

struct DemoViewer<'a> {
    graph: &'a mut Graph,
    bindings: &'a [(String, NodeId)],
    dirty: &'a mut bool,
}

impl SnarlViewer<CanvasNode> for DemoViewer<'_> {
    fn title(&mut self, node: &CanvasNode) -> String {
        match node {
            CanvasNode::Target => "Summary (target)".to_string(),
            CanvasNode::Graph(id) => self
                .graph
                .nodes
                .get(id)
                .map_or("<missing>".to_string(), node_title),
        }
    }

    fn inputs(&mut self, node: &CanvasNode) -> usize {
        match node {
            CanvasNode::Target => self.bindings.len(),
            CanvasNode::Graph(id) => self.graph.nodes.get(id).map_or(0, |n| node_inputs(n).len()),
        }
    }

    fn outputs(&mut self, node: &CanvasNode) -> usize {
        match node {
            CanvasNode::Target => 0,
            CanvasNode::Graph(_) => 1,
        }
    }

    #[allow(refining_impl_trait)]
    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) -> PinInfo {
        let label = match &snarl[pin.id.node] {
            CanvasNode::Target => self
                .bindings
                .get(pin.id.input)
                .map(|(label, _)| label.clone())
                .unwrap_or_default(),
            CanvasNode::Graph(id) => match self.graph.nodes.get(id) {
                Some(Node::Aggregate { .. }) => ["expr", "sep"][pin.id.input.min(1)].to_string(),
                Some(Node::If { .. }) => ["cond", "then", "else"][pin.id.input.min(2)].to_string(),
                _ => format!("arg {}", pin.id.input),
            },
        };
        ui.label(label);
        PinInfo::circle()
    }

    #[allow(refining_impl_trait)]
    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut egui::Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) -> PinInfo {
        if let CanvasNode::Graph(id) = snarl[pin.id.node]
            && let Some(Node::Const { value }) = self.graph.nodes.get_mut(&id)
        {
            // The one live edit on the canvas: constants.
            let mut text = match &*value {
                Value::String(s) => s.clone(),
                other => format!("{other:?}"),
            };
            if ui
                .add(egui::TextEdit::singleline(&mut text).desired_width(70.0))
                .changed()
            {
                *value = Value::String(text);
                *self.dirty = true;
            }
        }
        PinInfo::circle()
    }
}

struct DemoApp {
    project: Project,
    bindings: Vec<(String, NodeId)>,
    snarl: Snarl<CanvasNode>,
    source_text: String,
    output: String,
    dirty: bool,
}

impl DemoApp {
    fn new() -> Self {
        let project = demo_project();
        let mut bindings = Vec::new();
        flat_bindings(&project.root, "", &mut bindings);
        let snarl = build_snarl(&project, &bindings);
        Self {
            project,
            bindings,
            snarl,
            source_text: SAMPLE_XML.to_string(),
            output: String::new(),
            dirty: true,
        }
    }

    fn run(&mut self) {
        self.output = format_xml::from_str(&self.source_text, &self.project.source)
            .map_err(|e| e.to_string())
            .and_then(|source| engine::run(&self.project, &source).map_err(|e| e.to_string()))
            .and_then(|target| {
                format_xml::to_string(&self.project.target, &target).map_err(|e| e.to_string())
            })
            .unwrap_or_else(|error| format!("error: {error}"));
    }
}

impl eframe::App for DemoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        {
            egui::Panel::top("top").show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("ferrule playground");
                    ui.label("— the real mapping engine, running in your browser");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.hyperlink_to("GitHub", "https://github.com/DeandreT/ferrule");
                        if ui.button("Reset").clicked() {
                            *self = DemoApp::new();
                        }
                    });
                });
                ui.label(
                    "Edit the source XML (left) or the const separator on the canvas — \
                     the output (right) re-runs live. Wires feed function arguments and \
                     target fields; aggregate nodes name the collection they reduce.",
                );
            });

            egui::Panel::left("source").show(ui, |ui| {
                ui.set_min_width(280.0);
                ui.strong("Source · Orders.xml (editable)");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if ui
                        .add(
                            egui::TextEdit::multiline(&mut self.source_text)
                                .code_editor()
                                .desired_width(f32::INFINITY)
                                .desired_rows(24),
                        )
                        .changed()
                    {
                        self.dirty = true;
                    }
                });
            });

            egui::Panel::right("output").show(ui, |ui| {
                ui.set_min_width(280.0);
                ui.strong("Output · Summary.xml");
                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut text = self.output.as_str();
                    ui.add(
                        egui::TextEdit::multiline(&mut text)
                            .code_editor()
                            .desired_width(f32::INFINITY)
                            .desired_rows(24),
                    );
                });
            });

            egui::CentralPanel::default().show(ui, |ui| {
                let mut viewer = DemoViewer {
                    graph: &mut self.project.graph,
                    bindings: &self.bindings,
                    dirty: &mut self.dirty,
                };
                SnarlWidget::new().show(&mut self.snarl, &mut viewer, ui);
            });
        }

        if self.dirty {
            self.dirty = false;
            self.run();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    eframe::run_native(
        "ferrule playground",
        eframe::NativeOptions::default(),
        Box::new(|_cc| Ok(Box::new(DemoApp::new()))),
    )
}

#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document");
        let canvas = document
            .get_element_by_id("demo_canvas")
            .expect("no #demo_canvas element")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("#demo_canvas is not a canvas");
        eframe::WebRunner::new()
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|_cc| Ok(Box::new(DemoApp::new()))),
            )
            .await
            .expect("failed to start eframe");
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_project_runs_on_the_sample_input() {
        let mut app = DemoApp::new();
        app.run();
        assert!(
            app.output.contains("<AllIds>A-1, B-2</AllIds>"),
            "{}",
            app.output
        );
        assert!(
            app.output.contains("<ItemCount>2</ItemCount>"),
            "{}",
            app.output
        );
        assert!(app.output.contains("<Total>10</Total>"), "{}", app.output);
    }
}
