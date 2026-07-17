//! Browser playground for ferrule: a small eframe app around the real
//! `mapping` + `engine` crates, compiled to WebAssembly for the website
//! (and runnable natively for local testing). The browser editor supports
//! project JSON, XML/JSON/CSV/XBRL instance text, validation, and live execution.

use eframe::egui;
use egui_snarl::ui::{PinInfo, SnarlViewer, SnarlWidget};
use egui_snarl::{InPin, InPinId, OutPin, OutPinId, Snarl};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, Graph, Node, NodeId, Project, Scope, ScopeConstruction, ScopeIteration,
};
use web_demo::browser_download::download_utf8_text;
use web_demo::project_document::{self, ProjectDocumentError};
use web_demo::runtime::{self, DataFormat, DataSide};

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
        extra_targets: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            iteration: ScopeIteration::None,
            construction: ScopeConstruction::Constructed,
            filter: None,
            group_by: None,
            group_starting_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_filter_order: Default::default(),
            take: None,
            iteration_output: Default::default(),
            bindings: vec![Binding {
                target_field: "AllIds".into(),
                node: 2,
            }],
            dynamic_bindings: Vec::new(),
            children: vec![Scope {
                target_field: "Order".into(),
                iteration: ScopeIteration::Source(vec!["Order".into()]),
                construction: ScopeConstruction::Constructed,
                filter: None,
                group_by: None,
                group_starting_with: None,
                group_into_blocks: None,
                sort_by: None,
                sort_descending: false,
                sort_filter_order: Default::default(),
                take: None,
                iteration_output: Default::default(),
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
                dynamic_bindings: Vec::new(),
                children: vec![],
                dynamic_children: Vec::new(),
                merge_dynamic_fields: false,
            }],
            dynamic_children: Vec::new(),
            merge_dynamic_fields: false,
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
    if let Some(segments) = scope.concatenated() {
        for segment in segments.iter() {
            flat_bindings(segment, prefix, out);
        }
    }
    for child in &scope.children {
        let child_prefix = format!("{prefix}{}/", child.target_field);
        flat_bindings(child, &child_prefix, out);
    }
}

fn sequence_label(sequence: &mapping::SequenceExpr) -> &'static str {
    match sequence {
        mapping::SequenceExpr::Tokenize { .. } => "tokenize",
        mapping::SequenceExpr::TokenizeByLength { .. } => "tokenize-by-length",
        mapping::SequenceExpr::Generate { .. } => "generate-sequence",
        mapping::SequenceExpr::RecursiveCollect { .. } => "recursive-collect",
    }
}

fn sequence_pin_label(sequence: &mapping::SequenceExpr, index: usize) -> String {
    if index == sequence.inputs().len() {
        return "predicate".to_string();
    }
    match sequence {
        mapping::SequenceExpr::Tokenize { .. } => ["input", "delimiter"]
            .get(index)
            .copied()
            .unwrap_or("input"),
        mapping::SequenceExpr::TokenizeByLength { .. } => {
            ["input", "length"].get(index).copied().unwrap_or("input")
        }
        mapping::SequenceExpr::Generate { from: Some(_), .. } => {
            ["from", "to"].get(index).copied().unwrap_or("input")
        }
        mapping::SequenceExpr::Generate { from: None, .. } => "to",
        mapping::SequenceExpr::RecursiveCollect { .. } => ["prefix", "separator"]
            .get(index)
            .copied()
            .unwrap_or("input"),
    }
    .to_string()
}

/// The wired inputs a graph node has (pin order).
fn node_inputs(node: &Node) -> Vec<Option<NodeId>> {
    match node {
        Node::SourceField { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. }
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. } => vec![],
        Node::Call { args, .. } => args.iter().copied().map(Some).collect(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![Some(*condition), Some(*then), Some(*else_)],
        Node::ValueMap { input, .. } | Node::Lookup { matches: input, .. } => vec![Some(*input)],
        Node::DynamicSourceField { key, .. } => vec![Some(*key)],
        Node::CollectionFind {
            predicate, value, ..
        } => vec![Some(*predicate), Some(*value)],
        Node::SequenceExists {
            sequence,
            predicate,
        } => sequence
            .inputs()
            .into_iter()
            .map(Some)
            .chain([Some(*predicate)])
            .collect(),
        Node::Aggregate {
            expression, arg, ..
        }
        | Node::JoinAggregate {
            expression, arg, ..
        } => vec![*expression, *arg],
    }
}

