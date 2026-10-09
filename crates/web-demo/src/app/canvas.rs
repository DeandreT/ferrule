use std::collections::BTreeMap;

use eframe::egui;
use egui_snarl::ui::{PinInfo, SnarlViewer};
use egui_snarl::{InPin, InPinId, NodeId as CanvasNodeId, OutPin, OutPinId, Snarl};
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
        mapping::SequenceExpr::FilterMapV1(_) => "filter/map (unavailable)",
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
        mapping::SequenceExpr::FilterMapV1(_) => "read-only input",
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
        | Node::SourceRootXmlTypeEquals { .. }
        | Node::SourceRootField { .. }
        | Node::Unconnected
        | Node::Const { .. }
        | Node::FunctionParameter { .. }
        | Node::RuntimeValue { .. }
        | Node::RuntimeParameter { .. }
        | Node::Position { .. }
        | Node::JoinField { .. }
        | Node::JoinPosition { .. }
        | Node::XmlSerialize { .. } => vec![],
        Node::RuntimeParameterDefault { default, .. } => vec![Some(*default)],
        Node::Raise { message } => vec![*message],
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
        Node::SequenceAggregate {
            sequence,
            predicate,
            expression,
            arg,
            ..
        } => sequence
            .inputs()
            .into_iter()
            .map(Some)
            .chain([*predicate, *expression, *arg])
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
        Node::SourceRootXmlTypeEquals { .. } => "primary XML annotation equality".to_string(),
        Node::SourceRootField { path, required } => {
            let title = if *required {
                "required primary field"
            } else {
                "primary field"
            };
            format!("{title} · {}", path.join("/"))
        }
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
        Node::Unconnected => "unconnected".to_string(),
        Node::Const { .. } => "const".to_string(),
        Node::FunctionParameter { parameter } => {
            format!("function parameter {}", parameter.get())
        }
        Node::RuntimeValue { value } => format!("runtime · {value:?}"),
        Node::RuntimeParameter { name, ty, .. } => format!("runtime · {name}: {ty:?}"),
        Node::RuntimeParameterDefault { name, ty, .. } => {
            format!("optional input · {name}: {ty:?}")
        }
        Node::Call { function, .. } => function.clone(),
        Node::UserFunctionCall { function, .. } => {
            format!("user function {}", function.get())
        }
        Node::If { .. } => "if".to_string(),
        Node::Raise { .. } => "Raise error".to_string(),
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
        Node::SequenceAggregate {
            function, sequence, ..
        } => {
            let op = format!("{function:?}").to_lowercase();
            format!("{op} · {}", sequence_label(sequence))
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
    for (&id, node) in &project.graph.nodes {
        if matches!(node, Node::Unconnected) {
            continue;
        }
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
            if let (Some(from), Some(&to)) = (
                feed.and_then(|feed| snarl_ids.get(&feed).copied()),
                snarl_ids.get(&id),
            ) {
                snarl.connect(
                    OutPinId {
                        node: from,
                        output: 0,
                    },
                    InPinId { node: to, input },
                );
            }
        }
    }
    for (i, (_, node)) in bindings.iter().enumerate() {
        if let Some(&from) = snarl_ids.get(node) {
            snarl.connect(
                OutPinId {
                    node: from,
                    output: 0,
                },
                InPinId {
                    node: target,
                    input: i,
                },
            );
        }
    }
    snarl
}

const FIT_MARGIN: f32 = 12.0;
const MAX_FIT_PASSES: u8 = 32;
const MIN_CANVAS_SCALE: f32 = f32::MIN_POSITIVE;
const DEFAULT_MIN_CANVAS_SCALE: f32 = 0.2;

/// A Fit request changes only the canvas transform. Keep measuring until the
/// rendered rectangles and the applied fit agree, with a finite repaint budget.
#[derive(Debug)]
pub(super) struct CanvasView {
    fit_passes: u8,
    minimum_scale: f32,
    rectangles: BTreeMap<CanvasNodeId, egui::Rect>,
    viewport: Option<egui::Rect>,
    transform: Option<egui::emath::TSTransform>,
}

impl Default for CanvasView {
    fn default() -> Self {
        Self {
            fit_passes: MAX_FIT_PASSES,
            minimum_scale: DEFAULT_MIN_CANVAS_SCALE,
            rectangles: BTreeMap::new(),
            viewport: None,
            transform: None,
        }
    }
}

