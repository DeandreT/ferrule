//! Renders and edits a mapping as an egui-snarl canvas of [`CanvasNode`]s:
//! the Source/Target schema endpoints plus the mapping graph's function
//! nodes. The snarl's payload carries no node data -- the mapping graph
//! and scope tree stay the single source of truth, borrowed per frame.
//!
//! `SourceField` nodes whose path corresponds to a source leaf are not
//! shown as canvas nodes: a wire leaving the Source endpoint's pin *is*
//! the source field. Connecting a wire into a Target pin creates or
//! replaces the `Binding` in the scope owning that leaf (the scope whose
//! `target_field` chain matches the leaf's group chain), creating missing
//! non-iterating scopes for nested target groups.

use egui::Ui;
use egui_snarl::ui::{NodeLayout, PinInfo, SnarlViewer};
use egui_snarl::{InPin, InPinId, NodeId as SnarlNodeId, OutPin, Snarl};
use ir::{ScalarType, Value};
use mapping::{
    AggregateOp, Binding, FunctionId, FunctionParameterId, Graph, NamedTarget, Node, NodeId, Scope,
};

use crate::appearance::{SemanticThemeColors, WireColorMode};
use crate::canvas::{CanvasNode, SourceBlock, SourceLeaf, TargetBlock, TargetLeaf};
use crate::canvas_endpoints::EndpointDisplayPin;
use crate::path_picker::SourcePathCatalog;
use crate::value_editor::{show_value_editor, show_value_map_editor};
use crate::wire_colors::WireEmphasis;

#[path = "graph_references.rs"]
mod graph_references;
#[path = "graph_sequence.rs"]
mod graph_sequence;
#[path = "node_palette.rs"]
mod node_palette;

use graph_references::node_inputs;
use node_palette::NodeTemplate;

#[cfg(test)]
const ENDPOINT_LABEL_CHAR_LIMIT: usize = 30;
const GRAPH_TITLE_CHAR_LIMIT: usize = 36;
const SOURCE_FIELD_EDIT_WIDTH: f32 = 170.0;
const PATH_EDITOR_WIDTH: f32 = 250.0;

#[cfg(test)]
fn compact_endpoint_label(path: &str) -> String {
    compact_endpoint_label_to(path, ENDPOINT_LABEL_CHAR_LIMIT)
}

fn compact_endpoint_label_to(path: &str, limit: usize) -> String {
    if path.chars().count() <= limit {
        return path.to_string();
    }

    let segments = path.split('/').collect::<Vec<_>>();
    if segments.len() > 2 {
        let tail = format!(
            ".../{}/{}",
            segments[segments.len() - 2],
            segments[segments.len() - 1]
        );
        if tail.chars().count() <= limit {
            return tail;
        }
    }

    let suffix = path
        .chars()
        .rev()
        .take(limit.saturating_sub(3))
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("...{suffix}")
}

