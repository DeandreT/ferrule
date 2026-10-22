//! In-place wire and palette edits behind the canvas connect, disconnect,
//! and insert entry points, which re-apply them as typed edits.

use editor_ui::{CanvasPosition, GraphAction, WireSink};
use egui_snarl::{InPin, NodeId as SnarlNodeId, OutPin, Snarl};
use mapping::NodeId;

use super::node_palette::NodeTemplate;
use super::{GraphViewer, graph_sequence};
use crate::canvas::CanvasNode;

impl GraphViewer<'_> {
    /// Connects a wire in place, returning the fed value and its sink.
    pub(super) fn connect_in_place(
        &mut self,
        from: &OutPin,
        to: &InPin,
        snarl: &mut Snarl<CanvasNode>,
    ) -> Option<(NodeId, WireSink)> {
        self.error = None;
        let from_node = snarl[from.id.node];
        let to_node = snarl[to.id.node];
        if let CanvasNode::Graph(to_id) | CanvasNode::Placeholder(to_id) = to_node
            && self
                .graph
                .nodes
                .get(&to_id)
                .is_some_and(|node| self.read_only_inputs(node))
        {
            self.error = Some(graph_sequence::READ_ONLY_INPUTS_MESSAGE.into());
            return None;
        }
        if let CanvasNode::SourceBlock(block) = from_node
            && let Some(error) = self.source_block_connection_error(block)
        {
            self.error = Some(error);
            return None;
        }
        // Each arm yields the displaced value, the fed value, and the sink.
        type Connected = (Option<NodeId>, NodeId, Option<WireSink>);
        let mutation = (|| -> Result<Connected, String> {
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
                    let sink = WireSink::NodeInput {
                        node: to_id,
                        input: to.id.input,
                    };
                    Ok((displaced, field, Some(sink)))
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
                    let sink = self.binding_sink(&target_leaf.chain, &target_leaf.field);
                    Ok((displaced, field, sink))
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
                    self.check_filter_map_connection(from_id, None)?;
                    let displaced = self.binding_node(&target_leaf);
                    self.set_binding(&target_leaf, from_id);
                    let sink = self.binding_sink(&target_leaf.chain, &target_leaf.field);
                    Ok((displaced, from_id, sink))
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
                    self.check_filter_map_connection(from_id, Some((to_id, to.id.input)))?;
                    let displaced = self.input_at(to_id, to.id.input);
                    if !self.set_input(to_id, to.id.input, from_id) {
                        return Err(format!(
                            "input {} could not be updated on mapping node {to_id}",
                            to.id.input
                        ));
                    }
                    let sink = WireSink::NodeInput {
                        node: to_id,
                        input: to.id.input,
                    };
                    Ok((displaced, from_id, Some(sink)))
                }
                _ => Err("these canvas pins cannot be connected".to_string()),
            }
        })();
        let (displaced, source, sink) = match mutation {
            Ok(connected) => connected,
            Err(error) => {
                self.error = Some(error);
                return None;
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
        Some((source, sink?))
    }

    /// Disconnects a wire in place, returning the sink it changed.
    pub(super) fn disconnect_in_place(
        &mut self,
        from: &OutPin,
        to: &InPin,
        snarl: &mut Snarl<CanvasNode>,
    ) -> Option<WireSink> {
        if let CanvasNode::Graph(to_id) | CanvasNode::Placeholder(to_id) = snarl[to.id.node]
            && self
                .graph
                .nodes
                .get(&to_id)
                .is_some_and(|node| self.read_only_inputs(node))
        {
            self.error = Some(graph_sequence::READ_ONLY_INPUTS_MESSAGE.into());
            return None;
        }
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
        let mut sink = None;
        match (snarl[from.id.node], snarl[to.id.node]) {
            (_, CanvasNode::TargetBlock(block)) => {
                if let Some(leaf) = self.target_leaf(block, to.id.input).cloned() {
                    sink = self.binding_sink(&leaf.chain, &leaf.field);
                    self.remove_binding(&leaf);
                }
            }
            (_, CanvasNode::Graph(to_id) | CanvasNode::Placeholder(to_id)) => {
                if self.input_at(to_id, to.id.input).is_none() {
                    self.error = Some(format!(
                        "input {} does not exist on mapping node {to_id}",
                        to.id.input
                    ));
                    return None;
                }
                let sink = WireSink::NodeInput {
                    node: to_id,
                    input: to.id.input,
                };
                if self.clear_optional_input(to_id, to.id.input) {
                    snarl.disconnect(from.id, to.id);
                    if let Some(disconnected) = disconnected {
                        self.remove_orphaned_input(disconnected, snarl);
                    }
                    return Some(sink);
                }
                let unconnected = match self.fresh_unconnected() {
                    Ok(id) => id,
                    Err(error) => {
                        self.error = Some(error);
                        return None;
                    }
                };
                if !self.set_input(to_id, to.id.input, unconnected) {
                    self.remove_orphaned_input(unconnected, snarl);
                    return None;
                }
                snarl.disconnect(from.id, to.id);
                if let Some(disconnected) = disconnected {
                    self.remove_orphaned_input(disconnected, snarl);
                }
                return Some(sink);
            }
            _ => {}
        }
        snarl.disconnect(from.id, to.id);
        if let Some(disconnected) = disconnected {
            self.remove_orphaned_input(disconnected, snarl);
        }
        sink
    }

    /// Inserts a palette node and keeps the insertion for history.
    pub(super) fn insert_palette_node_recorded(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        pos: egui::Pos2,
        template: NodeTemplate,
    ) -> Result<(NodeId, SnarlNodeId), String> {
        self.plan_edit(
            |viewer| viewer.insert_palette_node(snarl, pos, template),
            |_, inserted| {
                let &(node, _) = inserted.as_ref().ok()?;
                Some(GraphAction::InsertNode {
                    node,
                    position: CanvasPosition { x: pos.x, y: pos.y },
                })
            },
        )
    }
}
