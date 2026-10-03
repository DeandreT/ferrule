use super::{
    GraphViewer, call_missing_minimum_inputs, graph_references, node_inputs, node_palette,
    sequence_item_ids,
};
use crate::canvas::CanvasNode;
use egui_snarl::{NodeId as SnarlNodeId, Snarl};
use mapping::Node;
use std::collections::BTreeSet;

use mapping::{Graph, NodeId};

/// One complete edit uses one append-only cursor initialized before mutation.
/// Missing generated-owned IDs remain reserved even if planning removes owners.
pub(super) struct NodeIdReservation<'a> {
    next: Option<NodeId>,
    owned_items: &'a BTreeSet<NodeId>,
}

impl<'a> NodeIdReservation<'a> {
    pub(super) fn new(graph: &Graph, owned_items: &'a BTreeSet<NodeId>) -> Self {
        Self {
            next: graph
                .nodes
                .keys()
                .next_back()
                .map_or(Some(0), |id| id.checked_add(1)),
            owned_items,
        }
    }

    pub(super) fn reserve(&mut self, count: usize) -> Result<Vec<NodeId>, String> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let exhausted = || "mapping node IDs are exhausted".to_string();
        let mut candidate = self.next.ok_or_else(exhausted)?;
        // Reject impossible requests before allocating the result buffer.
        let available = u64::from(NodeId::MAX) - u64::from(candidate) + 1;
        if u64::try_from(count).map_or(true, |count| count > available) {
            return Err(exhausted());
        }
        let mut ids = Vec::with_capacity(count);
        while ids.len() < count {
            if !self.owned_items.contains(&candidate) {
                ids.push(candidate);
            }
            self.next = candidate.checked_add(1);
            if ids.len() != count {
                candidate = self.next.ok_or_else(exhausted)?;
            }
        }
        Ok(ids)
    }
}