fn compact_graph_title(label: &str) -> String {
    if label.chars().count() <= GRAPH_TITLE_CHAR_LIMIT {
        return label.to_string();
    }
    let suffix = label
        .chars()
        .rev()
        .take(GRAPH_TITLE_CHAR_LIMIT - 3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();
    format!("...{suffix}")
}

fn show_lookup_editor(
    ui: &mut Ui,
    source_paths: &SourcePathCatalog,
    collection: &mut Vec<String>,
    key: &mut Vec<String>,
    value: &mut Vec<String>,
) {
    ui.set_min_width(PATH_EDITOR_WIDTH);
    ui.set_max_width(PATH_EDITOR_WIDTH);
    egui::Grid::new(ui.id().with("lookup_paths")).show(ui, |ui| {
        ui.label("collection");
        source_paths.show_collection_picker(ui, ui.id().with("lookup_collection"), collection);
        ui.end_row();
        ui.label("");
        source_paths.show_value_picker(ui, ui.id().with("lookup_key"), collection, key);
        ui.end_row();
        ui.label("value");
        source_paths.show_value_picker(ui, ui.id().with("lookup_value"), collection, value);
        ui.end_row();
    });
}

fn show_endpoint_label(
    ui: &mut Ui,
    path: &str,
    align: egui::Align,
    hover_text: impl Into<egui::WidgetText>,
    highlighted: bool,
) {
    let clip_rect = ui.clip_rect();
    let row_height = ui.spacing().interact_size.y;
    let row_rect = egui::Rect::from_min_max(
        egui::pos2(clip_rect.left(), ui.max_rect().top()),
        egui::pos2(clip_rect.right(), ui.max_rect().top() + row_height),
    );
    let (anchor, text_align) = match align {
        egui::Align::Min => (
            egui::pos2(row_rect.left() + row_height, row_rect.center().y),
            egui::Align2::LEFT_CENTER,
        ),
        egui::Align::Center => (row_rect.center(), egui::Align2::CENTER_CENTER),
        egui::Align::Max => (
            egui::pos2(row_rect.right() - row_height, row_rect.center().y),
            egui::Align2::RIGHT_CENTER,
        ),
    };
    let label_limit = ((row_rect.width() - row_height * 2.0) / 7.0)
        .floor()
        .clamp(12.0, 64.0) as usize;
    if highlighted {
        ui.painter().rect_filled(
            row_rect.shrink2(egui::vec2(row_height, 1.0)),
            2.0,
            ui.visuals().selection.bg_fill,
        );
    }
    ui.painter().text(
        anchor,
        text_align,
        compact_endpoint_label_to(path, label_limit),
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    ui.interact(row_rect, ui.next_auto_id(), egui::Sense::hover())
        .on_hover_text(hover_text);
}

fn show_endpoint_proxy_label(
    ui: &mut Ui,
    hidden: usize,
    above: bool,
    align: egui::Align,
    source: bool,
    connected: bool,
) {
    let direction = if above { "above" } else { "below" };
    let label = if above {
        format!("^ {hidden} hidden")
    } else {
        format!("v {hidden} hidden")
    };
    let side = if source { "source" } else { "target" };
    let connection = if connected {
        "Connected offscreen fields are routed through this edge pin. "
    } else {
        ""
    };
    show_endpoint_label(
        ui,
        &label,
        align,
        format!("{connection}Scroll to show {hidden} hidden {side} field(s) {direction}"),
        false,
    );
}

pub struct GraphViewer<'a> {
    pub graph: &'a mut Graph,
    pub root_scope: &'a mut Scope,
    pub extra_targets: &'a [NamedTarget],
    pub source_blocks: &'a [SourceBlock],
    pub target_blocks: &'a [TargetBlock],
    pub source_x12: bool,
    pub target_x12: bool,
    pub source_paths: &'a SourcePathCatalog,
    pub function_names: std::collections::BTreeMap<FunctionId, String>,
    pub function_inputs: std::collections::BTreeMap<FunctionId, Vec<String>>,
    pub parameter_names: std::collections::BTreeMap<FunctionParameterId, String>,
    pub protected_output: Option<NodeId>,
    pub requested_function_open: Option<FunctionId>,
    pub colors: SemanticThemeColors,
    pub wire_color_mode: WireColorMode,
    pub endpoint_scroll: &'a mut crate::canvas_endpoints::EndpointScrollState,
    pub endpoint_search_match: Option<(CanvasNode, usize)>,
    pub node_sizes: Option<&'a mut std::collections::BTreeMap<CanvasNode, egui::Vec2>>,
    pub hovered_node: Option<SnarlNodeId>,
    pub hovered_node_this_frame: Option<SnarlNodeId>,
    pub camera_pan: egui::Vec2,
    pub camera_focus: Option<(egui::Pos2, egui::Pos2, Option<f32>)>,
    pub canvas_transform: Option<egui::emath::TSTransform>,
    pub pin_interaction_ids: Vec<egui::Id>,
    /// Set when an interaction can't be completed (e.g. binding into a
    /// scope that doesn't exist yet); the app surfaces it in the status
    /// line.
    pub error: Option<String>,
}

fn input_wire_emphasis(hovered_node: Option<SnarlNodeId>, pin: &InPin) -> WireEmphasis {
    let Some(hovered_node) = hovered_node else {
        return WireEmphasis::Normal;
    };
    if pin.id.node == hovered_node || pin.remotes.iter().any(|remote| remote.node == hovered_node) {
        WireEmphasis::Incident
    } else {
        WireEmphasis::Unrelated
    }
}

fn output_wire_emphasis(hovered_node: Option<SnarlNodeId>, pin: &OutPin) -> WireEmphasis {
    let Some(hovered_node) = hovered_node else {
        return WireEmphasis::Normal;
    };
    if pin.id.node == hovered_node || pin.remotes.iter().any(|remote| remote.node == hovered_node) {
        WireEmphasis::Incident
    } else {
        WireEmphasis::Unrelated
    }
}

fn apply_camera_focus(
    transform: &mut egui::emath::TSTransform,
    graph_point: egui::Pos2,
    screen_point: egui::Pos2,
    zoom: Option<f32>,
) {
    if let Some(zoom) = zoom {
        transform.scaling = zoom;
    }
    transform.translation = screen_point.to_vec2() - graph_point.to_vec2() * transform.scaling;
}

impl GraphViewer<'_> {
    fn endpoint_at(
        &self,
        graph_position: egui::Pos2,
        snarl: &Snarl<CanvasNode>,
    ) -> Option<(CanvasNode, usize)> {
        let sizes = self.node_sizes.as_deref()?;
        snarl
            .nodes_pos_ids()
            .filter_map(|(_, position, &node)| {
                let total = match node {
                    CanvasNode::SourceBlock(block) => self.source_blocks.get(block)?.leaves.len(),
                    CanvasNode::TargetBlock(block) => self.target_blocks.get(block)?.leaves.len(),
                    CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => return None,
                };
                let size = sizes.get(&node)?;
                egui::Rect::from_min_size(position, *size)
                    .expand(4.0)
                    .contains(graph_position)
                    .then_some((node, total))
            })
            .last()
    }

    pub fn scroll_endpoint_at(
        &mut self,
        graph_position: egui::Pos2,
        delta_y: f32,
        snarl: &mut Snarl<CanvasNode>,
    ) -> bool {
        if delta_y == 0.0 || self.camera_focus.is_some() {
            return false;
        }
        let Some((node, total)) = self.endpoint_at(graph_position, snarl) else {
            return false;
        };
        let old = self.endpoint_scroll.offset(node, total);
        let rows = crate::canvas_endpoints::scroll_rows(delta_y);
        let max = total.saturating_sub(self.endpoint_scroll.visible_limit(node, total));
        let can_scroll = (rows < 0 && old > 0) || (rows > 0 && old < max);
        if !can_scroll {
            return false;
        }

        if self.endpoint_scroll.scroll_rows(node, total, rows) {
            crate::app::sync_endpoint_wires(
                self.graph,
                self.root_scope,
                self.source_blocks,
                self.target_blocks,
                self.endpoint_scroll,
                snarl,
            );
            true
        } else {
            false
        }
    }

    pub fn begin_node_hover_frame(&mut self, hovered_node: Option<SnarlNodeId>) {
        self.hovered_node = hovered_node;
        self.hovered_node_this_frame = None;
    }

    pub const fn end_node_hover_frame(&self) -> Option<SnarlNodeId> {
        self.hovered_node_this_frame
    }

    fn input_wire_color(&self, pin: &InPin, node: CanvasNode, input: usize) -> egui::Color32 {
        let base = crate::wire_colors::input_color(self.wire_color_mode, self.colors, node, input);
        crate::wire_colors::with_emphasis(
            base,
            self.colors.canvas.to_egui(),
            input_wire_emphasis(self.hovered_node, pin),
        )
    }

    fn output_wire_color(&self, pin: &OutPin) -> egui::Color32 {
        let base = crate::wire_colors::output_color(self.wire_color_mode, self.colors);
        crate::wire_colors::with_emphasis(
            base,
            self.colors.canvas.to_egui(),
            output_wire_emphasis(self.hovered_node, pin),
        )
    }

    fn record_pin_interaction_id(&mut self, ui: &Ui) {
        // egui-snarl 0.11 creates the pin's drag widget with this exact next
        // auto ID immediately after `show_input`/`show_output` returns.
        self.pin_interaction_ids.push(ui.next_auto_id());
    }

    fn fresh_id(&self) -> NodeId {
        self.graph.nodes.keys().next_back().map_or(0, |max| max + 1)
    }

    fn fresh_unconnected(&mut self) -> NodeId {
        let id = self.fresh_id();
        self.graph.nodes.insert(id, Node::Unconnected);
        id
    }

    fn mapping_id(node: CanvasNode) -> Option<NodeId> {
        match node {
            CanvasNode::Graph(id) | CanvasNode::Placeholder(id) => Some(id),
            CanvasNode::SourceBlock(_) | CanvasNode::TargetBlock(_) => None,
        }
    }

    fn source_leaf(&self, block: usize, pin: usize) -> Option<&SourceLeaf> {
        let section = self.source_blocks.get(block)?;
        let semantic = self.endpoint_scroll.semantic_pin(
            CanvasNode::SourceBlock(block),
            pin,
            section.leaves.len(),
        )?;
        section.leaves.get(semantic)
    }

    fn target_leaf(&self, block: usize, pin: usize) -> Option<&TargetLeaf> {
        let section = self.target_blocks.get(block)?;
        let semantic = self.endpoint_scroll.semantic_pin(
            CanvasNode::TargetBlock(block),
            pin,
            section.leaves.len(),
        )?;
        section.leaves.get(semantic)
    }

    fn endpoint_display_pin(
        &self,
        node: CanvasNode,
        displayed_pin: usize,
    ) -> Option<EndpointDisplayPin> {
        let total = match node {
            CanvasNode::SourceBlock(block) => self.source_blocks.get(block)?.leaves.len(),
            CanvasNode::TargetBlock(block) => self.target_blocks.get(block)?.leaves.len(),
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => {
                return Some(EndpointDisplayPin::Visible(displayed_pin));
            }
        };
        self.endpoint_scroll.display_pin(node, displayed_pin, total)
    }

    fn insert_palette_node(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        pos: egui::Pos2,
        template: NodeTemplate,
    ) -> (NodeId, SnarlNodeId) {
        match template {
            NodeTemplate::Constant => self.insert(snarl, pos, Node::Const { value: Value::Null }),
            NodeTemplate::SourceField => self.insert(
                snarl,
                pos,
                Node::SourceField {
                    path: Vec::new(),
                    frame: None,
                },
            ),
            NodeTemplate::Position => self.insert(
                snarl,
                pos,
                Node::Position {
                    collection: Vec::new(),
                },
            ),
            NodeTemplate::Call => self.insert(
                snarl,
                pos,
                Node::Call {
                    function: "concat".to_string(),
                    args: Vec::new(),
                },
            ),
            NodeTemplate::If => {
                self.insert_with_unconnected_inputs(snarl, pos, 3, |inputs| Node::If {
                    condition: inputs[0],
                    then: inputs[1],
                    else_: inputs[2],
                })
            }
            NodeTemplate::ValueMap => {
                self.insert_with_unconnected_inputs(snarl, pos, 1, |inputs| Node::ValueMap {
                    input: inputs[0],
                    input_type: None,
                    table: Vec::new(),
                    default: None,
                })
            }
            NodeTemplate::Lookup => {
                self.insert_with_unconnected_inputs(snarl, pos, 1, |inputs| Node::Lookup {
                    collection: Vec::new(),
                    key: Vec::new(),
                    matches: inputs[0],
                    value: Vec::new(),
                })
            }
            NodeTemplate::CollectionFind => {
                self.insert_with_unconnected_inputs(snarl, pos, 2, |inputs| Node::CollectionFind {
                    collection: Vec::new(),
                    predicate: inputs[0],
                    value: inputs[1],
                })
            }
            NodeTemplate::Aggregate(function) => {
                let inputs = usize::from(node_palette::aggregate_needs_arg(function));
                self.insert_with_unconnected_inputs(snarl, pos, inputs, |ids| {
                    node_palette::aggregate_node(function, ids.first().copied())
                })
            }
        }
    }

    fn insert(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        pos: egui::Pos2,
        node: Node,
    ) -> (NodeId, SnarlNodeId) {
        let id = self.fresh_id();
        self.graph.nodes.insert(id, node);
        let snarl_id = snarl.insert_node(pos, CanvasNode::Graph(id));
        (id, snarl_id)
    }

    fn insert_with_unconnected_inputs(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        pos: egui::Pos2,
        input_count: usize,
        build: impl FnOnce(&[NodeId]) -> Node,
    ) -> (NodeId, SnarlNodeId) {
        let inputs = (0..input_count)
            .map(|_| self.fresh_unconnected())
            .collect::<Vec<_>>();
        self.insert(snarl, pos, build(&inputs))
    }

    /// Reuses an existing `SourceField` with this exact frame and relative
    /// path, or creates one. These nodes back Source-pin wires.
    fn source_field_for(&mut self, frame: Option<Vec<String>>, path: Vec<String>) -> NodeId {
        let existing = self.graph.nodes.iter().find_map(|(id, node)| match node {
            Node::SourceField { path: p, frame: f } if p == &path && f == &frame => Some(*id),
            _ => None,
        });
        existing.unwrap_or_else(|| {
            let id = self.fresh_id();
            self.graph
                .nodes
                .insert(id, Node::SourceField { path, frame });
            id
        })
    }

    fn set_input(&mut self, node_id: NodeId, idx: usize, from_id: NodeId) -> bool {
        let Some(node) = self.graph.nodes.get_mut(&node_id) else {
            return false;
        };
        if idx >= Self::input_count(node) {
            return false;
        }
        match node {
            Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => {
                args[idx] = from_id;
            }
            Node::If {
                condition,
                then,
                else_,
            } => match idx {
                0 => *condition = from_id,
                1 => *then = from_id,
                2 => *else_ = from_id,
                _ => return false,
            },
            Node::ValueMap { input, .. } => *input = from_id,
            Node::Lookup { matches, .. } => *matches = from_id,
            Node::DynamicSourceField { key, .. } => *key = from_id,
            Node::CollectionFind {
                predicate, value, ..
            } => match idx {
                0 => *predicate = from_id,
                1 => *value = from_id,
                _ => return false,
            },
            Node::SequenceExists {
                sequence,
                predicate,
            } => {
                let sequence_inputs = sequence.inputs().len();
                if idx < sequence_inputs {
                    graph_sequence::set_input(sequence, idx, from_id);
                } else if idx == sequence_inputs {
                    *predicate = from_id;
                }
            }
            Node::SequenceItemAt { sequence, index } => {
                let sequence_inputs = sequence.inputs().len();
                if idx < sequence_inputs {
                    graph_sequence::set_input(sequence, idx, from_id);
                } else if idx == sequence_inputs {
                    *index = from_id;
                }
            }
            Node::Aggregate {
                expression, arg, ..
            }
            | Node::JoinAggregate {
                expression, arg, ..
            } => {
                if expression.is_some() && idx == 0 {
                    *expression = Some(from_id);
                } else if arg.is_some() && idx == usize::from(expression.is_some()) {
                    *arg = Some(from_id);
                }
            }
            _ => return false,
        }
        true
    }

    fn depends_on(&self, start: NodeId, needle: NodeId) -> bool {
        let mut pending = vec![start];
        let mut visited = std::collections::BTreeSet::new();
        while let Some(id) = pending.pop() {
            if id == needle {
                return true;
            }
            if visited.insert(id)
                && let Some(node) = self.graph.nodes.get(&id)
            {
                pending.extend(node_inputs(node));
            }
        }
        false
    }

    fn input_at(&self, node_id: NodeId, idx: usize) -> Option<NodeId> {
        match self.graph.nodes.get(&node_id)? {
            Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.get(idx).copied(),
            Node::If {
                condition,
                then,
                else_,
            } => [*condition, *then, *else_].get(idx).copied(),
            Node::ValueMap { input, .. } => (idx == 0).then_some(*input),
            Node::Lookup { matches, .. } => (idx == 0).then_some(*matches),
            Node::DynamicSourceField { key, .. } => (idx == 0).then_some(*key),
            Node::XmlMixedContent { replacements, .. } => replacements
                .get(idx)
                .map(|replacement| replacement.expression),
            Node::CollectionFind {
                predicate, value, ..
            } => [*predicate, *value].get(idx).copied(),
            Node::SequenceExists {
                sequence,
                predicate,
            } => graph_sequence::input_at(sequence, idx)
                .or_else(|| (idx == sequence.inputs().len()).then_some(*predicate)),
            Node::SequenceItemAt { sequence, index } => graph_sequence::input_at(sequence, idx)
                .or_else(|| (idx == sequence.inputs().len()).then_some(*index)),
            Node::Aggregate {
                expression, arg, ..
            }
            | Node::JoinAggregate {
                expression, arg, ..
            } => expression.iter().chain(arg).nth(idx).copied(),
            Node::SourceField { .. }
            | Node::SourceDocumentPath
            | Node::Position { .. }
            | Node::JoinField { .. }
            | Node::JoinPosition { .. }
            | Node::Unconnected
            | Node::Const { .. }
            | Node::FunctionParameter { .. }
            | Node::RuntimeValue { .. }
            | Node::RuntimeParameter { .. }
            | Node::XmlSerialize { .. } => None,
        }
    }

    fn scope_for_chain<'s>(scope: &'s mut Scope, chain: &[String]) -> Option<&'s mut Scope> {
        let Some((first, rest)) = chain.split_first() else {
            return Some(scope);
        };
        let child = scope
            .children
            .iter_mut()
            .find(|c| c.target_field == *first)?;
        Self::scope_for_chain(child, rest)
    }

    fn ensure_scope_for_chain<'s>(scope: &'s mut Scope, chain: &[String]) -> &'s mut Scope {
        let Some((first, rest)) = chain.split_first() else {
            return scope;
        };
        let child_index = scope
            .children
            .iter()
            .position(|child| child.target_field == *first)
            .unwrap_or_else(|| {
                scope.children.push(Scope {
                    target_field: first.clone(),
                    ..Scope::default()
                });
                scope.children.len() - 1
            });
        Self::ensure_scope_for_chain(&mut scope.children[child_index], rest)
    }

    /// Points the binding for `leaf` at `node`, creating any missing static,
    /// non-iterating target scopes along the way.
    fn set_binding(&mut self, leaf: &TargetLeaf, node: NodeId) {
        let scope = Self::ensure_scope_for_chain(self.root_scope, &leaf.chain);
        match scope
            .bindings
            .iter_mut()
            .find(|b| b.target_field == leaf.field)
        {
            Some(binding) => binding.node = node,
            None => scope.bindings.push(Binding {
                target_field: leaf.field.clone(),
                node,
            }),
        }
    }

    fn remove_binding(&mut self, leaf: &TargetLeaf) {
        if let Some(scope) = Self::scope_for_chain(self.root_scope, &leaf.chain) {
            scope.bindings.retain(|b| b.target_field != leaf.field);
        }
    }

    fn binding_node(&mut self, leaf: &TargetLeaf) -> Option<NodeId> {
        Self::scope_for_chain(self.root_scope, &leaf.chain)?
            .bindings
            .iter()
            .find(|binding| binding.target_field == leaf.field)
            .map(|binding| binding.node)
    }

    fn references_to(&self, needle: NodeId) -> Vec<String> {
        let mut references = graph_references::references_to(
            self.graph,
            self.root_scope,
            self.extra_targets,
            needle,
        );
        if self.protected_output == Some(needle) {
            references.push("function output".to_string());
        }
        references
    }

    fn blocking_references_to(&self, needle: NodeId) -> Vec<String> {
        let removable =
            graph_references::removable_wire_references(self.graph, self.root_scope, needle);
        self.references_to(needle)
            .into_iter()
            .filter(|reference| !removable.contains(reference))
            .collect()
    }

    fn disconnect_graph_consumers(&mut self, needle: NodeId, snarl: &mut Snarl<CanvasNode>) {
        let consumers =
            self.graph
                .nodes
                .iter()
                .filter(|(owner, _)| **owner != needle)
                .flat_map(|(&owner, node)| {
                    node_inputs(node).into_iter().enumerate().filter_map(
                        move |(input, dependency)| (dependency == needle).then_some((owner, input)),
                    )
                })
                .collect::<Vec<_>>();
        for (owner, input) in consumers {
            let unconnected = self.fresh_unconnected();
            self.set_input(owner, input, unconnected);
            let Some(node) = snarl.node_ids().find_map(|(node, canvas)| {
                (Self::mapping_id(*canvas) == Some(owner)).then_some(node)
            }) else {
                continue;
            };
            let input = InPinId { node, input };
            for remote in snarl.in_pin(input).remotes {
                snarl.disconnect(remote, input);
            }
        }
    }

    fn remove_orphaned_input(&mut self, needle: NodeId, snarl: &mut Snarl<CanvasNode>) {
        if !self.references_to(needle).is_empty() {
            return;
        }
        if matches!(self.graph.nodes.get(&needle), Some(Node::Unconnected)) {
            self.graph.nodes.remove(&needle);
            return;
        }
        let placeholder = snarl
            .node_ids()
            .find_map(|(id, &node)| (node == CanvasNode::Placeholder(needle)).then_some(id));
        if let Some(placeholder) = placeholder {
            snarl.remove_node(placeholder);
            self.graph.nodes.remove(&needle);
        } else {
            let shown = snarl
                .nodes()
                .copied()
                .filter_map(Self::mapping_id)
                .any(|id| id == needle);
            if !shown
                && matches!(
                    self.graph.nodes.get(&needle),
                    Some(Node::SourceField { .. })
                )
            {
                self.graph.nodes.remove(&needle);
            }
        }
    }

    fn remove_graph_node(
        &mut self,
        mapping_id: NodeId,
        node: SnarlNodeId,
        snarl: &mut Snarl<CanvasNode>,
    ) -> bool {
        let references = self.blocking_references_to(mapping_id);
        if !references.is_empty() {
            self.error = Some(format!(
                "mapping node {mapping_id} is still used by {}",
                references.join(", ")
            ));
            return false;
        }
        self.disconnect_graph_consumers(mapping_id, snarl);
        graph_references::remove_bindings_to(self.root_scope, mapping_id);
        let inputs = self
            .graph
            .nodes
            .get(&mapping_id)
            .map(node_inputs)
            .unwrap_or_default();
        self.graph.nodes.remove(&mapping_id);
        snarl.remove_node(node);
        for input in inputs {
            self.remove_orphaned_input(input, snarl);
        }
        true
    }

    pub fn remove_snarl_nodes(
        &mut self,
        selected: &[SnarlNodeId],
        snarl: &mut Snarl<CanvasNode>,
    ) -> usize {
        let mut pending = selected
            .iter()
            .filter_map(|&node| {
                snarl
                    .get_node(node)
                    .and_then(|canvas| Self::mapping_id(*canvas))
                    .map(|mapping| (mapping, node))
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut removed = 0;
        loop {
            let removable = pending.iter().find_map(|(&mapping, &node)| {
                self.blocking_references_to(mapping)
                    .is_empty()
                    .then_some((mapping, node))
            });
            let Some((mapping, node)) = removable else {
                break;
            };
            if self.remove_graph_node(mapping, node, snarl) {
                removed += 1;
            }
            pending.remove(&mapping);
        }
        if !pending.is_empty() {
            let blocked = pending
                .keys()
                .map(|mapping| mapping.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            self.error = Some(format!(
                "selected mapping node(s) {blocked} are still owned by a mapping control"
            ));
        }
        removed
    }

    fn input_count(node: &Node) -> usize {
        match node {
            Node::SourceField { .. }
            | Node::SourceDocumentPath
            | Node::Position { .. }
            | Node::JoinField { .. }
            | Node::JoinPosition { .. }
            | Node::Unconnected
            | Node::Const { .. }
            | Node::FunctionParameter { .. }
            | Node::RuntimeValue { .. }
            | Node::RuntimeParameter { .. }
            | Node::XmlSerialize { .. } => 0,
            Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.len(),
            Node::If { .. } => 3,
            Node::ValueMap { .. } | Node::Lookup { .. } | Node::DynamicSourceField { .. } => 1,
            Node::XmlMixedContent { replacements, .. } => replacements.len(),
            Node::CollectionFind { .. } => 2,
            Node::SequenceExists {
                sequence,
                predicate: _,
            } => sequence.inputs().len() + 1,
            Node::SequenceItemAt { sequence, .. } => sequence.inputs().len() + 1,
            Node::Aggregate {
                expression, arg, ..
            }
            | Node::JoinAggregate {
                expression, arg, ..
            } => usize::from(expression.is_some()) + usize::from(arg.is_some()),
        }
    }
}

impl SnarlViewer<CanvasNode> for GraphViewer<'_> {
    fn node_layout(
        &mut self,
        default: NodeLayout,
        node: SnarlNodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        snarl: &Snarl<CanvasNode>,
    ) -> NodeLayout {
        if Self::mapping_id(snarl[node])
            .is_some_and(|id| matches!(self.graph.nodes.get(&id), Some(Node::Lookup { .. })))
        {
            NodeLayout::sandwich()
        } else {
            default
        }
    }

    fn current_transform(
        &mut self,
        to_global: &mut egui::emath::TSTransform,
        _snarl: &mut Snarl<CanvasNode>,
    ) {
        if let Some((graph_point, screen_point, zoom)) = self.camera_focus {
            apply_camera_focus(to_global, graph_point, screen_point, zoom);
        }
        to_global.translation += self.camera_pan;
        self.canvas_transform = Some(*to_global);
    }

    fn title(&mut self, node: &CanvasNode) -> String {
        match node {
            CanvasNode::SourceBlock(block) => self
                .source_blocks
                .get(*block)
                .map_or_else(|| "Source".to_string(), |section| section.title.clone()),
            CanvasNode::TargetBlock(block) => self
                .target_blocks
                .get(*block)
                .map_or_else(|| "Target".to_string(), |section| section.title.clone()),
            CanvasNode::Graph(id) | CanvasNode::Placeholder(id) => {
                let title = match self.graph.nodes.get(id) {
                    Some(Node::SourceField { path, frame }) => {
                        let owner = frame
                            .as_ref()
                            .and_then(|frame| frame.last())
                            .map(|owner| format!("{owner}/"))
                            .unwrap_or_default();
                        compact_graph_title(&format!("Source: {owner}{}", path.join("/")))
                    }
                    Some(Node::SourceDocumentPath) => "Source document path".to_string(),
                    Some(Node::Position { collection }) if collection.is_empty() => {
                        "Position".to_string()
                    }
                    Some(Node::Position { collection }) => {
                        format!("Position: {}", collection.join("/"))
                    }
                    Some(Node::JoinField {
                        join,
                        collection,
                        path,
                    }) => {
                        let mut display = collection.clone();
                        display.extend(path.iter().cloned());
                        format!("Join field #{}: {}", join.get(), display.join("/"))
                    }
                    Some(Node::JoinPosition { join }) => {
                        format!("Join position #{}", join.get())
                    }
                    Some(Node::Unconnected) => "Unconnected".to_string(),
                    Some(Node::Const { value }) => {
                        format!("Const: {}", crate::value_editor::display_string(value))
                    }
                    Some(Node::FunctionParameter { parameter }) => {
                        self.parameter_names.get(parameter).map_or_else(
                            || "Function input".to_string(),
                            |name| format!("Input: {name}"),
                        )
                    }
                    Some(Node::RuntimeValue { value }) => format!("Runtime: {value:?}"),
                    Some(Node::RuntimeParameter { name, ty }) => {
                        format!("Runtime input: {name} ({ty:?})")
                    }
                    Some(Node::Call { function, .. }) => format!("Call: {function}"),
                    Some(Node::UserFunctionCall { function, .. }) => {
                        self.function_names.get(function).map_or_else(
                            || "Call: <missing function>".to_string(),
                            |name| format!("Call: {name}"),
                        )
                    }
                    Some(Node::If { .. }) => "If".to_string(),
                    Some(Node::ValueMap { .. }) => "Value Map".to_string(),
                    Some(Node::Lookup { collection, .. }) => {
                        format!("Lookup: {}", collection.join("/"))
                    }
                    Some(Node::DynamicSourceField { object, .. }) => {
                        format!("Dynamic field: {}", object.join("/"))
                    }
                    Some(Node::XmlMixedContent { path, .. }) => {
                        format!("XML mixed content: {}", path.join("/"))
                    }
                    Some(Node::XmlSerialize { path, .. }) => {
                        let path = if path.is_empty() {
                            "<current>".to_string()
                        } else {
                            path.join("/")
                        };
                        format!("XML serialize: {path}")
                    }
                    Some(Node::CollectionFind { collection, .. }) => {
                        format!("Find: {}", collection.join("/"))
                    }
                    Some(Node::SequenceExists { sequence, .. }) => {
                        format!("Exists: {}", graph_sequence::label(sequence))
                    }
                    Some(Node::SequenceItemAt { sequence, .. }) => {
                        format!("Item at: {}", graph_sequence::label(sequence))
                    }
                    Some(Node::Aggregate {
                        function,
                        collection,
                        value,
                        expression,
                        ..
                    }) => {
                        let mut path = collection.clone();
                        if expression.is_none() {
                            path.extend(value.iter().cloned());
                        }
                        let op = format!("{function:?}").to_lowercase();
                        let target =
                            expression.map_or_else(|| path.join("/"), |_| "computed".into());
                        format!("{op}: {target}")
                    }
                    Some(Node::JoinAggregate {
                        function,
                        join,
                        expression,
                        ..
                    }) => {
                        let op = format!("{function:?}").to_lowercase();
                        let target = if expression.is_some() {
                            "computed "
                        } else {
                            ""
                        };
                        format!("{op}: {target}join #{}", join.get())
                    }
                    None => "<missing>".to_string(),
                };
                if self.protected_output == Some(*id) {
                    format!("{title} (output)")
                } else {
                    title
                }
            }
        }
    }

    fn show_header(
        &mut self,
        node: SnarlNodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        let canvas_node = snarl[node];
        let (endpoint_width, endpoint_hint) = match canvas_node {
            CanvasNode::SourceBlock(block) => {
                self.source_blocks
                    .get(block)
                    .map_or((None, None), |section| {
                        (
                            Some(
                                crate::app::endpoint_block_size(
                                    &section.title,
                                    &section.pin_labels,
                                )
                                .x,
                            ),
                            crate::x12_tooltips::endpoint_header_hint(
                                self.source_x12,
                                &section.title,
                                section
                                    .frame
                                    .as_deref()
                                    .and_then(|frame| frame.last())
                                    .map(String::as_str),
                            ),
                        )
                    })
            }
            CanvasNode::TargetBlock(block) => {
                self.target_blocks
                    .get(block)
                    .map_or((None, None), |section| {
                        (
                            Some(
                                crate::app::endpoint_block_size(
                                    &section.title,
                                    &section.pin_labels,
                                )
                                .x,
                            ),
                            crate::x12_tooltips::endpoint_header_hint(
                                self.target_x12,
                                &section.title,
                                section.chain.last().map(String::as_str),
                            ),
                        )
                    })
            }
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => (None, None),
        };
        if let Some(width) = endpoint_width {
            let width = self.endpoint_scroll.width(canvas_node, width);
            // Account for the nested node/header frame margins. Pin labels are
            // painted independently so right-to-left rows cannot grow sideways.
            ui.set_min_width((width - 32.0).max(0.0));
        }
        let function_call = Self::mapping_id(canvas_node)
            .and_then(|id| self.graph.nodes.get(&id))
            .and_then(|node| match node {
                Node::UserFunctionCall { function, .. } => Some(*function),
                _ => None,
            });
        let response = if let Some(function) = function_call {
            ui.horizontal(|ui| {
                let response = ui.label(self.title(&canvas_node));
                let open = ui
                    .add(egui::Button::new(crate::icons::text(
                        lucide_icons::Icon::ExternalLink,
                        12.0,
                    )))
                    .on_hover_text("Open function mapping");
                if response.double_clicked() || open.clicked() {
                    self.requested_function_open = Some(function);
                }
                response
            })
            .inner
        } else {
            ui.label(self.title(&canvas_node))
        };
        if let Some(hint) = endpoint_hint {
            response.on_hover_text(hint);
        }
    }

    fn has_footer(&mut self, node: &CanvasNode) -> bool {
        matches!(
            node,
            CanvasNode::SourceBlock(_) | CanvasNode::TargetBlock(_)
        )
    }

    fn show_footer(
        &mut self,
        _node: SnarlNodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        _snarl: &mut Snarl<CanvasNode>,
    ) {
        // Reserve a compact control strip so the resize grip never overlaps
        // the last endpoint pin.
        ui.allocate_space(egui::vec2(1.0, 8.0));
    }

    fn final_node_rect(
        &mut self,
        node: SnarlNodeId,
        rect: egui::Rect,
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        if ui.rect_contains_pointer(rect) {
            self.hovered_node_this_frame = Some(node);
        }
        let canvas_node = snarl[node];
        let endpoint = match canvas_node {
            CanvasNode::SourceBlock(block) => self.source_blocks.get(block).map(|section| {
                (
                    section.leaves.len(),
                    crate::app::endpoint_block_size(&section.title, &section.pin_labels).x,
                    self.colors.source.to_egui(),
                )
            }),
            CanvasNode::TargetBlock(block) => self.target_blocks.get(block).map(|section| {
                (
                    section.leaves.len(),
                    crate::app::endpoint_block_size(&section.title, &section.pin_labels).x,
                    self.colors.target.to_egui(),
                )
            }),
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => None,
        };
        if let Some((total, natural_width, accent)) = endpoint {
            let scrolled = crate::canvas_endpoints::show_scrollbar(
                ui,
                canvas_node,
                rect,
                total,
                self.endpoint_scroll,
                accent,
            );
            let resized = crate::canvas_endpoints::show_resize_handles(
                ui,
                canvas_node,
                rect,
                total,
                natural_width,
                self.endpoint_scroll,
            );
            if scrolled || resized {
                ui.ctx().request_repaint();
            }
        }
        let size = rect.size();
        if size.x.is_finite()
            && size.y.is_finite()
            && size.x > 1.0
            && size.y > 1.0
            && let Some(node_sizes) = self.node_sizes.as_deref_mut()
        {
            node_sizes.insert(canvas_node, size);
        }
    }

    fn inputs(&mut self, node: &CanvasNode) -> usize {
        match node {
            CanvasNode::SourceBlock(_) => 0,
            CanvasNode::TargetBlock(block) => self.target_blocks.get(*block).map_or(0, |section| {
                self.endpoint_scroll
                    .display_pin_count(*node, section.leaves.len())
            }),
            CanvasNode::Graph(id) | CanvasNode::Placeholder(id) => {
                self.graph.nodes.get(id).map_or(0, Self::input_count)
            }
        }
    }

    fn outputs(&mut self, node: &CanvasNode) -> usize {
        match node {
            CanvasNode::SourceBlock(block) => self.source_blocks.get(*block).map_or(0, |section| {
                self.endpoint_scroll
                    .display_pin_count(*node, section.leaves.len())
            }),
            CanvasNode::TargetBlock(_) => 0,
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => 1,
        }
    }

    fn has_body(&mut self, node: &CanvasNode) -> bool {
        Self::mapping_id(*node)
            .is_some_and(|id| matches!(self.graph.nodes.get(&id), Some(Node::Lookup { .. })))
    }

    fn show_body(
        &mut self,
        node: SnarlNodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        let Some(node_id) = Self::mapping_id(snarl[node]) else {
            return;
        };
        let source_paths = self.source_paths;
        if let Some(Node::Lookup {
            collection,
            key,
            value,
            ..
        }) = self.graph.nodes.get_mut(&node_id)
        {
            show_lookup_editor(ui, source_paths, collection, key, value);
        }
    }

    #[allow(refining_impl_trait)]
    fn show_input(&mut self, pin: &InPin, ui: &mut Ui, snarl: &mut Snarl<CanvasNode>) -> PinInfo {
        let idx = pin.id.input;
        let canvas_node = snarl[pin.id.node];
        let endpoint_pin = self.endpoint_display_pin(canvas_node, idx);
        let semantic_idx = match endpoint_pin {
            Some(EndpointDisplayPin::Visible(semantic)) => semantic,
            Some(EndpointDisplayPin::HiddenBefore | EndpointDisplayPin::HiddenAfter) => idx,
            None => idx,
        };
        let fill = match canvas_node {
            CanvasNode::SourceBlock(_) => self.colors.source,
            CanvasNode::TargetBlock(_) => self.colors.target,
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => self.colors.transform,
        };
        let label = match snarl[pin.id.node] {
            CanvasNode::TargetBlock(_) => None,
            CanvasNode::SourceBlock(_) => Some(String::new()),
            CanvasNode::Graph(id) | CanvasNode::Placeholder(id) => {
                Some(match self.graph.nodes.get(&id) {
                    Some(Node::Call { .. }) => format!("arg {idx}"),
                    Some(Node::UserFunctionCall { function, .. }) => self
                        .function_inputs
                        .get(function)
                        .and_then(|parameters| parameters.get(idx))
                        .cloned()
                        .unwrap_or_else(|| format!("input {}", idx + 1)),
                    Some(Node::If { .. }) => ["condition", "then", "else"][idx].to_string(),
                    Some(Node::ValueMap { .. }) => "input".to_string(),
                    Some(Node::Lookup { .. }) => "match/key".to_string(),
                    Some(Node::DynamicSourceField { .. }) => "property name".to_string(),
                    Some(Node::CollectionFind { .. }) => ["predicate", "value"][idx].to_string(),
                    Some(Node::SequenceExists { sequence, .. }) => {
                        graph_sequence::pin_label(sequence, idx).to_string()
                    }
                    Some(Node::SequenceItemAt { sequence, .. }) => {
                        if idx == sequence.inputs().len() {
                            "index".to_string()
                        } else {
                            graph_sequence::pin_label(sequence, idx).to_string()
                        }
                    }
                    Some(
                        Node::Aggregate { expression, .. } | Node::JoinAggregate { expression, .. },
                    ) if expression.is_some() && idx == 0 => "values".to_string(),
                    Some(Node::Aggregate { .. } | Node::JoinAggregate { .. }) => "arg".to_string(),
                    _ => String::new(),
                })
            }
        };
        if let CanvasNode::TargetBlock(block) = snarl[pin.id.node] {
            if let Some(section) = self.target_blocks.get(block) {
                match endpoint_pin {
                    Some(EndpointDisplayPin::Visible(semantic_idx)) => {
                        if let (Some(leaf), Some(label)) = (
                            section.leaves.get(semantic_idx),
                            section.pin_labels.get(semantic_idx),
                        ) {
                            let state = if pin.remotes.is_empty() {
                                "Unmapped target"
                            } else {
                                "Mapped target"
                            };
                            let hover = crate::x12_tooltips::append_segment_for_path(
                                format!("{state}: {}", leaf.label),
                                self.target_x12,
                                &leaf.label,
                            );
                            show_endpoint_label(
                                ui,
                                label,
                                egui::Align::Min,
                                hover,
                                self.endpoint_search_match == Some((canvas_node, semantic_idx)),
                            );
                        }
                    }
                    Some(EndpointDisplayPin::HiddenBefore) => show_endpoint_proxy_label(
                        ui,
                        self.endpoint_scroll
                            .hidden_before(canvas_node, section.leaves.len()),
                        true,
                        egui::Align::Min,
                        false,
                        !pin.remotes.is_empty(),
                    ),
                    Some(EndpointDisplayPin::HiddenAfter) => show_endpoint_proxy_label(
                        ui,
                        self.endpoint_scroll
                            .hidden_after(canvas_node, section.leaves.len()),
                        false,
                        egui::Align::Min,
                        false,
                        !pin.remotes.is_empty(),
                    ),
                    None => {}
                }
            }
        } else if let Some(label) = label {
            ui.label(label);
        }
        self.record_pin_interaction_id(ui);
        let pin_info = if matches!(
            endpoint_pin,
            Some(EndpointDisplayPin::HiddenBefore | EndpointDisplayPin::HiddenAfter)
        ) {
            PinInfo::square()
        } else {
            PinInfo::circle()
        };
        pin_info
            .with_fill(fill.to_egui())
            .with_wire_color(self.input_wire_color(pin, canvas_node, semantic_idx))
    }

    #[allow(refining_impl_trait)]
    fn show_output(&mut self, pin: &OutPin, ui: &mut Ui, snarl: &mut Snarl<CanvasNode>) -> PinInfo {
        let canvas_node = snarl[pin.id.node];
        let fill = match canvas_node {
            CanvasNode::SourceBlock(_) => self.colors.source,
            CanvasNode::TargetBlock(_) => self.colors.target,
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => self.colors.transform,
        };
        let Some(node_id) = Self::mapping_id(canvas_node) else {
            if let CanvasNode::SourceBlock(block) = canvas_node
                && let Some(section) = self.source_blocks.get(block)
            {
                match self.endpoint_scroll.display_pin(
                    CanvasNode::SourceBlock(block),
                    pin.id.output,
                    section.leaves.len(),
                ) {
                    Some(EndpointDisplayPin::Visible(semantic)) => {
                        if let (Some(leaf), Some(label)) = (
                            section.leaves.get(semantic),
                            section.pin_labels.get(semantic),
                        ) {
                            let context = leaf.frame.as_ref().map_or_else(
                                || format!("Source: {}", leaf.label),
                                |frame| {
                                    let frame = if frame.is_empty() {
                                        "document rows".to_string()
                                    } else {
                                        frame.join("/")
                                    };
                                    format!("Source: {}\nRepeating context: {frame}", leaf.label)
                                },
                            );
                            let hover = crate::x12_tooltips::append_segment_for_path(
                                context,
                                self.source_x12,
                                &leaf.label,
                            );
                            show_endpoint_label(
                                ui,
                                label,
                                egui::Align::Max,
                                hover,
                                self.endpoint_search_match
                                    == Some((CanvasNode::SourceBlock(block), semantic)),
                            );
                        }
                    }
                    Some(EndpointDisplayPin::HiddenBefore) => show_endpoint_proxy_label(
                        ui,
                        self.endpoint_scroll
                            .hidden_before(CanvasNode::SourceBlock(block), section.leaves.len()),
                        true,
                        egui::Align::Max,
                        true,
                        !pin.remotes.is_empty(),
                    ),
                    Some(EndpointDisplayPin::HiddenAfter) => show_endpoint_proxy_label(
                        ui,
                        self.endpoint_scroll
                            .hidden_after(CanvasNode::SourceBlock(block), section.leaves.len()),
                        false,
                        egui::Align::Max,
                        true,
                        !pin.remotes.is_empty(),
                    ),
                    None => {}
                }
            }
            self.record_pin_interaction_id(ui);
            let pin_info = if matches!(
                self.endpoint_display_pin(canvas_node, pin.id.output),
                Some(EndpointDisplayPin::HiddenBefore | EndpointDisplayPin::HiddenAfter)
            ) {
                PinInfo::square()
            } else {
                PinInfo::circle()
            };
            return pin_info
                .with_fill(fill.to_egui())
                .with_wire_color(self.output_wire_color(pin));
        };
        let mut new_call_arg_needed = false;
        let mut remove_call_wire = None;
        let mut new_aggregate_arg_needed = false;
        let mut remove_aggregate_wire = None;
        if let Some(node) = self.graph.nodes.get_mut(&node_id) {
            match node {
                Node::SourceField { path, frame } => {
                    let mut joined = path.join("/");
                    if ui
                        .add_sized(
                            [SOURCE_FIELD_EDIT_WIDTH, ui.spacing().interact_size.y],
                            egui::TextEdit::singleline(&mut joined),
                        )
                        .on_hover_text(if joined.is_empty() {
                            "Source path".to_string()
                        } else {
                            joined.clone()
                        })
                        .changed()
                    {
                        *path = joined
                            .split('/')
                            .map(str::to_string)
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                    if let Some(frame) = frame {
                        ui.label(format!(
                            "@{}",
                            frame.last().map(String::as_str).unwrap_or("frame")
                        ))
                        .on_hover_text(format!("source frame: {}", frame.join("/")));
                    }
                }
                Node::SourceDocumentPath => {
                    ui.label("current source document path");
                }
                Node::Position { collection } => {
                    self.source_paths.show_collection_picker(
                        ui,
                        ui.id().with("position_collection"),
                        collection,
                    );
                }
                Node::JoinField {
                    join,
                    collection,
                    path,
                } => {
                    let mut display = collection.clone();
                    display.extend(path.iter().cloned());
                    ui.label(format!("#{} {}", join.get(), display.join("/")))
                        .on_hover_text("field projected from an imported inner join");
                }
                Node::JoinPosition { join } => {
                    ui.label(format!("#{}", join.get()))
                        .on_hover_text("flattened inner-join position");
                }
                Node::Unconnected => {
                    ui.weak("unconnected input");
                }
                Node::Const { value } => show_value_editor(ui, value),
                Node::FunctionParameter { parameter } => {
                    ui.label(
                        self.parameter_names
                            .get(parameter)
                            .map(String::as_str)
                            .unwrap_or("missing parameter"),
                    );
                }
                Node::RuntimeValue { value } => {
                    ui.label(format!("{value:?}"));
                }
                Node::RuntimeParameter { name, ty } => {
                    ui.horizontal(|ui| {
                        ui.label("name");
                        ui.text_edit_singleline(name);
                    });
                    egui::ComboBox::from_id_salt(ui.id().with("runtime_parameter_type"))
                        .selected_text(format!("{ty:?}").to_lowercase())
                        .show_ui(ui, |ui| {
                            for candidate in [
                                ScalarType::String,
                                ScalarType::Int,
                                ScalarType::Float,
                                ScalarType::Bool,
                            ] {
                                ui.selectable_value(
                                    ty,
                                    candidate,
                                    format!("{candidate:?}").to_lowercase(),
                                );
                            }
                        });
                }
                Node::Call { function, args } => {
                    egui::ComboBox::from_id_salt(ui.id().with("builtin"))
                        .selected_text(function.as_str())
                        .show_ui(ui, |ui| {
                            for builtin in functions::BUILTIN_NAMES {
                                ui.selectable_value(function, (*builtin).to_string(), *builtin);
                            }
                        });
                    ui.horizontal(|ui| {
                        if ui.small_button("+arg").clicked() {
                            new_call_arg_needed = true;
                        }
                        if !args.is_empty() && ui.small_button("-arg").clicked() {
                            let input = args.len() - 1;
                            remove_call_wire = args.pop().map(|node| (input, node));
                        }
                    });
                }
                Node::UserFunctionCall { function, args } => {
                    let name = self
                        .function_names
                        .get(function)
                        .map(String::as_str)
                        .unwrap_or("missing function");
                    ui.label(format!(
                        "{name} ({} input{})",
                        args.len(),
                        if args.len() == 1 { "" } else { "s" }
                    ));
                }
                Node::If { .. } => {
                    ui.label("condition ? then : else");
                }
                Node::ValueMap { table, default, .. } => show_value_map_editor(ui, table, default),
                Node::Lookup { .. } => {
                    ui.label("result");
                }
                Node::DynamicSourceField { object, frame, .. } => {
                    ui.label(format!(
                        "open source object: {}{}",
                        frame
                            .as_ref()
                            .map(|path| format!("{}/", path.join("/")))
                            .unwrap_or_default(),
                        object.join("/")
                    ));
                }
                Node::XmlMixedContent {
                    path, replacements, ..
                } => {
                    ui.label(format!(
                        "{} ({} replacement{})",
                        if path.is_empty() {
                            "<current>".to_string()
                        } else {
                            path.join("/")
                        },
                        replacements.len(),
                        if replacements.len() == 1 { "" } else { "s" }
                    ));
                }
                Node::XmlSerialize {
                    path,
                    declaration,
                    indent,
                    namespace,
                    ..
                } => {
                    let source = if path.is_empty() {
                        "<current>".to_string()
                    } else {
                        path.join("/")
                    };
                    ui.label(format!("source: {source}"));
                    ui.checkbox(declaration, "XML declaration");
                    ui.checkbox(indent, "indent output");
                    if let Some(namespace) = namespace {
                        ui.label(namespace.as_str())
                            .on_hover_text("default namespace");
                    }
                }
                Node::CollectionFind { collection, .. } => {
                    ui.horizontal(|ui| {
                        ui.label("collection");
                        self.source_paths.show_collection_picker(
                            ui,
                            ui.id().with("find_collection"),
                            collection,
                        );
                    });
                }
                Node::SequenceExists { sequence, .. } => {
                    ui.label(format!(
                        "any {} item matches",
                        graph_sequence::label(sequence)
                    ));
                }
                Node::SequenceItemAt { sequence, .. } => {
                    ui.label(format!(
                        "select one {} item",
                        graph_sequence::label(sequence)
                    ));
                }
                Node::Aggregate {
                    function,
                    collection,
                    value,
                    expression,
                    arg,
                } => {
                    let previous = *function;
                    ui.allocate_ui_with_layout(
                        egui::vec2(PATH_EDITOR_WIDTH, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::Grid::new(ui.id().with("aggregate_paths")).show(ui, |ui| {
                                ui.label("collection");
                                self.source_paths.show_collection_picker(
                                    ui,
                                    ui.id().with("aggregate_collection"),
                                    collection,
                                );
                                ui.end_row();
                                if expression.is_some() || arg.is_some() {
                                    ui.label("");
                                } else {
                                    ui.label("operation");
                                }
                                egui::ComboBox::from_id_salt(ui.id().with("aggregate_op"))
                                    .selected_text(
                                        node_palette::AGGREGATE_OPS
                                            .iter()
                                            .find(|(op, _)| op == function)
                                            .map_or("Aggregate", |(_, label)| *label),
                                    )
                                    .show_ui(ui, |ui| {
                                        for (op, label) in node_palette::AGGREGATE_OPS {
                                            ui.selectable_value(function, op, label);
                                        }
                                    });
                                ui.end_row();
                                if expression.is_some() {
                                    ui.label("value");
                                    ui.label("computed");
                                    ui.end_row();
                                } else if *function != AggregateOp::Count {
                                    ui.label("value");
                                    self.source_paths.show_value_picker(
                                        ui,
                                        ui.id().with("aggregate_value"),
                                        collection,
                                        value,
                                    );
                                    ui.end_row();
                                }
                            });
                        },
                    );
                    if previous != *function {
                        if node_palette::aggregate_needs_arg(*function) && arg.is_none() {
                            new_aggregate_arg_needed = true;
                        } else if !node_palette::aggregate_needs_arg(*function) {
                            remove_aggregate_wire = arg.take();
                        }
                    }
                }
                Node::JoinAggregate {
                    function,
                    join,
                    expression,
                    ..
                } => {
                    let op = format!("{function:?}").to_lowercase();
                    ui.label(format!("{op} over join #{}", join.get()))
                        .on_hover_text(if expression.is_some() {
                            "computed expression evaluated once per joined tuple"
                        } else {
                            "aggregate evaluated over joined tuples"
                        });
                }
            }
        }
        if new_call_arg_needed {
            let new_id = self.fresh_unconnected();
            if let Some(Node::Call { args, .. }) = self.graph.nodes.get_mut(&node_id) {
                args.push(new_id);
            }
        }
        if let Some((input_index, removed)) = remove_call_wire {
            let input = InPinId {
                node: pin.id.node,
                input: input_index,
            };
            let remotes = snarl.in_pin(input).remotes;
            for remote in remotes {
                snarl.disconnect(remote, input);
            }
            self.remove_orphaned_input(removed, snarl);
        }
        if new_aggregate_arg_needed {
            let new_id = self.fresh_unconnected();
            if let Some(Node::Aggregate { arg, .. }) = self.graph.nodes.get_mut(&node_id) {
                *arg = Some(new_id);
            }
        }
        if let Some(removed) = remove_aggregate_wire {
            let expression_input = self.graph.nodes.get(&node_id).is_some_and(|node| {
                matches!(
                    node,
                    Node::Aggregate {
                        expression: Some(_),
                        ..
                    }
                )
            });
            let input = InPinId {
                node: pin.id.node,
                input: usize::from(expression_input),
            };
            let remotes = snarl.in_pin(input).remotes;
            for remote in remotes {
                snarl.disconnect(remote, input);
            }
            self.remove_orphaned_input(removed, snarl);
        }
        self.record_pin_interaction_id(ui);
        PinInfo::circle()
            .with_fill(fill.to_egui())
            .with_wire_color(self.output_wire_color(pin))
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<CanvasNode>) {
        self.error = None;
        let from_node = snarl[from.id.node];
        let to_node = snarl[to.id.node];
        let mutation = (|| -> Result<Option<NodeId>, String> {
            match (from_node, to_node) {
                (
                    CanvasNode::SourceBlock(source_block),
                    CanvasNode::Graph(to_id) | CanvasNode::Placeholder(to_id),
                ) => {
                    let source_leaf = self
                        .source_leaf(source_block, from.id.output)
                        .cloned()
                        .ok_or_else(|| format!("source pin {} does not exist", from.id.output))?;
                    let to_node = self
                        .graph
                        .nodes
                        .get(&to_id)
                        .ok_or_else(|| format!("mapping node {to_id} does not exist"))?;
                    if to.id.input >= Self::input_count(to_node) {
                        return Err(format!(
                            "input {} does not exist on mapping node {to_id}",
                            to.id.input
                        ));
                    }
                    let displaced = self.input_at(to_id, to.id.input);
                    // The graph retains independent ownership after this pin
                    // catalog is rebuilt on the next UI frame.
                    let field =
                        self.source_field_for(source_leaf.frame.clone(), source_leaf.path.clone());
                    if !self.set_input(to_id, to.id.input, field) {
                        self.remove_orphaned_input(field, snarl);
                        return Err(format!(
                            "input {} could not be updated on mapping node {to_id}",
                            to.id.input
                        ));
                    }
                    Ok(displaced)
                }
                (CanvasNode::SourceBlock(source_block), CanvasNode::TargetBlock(target_block)) => {
                    let source_leaf = self
                        .source_leaf(source_block, from.id.output)
                        .cloned()
                        .ok_or_else(|| format!("source pin {} does not exist", from.id.output))?;
                    let target_leaf = self
                        .target_leaf(target_block, to.id.input)
                        .cloned()
                        .ok_or_else(|| format!("target pin {} does not exist", to.id.input))?;
                    let displaced = self.binding_node(&target_leaf);
                    let field =
                        self.source_field_for(source_leaf.frame.clone(), source_leaf.path.clone());
                    self.set_binding(&target_leaf, field);
                    Ok(displaced)
                }
                (
                    CanvasNode::Graph(from_id) | CanvasNode::Placeholder(from_id),
                    CanvasNode::TargetBlock(target_block),
                ) => {
                    if from.id.output != 0 || !self.graph.nodes.contains_key(&from_id) {
                        return Err(format!(
                            "output {} does not exist on mapping node {from_id}",
                            from.id.output
                        ));
                    }
                    let target_leaf = self
                        .target_leaf(target_block, to.id.input)
                        .cloned()
                        .ok_or_else(|| format!("target pin {} does not exist", to.id.input))?;
                    let displaced = self.binding_node(&target_leaf);
                    self.set_binding(&target_leaf, from_id);
                    Ok(displaced)
                }
                (
                    CanvasNode::Graph(from_id) | CanvasNode::Placeholder(from_id),
                    CanvasNode::Graph(to_id) | CanvasNode::Placeholder(to_id),
                ) => {
                    if from.id.output != 0 || !self.graph.nodes.contains_key(&from_id) {
                        return Err(format!(
                            "output {} does not exist on mapping node {from_id}",
                            from.id.output
                        ));
                    }
                    let to_node = self
                        .graph
                        .nodes
                        .get(&to_id)
                        .ok_or_else(|| format!("mapping node {to_id} does not exist"))?;
                    if to.id.input >= Self::input_count(to_node) {
                        return Err(format!(
                            "input {} does not exist on mapping node {to_id}",
                            to.id.input
                        ));
                    }
                    if self.depends_on(from_id, to_id) {
                        return Err(format!(
                            "connection from mapping node {from_id} to {to_id} would create a cycle"
                        ));
                    }
                    let displaced = self.input_at(to_id, to.id.input);
                    if !self.set_input(to_id, to.id.input, from_id) {
                        return Err(format!(
                            "input {} could not be updated on mapping node {to_id}",
                            to.id.input
                        ));
                    }
                    Ok(displaced)
                }
                _ => Err("these canvas pins cannot be connected".to_string()),
            }
        })();
        let displaced = match mutation {
            Ok(displaced) => displaced,
            Err(error) => {
                self.error = Some(error);
                return;
            }
        };
        // Every input takes exactly one value, so replace any existing wire.
        for &remote in &to.remotes {
            snarl.disconnect(remote, to.id);
        }
        snarl.connect(from.id, to.id);
        if let Some(displaced) = displaced {
            self.remove_orphaned_input(displaced, snarl);
        }
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<CanvasNode>) {
        let disconnected = match snarl[to.id.node] {
            CanvasNode::Graph(to_id) | CanvasNode::Placeholder(to_id) => {
                self.input_at(to_id, to.id.input)
            }
            CanvasNode::TargetBlock(block) => self
                .target_leaf(block, to.id.input)
                .cloned()
                .and_then(|leaf| self.binding_node(&leaf)),
            CanvasNode::SourceBlock(_) => None,
        };
        match (snarl[from.id.node], snarl[to.id.node]) {
            (_, CanvasNode::TargetBlock(block)) => {
                if let Some(leaf) = self.target_leaf(block, to.id.input).cloned() {
                    self.remove_binding(&leaf);
                }
            }
            (_, CanvasNode::Graph(to_id) | CanvasNode::Placeholder(to_id)) => {
                snarl.disconnect(from.id, to.id);
                let unconnected = self.fresh_unconnected();
                self.set_input(to_id, to.id.input, unconnected);
                if let Some(disconnected) = disconnected {
                    self.remove_orphaned_input(disconnected, snarl);
                }
                return;
            }
            _ => {}
        }
        snarl.disconnect(from.id, to.id);
        if let Some(disconnected) = disconnected {
            self.remove_orphaned_input(disconnected, snarl);
        }
    }

    fn has_graph_menu(&mut self, _pos: egui::Pos2, _snarl: &mut Snarl<CanvasNode>) -> bool {
        true
    }

    fn show_graph_menu(&mut self, pos: egui::Pos2, ui: &mut Ui, snarl: &mut Snarl<CanvasNode>) {
        if let Some(template) = node_palette::show(ui) {
            self.insert_palette_node(snarl, pos, template);
            ui.close();
        }
    }

    fn has_node_menu(&mut self, node: &CanvasNode) -> bool {
        matches!(node, CanvasNode::Graph(_) | CanvasNode::Placeholder(_))
    }

    fn show_node_menu(
        &mut self,
        node: SnarlNodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        let Some(mapping_id) = Self::mapping_id(snarl[node]) else {
            return;
        };
        let references = self.blocking_references_to(mapping_id);
        let remove = ui
            .add_enabled(references.is_empty(), egui::Button::new("Remove"))
            .on_disabled_hover_text(format!("Used by: {}", references.join(", ")));
        if remove.clicked() {
            self.remove_graph_node(mapping_id, node, snarl);
            ui.close();
        }
    }
}

#[cfg(test)]
#[path = "graph_viewer_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "graph_viewer_endpoint_tests.rs"]
mod endpoint_tests;
