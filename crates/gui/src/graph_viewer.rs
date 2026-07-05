//! Renders and edits a mapping as an egui-snarl canvas of [`CanvasNode`]s:
//! the Source/Target schema endpoints plus the mapping graph's function
//! nodes. The snarl's payload carries no node data -- the mapping graph
//! and scope tree stay the single source of truth, borrowed per frame.
//!
//! `SourceField` nodes whose path corresponds to a source leaf are not
//! shown as canvas nodes: a wire leaving the Source endpoint's pin *is*
//! the source field. Connecting a wire into a Target pin creates or
//! replaces the `Binding` in the scope owning that leaf (the scope whose
//! `target_field` chain matches the leaf's group chain -- create the
//! scope in the side panel first for nested targets).

use egui::Ui;
use egui_snarl::ui::{PinInfo, SnarlViewer};
use egui_snarl::{InPin, NodeId as SnarlNodeId, OutPin, Snarl};
use ir::Value;
use mapping::{Binding, Graph, Node, NodeId, Scope};

use crate::canvas::{CanvasNode, SourceLeaf, TargetLeaf};
use crate::value_editor::{show_value_editor, show_value_map_editor};

pub struct GraphViewer<'a> {
    pub graph: &'a mut Graph,
    pub root_scope: &'a mut Scope,
    pub source_leaves: &'a [SourceLeaf],
    pub target_leaves: &'a [TargetLeaf],
    /// Set when an interaction can't be completed (e.g. binding into a
    /// scope that doesn't exist yet); the app surfaces it in the status
    /// line.
    pub error: Option<String>,
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

    fn insert(&mut self, snarl: &mut Snarl<CanvasNode>, pos: egui::Pos2, node: Node) {
        let id = self.fresh_id();
        self.graph.nodes.insert(id, node);
        snarl.insert_node(pos, CanvasNode::Graph(id));
    }

    /// Reuses an existing `SourceField` with this exact path, or creates
    /// one. These nodes are the hidden backing of Source-pin wires.
    fn source_field_for(&mut self, path: &[String]) -> NodeId {
        let existing = self.graph.nodes.iter().find_map(|(id, node)| match node {
            Node::SourceField { path: p } if p == path => Some(*id),
            _ => None,
        });
        existing.unwrap_or_else(|| {
            let id = self.fresh_id();
            self.graph.nodes.insert(
                id,
                Node::SourceField {
                    path: path.to_vec(),
                },
            );
            id
        })
    }

    fn set_input(&mut self, node_id: NodeId, idx: usize, from_id: NodeId) {
        if let Some(node) = self.graph.nodes.get_mut(&node_id) {
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
                Node::Aggregate { arg, .. } => *arg = Some(from_id),
                _ => {}
            }
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

    /// Points the binding for `leaf` at `node`, creating it if absent.
    fn set_binding(&mut self, leaf: &TargetLeaf, node: NodeId) -> bool {
        let Some(scope) = Self::scope_for_chain(self.root_scope, &leaf.chain) else {
            self.error = Some(format!(
                "no scope for `{}` -- create the `{}` scope in the side panel first",
                leaf.label,
                leaf.chain.join("/")
            ));
            return false;
        };
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
        true
    }

    fn remove_binding(&mut self, leaf: &TargetLeaf) {
        if let Some(scope) = Self::scope_for_chain(self.root_scope, &leaf.chain) {
            scope.bindings.retain(|b| b.target_field != leaf.field);
        }
    }

    fn input_count(node: &Node) -> usize {
        match node {
            Node::SourceField { .. } | Node::Const { .. } => 0,
            Node::Call { args, .. } => args.len(),
            Node::If { .. } => 3,
            Node::ValueMap { .. } | Node::Lookup { .. } | Node::Aggregate { .. } => 1,
        }
    }
}

