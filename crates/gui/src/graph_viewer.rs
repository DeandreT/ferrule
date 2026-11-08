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

mod header;
mod menus;
mod palette;
mod pins;

use header::{complete_node_title, has_node_properties};
#[cfg(test)]
use pins::{input_wire_emphasis, output_wire_emphasis};

use editor_ui::GraphAction;
use egui::Ui;
use egui_snarl::ui::{NodeLayout, PinInfo, SnarlViewer};
use egui_snarl::{InPin, InPinId, NodeId as SnarlNodeId, OutPin, OutPinId, Snarl};
use ir::{ScalarType, Value};
use mapping::{
    AggregateOp, Binding, FunctionId, FunctionParameterId, Graph, NamedTarget, Node, NodeId,
    OwnedSourceRef, Scope,
};

use crate::appearance::{SemanticThemeColors, WireColorMode};
use crate::canvas::{CanvasNode, SourceBlock, SourceLeaf, TargetBlock, TargetLeaf};
use crate::canvas_endpoints::EndpointDisplayPin;
use crate::canvas_keyboard::focus::{self, CanvasPin};
use crate::path_picker::SourcePathCatalog;
use crate::value_editor::{show_value_editor, show_value_map_editor};
use crate::wire_colors::WireEmphasis;

#[path = "graph_endpoint_view.rs"]
mod graph_endpoint_view;
#[path = "graph_node_ids.rs"]
mod graph_node_ids;
#[path = "graph_node_presentation.rs"]
mod graph_node_presentation;
#[path = "graph_node_properties.rs"]
mod graph_node_properties;

/// Shared canvas-header wording for inspector and rule node pickers.
pub(crate) fn readable_node_label(node: &mapping::Node) -> String {
    graph_node_presentation::node_label(node)
}

#[path = "graph_planned_edits.rs"]
mod graph_planned_edits;
#[path = "graph_references.rs"]
mod graph_references;
#[path = "graph_sequence.rs"]
mod graph_sequence;
#[path = "graph_sequence_ownership.rs"]
mod graph_sequence_ownership;
#[path = "graph_wire_edits.rs"]
mod graph_wire_edits;
#[path = "node_palette.rs"]
mod node_palette;

use graph_node_ids::NodeIdReservation;
pub(crate) use graph_planned_edits::{PlannedGraphEdit, frame_has_edit_input};
use graph_references::node_inputs;
pub(crate) use graph_references::{
    InactiveTargetScope, ProjectGraphReferences, inactive_target_scopes, project_sequence_item_ids,
    references_outside_failure_rule, references_outside_scope, sequence_item_ids,
};
use node_palette::NodeTemplate;

pub(crate) fn filter_map_private_inputs(
    project: &mapping::Project,
) -> std::collections::BTreeSet<NodeId> {
    graph_references::filter_map_item_ids(
        &project.graph,
        &project.root,
        &project.extra_targets,
        &[],
        ProjectGraphReferences::new(&project.failure_rules, &project.extra_sources),
    )
    .0
}

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
const SOURCE_FIELD_EDIT_WIDTH: f32 = crate::theme::editor_size::PATH_PICKER;
const PATH_EDITOR_WIDTH: f32 = crate::theme::editor_size::TYPE_PATH_PICKER;

#[cfg(test)]
fn compact_endpoint_label(path: &str) -> String {
    compact_endpoint_label_to(path, ENDPOINT_LABEL_CHAR_LIMIT)
}