fn node_title(node: &Node) -> String {
    match node {
        Node::SourceField { path, .. } => format!("field · {}", path.join("/")),
        Node::Position { collection } => format!("position · {}", collection.join("/")),
        Node::JoinField {
            join,
            collection,
            path,
        } => {
            let mut field = collection.clone();
            field.extend(path.iter().cloned());
            format!("join {} field · {}", join.get(), field.join("/"))
        }
        Node::JoinPosition { join } => format!("join {} position", join.get()),
        Node::Const { .. } => "const".to_string(),
        Node::RuntimeValue { value } => format!("runtime · {value:?}"),
        Node::Call { function, .. } => function.clone(),
        Node::If { .. } => "if".to_string(),
        Node::ValueMap { .. } => "value-map".to_string(),
        Node::Lookup { collection, .. } => format!("lookup · {}", collection.join("/")),
        Node::DynamicSourceField { object, .. } => {
            format!("dynamic field · {}", object.join("/"))
        }
        Node::CollectionFind { collection, .. } => {
            format!("find · {}", collection.join("/"))
        }
        Node::SequenceExists { sequence, .. } => {
            format!("exists · {}", sequence_label(sequence))
        }
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
        Node::JoinAggregate { function, join, .. } => {
            let op = format!("{function:?}").to_lowercase();
            format!("{op} · join {}", join.get())
        }
    }
}

/// Builds the canvas: hand-placed nodes plus wires for function arguments
/// and target bindings.
fn build_snarl(
    project: &Project,
    bindings: &[(String, NodeId)],
    compact: bool,
) -> Snarl<CanvasNode> {
    let mut snarl = Snarl::new();
    let mut positions: std::collections::BTreeMap<NodeId, egui::Pos2> = Default::default();
    if compact {
        positions.insert(0, egui::pos2(20.0, 30.0));
        positions.insert(1, egui::pos2(20.0, 120.0));
        positions.insert(2, egui::pos2(220.0, 30.0));
        positions.insert(3, egui::pos2(220.0, 145.0));
        positions.insert(4, egui::pos2(220.0, 260.0));
    } else {
        positions.insert(0, egui::pos2(20.0, 30.0));
        positions.insert(1, egui::pos2(20.0, 120.0));
        positions.insert(2, egui::pos2(180.0, 80.0));
        positions.insert(3, egui::pos2(180.0, 175.0));
        positions.insert(4, egui::pos2(180.0, 250.0));
    }

    let mut snarl_ids = std::collections::BTreeMap::new();
    for &id in project.graph.nodes.keys() {
        let pos = positions
            .get(&id)
            .copied()
            .unwrap_or(egui::pos2(120.0, 60.0 + 90.0 * id as f32));
        snarl_ids.insert(id, snarl.insert_node(pos, CanvasNode::Graph(id)));
    }
    let target_position = if compact {
        egui::pos2(120.0, 390.0)
    } else {
        egui::pos2(330.0, 60.0)
    };
    let target = snarl.insert_node(target_position, CanvasNode::Target);

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
    run_pending: &'a mut bool,
    project_changed: &'a mut bool,
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
                Some(Node::Aggregate { .. } | Node::JoinAggregate { .. }) => {
                    ["expr", "arg"][pin.id.input.min(1)].to_string()
                }
                Some(Node::If { .. }) => ["cond", "then", "else"][pin.id.input.min(2)].to_string(),
                Some(Node::SequenceExists {
                    sequence,
                    predicate: _,
                }) => sequence_pin_label(sequence, pin.id.input),
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
                *self.run_pending = true;
                *self.project_changed = true;
            }
        }
        PinInfo::circle()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkspaceView {
    Input,
    Mapping,
    Output,
    Project,
}

