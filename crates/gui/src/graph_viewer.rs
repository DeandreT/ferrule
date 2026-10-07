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

#[path = "graph_node_ids.rs"]
mod graph_node_ids;
#[path = "graph_node_presentation.rs"]
mod graph_node_presentation;
#[path = "graph_node_properties.rs"]
mod graph_node_properties;
#[path = "graph_references.rs"]
mod graph_references;
#[path = "graph_sequence.rs"]
mod graph_sequence;
#[path = "graph_sequence_ownership.rs"]
mod graph_sequence_ownership;
#[path = "node_palette.rs"]
mod node_palette;

use graph_node_ids::NodeIdReservation;
use graph_references::node_inputs;
pub(crate) use graph_references::{
    InactiveTargetScope, ProjectGraphReferences, inactive_target_scopes, project_sequence_item_ids,
    references_outside_failure_rule, references_outside_scope, sequence_item_ids,
};
use node_palette::NodeTemplate;

pub(crate) fn reserve_project_node_ids(
    project: &mapping::Project,
    count: usize,
) -> Result<Vec<NodeId>, String> {
    let owned_items = project_sequence_item_ids(project);
    NodeIdReservation::new(&project.graph, &owned_items).reserve(count)
}

pub(crate) fn failure_rule_is_only_item_owner(
    project: &mapping::Project,
    rule: &mapping::FailureRule,
    item: NodeId,
) -> bool {
    let Some(index) = project
        .failure_rules
        .iter()
        .position(|candidate| std::ptr::eq(candidate, rule))
    else {
        return false;
    };
    let owners = graph_references::sequence_item_owners(
        &project.graph,
        &project.root,
        &project.extra_targets,
        &[],
        ProjectGraphReferences::new(&project.failure_rules, &project.extra_sources),
        item,
    );
    matches!(
        owners.as_slice(),
        [graph_sequence_ownership::SequenceItemOwner::FailureRule { index: owner }]
            if *owner == index
    )
}

pub(crate) fn sequence_arguments_have_no_private_items(
    project: &mapping::Project,
    sequence: &mapping::SequenceExpr,
) -> bool {
    let owned = project_sequence_item_ids(project);
    let mut pending = sequence
        .inputs()
        .into_iter()
        .map(|id| (id, false))
        .collect::<Vec<_>>();
    let mut active = std::collections::BTreeSet::new();
    let mut complete = std::collections::BTreeSet::new();
    while let Some((id, exiting)) = pending.pop() {
        if exiting {
            active.remove(&id);
            complete.insert(id);
            continue;
        }
        if complete.contains(&id) {
            continue;
        }
        if owned.contains(&id) || !active.insert(id) {
            return false;
        }
        let Some(node) = project.graph.nodes.get(&id) else {
            return false;
        };
        if matches!(node, Node::Unconnected) {
            return false;
        }
        pending.push((id, true));
        // Reducers own their private predicate/value context. Only their
        // sequence arguments and parent argument reach this root context,
        // matching the native validator's context dependency boundary.
        let inputs = match node {
            Node::SequenceExists { sequence, .. } => sequence.inputs(),
            Node::SequenceAggregate { sequence, arg, .. } => {
                sequence.inputs().into_iter().chain(*arg).collect()
            }
            _ => node_inputs(node),
        };
        pending.extend(inputs.into_iter().map(|input| (input, false)));
    }
    true
}

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

fn builtin_parameter(function: &str, index: usize) -> Option<&'static functions::BuiltinParameter> {
    let builtin = functions::builtin(function)?;
    if let Some(parameter) = builtin.parameters.get(index) {
        return Some(parameter);
    }
    let step = builtin.arity.step()?;
    let repeated_start = builtin.parameters.len().checked_sub(step)?;
    let repeated_offset = index.checked_sub(builtin.parameters.len())? % step;
    builtin.parameters.get(repeated_start + repeated_offset)
}

fn call_missing_minimum_inputs(function: &str, count: usize) -> usize {
    functions::builtin(function)
        .map(|builtin| builtin.arity.minimum().saturating_sub(count))
        .unwrap_or_default()
}

fn call_can_add_argument(function: &str, count: usize) -> bool {
    functions::builtin(function).is_none_or(|builtin| {
        builtin
            .arity
            .maximum()
            .is_none_or(|maximum| count < maximum)
    })
}

