//! Renders and edits `mapping::Graph` as an egui-snarl node canvas.
//!
//! The snarl's own node payload is just a `mapping::NodeId` -- the real node
//! data lives in `mapping::Graph`, which this viewer borrows for the
//! duration of one frame's `show()` call. This avoids keeping two copies of
//! the graph in sync.

use egui::Ui;
use egui_snarl::ui::{PinInfo, SnarlViewer};
use egui_snarl::{InPin, NodeId as SnarlNodeId, OutPin, Snarl};
use ir::Value;
use mapping::{Graph, Node, NodeId};

use crate::value_editor::{show_value_editor, show_value_map_editor};

pub struct GraphViewer<'a> {
    pub graph: &'a mut Graph,
}

impl GraphViewer<'_> {
    fn fresh_id(&self) -> NodeId {
        self.graph.nodes.keys().next_back().map_or(0, |max| max + 1)
    }

    fn fresh_const(&mut self) -> NodeId {
        let id = self.fresh_id();
        self.graph
            .nodes
            .insert(id, Node::Const { value: Value::Null });
        id
    }

    fn insert(&mut self, snarl: &mut Snarl<NodeId>, pos: egui::Pos2, node: Node) {
        let id = self.fresh_id();
        self.graph.nodes.insert(id, node);
        snarl.insert_node(pos, id);
    }

    fn input_count(node: &Node) -> usize {
        match node {
            Node::SourceField { .. } | Node::Const { .. } => 0,
            Node::Call { args, .. } => args.len(),
            Node::If { .. } => 3,
            Node::ValueMap { .. } | Node::Lookup { .. } => 1,
        }
    }
}