struct DemoApp {
    project: Project,
    bindings: Vec<(String, NodeId)>,
    snarl: Snarl<CanvasNode>,
    source_text: String,
    source_format: DataFormat,
    target_format: DataFormat,
    output: String,
    project_json: String,
    status: String,
    diagnostic: Option<String>,
    active_view: WorkspaceView,
    live_run: bool,
    run_pending: bool,
    project_changed: bool,
    canvas_view_generation: u64,
    canvas_compact: bool,
}

impl DemoApp {
    fn new() -> Self {
        let project = demo_project();
        let mut bindings = Vec::new();
        flat_bindings(&project.root, "", &mut bindings);
        let snarl = build_snarl(&project, &bindings, false);
        let (project_json, diagnostic) = match project_document::to_json(&project) {
            Ok(json) => (json, None),
            Err(error) => (String::new(), Some(error.to_string())),
        };
        Self {
            project,
            bindings,
            snarl,
            source_text: SAMPLE_XML.to_string(),
            source_format: DataFormat::Xml,
            target_format: DataFormat::Xml,
            output: String::new(),
            project_json,
            status: "Ready".to_string(),
            diagnostic,
            active_view: WorkspaceView::Mapping,
            live_run: true,
            run_pending: true,
            project_changed: false,
            canvas_view_generation: 0,
            canvas_compact: false,
        }
    }

    fn run(&mut self) {
        self.run_pending = false;
        match runtime::run(
            &self.project,
            &self.source_text,
            self.source_format,
            self.target_format,
        ) {
            Ok(output) => {
                self.output = output;
                self.status = "Mapping completed".to_string();
                self.diagnostic = None;
            }
            Err(error) => {
                self.status = "Run failed".to_string();
                self.diagnostic = Some(error.to_string());
            }
        }
    }