fn call_can_remove_argument(function: &str, count: usize) -> bool {
    functions::builtin(function).map_or(count > 0, |builtin| count > builtin.arity.minimum())
}

fn show_lookup_editor(
    ui: &mut Ui,
    source_paths: &SourcePathCatalog,
    collection: &mut Vec<String>,
    key: &mut Vec<String>,
    value: &mut Vec<String>,
) {
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
    pub(crate) primary_root_authoring: bool,
    pub extra_targets: &'a [NamedTarget],
    pub(crate) inactive_target_scopes: &'a [InactiveTargetScope<'a>],
    pub(crate) project_references: ProjectGraphReferences<'a>,
    pub source_blocks: &'a [SourceBlock],
    pub target_blocks: &'a [TargetBlock],
    pub source_x12: bool,
    pub target_x12: bool,
    pub source_paths: &'a SourcePathCatalog,
    pub function_names: std::collections::BTreeMap<FunctionId, String>,
    pub function_inputs: std::collections::BTreeMap<FunctionId, Vec<String>>,
    pub parameter_names: std::collections::BTreeMap<FunctionParameterId, String>,
    pub protected_output: Option<NodeId>,
    /// Writable only on an isolated user-function canvas.
    pub(crate) function_output: Option<&'a mut NodeId>,
    pub requested_function_open: Option<FunctionId>,
    pub colors: SemanticThemeColors,
    pub wire_color_mode: WireColorMode,
    pub endpoint_scroll: &'a mut crate::canvas_endpoints::EndpointScrollState,
    pub value_map_wheel: Option<(NodeId, f32)>,
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
            let owned_items = if self.function_output.is_none() {
                graph_references::sequence_item_ids(
                    self.graph,
                    self.root_scope,
                    self.extra_targets,
                    self.inactive_target_scopes,
                    self.project_references,
                )
            } else {
                Default::default()
            };
            if owned_items.is_empty() {
                crate::app::sync_endpoint_wires(
                    self.graph,
                    self.root_scope,
                    self.source_blocks,
                    self.target_blocks,
                    self.endpoint_scroll,
                    snarl,
                );
            } else {
                crate::app::sync_endpoint_wires_with_owned_items(
                    self.graph,
                    self.root_scope,
                    self.source_blocks,
                    self.target_blocks,
                    self.endpoint_scroll,
                    snarl,
                    &owned_items,
                );
            }
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
    ) -> Result<(NodeId, SnarlNodeId), String> {
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
            NodeTemplate::SourceRootField => {
                if !self.primary_root_authoring {
                    return Err("Primary root fields are available on the supported primary mapping canvas.".into());
                }
                let path = self
                    .source_paths
                    .first_primary_root_field()
                    .ok_or_else(|| {
                        "No supported primary root scalar field is available.".to_string()
                    })?;
                self.insert(
                    snarl,
                    pos,
                    Node::SourceRootField {
                        path,
                        required: false,
                    },
                )
            }
            NodeTemplate::SourceRootXmlTypeEquals => {
                if !self.primary_root_authoring {
                    return Err("Primary XML comparisons are available on the supported primary mapping canvas.".into());
                }
                let canonical_expanded_type = self
                    .source_paths
                    .first_primary_root_type()
                    .ok_or_else(|| "No supported primary XML type is available.".to_string())?;
                self.insert(
                    snarl,
                    pos,
                    Node::SourceRootXmlTypeEquals {
                        canonical_expanded_type,
                    },
                )
            }
            NodeTemplate::Position => self.insert(
                snarl,
                pos,
                Node::Position {
                    collection: Vec::new(),
                },
            ),
            NodeTemplate::HostInput => self.insert(
                snarl,
                pos,
                Node::RuntimeParameter {
                    name: String::new(),
                    ty: ScalarType::String,
                    preview: None,
                },
            ),
            NodeTemplate::HostInputDefault => {
                self.insert_with_unconnected_inputs(snarl, pos, 1, |inputs| {
                    Node::RuntimeParameterDefault {
                        name: String::new(),
                        ty: ScalarType::String,
                        default: inputs[0],
                        preview: None,
                    }
                })
            }
            NodeTemplate::Builtin(function) => {
                let input_count = functions::builtin(function)
                    .map(|builtin| builtin.arity.minimum())
                    .unwrap_or_default();
                self.insert_with_unconnected_inputs(snarl, pos, input_count, |args| Node::Call {
                    function: function.to_owned(),
                    args: args.to_vec(),
                })
            }
            NodeTemplate::Raise => self.insert(snarl, pos, Node::Raise { message: None }),
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
    ) -> Result<(NodeId, SnarlNodeId), String> {
        let id = self.reserve_node_ids(1)?[0];
        self.graph.nodes.insert(id, node);
        let snarl_id = snarl.insert_node(pos, CanvasNode::Graph(id));
        Ok((id, snarl_id))
    }

    fn insert_with_unconnected_inputs(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        pos: egui::Pos2,
        input_count: usize,
        build: impl FnOnce(&[NodeId]) -> Node,
    ) -> Result<(NodeId, SnarlNodeId), String> {
        let count = input_count
            .checked_add(1)
            .ok_or_else(|| "mapping node IDs are exhausted".to_string())?;
        let ids = self.reserve_node_ids(count)?;
        let Some((&id, inputs)) = ids.split_last() else {
            return Err("mapping node IDs are exhausted".to_string());
        };
        let node = build(inputs);
        for &input in inputs {
            self.graph.nodes.insert(input, Node::Unconnected);
        }
        self.graph.nodes.insert(id, node);
        let snarl_id = snarl.insert_node(pos, CanvasNode::Graph(id));
        Ok((id, snarl_id))
    }

    /// Reuses an unowned `SourceField` with this exact frame and relative
    /// path, or creates one. Generated items never back ordinary Source-pin wires.
    fn source_field_for(
        &mut self,
        frame: Option<Vec<String>>,
        path: Vec<String>,
    ) -> Result<NodeId, String> {
        let owned_items = self.owned_item_ids();
        let existing = self.graph.nodes.iter().find_map(|(id, node)| match node {
            Node::SourceField { path: p, frame: f }
                if !owned_items.contains(id) && p == &path && f == &frame =>
            {
                Some(*id)
            }
            _ => None,
        });
        if let Some(id) = existing {
            return Ok(id);
        }
        let id = NodeIdReservation::new(self.graph, &owned_items).reserve(1)?[0];
        self.graph
            .nodes
            .insert(id, Node::SourceField { path, frame });
        Ok(id)
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
            Node::Raise { message } => *message = Some(from_id),
            Node::RuntimeParameterDefault { default, .. } => *default = from_id,
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
            Node::XmlMixedContent { replacements, .. } => {
                let Some(replacement) = replacements.get_mut(idx) else {
                    return false;
                };
                replacement.expression = from_id;
            }
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
            Node::SequenceAggregate {
                sequence,
                predicate,
                expression,
                arg,
                ..
            } => {
                let sequence_inputs = sequence.inputs().len();
                if idx < sequence_inputs {
                    graph_sequence::set_input(sequence, idx, from_id);
                } else {
                    let mut optional_index = idx - sequence_inputs;
                    for slot in [predicate, expression, arg] {
                        if slot.is_some() {
                            if optional_index == 0 {
                                *slot = Some(from_id);
                                return true;
                            }
                            optional_index -= 1;
                        }
                    }
                    return false;
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

    fn check_primary_root_input(&self, from: NodeId, to: NodeId) -> Result<(), String> {
        if !crate::primary_root_authoring::has_dependency(self.graph, from) {
            return Ok(());
        }
        crate::primary_root_authoring::check_binding(
            self.graph,
            from,
            self.primary_root_authoring,
        )?;
        // The graph is shared by every target. Adding a root input to an
        // ordinary expression can also affect its existing downstream owners.
        let mut pending = vec![to];
        let mut affected = std::collections::BTreeSet::new();
        while let Some(id) = pending.pop() {
            if !affected.insert(id) {
                continue;
            }
            let Some(node) = self.graph.nodes.get(&id) else {
                continue;
            };
            if !matches!(
                node,
                Node::Call { .. } | Node::If { .. } | Node::ValueMap { .. } | Node::Raise { .. }
            ) {
                return Err(format!(
                    "Primary root expressions cannot feed mapping node {id}, which changes evaluation ownership"
                ));
            }
            let owners = self.blocking_references_to(id);
            if !owners.is_empty() {
                return Err(format!(
                    "Primary root expressions cannot feed mapping node {id}, used by {}",
                    owners.join(", ")
                ));
            }
            pending.extend(self.graph.nodes.iter().filter_map(|(&consumer, node)| {
                node.dependencies().contains(&id).then_some(consumer)
            }));
        }
        Ok(())
    }

    fn input_at(&self, node_id: NodeId, idx: usize) -> Option<NodeId> {
        match self.graph.nodes.get(&node_id)? {
            Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.get(idx).copied(),
            Node::Raise { message } => (idx == 0).then_some(*message).flatten(),
            Node::RuntimeParameterDefault { default, .. } => (idx == 0).then_some(*default),
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
            Node::SequenceAggregate {
                sequence,
                predicate,
                expression,
                arg,
                ..
            } => graph_sequence::input_at(sequence, idx).or_else(|| {
                predicate
                    .iter()
                    .chain(expression)
                    .chain(arg)
                    .nth(idx.saturating_sub(sequence.inputs().len()))
                    .copied()
            }),
            Node::Aggregate {
                expression, arg, ..
            }
            | Node::JoinAggregate {
                expression, arg, ..
            } => expression.iter().chain(arg).nth(idx).copied(),
            Node::SourceField { .. }
            | Node::SourceDocumentPath
            | Node::SourceRootXmlTypeEquals { .. }
            | Node::SourceRootField { .. }
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

    /// The same action backs the function node menu and result selection.
    /// Protection moves immediately so further edits in this frame cannot
    /// remove the newly selected expression.
    pub(crate) fn select_function_output(&mut self, node: NodeId) -> bool {
        let Some(output) = self.function_output.as_mut() else {
            return false;
        };
        if !self.graph.nodes.contains_key(&node) {
            self.error = Some(format!("function output node {node} no longer exists"));
            return false;
        }
        **output = node;
        self.protected_output = Some(node);
        true
    }

    fn node_references(&self, needle: NodeId) -> graph_references::NodeReferences {
        let mut references = graph_references::references_to(
            self.graph,
            self.root_scope,
            self.extra_targets,
            self.inactive_target_scopes,
            self.project_references,
            needle,
        );
        if self.protected_output == Some(needle) {
            references.all.push("function output".to_string());
            references.blocking.push("function output".to_string());
        }
        references
    }

    fn references_to(&self, needle: NodeId) -> Vec<String> {
        self.node_references(needle).all
    }

    fn blocking_references_to(&self, needle: NodeId) -> Vec<String> {
        self.node_references(needle).blocking
    }

    fn graph_consumers(&self, needle: NodeId) -> Vec<(NodeId, usize)> {
        self.graph
            .nodes
            .iter()
            .filter(|(owner, _)| **owner != needle)
            .flat_map(|(&owner, node)| {
                node_inputs(node)
                    .into_iter()
                    .enumerate()
                    .filter_map(move |(input, dependency)| {
                        (dependency == needle).then_some((owner, input))
                    })
            })
            .collect()
    }

    fn clear_raise_message(&mut self, node_id: NodeId, input: usize) -> bool {
        if input == 0
            && let Some(Node::Raise { message }) = self.graph.nodes.get_mut(&node_id)
        {
            *message = None;
            true
        } else {
            false
        }
    }

    fn disconnect_graph_consumers(
        &mut self,
        consumers: &[(NodeId, usize)],
        ids: &[NodeId],
        snarl: &mut Snarl<CanvasNode>,
    ) {
        let mut replacements = ids.iter();
        for &(owner, input) in consumers {
            if !self.clear_raise_message(owner, input) {
                let &unconnected = replacements
                    .next()
                    .expect("one reserved ID per required input");
                self.graph.nodes.insert(unconnected, Node::Unconnected);
                self.set_input(owner, input, unconnected);
            }
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

    fn input_count(node: &Node) -> usize {
        match node {
            Node::SourceField { .. }
            | Node::SourceDocumentPath
            | Node::SourceRootXmlTypeEquals { .. }
            | Node::SourceRootField { .. }
            | Node::Position { .. }
            | Node::JoinField { .. }
            | Node::JoinPosition { .. }
            | Node::Unconnected
            | Node::Const { .. }
            | Node::FunctionParameter { .. }
            | Node::RuntimeValue { .. }
            | Node::RuntimeParameter { .. }
            | Node::XmlSerialize { .. } => 0,
            Node::RuntimeParameterDefault { .. } | Node::Raise { .. } => 1,
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
            Node::SequenceAggregate {
                sequence,
                predicate,
                expression,
                arg,
                ..
            } => {
                sequence.inputs().len()
                    + usize::from(predicate.is_some())
                    + usize::from(expression.is_some())
                    + usize::from(arg.is_some())
            }
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
        if Self::mapping_id(snarl[node]).is_some_and(|id| {
            matches!(
                self.graph.nodes.get(&id),
                Some(Node::Lookup { .. } | Node::ValueMap { .. })
            )
        }) {
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
                    Some(Node::SourceField { .. })
                        if self.function_output.is_none()
                            && !graph_references::sequence_item_owners(
                                self.graph,
                                self.root_scope,
                                self.extra_targets,
                                self.inactive_target_scopes,
                                self.project_references,
                                *id,
                            )
                            .is_empty() =>
                    {
                        format!("Generated item #{id}")
                    }
                    Some(Node::SourceField { path, frame }) => {
                        let owner = frame
                            .as_ref()
                            .and_then(|frame| frame.last())
                            .map(|owner| format!("{owner}/"))
                            .unwrap_or_default();
                        compact_graph_title(&format!("Source: {owner}{}", path.join("/")))
                    }
                    Some(Node::SourceDocumentPath) => "Source document path".to_string(),
                    Some(Node::SourceRootXmlTypeEquals { .. }) => {
                        "Primary XML annotation equality".to_string()
                    }
                    Some(Node::SourceRootField { path, required }) => {
                        let title = if *required {
                            "Required primary field"
                        } else {
                            "Primary field"
                        };
                        compact_graph_title(&format!("{title}: {}", path.join("/")))
                    }
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
                        let preview = crate::value_editor::title_preview(value);
                        if preview.is_empty() {
                            "Const".to_string()
                        } else {
                            format!("Const: {preview}")
                        }
                    }
                    Some(Node::FunctionParameter { parameter }) => {
                        self.parameter_names.get(parameter).map_or_else(
                            || "Function input".to_string(),
                            |name| format!("Input: {name}"),
                        )
                    }
                    Some(Node::RuntimeValue { value }) => format!("Runtime: {value:?}"),
                    Some(Node::RuntimeParameter { name, ty, .. }) => {
                        let name = if name.is_empty() {
                            "<name required>"
                        } else {
                            name
                        };
                        format!("Runtime input: {name} ({ty:?})")
                    }
                    Some(Node::RuntimeParameterDefault { name, ty, .. }) => {
                        let name = if name.is_empty() {
                            "<name required>"
                        } else {
                            name
                        };
                        format!("Runtime input: {name} ({ty:?}, optional)")
                    }
                    Some(Node::Call { function, .. }) => functions::builtin(function).map_or_else(
                        || format!("Call: {function}"),
                        |builtin| format!("Call: {}", builtin.display_name),
                    ),
                    Some(Node::UserFunctionCall { function, .. }) => {
                        self.function_names.get(function).map_or_else(
                            || "Call: <missing function>".to_string(),
                            |name| format!("Call: {name}"),
                        )
                    }
                    Some(Node::If { .. }) => "If".to_string(),
                    Some(Node::Raise { .. }) => "Raise error".to_string(),
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
                    Some(Node::SequenceAggregate {
                        function, sequence, ..
                    }) => {
                        let op = format!("{function:?}").to_lowercase();
                        format!("{op}: {}", graph_sequence::label(sequence))
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
        let full_title = self.title(&canvas_node);
        let is_output =
            Self::mapping_id(canvas_node).is_some_and(|id| self.protected_output == Some(id));
        let graph_node = Self::mapping_id(canvas_node).and_then(|id| self.graph.nodes.get(&id));
        let compact_header = graph_node
            .and_then(|node| graph_node_presentation::header(node, &full_title, is_output));
        let full_title = graph_node_presentation::complete_title(graph_node, full_title);
        let header_hint = endpoint_hint
            .or_else(|| graph_node.map(|node| graph_node_presentation::hint(node, &full_title)));
        let show_title = |ui: &mut Ui| {
            if let Some(header) = compact_header {
                graph_node_presentation::show_header(ui, header, &full_title)
            } else {
                ui.label(&full_title)
            }
        };
        let response = if let Some(function) = function_call {
            ui.horizontal(|ui| {
                let response = show_title(ui);
                let open = ui.add(egui::Button::new(crate::icons::text(
                    lucide_icons::Icon::ExternalLink,
                    12.0,
                )));
                open.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::WidgetType::Button,
                        true,
                        "Open function mapping",
                    )
                });
                let open = open.on_hover_text("Open function mapping");
                if response.double_clicked() || open.clicked() {
                    self.requested_function_open = Some(function);
                }
                response
            })
            .inner
        } else {
            show_title(ui)
        };
        if let Some(hint) = header_hint {
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
        // egui-snarl's node frame already participates in transformed, clipped
        // widget hit-testing. Area-only rectangle checks miss its child layer.
        // Reuse its response without adding a click/drag target or an auto ID.
        let frame_id = ui.layer_id().id.with(("snarl-node", node)).with("frame");
        if ui
            .ctx()
            .read_response(frame_id)
            .is_some_and(|response| response.contains_pointer())
        {
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

    fn has_body(&mut self, _node: &CanvasNode) -> bool {
        false
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
        let value_map_wheel = match self.value_map_wheel {
            Some((id, delta_y)) if id == node_id => {
                self.value_map_wheel = None;
                Some(delta_y)
            }
            _ => None,
        };
        if let Some(Node::ValueMap { table, default, .. }) = self.graph.nodes.get_mut(&node_id) {
            show_value_map_editor(ui, table, default, value_map_wheel);
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
                    Some(Node::Call { function, .. }) => builtin_parameter(function, idx)
                        .map_or_else(
                            || format!("arg {idx}"),
                            |parameter| parameter.name.to_owned(),
                        ),
                    Some(Node::UserFunctionCall { function, .. }) => self
                        .function_inputs
                        .get(function)
                        .and_then(|parameters| parameters.get(idx))
                        .cloned()
                        .unwrap_or_else(|| format!("input {}", idx + 1)),
                    Some(Node::If { .. }) => ["condition", "then", "else"][idx].to_string(),
                    Some(Node::Raise { .. }) => "message".to_string(),
                    Some(Node::RuntimeParameterDefault { .. }) => "default".to_string(),
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
                    Some(Node::SequenceAggregate {
                        sequence,
                        predicate,
                        expression,
                        arg,
                        ..
                    }) => {
                        if idx < sequence.inputs().len() {
                            graph_sequence::pin_label(sequence, idx).to_string()
                        } else {
                            [
                                predicate.as_ref().map(|_| "filter"),
                                expression.as_ref().map(|_| "values"),
                                arg.as_ref().map(|_| "arg"),
                            ]
                            .into_iter()
                            .flatten()
                            .nth(idx - sequence.inputs().len())
                            .unwrap_or_default()
                            .to_string()
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
                                format!("{state}: {}\nType: {}", leaf.label, leaf.ty.label()),
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
            let node = Self::mapping_id(canvas_node).and_then(|id| self.graph.nodes.get(&id));
            graph_node_presentation::show_input(ui, &label, node, idx);
        }
        // Snarl creates this exact native pin widget immediately after this
        // callback. egui can read its current hit response before registration
        // using the preceding pass, without a second hit target or auto ID.
        let graph_node = Self::mapping_id(canvas_node).and_then(|id| self.graph.nodes.get(&id));
        if matches!(graph_node, Some(Node::XmlMixedContent { .. }))
            && let Some(response) = ui.ctx().read_response(ui.next_auto_id())
            && response.contains_pointer()
            && let Some(hint) = graph_node_presentation::input_pin_hint(graph_node, idx)
        {
            response.on_hover_text(hint);
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
                                || format!("Source: {}\nType: {}", leaf.label, leaf.ty.label()),
                                |frame| {
                                    let frame = if frame.is_empty() {
                                        "document rows".to_string()
                                    } else {
                                        frame.join("/")
                                    };
                                    format!(
                                        "Source: {}\nType: {}\nRepeating context: {frame}",
                                        leaf.label,
                                        leaf.ty.label()
                                    )
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
        let full_title = self.title(&canvas_node);
        let compact_node = self
            .graph
            .nodes
            .get(&node_id)
            .and_then(|node| {
                graph_node_presentation::header(
                    node,
                    &full_title,
                    self.protected_output == Some(node_id),
                )
            })
            .is_some();
        let full_title =
            graph_node_presentation::complete_title(self.graph.nodes.get(&node_id), full_title);
        if self
            .graph
            .nodes
            .get(&node_id)
            .is_some_and(graph_node_presentation::has_properties)
        {
            let edit = ui.add(
                egui::Button::new(crate::icons::text(lucide_icons::Icon::Pencil, 12.0)).small(),
            );
            let label = format!("Edit {full_title}");
            edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
            let edit = edit.on_hover_text(label);
            let popup_id = ui.id().with(("node_properties", node_id));
            // Keep the editor open independently of the single shared popup
            // slot used by its nested type and function selectors.
            let mut open = ui
                .ctx()
                .data(|data| data.get_temp::<bool>(popup_id))
                .unwrap_or_default();
            if edit.clicked() {
                open = !open;
            }
            // A nested selector handles its own click, even when selection
            // closes it during this frame. Keep that click inside the editor.
            let close_behavior = if egui::Popup::is_any_open(ui.ctx()) {
                egui::PopupCloseBehavior::IgnoreClicks
            } else {
                egui::PopupCloseBehavior::CloseOnClickOutside
            };
            let properties_width = match self.graph.nodes.get(&node_id) {
                Some(Node::ValueMap { table, .. }) => {
                    crate::value_editor::value_map_editor_width(table.len()) + 22.0
                }
                _ => PATH_EDITOR_WIDTH,
            };
            egui::Popup::from_response(&edit)
                .id(popup_id)
                .open_bool(&mut open)
                .width(properties_width)
                .layout(egui::Layout::top_down(egui::Align::Min))
                .close_behavior(close_behavior)
                .show(|ui| {
                    ui.set_max_width(properties_width);
                    ui.add(egui::Label::new(&full_title).wrap());
                    ui.separator();
                    if matches!(self.graph.nodes.get(&node_id), Some(Node::ValueMap { .. })) {
                        ui.add_enabled_ui(edit.enabled(), |ui| {
                            self.show_node_properties(pin, ui, snarl);
                        });
                    } else {
                        self.show_node_properties(pin, ui, snarl);
                    }
                });
            ui.ctx().data_mut(|data| {
                if open {
                    data.insert_temp(popup_id, true);
                } else {
                    data.remove::<bool>(popup_id);
                }
            });
        } else if compact_node {
            ui.label("").on_hover_text(&full_title);
        } else {
            self.show_node_properties(pin, ui, snarl);
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
                        self.source_field_for(source_leaf.frame.clone(), source_leaf.path.clone())?;
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
                    crate::scope_editor::check_static_binding(
                        self.root_scope,
                        &target_leaf.chain,
                        &target_leaf.field,
                    )?;
                    let displaced = self.binding_node(&target_leaf);
                    let field =
                        self.source_field_for(source_leaf.frame.clone(), source_leaf.path.clone())?;
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
                    crate::scope_editor::check_static_binding(
                        self.root_scope,
                        &target_leaf.chain,
                        &target_leaf.field,
                    )?;
                    crate::primary_root_authoring::check_binding(
                        self.graph,
                        from_id,
                        self.primary_root_authoring && target_leaf.chain.is_empty(),
                    )?;
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
                    self.check_primary_root_input(from_id, to_id)?;
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
                if self.input_at(to_id, to.id.input).is_none() {
                    self.error = Some(format!(
                        "input {} does not exist on mapping node {to_id}",
                        to.id.input
                    ));
                    return;
                }
                if self.clear_raise_message(to_id, to.id.input) {
                    snarl.disconnect(from.id, to.id);
                    if let Some(disconnected) = disconnected {
                        self.remove_orphaned_input(disconnected, snarl);
                    }
                    return;
                }
                let unconnected = match self.fresh_unconnected() {
                    Ok(id) => id,
                    Err(error) => {
                        self.error = Some(error);
                        return;
                    }
                };
                snarl.disconnect(from.id, to.id);
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
        if let Some(template) = node_palette::show_available(
            ui,
            self.primary_root_authoring && self.source_paths.first_primary_root_field().is_some(),
            self.primary_root_authoring && self.source_paths.first_primary_root_type().is_some(),
        ) {
            self.error = self.insert_palette_node(snarl, pos, template).err();
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
        if self.function_output.is_some() {
            let select = ui.add_enabled(
                self.protected_output != Some(mapping_id),
                egui::Button::new("Use as function output"),
            );
            if select.clicked() {
                self.select_function_output(mapping_id);
                ui.close();
            }
        }
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

#[cfg(test)]
#[path = "graph_viewer_shared_target_tests.rs"]
mod shared_target_tests;

#[cfg(test)]
#[path = "graph_viewer_copy_target_tests.rs"]
mod copy_target_tests;

#[cfg(test)]
#[path = "graph_viewer_sequence_item_tests.rs"]
mod sequence_item_tests;