impl SnarlViewer<NodeId> for GraphViewer<'_> {
    fn title(&mut self, node: &NodeId) -> String {
        match self.graph.nodes.get(node) {
            Some(Node::SourceField { path }) => format!("Source: {}", path.join("/")),
            Some(Node::Const { value }) => {
                format!("Const: {}", crate::value_editor::display_string(value))
            }
            Some(Node::Call { function, .. }) => format!("Call: {function}"),
            Some(Node::If { .. }) => "If".to_string(),
            Some(Node::ValueMap { .. }) => "Value Map".to_string(),
            Some(Node::Lookup { collection, .. }) => format!("Lookup: {}", collection.join("/")),
            None => "<missing>".to_string(),
        }
    }

    fn inputs(&mut self, node: &NodeId) -> usize {
        self.graph.nodes.get(node).map_or(0, Self::input_count)
    }

    fn outputs(&mut self, _node: &NodeId) -> usize {
        1
    }

    #[allow(refining_impl_trait)]
    fn show_input(&mut self, pin: &InPin, ui: &mut Ui, snarl: &mut Snarl<NodeId>) -> PinInfo {
        let node_id = snarl[pin.id.node];
        let idx = pin.id.input;
        let label = match self.graph.nodes.get(&node_id) {
            Some(Node::Call { .. }) => format!("arg {idx}"),
            Some(Node::If { .. }) => ["condition", "then", "else"][idx].to_string(),
            Some(Node::ValueMap { .. }) => "input".to_string(),
            Some(Node::Lookup { .. }) => "matches".to_string(),
            _ => String::new(),
        };
        ui.label(label);
        PinInfo::circle()
    }

    #[allow(refining_impl_trait)]
    fn show_output(&mut self, pin: &OutPin, ui: &mut Ui, snarl: &mut Snarl<NodeId>) -> PinInfo {
        let node_id = snarl[pin.id.node];
        let mut new_arg_needed = false;
        if let Some(node) = self.graph.nodes.get_mut(&node_id) {
            match node {
                Node::SourceField { path } => {
                    let mut joined = path.join("/");
                    if ui.text_edit_singleline(&mut joined).changed() {
                        *path = joined
                            .split('/')
                            .map(str::to_string)
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
                Node::Const { value } => show_value_editor(ui, value),
                Node::Call { function, args } => {
                    ui.text_edit_singleline(function);
                    ui.horizontal(|ui| {
                        if ui.small_button("+arg").clicked() {
                            new_arg_needed = true;
                        }
                        if !args.is_empty() && ui.small_button("-arg").clicked() {
                            args.pop();
                        }
                    });
                }
                Node::If { .. } => {
                    ui.label("condition ? then : else");
                }
                Node::ValueMap { table, default, .. } => show_value_map_editor(ui, table, default),
                Node::Lookup {
                    collection,
                    key,
                    value,
                    ..
                } => {
                    for (label, path) in
                        [("collection", collection), ("key", key), ("value", value)]
                    {
                        ui.horizontal(|ui| {
                            ui.label(label);
                            let mut joined = path.join("/");
                            if ui.text_edit_singleline(&mut joined).changed() {
                                *path = joined
                                    .split('/')
                                    .map(str::to_string)
                                    .filter(|s| !s.is_empty())
                                    .collect();
                            }
                        });
                    }
                }
            }
        }
        if new_arg_needed {
            let new_id = self.fresh_const();
            if let Some(Node::Call { args, .. }) = self.graph.nodes.get_mut(&node_id) {
                args.push(new_id);
            }
        }
        PinInfo::circle()
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<NodeId>) {
        let from_id = snarl[from.id.node];
        let to_id = snarl[to.id.node];
        let idx = to.id.input;
        if let Some(node) = self.graph.nodes.get_mut(&to_id) {
            match node {
                Node::Call { args, .. } => {
                    if idx < args.len() {
                        args[idx] = from_id;
                    }
                }
                Node::If {
                    condition,
                    then,
                    else_,
                } => match idx {
                    0 => *condition = from_id,
                    1 => *then = from_id,
                    2 => *else_ = from_id,
                    _ => {}
                },
                Node::ValueMap { input, .. } => *input = from_id,
                Node::Lookup { matches, .. } => *matches = from_id,
                _ => {}
            }
        }
        // Every input takes exactly one value, so replace any existing wire.
        for &remote in &to.remotes {
            snarl.disconnect(remote, to.id);
        }
        snarl.connect(from.id, to.id);
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<NodeId>) {
        let to_id = snarl[to.id.node];
        let idx = to.id.input;
        let placeholder = self.fresh_id();
        let mut used_placeholder = false;
        if let Some(node) = self.graph.nodes.get_mut(&to_id) {
            match node {
                Node::Call { args, .. } => {
                    if idx < args.len() {
                        args[idx] = placeholder;
                        used_placeholder = true;
                    }
                }
                Node::If {
                    condition,
                    then,
                    else_,
                } => {
                    used_placeholder = true;
                    match idx {
                        0 => *condition = placeholder,
                        1 => *then = placeholder,
                        2 => *else_ = placeholder,
                        _ => used_placeholder = false,
                    }
                }
                Node::ValueMap { input, .. } => {
                    *input = placeholder;
                    used_placeholder = true;
                }
                Node::Lookup { matches, .. } => {
                    *matches = placeholder;
                    used_placeholder = true;
                }
                _ => {}
            }
        }
        if used_placeholder {
            self.graph
                .nodes
                .insert(placeholder, Node::Const { value: Value::Null });
        }
        snarl.disconnect(from.id, to.id);
    }

    fn has_graph_menu(&mut self, _pos: egui::Pos2, _snarl: &mut Snarl<NodeId>) -> bool {
        true
    }

    fn show_graph_menu(&mut self, pos: egui::Pos2, ui: &mut Ui, snarl: &mut Snarl<NodeId>) {
        ui.label("Add node");
        if ui.button("Source field").clicked() {
            self.insert(snarl, pos, Node::SourceField { path: vec![] });
            ui.close();
        }
        if ui.button("Const").clicked() {
            self.insert(snarl, pos, Node::Const { value: Value::Null });
            ui.close();
        }
        if ui.button("Call").clicked() {
            self.insert(
                snarl,
                pos,
                Node::Call {
                    function: "concat".to_string(),
                    args: vec![],
                },
            );
            ui.close();
        }
        if ui.button("If").clicked() {
            let condition = self.fresh_const();
            let then = self.fresh_const();
            let else_ = self.fresh_const();
            self.insert(
                snarl,
                pos,
                Node::If {
                    condition,
                    then,
                    else_,
                },
            );
            ui.close();
        }
        if ui.button("Value map").clicked() {
            let input = self.fresh_const();
            self.insert(
                snarl,
                pos,
                Node::ValueMap {
                    input,
                    table: vec![],
                    default: None,
                },
            );
            ui.close();
        }
        if ui.button("Lookup").clicked() {
            let matches = self.fresh_const();
            self.insert(
                snarl,
                pos,
                Node::Lookup {
                    collection: vec![],
                    key: vec![],
                    matches,
                    value: vec![],
                },
            );
            ui.close();
        }
    }

    fn has_node_menu(&mut self, _node: &NodeId) -> bool {
        true
    }

    fn show_node_menu(
        &mut self,
        node: SnarlNodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeId>,
    ) {
        if ui.button("Remove").clicked() {
            let mapping_id = snarl[node];
            self.graph.nodes.remove(&mapping_id);
            snarl.remove_node(node);
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_snarl::{InPinId, OutPinId};

    /// graph: 0 = Const(1), 1 = Const(2), 2 = concat(node 0); snarl mirrors
    /// it with the 0 -> (2, arg 0) wire already present.
    fn wired_fixture() -> (Graph, Snarl<NodeId>, [SnarlNodeId; 3]) {
        let mut graph = Graph::default();
        graph.nodes.insert(
            0,
            Node::Const {
                value: Value::Int(1),
            },
        );
        graph.nodes.insert(
            1,
            Node::Const {
                value: Value::Int(2),
            },
        );
        graph.nodes.insert(
            2,
            Node::Call {
                function: "concat".to_string(),
                args: vec![0],
            },
        );

        let mut snarl = Snarl::new();
        let s0 = snarl.insert_node(egui::pos2(0.0, 0.0), 0);
        let s1 = snarl.insert_node(egui::pos2(0.0, 100.0), 1);
        let s2 = snarl.insert_node(egui::pos2(200.0, 0.0), 2);
        snarl.connect(
            OutPinId {
                node: s0,
                output: 0,
            },
            InPinId { node: s2, input: 0 },
        );
        (graph, snarl, [s0, s1, s2])
    }

    fn wire_set(snarl: &Snarl<NodeId>) -> Vec<(NodeId, NodeId, usize)> {
        let mut wires: Vec<_> = snarl
            .wires()
            .map(|(from, to)| (snarl[from.node], snarl[to.node], to.input))
            .collect();
        wires.sort_unstable();
        wires
    }

    #[test]
    fn connect_updates_call_arg_and_replaces_existing_wire() {
        let (mut graph, mut snarl, [_, s1, s2]) = wired_fixture();
        let mut viewer = GraphViewer { graph: &mut graph };

        let from = snarl.out_pin(OutPinId {
            node: s1,
            output: 0,
        });
        let to = snarl.in_pin(InPinId { node: s2, input: 0 });
        viewer.connect(&from, &to, &mut snarl);

        assert!(
            matches!(&graph.nodes[&2], Node::Call { args, .. } if args == &vec![1]),
            "call arg should now reference node 1"
        );
        assert_eq!(
            wire_set(&snarl),
            vec![(1, 2, 0)],
            "the old 0 -> (2, 0) wire should be replaced, not accumulated"
        );
    }

    #[test]
    fn disconnect_rewires_input_to_a_fresh_null_const() {
        let (mut graph, mut snarl, [s0, _, s2]) = wired_fixture();
        let mut viewer = GraphViewer { graph: &mut graph };

        let from = snarl.out_pin(OutPinId {
            node: s0,
            output: 0,
        });
        let to = snarl.in_pin(InPinId { node: s2, input: 0 });
        viewer.disconnect(&from, &to, &mut snarl);

        assert!(wire_set(&snarl).is_empty(), "the snarl wire should be gone");
        let Node::Call { args, .. } = &graph.nodes[&2] else {
            panic!("node 2 should still be a call");
        };
        let placeholder = args[0];
        assert_ne!(placeholder, 0, "arg should no longer reference node 0");
        assert!(
            matches!(
                graph.nodes[&placeholder],
                Node::Const { value: Value::Null }
            ),
            "arg should point at a fresh null const placeholder"
        );
    }
}