    fn validate(&mut self) {
        let issues = engine::validate(&self.project);
        if issues.is_empty() {
            self.status = "Project is valid".to_string();
            self.diagnostic = None;
        } else {
            self.status = format!("{} validation issue(s)", issues.len());
            self.diagnostic = Some(
                issues
                    .into_iter()
                    .map(|issue| issue.to_string())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    }

    fn apply_project_json(&mut self) {
        match project_document::parse_and_validate(&self.project_json) {
            Ok(project) => {
                let mut bindings = Vec::new();
                flat_bindings(&project.root, "", &mut bindings);
                self.snarl = build_snarl(&project, &bindings, self.canvas_compact);
                self.source_format =
                    boundary_format(&project, DataSide::Source, self.source_format);
                self.target_format =
                    boundary_format(&project, DataSide::Target, self.target_format);
                self.project = project;
                self.bindings = bindings;
                self.canvas_view_generation = self.canvas_view_generation.wrapping_add(1);
                self.project_changed = false;
                self.run_pending = true;
                self.status = "Project applied".to_string();
                self.diagnostic = None;
                self.active_view = WorkspaceView::Mapping;
            }
            Err(error) => {
                self.status = "Project not applied".to_string();
                self.diagnostic = Some(project_document_error(&error));
            }
        }
    }

    fn sync_project_json(&mut self) {
        match project_document::to_json(&self.project) {
            Ok(json) => self.project_json = json,
            Err(error) => {
                self.status = "Project serialization failed".to_string();
                self.diagnostic = Some(error.to_string());
            }
        }
    }

    fn download_project(&mut self) {
        match download_utf8_text("ferrule-project.json", &self.project_json) {
            Ok(()) => self.status = "Project download started".to_string(),
            Err(error) => {
                self.status = "Project download failed".to_string();
                self.diagnostic = Some(error.to_string());
            }
        }
    }

    fn download_output(&mut self) {
        let filename = format!("mapped-output.{}", format_extension(self.target_format));
        match download_utf8_text(&filename, &self.output) {
            Ok(()) => self.status = "Output download started".to_string(),
            Err(error) => {
                self.status = "Output download failed".to_string();
                self.diagnostic = Some(error.to_string());
            }
        }
    }

    fn accept_dropped_project(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        for file in dropped {
            let text = file
                .bytes
                .map(|bytes| String::from_utf8(bytes.to_vec()).map_err(|error| error.to_string()))
                .or_else(|| file.path.as_deref().and_then(read_native_drop));
            let Some(text) = text else {
                continue;
            };
            match text {
                Ok(text) => {
                    self.project_json = text;
                    self.apply_project_json();
                }
                Err(error) => {
                    self.status = "Project drop failed".to_string();
                    self.diagnostic = Some(error);
                }
            }
            break;
        }
    }

    fn show_top_bar(&mut self, ui: &mut egui::Ui, compact: bool) {
        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("ferrule");
                let views: &[(WorkspaceView, &str)] = if compact {
                    &[
                        (WorkspaceView::Input, "Input"),
                        (WorkspaceView::Mapping, "Mapping"),
                        (WorkspaceView::Output, "Output"),
                        (WorkspaceView::Project, "Project"),
                    ]
                } else {
                    &[
                        (WorkspaceView::Mapping, "Mapping"),
                        (WorkspaceView::Project, "Project"),
                    ]
                };
                for &(view, label) in views {
                    ui.selectable_value(&mut self.active_view, view, label);
                }
                ui.separator();
                if ui.button("Run").clicked() {
                    self.run();
                }
                ui.checkbox(&mut self.live_run, "Live");
                if ui.button("Validate").clicked() {
                    self.validate();
                }
                if ui.button("Reset").clicked() {
                    *self = Self::new();
                }
                if ui.button("Fit").clicked() {
                    self.canvas_view_generation = self.canvas_view_generation.wrapping_add(1);
                }
                ui.hyperlink_to("GitHub", "https://github.com/DeandreT/ferrule");
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Input format");
                if format_picker(ui, "source_format", &mut self.source_format) {
                    self.run_pending = true;
                }
                ui.label("Output format");
                if format_picker(ui, "target_format", &mut self.target_format) {
                    self.run_pending = true;
                }
                if ui.button("Download output").clicked() {
                    self.download_output();
                }
                ui.separator();
                ui.label(&self.status);
            });
        });
    }

    fn show_input(&mut self, ui: &mut egui::Ui) {
        ui.strong(format!("Input ({})", self.source_format));
        egui::ScrollArea::vertical().show(ui, |ui| {
            if ui
                .add(
                    egui::TextEdit::multiline(&mut self.source_text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(28),
                )
                .changed()
            {
                self.run_pending = true;
            }
        });
    }

    fn show_output(&mut self, ui: &mut egui::Ui) {
        ui.strong(format!("Output ({})", self.target_format));
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut text = self.output.as_str();
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(28),
            );
        });
    }

    fn show_project(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Project JSON");
            if ui.button("Apply").clicked() {
                self.apply_project_json();
            }
            if ui.button("Download").clicked() {
                self.download_project();
            }
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.project_json)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(30),
            );
        });
    }

    fn show_mapping(&mut self, ui: &mut egui::Ui) {
        let mut viewer = DemoViewer {
            graph: &mut self.project.graph,
            bindings: &self.bindings,
            run_pending: &mut self.run_pending,
            project_changed: &mut self.project_changed,
        };
        SnarlWidget::new()
            .id(egui::Id::new((
                "web_mapping_canvas",
                self.canvas_view_generation,
            )))
            .show(&mut self.snarl, &mut viewer, ui);
    }
}