impl CanvasView {
    pub(super) fn request_fit(&mut self) {
        self.fit_passes = MAX_FIT_PASSES;
    }

    pub(super) fn previous_transform(&self) -> Option<egui::emath::TSTransform> {
        self.transform
    }

    pub(super) fn minimum_scale(&self) -> f32 {
        self.minimum_scale
    }

    pub(super) fn prepare(
        &mut self,
        snarl: &Snarl<CanvasNode>,
        viewport: egui::Rect,
    ) -> Option<egui::emath::TSTransform> {
        if self.fit_passes == 0 {
            return None;
        }
        let transform = fit_transform(measured_bounds(snarl, &self.rectangles)?, viewport)?;
        // Only a valid requested Fit establishes a smaller manual zoom floor.
        self.minimum_scale = DEFAULT_MIN_CANVAS_SCALE.min(transform.scaling);
        Some(transform)
    }

    pub(super) fn finish(
        &mut self,
        snarl: &Snarl<CanvasNode>,
        rectangles: BTreeMap<CanvasNodeId, egui::Rect>,
        viewport: egui::Rect,
        transform: Option<egui::emath::TSTransform>,
    ) -> bool {
        let stable = self.fit_passes != 0
            && rectangles == self.rectangles
            && self.viewport == Some(viewport);
        self.rectangles = rectangles;
        self.viewport = Some(viewport);
        self.transform = transform;
        if self.fit_passes == 0 {
            return false;
        }
        let desired = measured_bounds(snarl, &self.rectangles)
            .and_then(|bounds| fit_transform(bounds, viewport));
        if stable && desired.is_some() && desired == self.transform {
            self.fit_passes = 0;
        } else {
            self.fit_passes -= 1;
        }
        self.fit_passes != 0
    }
}

fn measured_bounds(
    snarl: &Snarl<CanvasNode>,
    rectangles: &BTreeMap<CanvasNodeId, egui::Rect>,
) -> Option<Option<egui::Rect>> {
    if rectangles.len() != snarl.node_ids().count() {
        return None;
    }
    let mut bounds = None;
    for (id, _) in snarl.node_ids() {
        let rect = *rectangles.get(&id)?;
        if !rect.is_finite() || rect.min.x > rect.max.x || rect.min.y > rect.max.y {
            return None;
        }
        bounds = Some(bounds.map_or(rect, |bounds: egui::Rect| bounds.union(rect)));
    }
    Some(bounds)
}

pub(super) fn valid_viewport(viewport: egui::Rect) -> bool {
    viewport.is_finite() && viewport.is_positive() && viewport.size().is_finite()
}

fn valid_canvas_transform(transform: egui::emath::TSTransform, viewport: egui::Rect) -> bool {
    if !valid_viewport(viewport)
        || !transform.scaling.is_finite()
        || transform.scaling <= 0.0
        || !transform.translation.is_finite()
    {
        return false;
    }
    let inverse = transform.inverse();
    inverse.scaling.is_finite()
        && inverse.translation.is_finite()
        && (inverse * viewport).is_finite()
}

fn safe_transform(
    current: egui::emath::TSTransform,
    retained: Option<egui::emath::TSTransform>,
    viewport: egui::Rect,
) -> Option<egui::emath::TSTransform> {
    if valid_canvas_transform(current, viewport) {
        Some(current)
    } else {
        retained
            .filter(|transform| valid_canvas_transform(*transform, viewport))
            .or_else(|| fit_transform(None, viewport))
    }
}

