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
use egui_snarl::{InPin, InPinId, NodeId as SnarlNodeId, OutPin, OutPinId, Snarl};
use ir::Value;
use mapping::{AggregateOp, Binding, Graph, Node, NodeId, Scope};

use crate::canvas::{CanvasNode, SourceLeaf, TargetLeaf};
use crate::path_picker::SourcePathCatalog;
use crate::value_editor::{show_value_editor, show_value_map_editor};

pub struct GraphViewer<'a> {
    pub graph: &'a mut Graph,
    pub root_scope: &'a mut Scope,
    pub source_leaves: &'a [SourceLeaf],
    pub target_leaves: &'a [TargetLeaf],
    pub source_paths: &'a SourcePathCatalog,
    /// Set when an interaction can't be completed (e.g. binding into a
    /// scope that doesn't exist yet); the app surfaces it in the status
    /// line.
    pub error: Option<String>,
}

impl GraphViewer<'_> {
    const AGGREGATE_OPS: [(AggregateOp, &'static str); 7] = [
        (AggregateOp::Count, "Count"),
        (AggregateOp::Sum, "Sum"),
        (AggregateOp::Avg, "Average"),
        (AggregateOp::Min, "Minimum"),
        (AggregateOp::Max, "Maximum"),
        (AggregateOp::Join, "String join"),
        (AggregateOp::ItemAt, "Item at"),
    ];

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

    fn mapping_id(node: CanvasNode) -> Option<NodeId> {
        match node {
            CanvasNode::Graph(id) | CanvasNode::Placeholder(id) => Some(id),
            CanvasNode::Source | CanvasNode::Target => None,
        }
    }

    fn placeholder_position(owner: egui::Pos2, input: usize, inputs: usize) -> egui::Pos2 {
        let offset = input as f32 - (inputs.saturating_sub(1) as f32 / 2.0);
        egui::pos2(owner.x - 260.0, owner.y + offset * 90.0)
    }

    fn insert_placeholder(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        pos: egui::Pos2,
    ) -> (NodeId, SnarlNodeId) {
        let id = self.fresh_const();
        let snarl_id = snarl.insert_node(pos, CanvasNode::Placeholder(id));
        (id, snarl_id)
    }

    fn aggregate_needs_arg(function: AggregateOp) -> bool {
        matches!(function, AggregateOp::Join | AggregateOp::ItemAt)
    }

    fn aggregate_node(function: AggregateOp, arg: Option<NodeId>) -> Node {
        Node::Aggregate {
            function,
            collection: Vec::new(),
            value: Vec::new(),
            expression: None,
            arg,
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

    fn insert_with_placeholders(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        pos: egui::Pos2,
        input_count: usize,
        build: impl FnOnce(&[NodeId]) -> Node,
    ) -> (NodeId, SnarlNodeId) {
        let placeholders: Vec<_> = (0..input_count)
            .map(|input| {
                self.insert_placeholder(snarl, Self::placeholder_position(pos, input, input_count))
            })
            .collect();
        let ids: Vec<_> = placeholders.iter().map(|(id, _)| *id).collect();
        let result = self.insert(snarl, pos, build(&ids));
        for (input, (_, placeholder)) in placeholders.into_iter().enumerate() {
            snarl.connect(
                OutPinId {
                    node: placeholder,
                    output: 0,
                },
                InPinId {
                    node: result.1,
                    input,
                },
            );
        }
        result
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
                Node::Aggregate {
                    expression, arg, ..
                } => {
                    if expression.is_some() && idx == 0 {
                        *expression = Some(from_id);
                    } else if arg.is_some() && idx == usize::from(expression.is_some()) {
                        *arg = Some(from_id);
                    }
                }
                _ => {}
            }
        }
    }

    fn input_at(&self, node_id: NodeId, idx: usize) -> Option<NodeId> {
        match self.graph.nodes.get(&node_id)? {
            Node::Call { args, .. } => args.get(idx).copied(),
            Node::If {
                condition,
                then,
                else_,
            } => [*condition, *then, *else_].get(idx).copied(),
            Node::ValueMap { input, .. } => (idx == 0).then_some(*input),
            Node::Lookup { matches, .. } => (idx == 0).then_some(*matches),
            Node::Aggregate {
                expression, arg, ..
            } => expression.iter().chain(arg).nth(idx).copied(),
            Node::SourceField { .. }
            | Node::Position { .. }
            | Node::Const { .. }
            | Node::RuntimeValue { .. } => None,
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

    fn binding_node(&mut self, leaf: &TargetLeaf) -> Option<NodeId> {
        Self::scope_for_chain(self.root_scope, &leaf.chain)?
            .bindings
            .iter()
            .find(|binding| binding.target_field == leaf.field)
            .map(|binding| binding.node)
    }

    fn references_to(&self, needle: NodeId) -> Vec<String> {
        fn graph_inputs(node: &Node) -> Vec<NodeId> {
            match node {
                Node::SourceField { .. }
                | Node::Position { .. }
                | Node::Const { .. }
                | Node::RuntimeValue { .. } => Vec::new(),
                Node::Call { args, .. } => args.clone(),
                Node::If {
                    condition,
                    then,
                    else_,
                } => vec![*condition, *then, *else_],
                Node::ValueMap { input, .. } => vec![*input],
                Node::Lookup { matches, .. } => vec![*matches],
                Node::Aggregate {
                    expression, arg, ..
                } => expression.iter().chain(arg).copied().collect(),
            }
        }

        fn scope_references(
            scope: &Scope,
            path: &mut Vec<String>,
            needle: NodeId,
            found: &mut std::collections::BTreeSet<String>,
        ) {
            let label = if path.is_empty() {
                "root scope".to_string()
            } else {
                format!("scope {}", path.join("/"))
            };
            if scope.filter == Some(needle) {
                found.insert(format!("{label} filter"));
            }
            if scope.group_by == Some(needle) {
                found.insert(format!("{label} group-by key"));
            }
            if scope.sort_by == Some(needle) {
                found.insert(format!("{label} sort key"));
            }
            if scope.take == Some(needle) {
                found.insert(format!("{label} take count"));
            }
            if let Some(sequence) = &scope.sequence {
                if sequence.inputs().contains(&needle) {
                    found.insert(format!("{label} sequence input"));
                }
                if sequence.item() == needle {
                    found.insert(format!("{label} sequence item"));
                }
            }
            for binding in &scope.bindings {
                if binding.node == needle {
                    found.insert(format!("{label} binding {}", binding.target_field));
                }
            }
            for child in &scope.children {
                path.push(child.target_field.clone());
                scope_references(child, path, needle, found);
                path.pop();
            }
        }

        let mut found = std::collections::BTreeSet::new();
        for (&owner, node) in &self.graph.nodes {
            if owner != needle && graph_inputs(node).contains(&needle) {
                found.insert(format!("graph node {owner}"));
            }
        }
        scope_references(self.root_scope, &mut Vec::new(), needle, &mut found);
        found.into_iter().collect()
    }

    fn remove_orphaned_placeholder(&mut self, needle: NodeId, snarl: &mut Snarl<CanvasNode>) {
        if !self.references_to(needle).is_empty() {
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
    ) {
        let inputs = self
            .graph
            .nodes
            .get(&mapping_id)
            .map(node_inputs)
            .unwrap_or_default();
        self.graph.nodes.remove(&mapping_id);
        snarl.remove_node(node);
        for input in inputs {
            self.remove_orphaned_placeholder(input, snarl);
        }
    }

    fn input_count(node: &Node) -> usize {
        match node {
            Node::SourceField { .. }
            | Node::Position { .. }
            | Node::Const { .. }
            | Node::RuntimeValue { .. } => 0,
            Node::Call { args, .. } => args.len(),
            Node::If { .. } => 3,
            Node::ValueMap { .. } | Node::Lookup { .. } => 1,
            Node::Aggregate {
                expression, arg, ..
            } => usize::from(expression.is_some()) + usize::from(arg.is_some()),
        }
    }
}

fn node_inputs(node: &Node) -> Vec<NodeId> {
    match node {
        Node::SourceField { .. }
        | Node::Position { .. }
        | Node::Const { .. }
        | Node::RuntimeValue { .. } => Vec::new(),
        Node::Call { args, .. } => args.clone(),
        Node::If {
            condition,
            then,
            else_,
        } => vec![*condition, *then, *else_],
        Node::ValueMap { input, .. } => vec![*input],
        Node::Lookup { matches, .. } => vec![*matches],
        Node::Aggregate {
            expression, arg, ..
        } => expression.iter().chain(arg).copied().collect(),
    }
}

impl SnarlViewer<CanvasNode> for GraphViewer<'_> {
    fn title(&mut self, node: &CanvasNode) -> String {
        match node {
            CanvasNode::Source => "Source".to_string(),
            CanvasNode::Target => "Target".to_string(),
            CanvasNode::Graph(id) | CanvasNode::Placeholder(id) => match self.graph.nodes.get(id) {
                Some(Node::SourceField { path, frame }) => {
                    let owner = frame
                        .as_ref()
                        .and_then(|frame| frame.last())
                        .map(|owner| format!("{owner}/"))
                        .unwrap_or_default();
                    format!("Source: {owner}{}", path.join("/"))
                }
                Some(Node::Position { collection }) if collection.is_empty() => {
                    "Position".to_string()
                }
                Some(Node::Position { collection }) => {
                    format!("Position: {}", collection.join("/"))
                }
                Some(Node::Const { value }) => {
                    format!("Const: {}", crate::value_editor::display_string(value))
                }
                Some(Node::RuntimeValue { value }) => format!("Runtime: {value:?}"),
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
                    expression,
                    ..
                }) => {
                    let mut path = collection.clone();
                    if expression.is_none() {
                        path.extend(value.iter().cloned());
                    }
                    let op = format!("{function:?}").to_lowercase();
                    let target = expression.map_or_else(|| path.join("/"), |_| "computed".into());
                    format!("{op}: {target}")
                }
                None => "<missing>".to_string(),
            },
        }
    }

    fn inputs(&mut self, node: &CanvasNode) -> usize {
        match node {
            CanvasNode::Source => 0,
            CanvasNode::Target => self.target_leaves.len(),
            CanvasNode::Graph(id) | CanvasNode::Placeholder(id) => {
                self.graph.nodes.get(id).map_or(0, Self::input_count)
            }
        }
    }

    fn outputs(&mut self, node: &CanvasNode) -> usize {
        match node {
            CanvasNode::Source => self.source_leaves.len(),
            CanvasNode::Target => 0,
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => 1,
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
            CanvasNode::Graph(id) | CanvasNode::Placeholder(id) => {
                match self.graph.nodes.get(&id) {
                    Some(Node::Call { .. }) => format!("arg {idx}"),
                    Some(Node::If { .. }) => ["condition", "then", "else"][idx].to_string(),
                    Some(Node::ValueMap { .. }) => "input".to_string(),
                    Some(Node::Lookup { .. }) => "match/key".to_string(),
                    Some(Node::Aggregate { expression, .. })
                        if expression.is_some() && idx == 0 =>
                    {
                        "values".to_string()
                    }
                    Some(Node::Aggregate { .. }) => "arg".to_string(),
                    _ => String::new(),
                }
            }
        };
        ui.label(label);
        PinInfo::circle()
    }

    #[allow(refining_impl_trait)]
    fn show_output(&mut self, pin: &OutPin, ui: &mut Ui, snarl: &mut Snarl<CanvasNode>) -> PinInfo {
        let Some(node_id) = Self::mapping_id(snarl[pin.id.node]) else {
            if let CanvasNode::Source = snarl[pin.id.node]
                && let Some(leaf) = self.source_leaves.get(pin.id.output)
            {
                ui.label(&leaf.label);
            }
            return PinInfo::circle();
        };
        let mut new_call_arg_needed = false;
        let mut remove_call_wire = None;
        let mut new_aggregate_arg_needed = false;
        let mut remove_aggregate_wire = None;
        if let Some(node) = self.graph.nodes.get_mut(&node_id) {
            match node {
                Node::SourceField { path, frame } => {
                    if let Some(frame) = frame {
                        ui.label(format!(
                            "@{}",
                            frame.last().map(String::as_str).unwrap_or("frame")
                        ))
                        .on_hover_text(format!("source frame: {}", frame.join("/")));
                    }
                    let mut joined = path.join("/");
                    if ui.text_edit_singleline(&mut joined).changed() {
                        *path = joined
                            .split('/')
                            .map(str::to_string)
                            .filter(|s| !s.is_empty())
                            .collect();
                    }
                }
                Node::Position { collection } => {
                    self.source_paths.show_collection_picker(
                        ui,
                        ui.id().with("position_collection"),
                        collection,
                    );
                }
                Node::Const { value } => show_value_editor(ui, value),
                Node::RuntimeValue { value } => {
                    ui.label(format!("{value:?}"));
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
                    ui.vertical(|ui| {
                        egui::Grid::new(ui.id().with("lookup_paths")).show(ui, |ui| {
                            ui.label("collection");
                            self.source_paths.show_collection_picker(
                                ui,
                                ui.id().with("lookup_collection"),
                                collection,
                            );
                            ui.end_row();
                            ui.label("");
                            self.source_paths.show_value_picker(
                                ui,
                                ui.id().with("lookup_key"),
                                collection,
                                key,
                            );
                            ui.end_row();
                            ui.label("value");
                            self.source_paths.show_value_picker(
                                ui,
                                ui.id().with("lookup_value"),
                                collection,
                                value,
                            );
                            ui.end_row();
                        });
                    });
                }
                Node::Aggregate {
                    function,
                    collection,
                    value,
                    expression,
                    arg,
                } => {
                    let previous = *function;
                    ui.vertical(|ui| {
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
                                    Self::AGGREGATE_OPS
                                        .iter()
                                        .find(|(op, _)| op == function)
                                        .map_or("Aggregate", |(_, label)| *label),
                                )
                                .show_ui(ui, |ui| {
                                    for (op, label) in Self::AGGREGATE_OPS {
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
                        if previous != *function {
                            if Self::aggregate_needs_arg(*function) && arg.is_none() {
                                new_aggregate_arg_needed = true;
                            } else if !Self::aggregate_needs_arg(*function) {
                                remove_aggregate_wire = arg.take();
                            }
                        }
                    });
                }
            }
        }
        if new_call_arg_needed {
            let input = self.graph.nodes.get(&node_id).map_or(0, Self::input_count);
            let owner = snarl
                .get_node_info(pin.id.node)
                .map_or(egui::Pos2::ZERO, |node| node.pos);
            let (new_id, placeholder) =
                self.insert_placeholder(snarl, Self::placeholder_position(owner, input, input + 1));
            if let Some(Node::Call { args, .. }) = self.graph.nodes.get_mut(&node_id) {
                args.push(new_id);
            }
            snarl.connect(
                OutPinId {
                    node: placeholder,
                    output: 0,
                },
                InPinId {
                    node: pin.id.node,
                    input,
                },
            );
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
            self.remove_orphaned_placeholder(removed, snarl);
        }
        if new_aggregate_arg_needed {
            let input = self.graph.nodes.get(&node_id).map_or(0, Self::input_count);
            let owner = snarl
                .get_node_info(pin.id.node)
                .map_or(egui::Pos2::ZERO, |node| node.pos);
            let (new_id, placeholder) =
                self.insert_placeholder(snarl, Self::placeholder_position(owner, input, input + 1));
            if let Some(Node::Aggregate { arg, .. }) = self.graph.nodes.get_mut(&node_id) {
                *arg = Some(new_id);
            }
            snarl.connect(
                OutPinId {
                    node: placeholder,
                    output: 0,
                },
                InPinId {
                    node: pin.id.node,
                    input,
                },
            );
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
            self.remove_orphaned_placeholder(removed, snarl);
        }
        PinInfo::circle()
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<CanvasNode>) {
        let from_node = snarl[from.id.node];
        let to_node = snarl[to.id.node];
        let displaced = match to_node {
            CanvasNode::Graph(to_id) | CanvasNode::Placeholder(to_id) => {
                self.input_at(to_id, to.id.input)
            }
            CanvasNode::Target => self
                .target_leaves
                .get(to.id.input)
                .cloned()
                .and_then(|leaf| self.binding_node(&leaf)),
            CanvasNode::Source => None,
        };
        let accepted = match (from_node, to_node) {
            (CanvasNode::Source, CanvasNode::Graph(to_id) | CanvasNode::Placeholder(to_id)) => {
                let Some(leaf) = self.source_leaves.get(from.id.output) else {
                    return;
                };
                // The graph retains independent ownership after this pin
                // catalog is rebuilt on the next UI frame.
                let field = self.source_field_for(leaf.frame.clone(), leaf.path.clone());
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
                let field =
                    self.source_field_for(source_leaf.frame.clone(), source_leaf.path.clone());
                self.set_binding(&target_leaf, field)
            }
            (CanvasNode::Graph(from_id) | CanvasNode::Placeholder(from_id), CanvasNode::Target) => {
                let Some(target_leaf) = self.target_leaves.get(to.id.input).cloned() else {
                    return;
                };
                self.set_binding(&target_leaf, from_id)
            }
            (
                CanvasNode::Graph(from_id) | CanvasNode::Placeholder(from_id),
                CanvasNode::Graph(to_id) | CanvasNode::Placeholder(to_id),
            ) => {
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
        if let Some(displaced) = displaced {
            self.remove_orphaned_placeholder(displaced, snarl);
        }
    }

    fn disconnect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<CanvasNode>) {
        let disconnected = match snarl[to.id.node] {
            CanvasNode::Graph(to_id) | CanvasNode::Placeholder(to_id) => {
                self.input_at(to_id, to.id.input)
            }
            CanvasNode::Target => self
                .target_leaves
                .get(to.id.input)
                .cloned()
                .and_then(|leaf| self.binding_node(&leaf)),
            CanvasNode::Source => None,
        };
        match (snarl[from.id.node], snarl[to.id.node]) {
            (_, CanvasNode::Target) => {
                if let Some(leaf) = self.target_leaves.get(to.id.input).cloned() {
                    self.remove_binding(&leaf);
                }
            }
            (_, CanvasNode::Graph(to_id) | CanvasNode::Placeholder(to_id)) => {
                let owner = snarl
                    .get_node_info(to.id.node)
                    .map_or(egui::Pos2::ZERO, |node| node.pos);
                snarl.disconnect(from.id, to.id);
                let (placeholder, placeholder_node) = self.insert_placeholder(
                    snarl,
                    Self::placeholder_position(
                        owner,
                        to.id.input,
                        self.graph.nodes.get(&to_id).map_or(1, Self::input_count),
                    ),
                );
                self.set_input(to_id, to.id.input, placeholder);
                snarl.connect(
                    OutPinId {
                        node: placeholder_node,
                        output: 0,
                    },
                    to.id,
                );
                if let Some(disconnected) = disconnected {
                    self.remove_orphaned_placeholder(disconnected, snarl);
                }
                return;
            }
            _ => {}
        }
        snarl.disconnect(from.id, to.id);
        if let Some(disconnected) = disconnected {
            self.remove_orphaned_placeholder(disconnected, snarl);
        }
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
        if ui.button("Position").clicked() {
            self.insert(
                snarl,
                pos,
                Node::Position {
                    collection: Vec::new(),
                },
            );
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
            self.insert_with_placeholders(snarl, pos, 3, |inputs| Node::If {
                condition: inputs[0],
                then: inputs[1],
                else_: inputs[2],
            });
            ui.close();
        }
        if ui.button("Value map").clicked() {
            self.insert_with_placeholders(snarl, pos, 1, |inputs| Node::ValueMap {
                input: inputs[0],
                table: vec![],
                default: None,
            });
            ui.close();
        }
        if ui.button("Lookup").clicked() {
            self.insert_with_placeholders(snarl, pos, 1, |inputs| Node::Lookup {
                collection: vec![],
                key: vec![],
                matches: inputs[0],
                value: vec![],
            });
            ui.close();
        }
        ui.menu_button("Aggregate", |ui| {
            for (function, label) in Self::AGGREGATE_OPS {
                if ui.button(label).clicked() {
                    let inputs = usize::from(Self::aggregate_needs_arg(function));
                    self.insert_with_placeholders(snarl, pos, inputs, |ids| {
                        Self::aggregate_node(function, ids.first().copied())
                    });
                    ui.close();
                }
            }
        });
        if ui.button("Source field (manual path)").clicked() {
            self.insert(
                snarl,
                pos,
                Node::SourceField {
                    path: vec![],
                    frame: None,
                },
            );
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
        let references = self.references_to(mapping_id);
        let remove = ui
            .add_enabled(references.is_empty(), egui::Button::new("Remove"))
            .on_disabled_hover_text(format!("Disconnect first: {}", references.join(", ")));
        if remove.clicked() {
            self.remove_graph_node(mapping_id, node, snarl);
            ui.close();
        }
    }
}

#[cfg(test)]
#[path = "graph_viewer_tests.rs"]
mod tests;