impl GraphViewer<'_> {
    pub(super) fn owned_item_ids(&self) -> std::collections::BTreeSet<NodeId> {
        if self.function_output.is_some() {
            return Default::default();
        }
        sequence_item_ids(
            self.graph,
            self.root_scope,
            self.extra_targets,
            self.inactive_target_scopes,
            self.project_references,
        )
    }

    pub(super) fn reserve_node_ids(&self, count: usize) -> Result<Vec<NodeId>, String> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let owned_items = self.owned_item_ids();
        NodeIdReservation::new(self.graph, &owned_items).reserve(count)
    }

    pub(super) fn fresh_unconnected(&mut self) -> Result<NodeId, String> {
        let id = self.reserve_node_ids(1)?[0];
        self.graph.nodes.insert(id, Node::Unconnected);
        Ok(id)
    }

    /// Commit only after every required input has an identity. The UI stages
    /// the complete Call/Aggregate node so exhaustion also preserves properties.
    pub(super) fn commit_node_property_edit(
        &mut self,
        node_id: NodeId,
        mut node: Node,
        reconcile_call: bool,
        add_call_arg: bool,
    ) -> Result<usize, String> {
        let missing = match &node {
            Node::Call { function, args } if reconcile_call => {
                call_missing_minimum_inputs(function, args.len())
            }
            _ => 0,
        };
        let aggregate_arg = matches!(&node, Node::Aggregate { function, arg: None, .. }
            if node_palette::aggregate_needs_arg(*function)
                && matches!(self.graph.nodes.get(&node_id), Some(Node::Aggregate { function: old, .. }) if old != function));
        let count = missing + usize::from(add_call_arg) + usize::from(aggregate_arg);
        let ids = self.reserve_node_ids(count)?;
        match &mut node {
            Node::Call { args, .. } => args.extend(ids.iter().copied()),
            Node::Aggregate { arg, .. } if aggregate_arg => *arg = ids.first().copied(),
            _ => {}
        }
        for &id in &ids {
            self.graph.nodes.insert(id, Node::Unconnected);
        }
        self.graph.nodes.insert(node_id, node);
        Ok(count)
    }

    pub(super) fn remove_graph_node(
        &mut self,
        mapping_id: NodeId,
        node: SnarlNodeId,
        snarl: &mut Snarl<CanvasNode>,
    ) -> bool {
        let owned_items = self.owned_item_ids();
        let mut ids = NodeIdReservation::new(self.graph, &owned_items);
        match self.remove_graph_node_reserved(mapping_id, node, snarl, &mut ids) {
            Ok(removed) => removed,
            Err(error) => {
                self.error = Some(error);
                false
            }
        }
    }

    fn remove_graph_node_reserved(
        &mut self,
        mapping_id: NodeId,
        node: SnarlNodeId,
        snarl: &mut Snarl<CanvasNode>,
        ids: &mut NodeIdReservation<'_>,
    ) -> Result<bool, String> {
        let references = self.blocking_references_to(mapping_id);
        if !references.is_empty() {
            self.error = Some(format!(
                "mapping node {mapping_id} is still used by {}",
                references.join(", ")
            ));
            return Ok(false);
        }
        let consumers = self.graph_consumers(mapping_id);
        let replacements = ids.reserve(consumers.len())?;
        self.disconnect_graph_consumers(&consumers, &replacements, snarl);
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
        Ok(true)
    }

    pub fn remove_snarl_nodes(
        &mut self,
        selected: &[SnarlNodeId],
        snarl: &mut Snarl<CanvasNode>,
    ) -> usize {
        // Planning clones only on an explicit delete action. A cursor beginning
        // at the original maximum never reuses IDs freed by earlier removals.
        let owned_items = self.owned_item_ids();
        let mut ids = NodeIdReservation::new(self.graph, &owned_items);
        let mut graph = self.graph.clone();
        let mut scope = self.root_scope.clone();
        let mut staged_snarl = snarl.clone();
        let mut scroll = crate::canvas_endpoints::EndpointScrollState::default();
        let mut planner = GraphViewer {
            graph: &mut graph,
            root_scope: &mut scope,
            primary_root_authoring: false,
            extra_targets: self.extra_targets,
            inactive_target_scopes: self.inactive_target_scopes,
            project_references: self.project_references,
            source_blocks: self.source_blocks,
            target_blocks: self.target_blocks,
            source_x12: self.source_x12,
            target_x12: self.target_x12,
            source_paths: self.source_paths,
            function_names: Default::default(),
            function_inputs: Default::default(),
            parameter_names: Default::default(),
            protected_output: self.protected_output,
            function_output: None,
            requested_function_open: None,
            colors: self.colors,
            wire_color_mode: self.wire_color_mode,
            endpoint_scroll: &mut scroll,
            value_map_wheel: None,
            endpoint_search_match: None,
            node_sizes: None,
            hovered_node: None,
            hovered_node_this_frame: None,
            camera_pan: egui::Vec2::ZERO,
            camera_focus: None,
            canvas_transform: None,
            pin_interaction_ids: Vec::new(),
            error: None,
        };
        let mut pending = selected
            .iter()
            .filter_map(|&node| {
                staged_snarl
                    .get_node(node)
                    .and_then(|canvas| Self::mapping_id(*canvas))
                    .map(|mapping| (mapping, node))
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let mut removed = 0;
        loop {
            // Earlier orphan cleanup may also remove a selected legacy placeholder.
            pending.retain(|_, node| staged_snarl.get_node(*node).is_some());
            let removable = pending.iter().find_map(|(&mapping, &node)| {
                planner
                    .blocking_references_to(mapping)
                    .is_empty()
                    .then_some((mapping, node))
            });
            let Some((mapping, node)) = removable else {
                break;
            };
            match planner.remove_graph_node_reserved(mapping, node, &mut staged_snarl, &mut ids) {
                Ok(true) => removed += 1,
                Ok(false) => {}
                Err(error) => {
                    self.error = Some(error);
                    return 0;
                }
            }
            pending.remove(&mapping);
        }
        if !pending.is_empty() {
            let blocked = pending
                .keys()
                .map(|mapping| mapping.to_string())
                .collect::<Vec<_>>()
                .join(", ");
            planner.error = Some(format!(
                "selected mapping node(s) {blocked} are still owned by a mapping control"
            ));
        }
        let error = planner.error.take();
        drop(planner);
        *self.graph = graph;
        *self.root_scope = scope;
        *snarl = staged_snarl;
        self.error = error;
        removed
    }
}