#[allow(clippy::cast_possible_truncation)]
fn fit_transform(
    bounds: Option<egui::Rect>,
    viewport: egui::Rect,
) -> Option<egui::emath::TSTransform> {
    if !valid_viewport(viewport) {
        return None;
    }
    let margin = egui::vec2(
        FIT_MARGIN.min(viewport.width() / 4.0),
        FIT_MARGIN.min(viewport.height() / 4.0),
    );
    let inner = viewport.shrink2(margin);
    let screen_x = (f64::from(inner.min.x) + f64::from(inner.max.x)) / 2.0;
    let screen_y = (f64::from(inner.min.y) + f64::from(inner.max.y)) / 2.0;
    let (scaling, x, y) = if let Some(bounds) = bounds {
        if !bounds.is_finite() || bounds.min.x > bounds.max.x || bounds.min.y > bounds.max.y {
            return None;
        }
        let width = (f64::from(bounds.max.x) - f64::from(bounds.min.x)).max(1.0);
        let height = (f64::from(bounds.max.y) - f64::from(bounds.min.y)).max(1.0);
        let magnitude = [bounds.min.x, bounds.min.y, bounds.max.x, bounds.max.y]
            .into_iter()
            .map(|value| f64::from(value).abs())
            .fold(1.0, f64::max);
        // Keep cancellation in the f32 layer transform below a screen pixel
        // even for a small node with a very distant finite world position.
        let scaling = (f64::from(inner.width()) / width)
            .min(f64::from(inner.height()) / height)
            .min(1.0)
            .min(1_048_576.0 / magnitude)
            .max(f64::from(MIN_CANVAS_SCALE)) as f32;
        let x = (f64::from(bounds.min.x) + f64::from(bounds.max.x)) / 2.0;
        let y = (f64::from(bounds.min.y) + f64::from(bounds.max.y)) / 2.0;
        (scaling, x, y)
    } else {
        (1.0, 0.0, 0.0)
    };
    let transform = egui::emath::TSTransform {
        scaling,
        translation: egui::vec2(
            (screen_x - x * f64::from(scaling)) as f32,
            (screen_y - y * f64::from(scaling)) as f32,
        ),
    };
    valid_canvas_transform(transform, viewport).then_some(transform)
}

pub(super) struct DemoViewer<'a> {
    graph: &'a mut Graph,
    target_name: &'a str,
    bindings: &'a [(String, NodeId)],
    run_pending: &'a mut bool,
    project_changed: &'a mut bool,
    edited_constant: &'a mut Option<NodeId>,
    focused_constant: &'a mut Option<NodeId>,
    pub(super) viewport: Option<egui::Rect>,
    pub(super) previous_transform: Option<egui::emath::TSTransform>,
    pub(super) requested_transform: Option<egui::emath::TSTransform>,
    pub(super) rendered_transform: Option<egui::emath::TSTransform>,
    pub(super) rectangles: BTreeMap<CanvasNodeId, egui::Rect>,
}

impl<'a> DemoViewer<'a> {
    pub(super) fn new(
        graph: &'a mut Graph,
        target_name: &'a str,
        bindings: &'a [(String, NodeId)],
        run_pending: &'a mut bool,
        project_changed: &'a mut bool,
        edited_constant: &'a mut Option<NodeId>,
        focused_constant: &'a mut Option<NodeId>,
    ) -> Self {
        Self {
            graph,
            target_name,
            bindings,
            run_pending,
            project_changed,
            edited_constant,
            focused_constant,
            viewport: None,
            previous_transform: None,
            requested_transform: None,
            rendered_transform: None,
            rectangles: BTreeMap::new(),
        }
    }
}

impl SnarlViewer<CanvasNode> for DemoViewer<'_> {
    fn current_transform(
        &mut self,
        to_global: &mut egui::emath::TSTransform,
        _snarl: &mut Snarl<CanvasNode>,
    ) {
        let current = self.requested_transform.unwrap_or(*to_global);
        self.rendered_transform = self
            .viewport
            .and_then(|viewport| safe_transform(current, self.previous_transform, viewport));
        *to_global = self
            .rendered_transform
            .unwrap_or(egui::emath::TSTransform::IDENTITY);
    }

    fn final_node_rect(
        &mut self,
        node: CanvasNodeId,
        rect: egui::Rect,
        _ui: &mut egui::Ui,
        _snarl: &mut Snarl<CanvasNode>,
    ) {
        self.rectangles.insert(node, rect);
    }

    fn title(&mut self, node: &CanvasNode) -> String {
        match node {
            CanvasNode::Target => format!("{} (target)", self.target_name),
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
                Some(Node::Raise { .. }) => "message (optional)".to_string(),
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
            let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(70.0));
            if response.has_focus() {
                *self.focused_constant = Some(id);
            }
            if response.changed() {
                *value = Value::String(text);
                *self.run_pending = true;
                *self.project_changed = true;
                *self.edited_constant = Some(id);
            }
        }
        PinInfo::circle()
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod fit_tests;