#[cfg(test)]
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
    egui::Grid::new(ui.scope_id().with("lookup_paths")).show(ui, |ui| {
        ui.label("collection");
        source_paths.show_collection_picker(
            ui,
            ui.scope_id().with("lookup_collection"),
            collection,
        );
        ui.end_row();
        ui.label("");
        source_paths.show_value_picker(ui, ui.scope_id().with("lookup_key"), collection, key);
        ui.end_row();
        ui.label("value");
        source_paths.show_value_picker(ui, ui.scope_id().with("lookup_value"), collection, value);
        ui.end_row();
    });
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
    /// Edits this frame applied through typed patches, in order, for the
    /// hosting document to record as commands.
    pub(crate) planned_edits: Vec<PlannedGraphEdit>,
    /// Set when an interaction can't be completed (e.g. binding into a
    /// scope that doesn't exist yet); the app surfaces it in the status
    /// line.
    pub error: Option<String>,
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
    pub(crate) fn properties_open(ctx: &egui::Context) -> bool {
        ctx.data(|data| data.get_temp::<u64>(egui::Id::unique("node_properties_frame")))
            .is_some_and(|frame| ctx.cumulative_frame_nr().saturating_sub(frame) <= 1)
    }

    fn mark_properties_open(ctx: &egui::Context) {
        let frame = ctx.cumulative_frame_nr();
        ctx.data_mut(|data| data.insert_temp(egui::Id::unique("node_properties_frame"), frame));
    }

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
        let max = self.endpoint_scroll.max_offset(node, total);
        let can_scroll = (rows < 0 && old > 0) || (rows > 0 && old < max);
        if !can_scroll {
            return false;
        }

        if self.endpoint_scroll.scroll_rows(node, total, rows) {
            self.sync_endpoint_view(snarl);
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

    fn mapping_id(node: CanvasNode) -> Option<NodeId> {
        match node {
            CanvasNode::Graph(id) | CanvasNode::Placeholder(id) => Some(id),
            CanvasNode::SourceBlock(_) | CanvasNode::TargetBlock(_) => None,
        }
    }

    /// Inserts a palette position node through the recorded canvas path.
    #[cfg(test)]
    pub(crate) fn insert_position_for_test(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        pos: egui::Pos2,
    ) -> Result<(NodeId, SnarlNodeId), String> {
        self.insert_palette_node_recorded(snarl, pos, NodeTemplate::Position)
    }

    /// Shows a node's property editor exactly as its canvas popover does.
    #[cfg(test)]
    pub(crate) fn show_properties_for_test(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        self.show_node_properties(pin, ui, snarl);
    }

    pub(crate) fn source_block_connection_error(&self, block: usize) -> Option<String> {
        let index = block.checked_sub(1)?;
        let source = self.project_references.named_source(index)?;
        let owner = self.project_references.source_alias_conflict(index)?;
        // An exact owner addresses a shadowed alias; only unresolvable names stay closed.
        if self.source_paths.ownership().block_reads_owned(block) {
            return None;
        }
        Some(format!(
            "Named input `{}` is unavailable: its name/path conflicts with {owner}. Rename the input to use an unambiguous name. Existing bindings are unchanged.",
            source.name
        ))
    }

    pub(crate) fn source_block_x12(&self, block: usize) -> bool {
        match block.checked_sub(1) {
            None => self.source_x12,
            Some(index) => self
                .project_references
                .named_source(index)
                .is_some_and(|source| {
                    crate::x12_tooltips::boundary_has_x12(
                        &source.schema,
                        Some(&source.path),
                        &source.options,
                    )
                }),
        }
    }

    pub(crate) fn source_endpoint_hint(
        &self,
        block: usize,
        section: &SourceBlock,
    ) -> Option<String> {
        let context = if let Some(source) = block
            .checked_sub(1)
            .and_then(|index| self.project_references.named_source(index))
        {
            section
                .frame
                .as_deref()
                .and_then(|frame| frame.get(1..))
                .and_then(|frame| frame.last())
                .map(String::as_str)
                .or(Some(source.schema.name.as_str()))
        } else {
            section
                .frame
                .as_deref()
                .and_then(|frame| frame.last())
                .map(String::as_str)
        };
        let hint = crate::x12_tooltips::endpoint_header_hint(
            self.source_block_x12(block),
            &section.title,
            context,
        );
        let note = self.source_block_connection_error(block).or_else(|| {
            self.source_paths
                .ownership()
                .block_reads_owned(block)
                .then(|| {
                    "Wires from this input create owned reads of exactly this input.".to_owned()
                })
        });
        match note {
            Some(note) => Some(hint.map_or_else(|| note.clone(), |hint| format!("{hint}\n{note}"))),
            None => hint,
        }
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

    /// The node wired to input pin `idx`. Pins follow the node's evaluated
    /// inputs, in the order the canvas draws them.
    fn input_at(&self, node_id: NodeId, idx: usize) -> Option<NodeId> {
        self.graph
            .nodes
            .get(&node_id)?
            .evaluated_inputs()
            .get(idx)
            .copied()
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
        if !crate::source_ownership::lookup_assignment_allowed(self.graph, node, false, |_| false) {
            self.error =
                Some("Owned expressions cannot be selected as an isolated function output.".into());
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

    fn clear_optional_input(&mut self, node_id: NodeId, input: usize) -> bool {
        if input == 0
            && let Some(Node::Raise { message }) = self.graph.nodes.get_mut(&node_id)
        {
            *message = None;
            true
        } else if input == 2
            && let Some(Node::InvocationCounter { restart, .. }) =
                self.graph.nodes.get_mut(&node_id)
        {
            *restart = None;
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
            if !self.clear_optional_input(owner, input) {
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
                && self
                    .graph
                    .nodes
                    .get(&needle)
                    .is_some_and(editor_ui::is_hidden_input)
            {
                self.graph.nodes.remove(&needle);
            }
        }
    }
}

impl SnarlViewer<CanvasNode> for GraphViewer<'_> {
    fn wire_color(
        &mut self,
        from: OutPinId,
        to: InPinId,
        color: egui::Color32,
        ui: &Ui,
        snarl: &Snarl<CanvasNode>,
    ) -> egui::Color32 {
        let Some(bright) = crate::canvas_tools::wire_bright(ui.ctx(), self, snarl, from, to) else {
            return color;
        };
        let node = snarl[to.node];
        let semantic = match node {
            CanvasNode::TargetBlock(block) => self
                .target_blocks
                .get(block)
                .and_then(|b| {
                    self.endpoint_scroll
                        .semantic_pin(node, to.input, b.leaves.len())
                })
                .unwrap_or(to.input),
            _ => to.input,
        };
        let a = crate::wire_colors::output_color(self.wire_color_mode, self.colors);
        let b = crate::wire_colors::input_color(self.wire_color_mode, self.colors, node, semantic);
        let base = egui::Color32::from_rgba_premultiplied(
            u8::midpoint(a.r(), b.r()),
            u8::midpoint(a.g(), b.g()),
            u8::midpoint(a.b(), b.b()),
            u8::midpoint(a.a(), b.a()),
        );
        crate::wire_colors::with_emphasis(
            base,
            self.colors.canvas.to_egui(),
            if bright {
                WireEmphasis::Incident
            } else {
                WireEmphasis::Unrelated
            },
        )
    }

    fn wire_clicked(
        &mut self,
        candidates: &[(OutPinId, InPinId)],
        _pos: egui::Pos2,
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        crate::canvas_tools::wire_click(ui.ctx(), self, snarl, candidates);
    }

    fn node_clicked(&mut self, node: SnarlNodeId, ui: &mut Ui, snarl: &mut Snarl<CanvasNode>) {
        crate::canvas_tools::node_click(ui.ctx(), snarl[node]);
    }

    fn background_clicked(&mut self, ui: &mut Ui, _snarl: &mut Snarl<CanvasNode>) {
        crate::canvas_tools::clear(ui.ctx());
    }
    fn inline_node(&mut self, node: &CanvasNode) -> bool {
        Self::mapping_id(*node)
            .and_then(|id| self.graph.nodes.get(&id))
            .is_some_and(graph_node_presentation::is_inline)
    }
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
        self.node_title(node)
    }

    fn show_header(
        &mut self,
        node: SnarlNodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        self.show_node_header(node, _inputs, _outputs, ui, snarl)
    }

    fn has_footer(&mut self, node: &CanvasNode) -> bool {
        matches!(
            node,
            CanvasNode::SourceBlock(_) | CanvasNode::TargetBlock(_)
        )
    }

    fn show_footer(
        &mut self,
        node: SnarlNodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        // Reserve a compact control strip so the resize grip never overlaps
        // the last endpoint pin.
        let top = ui.max_rect().top();
        ui.allocate_space(egui::vec2(1.0, 8.0));
        self.endpoint_scroll.record_footer_rect(
            snarl[node],
            egui::Rect::from_min_max(egui::pos2(ui.min_rect().left(), top), ui.min_rect().max),
        );
    }

    fn node_geometry(
        &mut self,
        node: SnarlNodeId,
        header: egui::Rect,
        inputs: &[(usize, egui::Pos2)],
        outputs: &[(usize, egui::Pos2)],
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        let canvas_node = snarl[node];
        crate::canvas_tools::record_ports(ui.ctx(), self, node, inputs, outputs, snarl);
        let pins = match canvas_node {
            CanvasNode::SourceBlock(_) => outputs,
            CanvasNode::TargetBlock(_) => inputs,
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => return,
        };
        self.endpoint_scroll.record_header_rect(canvas_node, header);
        let mut ys = pins
            .iter()
            .map(|(_, pos)| pos.y)
            .filter(|y| y.is_finite())
            .collect::<Vec<_>>();
        ys.sort_by(f32::total_cmp);
        if let Some(pitch) = ys
            .windows(2)
            .map(|ys| ys[1] - ys[0])
            .filter(|pitch| pitch.is_finite() && *pitch > 0.0)
            .min_by(f32::total_cmp)
        {
            self.endpoint_scroll.set_row_height(canvas_node, pitch);
        }
    }

    fn final_node_rect(
        &mut self,
        node: SnarlNodeId,
        rect: egui::Rect,
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        crate::canvas_keyboard::record_node_rect(ui, node, rect, snarl);
        crate::canvas_tools::record_rect(
            ui,
            snarl[node],
            snarl.get_node_info(node).expect("rendered node exists").pos,
            rect,
        );
        // egui-snarl's node frame already participates in transformed, clipped
        // widget hit-testing. Area-only rectangle checks miss its child layer.
        // Reuse its response without adding a click/drag target or an auto ID.
        let frame_id = focus::frame_id(crate::graph_draft_ui::property_canvas(ui), node);
        let canvas_node = snarl[node];
        if let Some(frame) = ui.ctx().read_response(frame_id) {
            if frame.contains_pointer() {
                self.hovered_node_this_frame = Some(node);
            }
            // Enter or Space on a focused frame selects the node as a click does.
            focus::name(&frame, &self.title(&canvas_node));
            focus::record_control(ui, frame_id, node);
            if focus::keyboard_activated(&frame) {
                crate::canvas_tools::node_click(ui.ctx(), canvas_node);
            }
            if frame.has_focus() {
                focus::paint_focus(ui, rect.expand(5.0), 6.0);
            }
        }
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
            self.endpoint_scroll.finish_geometry(canvas_node, rect);
            let title = self.title(&canvas_node);
            let scrolled = crate::canvas_endpoints::show_scrollbar(
                ui,
                canvas_node,
                rect,
                total,
                self.endpoint_scroll,
                accent,
                &title,
            );
            let resized = crate::canvas_endpoints::show_resize_handles(
                ui,
                canvas_node,
                rect,
                total,
                natural_width,
                self.endpoint_scroll,
                &title,
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
        self.input_pin_count(node)
    }

    fn outputs(&mut self, node: &CanvasNode) -> usize {
        self.output_pin_count(node)
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
            crate::graph_draft_ui::node(ui, node_id, "value_map", |ui| {
                show_value_map_editor(ui, table, default, value_map_wheel);
            });
        }
    }

    #[allow(refining_impl_trait)]
    fn show_input(&mut self, pin: &InPin, ui: &mut Ui, snarl: &mut Snarl<CanvasNode>) -> PinInfo {
        self.show_input_pin(pin, ui, snarl)
    }

    #[allow(refining_impl_trait)]
    fn show_output(&mut self, pin: &OutPin, ui: &mut Ui, snarl: &mut Snarl<CanvasNode>) -> PinInfo {
        self.show_output_pin(pin, ui, snarl)
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<CanvasNode>) {
        if let Err(error) = self.check_lookup_connection(from, to, snarl) {
            self.error = Some(error);
            return;
        }
        self.plan_edit(
            |viewer| viewer.connect_in_place(from, to, snarl),
            |viewer, connected| {
                let (source, sink) = connected.clone()?;
                (viewer.error.is_none() && viewer.sink_value(&sink) == Some(source))
                    .then_some(GraphAction::ConnectWire { source, sink })
            },
        );
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<CanvasNode>) {
        let earlier_error = self.error.take();
        self.plan_edit(
            |viewer| viewer.disconnect_in_place(from, to, snarl),
            |viewer, disconnected| {
                let sink = disconnected.clone()?;
                (viewer.error.is_none() && viewer.sink_disconnected(&sink))
                    .then_some(GraphAction::DisconnectWire { sink })
            },
        );
        if self.error.is_none() {
            self.error = earlier_error;
        }
    }

    fn has_graph_menu(&mut self, _pos: egui::Pos2, _snarl: &mut Snarl<CanvasNode>) -> bool {
        true
    }

    fn show_graph_menu(&mut self, pos: egui::Pos2, ui: &mut Ui, snarl: &mut Snarl<CanvasNode>) {
        self.show_canvas_menu(pos, ui, snarl)
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
        self.show_mapping_node_menu(node, _inputs, _outputs, ui, snarl)
    }
}

#[cfg(test)]
#[path = "graph_viewer_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "graph_viewer_endpoint_tests.rs"]
mod endpoint_tests;

#[cfg(test)]
#[path = "graph_viewer_node_header_tests.rs"]
mod node_header_tests;

#[cfg(test)]
#[path = "graph_viewer_shared_target_tests.rs"]
mod shared_target_tests;

#[cfg(test)]
#[path = "graph_viewer_copy_target_tests.rs"]
mod copy_target_tests;

#[cfg(test)]
#[path = "graph_viewer_sequence_item_tests.rs"]
mod sequence_item_tests;