impl eframe::App for DemoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.accept_dropped_project(ui.ctx());
        let compact = ui.available_width() < 900.0;
        if compact != self.canvas_compact {
            self.canvas_compact = compact;
            self.snarl = build_snarl(&self.project, &self.bindings, compact);
            self.canvas_view_generation = self.canvas_view_generation.wrapping_add(1);
        }
        if !compact
            && matches!(
                self.active_view,
                WorkspaceView::Input | WorkspaceView::Output
            )
        {
            self.active_view = WorkspaceView::Mapping;
        }
        self.show_top_bar(ui, compact);

        if let Some(diagnostic) = &self.diagnostic {
            egui::Panel::bottom("diagnostic")
                .resizable(true)
                .default_size(80.0)
                .show(ui, |ui| {
                    ui.strong("Diagnostics");
                    egui::ScrollArea::vertical().show(ui, |ui| ui.monospace(diagnostic));
                });
        }

        if compact {
            egui::CentralPanel::default().show(ui, |ui| match self.active_view {
                WorkspaceView::Input => self.show_input(ui),
                WorkspaceView::Mapping => self.show_mapping(ui),
                WorkspaceView::Output => self.show_output(ui),
                WorkspaceView::Project => self.show_project(ui),
            });
        } else if self.active_view == WorkspaceView::Project {
            egui::CentralPanel::default().show(ui, |ui| self.show_project(ui));
        } else {
            egui::Panel::left("source")
                .default_size(300.0)
                .min_size(220.0)
                .max_size(420.0)
                .show(ui, |ui| self.show_input(ui));
            egui::Panel::right("output")
                .default_size(300.0)
                .min_size(220.0)
                .max_size(420.0)
                .show(ui, |ui| self.show_output(ui));
            egui::CentralPanel::default().show(ui, |ui| self.show_mapping(ui));
        }

        if self.project_changed {
            self.project_changed = false;
            self.sync_project_json();
        }
        if self.run_pending && self.live_run {
            self.run();
        }
    }
}

fn format_picker(ui: &mut egui::Ui, id: &str, format: &mut DataFormat) -> bool {
    let before = *format;
    egui::ComboBox::from_id_salt(id)
        .selected_text(format.to_string())
        .show_ui(ui, |ui| {
            for choice in [
                DataFormat::Xml,
                DataFormat::Json,
                DataFormat::Csv,
                DataFormat::Xbrl,
            ] {
                ui.selectable_value(format, choice, choice.to_string());
            }
        });
    *format != before
}

fn format_extension(format: DataFormat) -> &'static str {
    match format {
        DataFormat::Xml => "xml",
        DataFormat::Json => "json",
        DataFormat::Csv => "csv",
        DataFormat::Xbrl => "xbrl",
    }
}

fn boundary_format(project: &Project, side: DataSide, current: DataFormat) -> DataFormat {
    let has_xbrl = match side {
        DataSide::Source => project.source_options.xbrl.is_some(),
        DataSide::Target => project.target_options.xbrl.is_some(),
    };
    if has_xbrl {
        DataFormat::Xbrl
    } else if current == DataFormat::Xbrl {
        DataFormat::Xml
    } else {
        current
    }
}

fn project_document_error(error: &ProjectDocumentError) -> String {
    match error {
        ProjectDocumentError::Validation(issues) => issues
            .iter()
            .map(|issue| issue.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        ProjectDocumentError::Serialize(_) | ProjectDocumentError::Parse(_) => error.to_string(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_native_drop(path: &std::path::Path) -> Option<Result<String, String>> {
    (!path.as_os_str().is_empty())
        .then(|| std::fs::read_to_string(path).map_err(|error| error.to_string()))
}

#[cfg(target_arch = "wasm32")]
fn read_native_drop(_path: &std::path::Path) -> Option<Result<String, String>> {
    None
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
        let Some(document) = web_sys::window().and_then(|window| window.document()) else {
            return;
        };
        let Some(element) = document.get_element_by_id("demo_canvas") else {
            return;
        };
        let Ok(canvas) = element.dyn_into::<web_sys::HtmlCanvasElement>() else {
            return;
        };
        let started = eframe::WebRunner::new()
            .start(
                canvas.clone(),
                eframe::WebOptions::default(),
                Box::new(|_cc| Ok(Box::new(DemoApp::new()))),
            )
            .await;
        if started.is_ok() {
            let _ = canvas.set_attribute("data-ferrule-ready", "true");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use mapping::XbrlBoundaryOptions;

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

    #[test]
    fn project_boundaries_select_xbrl_without_leaking_previous_xbrl_state() {
        let mut project = demo_project();
        project.source_options.xbrl = XbrlBoundaryOptions::external_source("taxonomy.xsd").ok();

        assert_eq!(
            boundary_format(&project, DataSide::Source, DataFormat::Json),
            DataFormat::Xbrl
        );
        assert_eq!(
            boundary_format(&project, DataSide::Target, DataFormat::Xbrl),
            DataFormat::Xml
        );
        assert_eq!(
            boundary_format(&project, DataSide::Target, DataFormat::Csv),
            DataFormat::Csv
        );
    }
}
