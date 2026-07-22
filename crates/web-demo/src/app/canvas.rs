use std::collections::BTreeMap;

use eframe::egui;
use egui_snarl::ui::{PinInfo, SnarlViewer};
use egui_snarl::{InPin, InPinId, OutPin, OutPinId, Snarl};
use ir::Value;
use mapping::{Graph, Node, NodeId, Scope};

/// What a snarl node on the demo canvas stands for.
pub(super) enum CanvasNode {
    /// One mapping-graph node (indexes into the project graph).
    Graph(NodeId),
    /// The target document: one input pin per binding.
    Target,
}

/// `(label, node)` for every binding, outer scopes first.
pub(super) fn flat_bindings(scope: &Scope, prefix: &str, out: &mut Vec<(String, NodeId)>) {
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
        mapping::SequenceExpr::TokenizeRegex { .. } => "tokenize-regexp",
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
        mapping::SequenceExpr::TokenizeRegex { flags, .. } => {
            if flags.is_some() {
                ["input", "pattern", "flags"]
                    .get(index)
                    .copied()
                    .unwrap_or("input")
            } else {
                ["input", "pattern"].get(index).copied().unwrap_or("input")
            }
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
        | Node::SourceDocumentPath
        | Node::Const { .. }
        | Node::FunctionParameter { .. }
        | Node::RuntimeValue { .. }
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::XmlSerialize { .. } => vec![],
        Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => {
            args.iter().copied().map(Some).collect()
        }
        Node::If {
            condition,
            then,
            else_,
        } => vec![Some(*condition), Some(*then), Some(*else_)],
        Node::ValueMap { input, .. } | Node::Lookup { matches: input, .. } => vec![Some(*input)],
        Node::DynamicSourceField { key, .. } => vec![Some(*key)],
        Node::XmlMixedContent { replacements, .. } => replacements
            .iter()
            .map(|replacement| Some(replacement.expression))
            .collect(),
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
        Node::SequenceItemAt { sequence, index } => sequence
            .inputs()
            .into_iter()
            .map(Some)
            .chain([Some(*index)])
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
        Node::SourceDocumentPath => "source document path".to_string(),
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
        Node::FunctionParameter { parameter } => {
            format!("function parameter {}", parameter.get())
        }
        Node::RuntimeValue { value } => format!("runtime · {value:?}"),
        Node::Call { function, .. } => function.clone(),
        Node::UserFunctionCall { function, .. } => {
            format!("user function {}", function.get())
        }
        Node::If { .. } => "if".to_string(),
        Node::ValueMap { .. } => "value-map".to_string(),
        Node::Lookup { collection, .. } => format!("lookup · {}", collection.join("/")),
        Node::DynamicSourceField { object, .. } => {
            format!("dynamic field · {}", object.join("/"))
        }
        Node::XmlMixedContent { path, .. } => {
            format!("XML mixed content · {}", path.join("/"))
        }
        Node::XmlSerialize { path, .. } => {
            format!("XML serialize · {}", path.join("/"))
        }
        Node::CollectionFind { collection, .. } => {
            format!("find · {}", collection.join("/"))
        }
        Node::SequenceExists { sequence, .. } => {
            format!("exists · {}", sequence_label(sequence))
        }
        Node::SequenceItemAt { sequence, .. } => {
            format!("item-at · {}", sequence_label(sequence))
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
pub(super) fn build_snarl(
    project: &mapping::Project,
    bindings: &[(String, NodeId)],
    compact: bool,
) -> Snarl<CanvasNode> {
    let mut snarl = Snarl::new();
    let mut positions: BTreeMap<NodeId, egui::Pos2> = Default::default();
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

    let mut snarl_ids = BTreeMap::new();
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

pub(super) struct DemoViewer<'a> {
    graph: &'a mut Graph,
    bindings: &'a [(String, NodeId)],
    run_pending: &'a mut bool,
    project_changed: &'a mut bool,
}

impl<'a> DemoViewer<'a> {
    pub(super) fn new(
        graph: &'a mut Graph,
        bindings: &'a [(String, NodeId)],
        run_pending: &'a mut bool,
        project_changed: &'a mut bool,
    ) -> Self {
        Self {
            graph,
            bindings,
            run_pending,
            project_changed,
        }
    }
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
                Some(Node::SequenceExists { sequence, .. }) => {
                    sequence_pin_label(sequence, pin.id.input)
                }
                Some(Node::SequenceItemAt { sequence, .. }) => {
                    if pin.id.input == sequence.inputs().len() {
                        "index".to_string()
                    } else {
                        sequence_pin_label(sequence, pin.id.input)
                    }
                }
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