impl SnarlViewer<CanvasNode> for GraphViewer<'_> {
    fn title(&mut self, node: &CanvasNode) -> String {
        match node {
            CanvasNode::Source => "Source".to_string(),
            CanvasNode::Target => "Target".to_string(),
            CanvasNode::Graph(id) => match self.graph.nodes.get(id) {
                Some(Node::SourceField { path }) => format!("Source: {}", path.join("/")),
                Some(Node::Const { value }) => {
                    format!("Const: {}", crate::value_editor::display_string(value))
                }
                Some(Node::Call { function, .. }) => format!("Call: {function}"),
                Some(Node::If { .. }) => "If".to_string(),
                Some(Node::ValueMap { .. }) => "Value Map".to_string(),
                Some(Node::Lookup { collection, .. }) => {
                    format!("Lookup: {}", collection.join("/"))
                }
                Some(Node::Aggregate {
                    function,
                    collection,
                    value,
                    ..
                }) => {
                    let mut path = collection.clone();
                    path.extend(value.iter().cloned());
                    let op = format!("{function:?}").to_lowercase();
                    format!("{op}: {}", path.join("/"))
                }
                None => "<missing>".to_string(),
            },
        }
    }

    fn inputs(&mut self, node: &CanvasNode) -> usize {
        match node {
            CanvasNode::Source => 0,
            CanvasNode::Target => self.target_leaves.len(),
            CanvasNode::Graph(id) => self.graph.nodes.get(id).map_or(0, Self::input_count),
        }
    }

    fn outputs(&mut self, node: &CanvasNode) -> usize {
        match node {
            CanvasNode::Source => self.source_leaves.len(),
            CanvasNode::Target => 0,
            CanvasNode::Graph(_) => 1,
        }
    }

    #[allow(refining_impl_trait)]
    fn show_input(&mut self, pin: &InPin, ui: &mut Ui, snarl: &mut Snarl<CanvasNode>) -> PinInfo {
        let idx = pin.id.input;
        let label = match snarl[pin.id.node] {
            CanvasNode::Target => self
                .target_leaves
                .get(idx)
                .map_or_else(String::new, |l| l.label.clone()),
            CanvasNode::Source => String::new(),
            CanvasNode::Graph(id) => match self.graph.nodes.get(&id) {
                Some(Node::Call { .. }) => format!("arg {idx}"),
                Some(Node::If { .. }) => ["condition", "then", "else"][idx].to_string(),
                Some(Node::ValueMap { .. }) => "input".to_string(),
                Some(Node::Lookup { .. }) => "matches".to_string(),
                Some(Node::Aggregate { .. }) => "arg".to_string(),
                _ => String::new(),
            },
        };
        ui.label(label);
        PinInfo::circle()
    }

    #[allow(refining_impl_trait)]
    fn show_output(&mut self, pin: &OutPin, ui: &mut Ui, snarl: &mut Snarl<CanvasNode>) -> PinInfo {
        let CanvasNode::Graph(node_id) = snarl[pin.id.node] else {
            if let CanvasNode::Source = snarl[pin.id.node]
                && let Some(leaf) = self.source_leaves.get(pin.id.output)
            {
                ui.label(&leaf.label);
            }
            return PinInfo::circle();
        };
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
                Node::Aggregate {
                    collection, value, ..
                } => {
                    for (label, path) in [("collection", collection), ("value", value)] {
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

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<CanvasNode>) {
        let from_node = snarl[from.id.node];
        let to_node = snarl[to.id.node];
        let accepted = match (from_node, to_node) {
            (CanvasNode::Source, CanvasNode::Graph(to_id)) => {
                let Some(leaf) = self.source_leaves.get(from.id.output) else {
                    return;
                };
                let path = leaf.path.clone();
                let field = self.source_field_for(&path);
                self.set_input(to_id, to.id.input, field);
                true
            }
            (CanvasNode::Source, CanvasNode::Target) => {
                let (Some(source_leaf), Some(target_leaf)) = (
                    self.source_leaves.get(from.id.output),
                    self.target_leaves.get(to.id.input).cloned(),
                ) else {
                    return;
                };
                let path = source_leaf.path.clone();
                let field = self.source_field_for(&path);
                self.set_binding(&target_leaf, field)
            }
            (CanvasNode::Graph(from_id), CanvasNode::Target) => {
                let Some(target_leaf) = self.target_leaves.get(to.id.input).cloned() else {
                    return;
                };
                self.set_binding(&target_leaf, from_id)
            }
            (CanvasNode::Graph(from_id), CanvasNode::Graph(to_id)) => {
                self.set_input(to_id, to.id.input, from_id);
                true
            }
            _ => false,
        };
        if !accepted {
            return;
        }
        // Every input takes exactly one value, so replace any existing wire.
        for &remote in &to.remotes {
            snarl.disconnect(remote, to.id);
        }
        snarl.connect(from.id, to.id);
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<CanvasNode>) {
        match (snarl[from.id.node], snarl[to.id.node]) {
            (_, CanvasNode::Target) => {
                if let Some(leaf) = self.target_leaves.get(to.id.input).cloned() {
                    self.remove_binding(&leaf);
                }
            }
            (_, CanvasNode::Graph(to_id)) => {
                let placeholder = self.fresh_const();
                self.set_input(to_id, to.id.input, placeholder);
            }
            _ => {}
        }
        snarl.disconnect(from.id, to.id);
    }

    fn has_graph_menu(&mut self, _pos: egui::Pos2, _snarl: &mut Snarl<CanvasNode>) -> bool {
        true
    }

    fn show_graph_menu(&mut self, pos: egui::Pos2, ui: &mut Ui, snarl: &mut Snarl<CanvasNode>) {
        ui.label("Add node");
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
        if ui.button("Source field (manual path)").clicked() {
            self.insert(snarl, pos, Node::SourceField { path: vec![] });
            ui.close();
        }
    }

    fn has_node_menu(&mut self, node: &CanvasNode) -> bool {
        matches!(node, CanvasNode::Graph(_))
    }

    fn show_node_menu(
        &mut self,
        node: SnarlNodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        if ui.button("Remove").clicked() {
            if let CanvasNode::Graph(mapping_id) = snarl[node] {
                self.graph.nodes.remove(&mapping_id);
            }
            snarl.remove_node(node);
            ui.close();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::canvas::{source_leaves, target_leaves};
    use egui_snarl::{InPinId, OutPinId};
    use ir::{ScalarType, SchemaNode};

    struct Fixture {
        graph: Graph,
        root_scope: Scope,
        source_leaves: Vec<SourceLeaf>,
        target_leaves: Vec<TargetLeaf>,
        snarl: Snarl<CanvasNode>,
        source: SnarlNodeId,
        target: SnarlNodeId,
        call: SnarlNodeId,
    }

    /// source: row { name, age }; target: row { out };
    /// graph: 0 = concat() shown on the canvas.
    fn fixture() -> Fixture {
        let source_schema = SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("age", ScalarType::Int),
            ],
        );
        let target_schema =
            SchemaNode::group("row", vec![SchemaNode::scalar("out", ScalarType::String)]);
        let mut graph = Graph::default();
        graph.nodes.insert(
            0,
            Node::Call {
                function: "concat".to_string(),
                args: vec![],
            },
        );
        let mut snarl = Snarl::new();
        let source = snarl.insert_node(egui::pos2(0.0, 0.0), CanvasNode::Source);
        let target = snarl.insert_node(egui::pos2(400.0, 0.0), CanvasNode::Target);
        let call = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(0));
        Fixture {
            graph,
            root_scope: Scope::default(),
            source_leaves: source_leaves(&source_schema),
            target_leaves: target_leaves(&target_schema),
            snarl,
            source,
            target,
            call,
        }
    }

    impl Fixture {
        fn viewer(&mut self) -> GraphViewer<'_> {
            GraphViewer {
                graph: &mut self.graph,
                root_scope: &mut self.root_scope,
                source_leaves: &self.source_leaves,
                target_leaves: &self.target_leaves,
                error: None,
            }
        }
    }

    #[test]
    fn source_pin_to_target_pin_creates_a_source_field_and_binding() {
        let mut fx = fixture();
        let mut snarl = std::mem::take(&mut fx.snarl);
        let from = snarl.out_pin(OutPinId {
            node: fx.source,
            output: 0, // "name"
        });
        let to = snarl.in_pin(InPinId {
            node: fx.target,
            input: 0, // "out"
        });
        let (source, target) = (fx.source, fx.target);
        fx.viewer().connect(&from, &to, &mut snarl);

        let field_id = fx
            .graph
            .nodes
            .iter()
            .find_map(|(id, n)| {
                matches!(n, Node::SourceField { path } if path == &["name"]).then_some(*id)
            })
            .expect("a SourceField for `name` should exist");
        assert_eq!(fx.root_scope.bindings.len(), 1);
        assert_eq!(fx.root_scope.bindings[0].target_field, "out");
        assert_eq!(fx.root_scope.bindings[0].node, field_id);
        let wired: Vec<_> = snarl.wires().collect();
        assert_eq!(
            wired,
            vec![(
                OutPinId {
                    node: source,
                    output: 0
                },
                InPinId {
                    node: target,
                    input: 0
                }
            )]
        );
    }

    #[test]
    fn source_pin_to_call_arg_reuses_one_source_field() {
        let mut fx = fixture();
        // Give the call two args to wire into.
        if let Some(Node::Call { args, .. }) = fx.graph.nodes.get_mut(&0) {
            args.extend([100, 100]); // dangling placeholders
        }
        let mut snarl = std::mem::take(&mut fx.snarl);
        for input in 0..2 {
            let from = snarl.out_pin(OutPinId {
                node: fx.source,
                output: 1, // "age"
            });
            let to = snarl.in_pin(InPinId {
                node: fx.call,
                input,
            });
            fx.viewer().connect(&from, &to, &mut snarl);
        }
        let field_ids: Vec<_> = fx
            .graph
            .nodes
            .iter()
            .filter(|(_, n)| matches!(n, Node::SourceField { .. }))
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(field_ids.len(), 1, "the same SourceField should be reused");
        if let Some(Node::Call { args, .. }) = fx.graph.nodes.get(&0) {
            assert_eq!(args, &vec![field_ids[0], field_ids[0]]);
        } else {
            panic!("call node vanished");
        }
    }

    #[test]
    fn disconnecting_a_target_pin_removes_the_binding() {
        let mut fx = fixture();
        let mut snarl = std::mem::take(&mut fx.snarl);
        let from = snarl.out_pin(OutPinId {
            node: fx.source,
            output: 0,
        });
        let to = snarl.in_pin(InPinId {
            node: fx.target,
            input: 0,
        });
        fx.viewer().connect(&from, &to, &mut snarl);
        assert_eq!(fx.root_scope.bindings.len(), 1);

        // Re-fetch the pins so `remotes` reflects the wire.
        let from = snarl.out_pin(OutPinId {
            node: fx.source,
            output: 0,
        });
        let to = snarl.in_pin(InPinId {
            node: fx.target,
            input: 0,
        });
        fx.viewer().disconnect(&from, &to, &mut snarl);
        assert!(fx.root_scope.bindings.is_empty());
        assert_eq!(snarl.wires().count(), 0);
    }

    #[test]
    fn binding_into_a_missing_scope_reports_instead_of_wiring() {
        let mut fx = fixture();
        fx.target_leaves = vec![TargetLeaf {
            label: "Order/b".into(),
            chain: vec!["Order".into()],
            field: "b".into(),
        }];
        let mut snarl = std::mem::take(&mut fx.snarl);
        let from = snarl.out_pin(OutPinId {
            node: fx.source,
            output: 0,
        });
        let to = snarl.in_pin(InPinId {
            node: fx.target,
            input: 0,
        });
        let mut viewer = fx.viewer();
        viewer.connect(&from, &to, &mut snarl);
        assert!(viewer.error.is_some());
        assert_eq!(snarl.wires().count(), 0);
        assert!(fx.root_scope.bindings.is_empty());
    }
}
