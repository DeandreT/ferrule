//! Canvas pin rendering, labels, and input access.

use super::*;

/// Where an endpoint label's tooltip is hit-tested.
#[derive(Clone, Copy)]
enum LabelHover {
    /// The label has its own Ui, so it registers its own hover target.
    Row,
    /// The label is drawn in the pin row, whose next auto ID is the pin widget
    /// egui-snarl creates afterwards. A second, unfocusable widget under that
    /// ID would clear the pin's keyboard focus every frame, so the tooltip
    /// follows the pin's response from the preceding pass instead.
    Pin,
}

fn show_endpoint_label(
    ui: &mut Ui,
    path: &str,
    align: egui::Align,
    hover_text: impl Into<egui::WidgetText>,
    highlighted: bool,
    hover: LabelHover,
) -> egui::Rect {
    let clip_rect = ui.clip_rect();
    let row_height = ui
        .spacing()
        .interact_size
        .y
        .max(ui.text_style_height(&egui::TextStyle::Body));
    let row_rect = egui::Rect::from_min_max(
        egui::pos2(clip_rect.left(), ui.max_rect().top()),
        egui::pos2(clip_rect.right(), ui.max_rect().top() + row_height),
    );
    // Reserve the same measured vertical space that we paint and hit-test.
    // Do not allocate the full horizontal clip width, which would grow nodes.
    ui.expand_to_include_y(row_rect.bottom());
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
    if highlighted {
        ui.painter().rect_filled(
            row_rect.shrink2(egui::vec2(row_height, 1.0)),
            2.0,
            ui.visuals().selection.bg_fill,
        );
    }
    let mut job = egui::text::LayoutJob::simple(
        path.chars().take(4096).collect(),
        egui::TextStyle::Body.resolve(ui.style()),
        ui.visuals().text_color(),
        (row_rect.width() - row_height * 2.0).max(0.0),
    );
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    let galley = ui.fonts_mut(|fonts| fonts.layout_job(job));
    let painted = text_align.anchor_size(anchor, galley.size());
    ui.painter()
        .galley(painted.min, galley, ui.visuals().text_color());
    match hover {
        LabelHover::Row => {
            ui.interact(row_rect, ui.next_auto_id(), egui::Sense::hover())
                .on_hover_text(hover_text);
        }
        LabelHover::Pin => {
            if let Some(response) = ui.ctx().read_response(ui.next_auto_id()) {
                response.on_hover_text(hover_text);
            }
        }
    }
    row_rect
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
    let label = format!("{hidden} hidden");
    let side = if source { "source" } else { "target" };
    let connection = if connected {
        "Connected offscreen fields are routed through this edge pin. "
    } else {
        ""
    };
    let row = show_endpoint_label(
        ui,
        &label,
        align,
        format!("{connection}Scroll to show {hidden} hidden {side} fields {direction}"),
        false,
        LabelHover::Pin,
    );
    let icon = egui::WidgetText::from(crate::icons::text(
        if above {
            lucide_icons::Icon::ChevronUp
        } else {
            lucide_icons::Icon::ChevronDown
        },
        crate::ui_kit::METRICS.icon_small,
    ))
    .into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    // Endpoint output rows run right-to-left. Paint in the reserved inner
    // margin instead of allocating an icon beside (and under) the edge pin.
    let x = if align == egui::Align::Max {
        row.left() + 2.0
    } else {
        row.right() - icon.size().x - 2.0
    };
    ui.painter().galley(
        egui::pos2(x, row.center().y - icon.size().y * 0.5),
        icon,
        ui.visuals().text_color(),
    );
}

pub(super) fn input_wire_emphasis(hovered_node: Option<SnarlNodeId>, pin: &InPin) -> WireEmphasis {
    let Some(hovered_node) = hovered_node else {
        return WireEmphasis::Normal;
    };
    if pin.id.node == hovered_node || pin.remotes.iter().any(|remote| remote.node == hovered_node) {
        WireEmphasis::Incident
    } else {
        WireEmphasis::Unrelated
    }
}

pub(super) fn output_wire_emphasis(
    hovered_node: Option<SnarlNodeId>,
    pin: &OutPin,
) -> WireEmphasis {
    let Some(hovered_node) = hovered_node else {
        return WireEmphasis::Normal;
    };
    if pin.id.node == hovered_node || pin.remotes.iter().any(|remote| remote.node == hovered_node) {
        WireEmphasis::Incident
    } else {
        WireEmphasis::Unrelated
    }
}

impl GraphViewer<'_> {
    pub(super) fn input_wire_color(
        &self,
        pin: &InPin,
        node: CanvasNode,
        input: usize,
    ) -> egui::Color32 {
        let base = crate::wire_colors::input_color(self.wire_color_mode, self.colors, node, input);
        crate::wire_colors::with_emphasis(
            base,
            self.colors.canvas.to_egui(),
            input_wire_emphasis(self.hovered_node, pin),
        )
    }

    pub(super) fn output_wire_color(&self, pin: &OutPin) -> egui::Color32 {
        let base = crate::wire_colors::output_color(self.wire_color_mode, self.colors);
        crate::wire_colors::with_emphasis(
            base,
            self.colors.canvas.to_egui(),
            output_wire_emphasis(self.hovered_node, pin),
        )
    }

    /// Records a pin widget; `field` is the schema field or graph input the
    /// pin stands for, if it has one.
    pub(super) fn record_pin_interaction_id(
        &mut self,
        ui: &Ui,
        pin: CanvasPin,
        node: CanvasNode,
        field: Option<String>,
    ) {
        let direction = match pin {
            CanvasPin::Input(_) => "Input",
            CanvasPin::Output(_) => "Output",
        };
        let title = self.title(&node);
        let name = match field.filter(|field| !field.is_empty()) {
            Some(field) => format!("{direction} {field} · {title}"),
            None => format!("{direction} · {title}"),
        };
        // egui-snarl 0.11 creates the pin's drag widget with this exact next
        // auto ID immediately after `show_input`/`show_output` returns.
        let id = ui.next_auto_id();
        self.pin_interaction_ids.push(id);
        focus::record_pin(ui, focus::PinWidget { id, pin, name });
    }

    /// The schema field or proxy row an endpoint pin stands for.
    pub(super) fn endpoint_pin_field(&self, node: CanvasNode, pin: usize) -> Option<String> {
        let labels = match node {
            CanvasNode::SourceBlock(block) => self
                .source_blocks
                .get(block)?
                .leaves
                .iter()
                .map(|leaf| leaf.label.as_str())
                .collect::<Vec<_>>(),
            CanvasNode::TargetBlock(block) => self
                .target_blocks
                .get(block)?
                .leaves
                .iter()
                .map(|leaf| leaf.label.as_str())
                .collect(),
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => return None,
        };
        match self.endpoint_display_pin(node, pin)? {
            EndpointDisplayPin::Visible(semantic) => {
                labels.get(semantic).map(|label| (*label).to_owned())
            }
            EndpointDisplayPin::HiddenBefore => Some(format!(
                "{} hidden fields above",
                self.endpoint_scroll.hidden_before(node, labels.len())
            )),
            EndpointDisplayPin::HiddenAfter => Some(format!(
                "{} hidden fields below",
                self.endpoint_scroll.hidden_after(node, labels.len())
            )),
        }
    }

    pub(super) fn source_leaf(&self, block: usize, pin: usize) -> Option<&SourceLeaf> {
        let section = self.source_blocks.get(block)?;
        let semantic = self.endpoint_scroll.semantic_pin(
            CanvasNode::SourceBlock(block),
            pin,
            section.leaves.len(),
        )?;
        section.leaves.get(semantic)
    }

    pub(super) fn target_leaf(&self, block: usize, pin: usize) -> Option<&TargetLeaf> {
        let section = self.target_blocks.get(block)?;
        let semantic = self.endpoint_scroll.semantic_pin(
            CanvasNode::TargetBlock(block),
            pin,
            section.leaves.len(),
        )?;
        section.leaves.get(semantic)
    }

    pub(super) fn endpoint_display_pin(
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

    pub(super) fn filter_map_inputs_writable(&self, node: &Node) -> bool {
        self.function_output.is_none()
            && self.project_references.user_functions().is_some()
            && crate::filter_map_editor::sequence(node)
                .is_some_and(crate::filter_map_editor::editable)
    }

    pub(super) fn read_only_inputs(&self, id: NodeId, node: &Node) -> bool {
        // The authored Count/direct Sum subset has no inputs. Every retained
        // aggregate input therefore belongs to a read-only expression or arg.
        if matches!(node, Node::OwnedAggregate(_)) {
            return true;
        }
        if let Node::OwnedLookup(lookup) = node {
            return self.function_output.is_some()
                || !self.source_paths.lookup_editable(self.graph, id)
                || !self
                    .source_paths
                    .lookup_owner_available(&lookup.collection.owner);
        }
        graph_sequence::inputs_are_read_only(node) && !self.filter_map_inputs_writable(node)
    }

    pub(super) fn read_only_input_message(&self, id: NodeId) -> &'static str {
        if matches!(self.graph.nodes.get(&id), Some(Node::OwnedAggregate(_))) {
            "Saved owned aggregate expression and argument inputs are read-only."
        } else if matches!(self.graph.nodes.get(&id), Some(Node::OwnedLookup(_))) {
            "This owned Lookup is read-only in its current source or evaluation context."
        } else {
            graph_sequence::READ_ONLY_INPUTS_MESSAGE
        }
    }

    pub(super) fn check_filter_map_connection(
        &self,
        from: NodeId,
        to: Option<(NodeId, usize)>,
    ) -> Result<(), String> {
        let (inputs, _) = graph_references::filter_map_item_ids(
            self.graph,
            self.root_scope,
            self.extra_targets,
            self.inactive_target_scopes,
            self.project_references,
        );
        let mut pending = vec![from];
        let mut seen = std::collections::BTreeSet::new();
        while let Some(id) = pending.pop() {
            if inputs.contains(&id) {
                return Err("Stage input values are private. Connect the mapped output in its owning context instead.".into());
            }
            if seen.insert(id)
                && let Some(node) = self.graph.nodes.get(&id)
            {
                // Owned item identities belong to the reducer, not its parent inputs.
                // Actual input edges still reach and refuse raw stage inputs above.
                pending.extend(node_inputs(node));
            }
        }
        if let Some((id, index)) = to
            && let Some(node) = self.graph.nodes.get(&id)
            && let Some(error) = crate::filter_map_editor::input_error(
                node,
                id,
                index,
                from,
                self.graph,
                &self.owned_item_ids(),
            )
        {
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn check_lookup_connection(
        &self,
        from: &OutPin,
        to: &InPin,
        snarl: &Snarl<CanvasNode>,
    ) -> Result<(), String> {
        let Some(from_id) = Self::mapping_id(snarl[from.id.node]) else {
            return Ok(());
        };
        let lookups = crate::source_ownership::lookup_dependencies(self.graph, from_id);
        if lookups.is_empty() {
            return Ok(());
        }
        let refused = || {
            "Owned Lookup and aggregate values require an ungrouped ordinary receiving context; this connection would change their source ownership context.".to_owned()
        };
        if self.function_output.is_some()
            || !crate::source_ownership::lookup_assignment_allowed(
                self.graph,
                from_id,
                true,
                |id| self.source_paths.lookup_editable(self.graph, id),
            )
        {
            return Err(refused());
        }
        match snarl[to.id.node] {
            CanvasNode::TargetBlock(block) => {
                let leaf = self.target_leaf(block, to.id.input).ok_or_else(refused)?;
                if !self
                    .source_paths
                    .lookup_binding_supported(self.root_scope, &leaf.chain)
                {
                    return Err(refused());
                }
            }
            CanvasNode::Graph(id) | CanvasNode::Placeholder(id) => {
                let node = self.graph.nodes.get(&id).ok_or_else(refused)?;
                let mut inputs = Vec::new();
                node.visit_inputs(&mut |input| {
                    if !matches!(
                        input.role,
                        mapping::NodeInputRole::ItemIdentity
                            | mapping::NodeInputRole::ItemReference
                    ) {
                        inputs.push(input);
                    }
                });
                let input = inputs.get(to.id.input).copied().ok_or_else(refused)?;
                if !crate::source_ownership::lookup_input_supported(node, input)
                    || !self.source_paths.lookup_editable(self.graph, id)
                {
                    return Err(refused());
                }
            }
            CanvasNode::SourceBlock(_) => return Err(refused()),
        }
        Ok(())
    }

    pub(super) fn set_input(&mut self, node_id: NodeId, idx: usize, from_id: NodeId) -> bool {
        let Some(node) = self.graph.nodes.get(&node_id) else {
            return false;
        };
        if idx >= Self::input_count(node) || self.read_only_inputs(node_id, node) {
            return false;
        }
        if let Err(error) = self.check_filter_map_connection(from_id, Some((node_id, idx))) {
            self.error = Some(error);
            return false;
        }
        let Some(node) = self.graph.nodes.get_mut(&node_id) else {
            return false;
        };
        if editor_ui::set_scalar_input(node, idx, from_id) {
            return true;
        }
        match node {
            Node::OwnedLookup(lookup) => lookup.matches = from_id,
            Node::DynamicSourceField { key, .. } => *key = from_id,
            Node::XmlMixedContent { replacements, .. } => {
                let Some(replacement) = replacements.get_mut(idx) else {
                    return false;
                };
                replacement.expression = from_id;
            }
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
            Node::JoinAggregate {
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

    pub(super) fn input_count(node: &Node) -> usize {
        match node {
            Node::OwnedSourceField(_)
            | Node::OwnedPosition(_)
            | Node::OwnedRowDocumentPath(_)
            | Node::OwnedSourceDocumentPath(_)
            | Node::OwnedXmlSerialize(_)
            | Node::OwnedJoinField(_)
            | Node::OwnedJoinSourcePosition(_)
            | Node::SourceField { .. }
            | Node::SourceDocumentPath
            | Node::SourceRootXmlTypeEquals { .. }
            | Node::SourceRootField { .. }
            | Node::Position { .. }
            | Node::GeneratedPosition { .. }
            | Node::JoinField { .. }
            | Node::JoinPosition { .. }
            | Node::Unconnected
            | Node::Const { .. }
            | Node::FunctionParameter { .. }
            | Node::RuntimeValue { .. }
            | Node::RuntimeParameter { .. }
            | Node::XmlSerialize { .. } => 0,
            Node::RuntimeParameterDefault { .. } | Node::Raise { .. } => 1,
            Node::InvocationCounter { .. } => 3,
            Node::Call { args, .. } | Node::UserFunctionCall { args, .. } => args.len(),
            Node::If { .. } => 3,
            Node::ValueMap { .. }
            | Node::Lookup { .. }
            | Node::DynamicSourceField { .. }
            | Node::OwnedDynamicSourceField(_) => 1,
            Node::XmlMixedContent { replacements, .. } => replacements.len(),
            Node::OwnedXmlMixedContent(mixed) => mixed.replacements.len(),
            Node::CollectionFind { .. } | Node::OwnedCollectionFind(_) => 2,
            Node::OwnedLookup(_) => 1,
            Node::OwnedAggregate(aggregate) => aggregate.inputs().len(),
            Node::OwnedJoinAggregate(aggregate) => aggregate.inputs().len(),
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
            } => usize::from(expression.is_some()) + usize::from(arg.is_some()),
            // A computed key's inputs are the first, read-only pins.
            Node::JoinAggregate {
                plan,
                expression,
                arg,
                ..
            } => {
                plan.computed_source()
                    .map_or(0, |computed| computed.key_inputs().len())
                    + usize::from(expression.is_some())
                    + usize::from(arg.is_some())
            }
        }
    }

    pub(super) fn input_pin_count(&mut self, node: &CanvasNode) -> usize {
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

    pub(super) fn output_pin_count(&mut self, node: &CanvasNode) -> usize {
        match node {
            CanvasNode::SourceBlock(block) => self.source_blocks.get(*block).map_or(0, |section| {
                self.endpoint_scroll
                    .display_pin_count(*node, section.leaves.len())
            }),
            CanvasNode::TargetBlock(_) => 0,
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => 1,
        }
    }

    pub(super) fn show_input_pin(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) -> PinInfo {
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
                    Some(Node::InvocationCounter { .. }) => {
                        ["start", "step", "restart (optional)"][idx].to_string()
                    }
                    Some(Node::ValueMap { .. }) => "input".to_string(),
                    Some(Node::Lookup { .. } | Node::OwnedLookup(_)) => "match/key".to_string(),
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
                    Some(Node::JoinAggregate {
                        plan,
                        expression,
                        arg,
                        ..
                    }) => plan
                        .computed_source()
                        .map(|computed| computed.key_inputs())
                        .unwrap_or_default()
                        .into_iter()
                        .map(|_| "key")
                        .chain(expression.as_ref().map(|_| "values"))
                        .chain(arg.as_ref().map(|_| "arg"))
                        .nth(idx)
                        .unwrap_or_default()
                        .to_string(),
                    Some(Node::Aggregate { expression, .. })
                        if expression.is_some() && idx == 0 =>
                    {
                        "values".to_string()
                    }
                    Some(Node::Aggregate { .. }) => "arg".to_string(),
                    Some(Node::OwnedAggregate(aggregate))
                        if aggregate.expression().is_some() && idx == 0 =>
                    {
                        "values".to_string()
                    }
                    Some(Node::OwnedAggregate(_)) => "arg".to_string(),
                    Some(Node::OwnedJoinAggregate(aggregate))
                        if aggregate.expression.is_some() && idx == 0 =>
                    {
                        "values".to_string()
                    }
                    Some(Node::OwnedJoinAggregate(_)) => "arg".to_string(),
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
                            let row_rect = show_endpoint_label(
                                ui,
                                label,
                                egui::Align::Min,
                                hover,
                                self.endpoint_search_match == Some((canvas_node, semantic_idx)),
                                LabelHover::Pin,
                            );
                            self.endpoint_scroll.record_field_row(canvas_node, row_rect);
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
        } else if let Some(label) = &label {
            let node = Self::mapping_id(canvas_node).and_then(|id| self.graph.nodes.get(&id));
            if !node.is_some_and(graph_node_presentation::is_inline) {
                graph_node_presentation::show_input(ui, label, node, idx);
            }
        }
        // Snarl creates this exact native pin widget immediately after this
        // callback. egui can read its current hit response before registration
        // using the preceding pass, without a second hit target or auto ID.
        let graph_node = Self::mapping_id(canvas_node).and_then(|id| self.graph.nodes.get(&id));
        if matches!(
            graph_node,
            Some(Node::XmlMixedContent { .. } | Node::OwnedXmlMixedContent(_))
        ) && let Some(response) = ui.ctx().read_response(ui.next_auto_id())
            && response.contains_pointer()
            && let Some(hint) = graph_node_presentation::input_pin_hint(graph_node, idx)
        {
            response.on_hover_text(hint);
        }
        let field = match canvas_node {
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => label,
            CanvasNode::SourceBlock(_) | CanvasNode::TargetBlock(_) => {
                self.endpoint_pin_field(canvas_node, idx)
            }
        };
        self.record_pin_interaction_id(ui, CanvasPin::Input(pin.id), canvas_node, field);
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

    pub(super) fn show_output_pin(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) -> PinInfo {
        let canvas_node = snarl[pin.id.node];
        let fill = match canvas_node {
            CanvasNode::SourceBlock(_) => self.colors.source,
            CanvasNode::TargetBlock(_) => self.colors.target,
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => self.colors.transform,
        };
        let Some(node_id) = Self::mapping_id(canvas_node) else {
            // Output rows use a right-to-left layout. Reserve the measured badge
            // first so the independently painted field/proxy label cannot sit
            // underneath it. Restore the complete clip before drawing the pin.
            let row_clip = ui.clip_rect();
            if let Some(badge) = crate::canvas_tools::fanout(ui, self, pin.id, snarl) {
                let mut label_clip = row_clip;
                label_clip.max.x = (badge.left() - ui.spacing().item_spacing.x)
                    .max(row_clip.left())
                    .min(row_clip.right());
                ui.set_clip_rect(label_clip);
            }
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
                            let source_label = block
                                .checked_sub(1)
                                .and_then(|index| self.project_references.named_source(index))
                                .map_or_else(
                                    || leaf.label.clone(),
                                    |source| format!("{}/{}", source.name, leaf.label),
                                );
                            let context = leaf.frame.as_ref().map_or_else(
                                || format!("Source: {}\nType: {}", source_label, leaf.ty.label()),
                                |frame| {
                                    let frame = if frame.is_empty() {
                                        "document rows".to_string()
                                    } else {
                                        frame.join("/")
                                    };
                                    format!(
                                        "Source: {}\nType: {}\nRepeating context: {frame}",
                                        source_label,
                                        leaf.ty.label()
                                    )
                                },
                            );
                            let hover = crate::x12_tooltips::append_segment_for_path(
                                context,
                                self.source_block_x12(block),
                                &leaf.label,
                            );
                            let unavailable = self.source_block_connection_error(block);
                            let (label, hover) = if let Some(error) = &unavailable {
                                (
                                    format!("{label} (unavailable)"),
                                    format!("{hover}\n{error}"),
                                )
                            } else {
                                (label.clone(), hover)
                            };
                            ui.add_enabled_ui(unavailable.is_none(), |ui| {
                                let row_rect = show_endpoint_label(
                                    ui,
                                    &label,
                                    egui::Align::Max,
                                    hover,
                                    self.endpoint_search_match
                                        == Some((CanvasNode::SourceBlock(block), semantic)),
                                    LabelHover::Row,
                                );
                                self.endpoint_scroll.record_field_row(canvas_node, row_rect);
                            });
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
            ui.set_clip_rect(row_clip);
            let field = self.endpoint_pin_field(canvas_node, pin.id.output);
            self.record_pin_interaction_id(ui, CanvasPin::Output(pin.id), canvas_node, field);
            let pin_info = if matches!(
                self.endpoint_display_pin(canvas_node, pin.id.output),
                Some(EndpointDisplayPin::HiddenBefore | EndpointDisplayPin::HiddenAfter)
            ) {
                PinInfo::square()
            } else {
                PinInfo::circle()
            };
            let unavailable = match canvas_node {
                CanvasNode::SourceBlock(block) => {
                    self.source_block_connection_error(block).is_some()
                }
                _ => false,
            };
            return pin_info
                .with_fill(if unavailable {
                    ui.visuals().weak_text_color()
                } else {
                    fill.to_egui()
                })
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
        let full_title = complete_node_title(self.graph.nodes.get(&node_id), full_title);
        if self
            .graph
            .nodes
            .get(&node_id)
            .is_some_and(has_node_properties)
        {
            let (anchor, popup_id) = crate::graph_draft_ui::node_controls(ui.ctx(), node_id)
                .unwrap_or_else(|| {
                    (
                        crate::graph_draft_ui::property_canvas(ui)
                            .with(("node_edit_anchor", node_id)),
                        crate::graph_draft_ui::property_canvas(ui)
                            .with(("node_properties", node_id)),
                    )
                });
            let edit = ui
                .ctx()
                .data(|data| data.get_temp::<egui::Response>(anchor))
                .unwrap_or_else(|| ui.allocate_response(egui::Vec2::ZERO, egui::Sense::hover()));
            // Keep the editor open independently of the single shared popup
            // slot used by its nested type and function selectors.
            let mut open = ui
                .ctx()
                .data(|data| data.get_temp::<bool>(popup_id))
                .unwrap_or_default();
            if open {
                Self::mark_properties_open(ui.ctx());
                // Handle dismissal before a focused TextEdit consumes Escape.
                // A nested selector or a modal dialog gets the first Escape
                // when it is open.
                if !egui::Popup::is_any_open(ui.ctx())
                    && !crate::ui_kit::modal_open(ui.ctx())
                    && ui.input_mut(|input| {
                        input.consume_key(egui::Modifiers::NONE, egui::Key::Escape)
                    })
                {
                    open = false;
                }
            }
            // A nested selector handles its own click, even when selection
            // closes it during this frame. Keep that click inside the editor.
            let close_behavior = if egui::Popup::is_any_open(ui.ctx()) {
                egui::PopupCloseBehavior::IgnoreClicks
            } else {
                egui::PopupCloseBehavior::CloseOnClickOutside
            };
            let properties_width = match self.graph.nodes.get(&node_id) {
                Some(Node::Const { .. }) => crate::ui_kit::DialogWidth::Narrow.px(),
                Some(Node::ValueMap { table, .. }) => {
                    crate::value_editor::value_map_editor_width(table.len()) + 22.0
                }
                _ => PATH_EDITOR_WIDTH,
            };
            // Snarl draws pins before headers, so this anchor can come from the
            // previous pass. A newly locked canvas must disable its editor now.
            let properties_enabled = ui.is_enabled() && edit.enabled();
            let mut done = false;
            egui::Popup::from_response(&edit)
                .id(popup_id)
                .open_bool(&mut open)
                .width(properties_width)
                .layout(egui::Layout::top_down(egui::Align::Min))
                .close_behavior(close_behavior)
                .show(|ui| {
                    ui.set_style(ui.ctx().global_style());
                    ui.set_max_width(properties_width);
                    ui.horizontal(|ui| {
                        ui.strong(
                            if matches!(self.graph.nodes.get(&node_id), Some(Node::Const { .. })) {
                                "Edit constant"
                            } else {
                                "Node settings"
                            },
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            done |= crate::ui_kit::icon_button(
                                ui,
                                lucide_icons::Icon::X,
                                "Close node settings",
                                None,
                            )
                            .clicked();
                        });
                    });
                    ui.separator();
                    if !matches!(self.graph.nodes.get(&node_id), Some(Node::Const { .. })) {
                        ui.add(egui::Label::new(&full_title).wrap());
                    }
                    ui.add_enabled_ui(properties_enabled, |ui| {
                        self.show_node_properties(pin, ui, snarl);
                    });
                    ui.separator();
                    done |= crate::ui_kit::dialog_buttons(ui, "Done".into(), None, None).is_some();
                });
            open &= !done;
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
        if !self
            .graph
            .nodes
            .get(&node_id)
            .is_some_and(graph_node_presentation::is_inline)
        {
            let _ = crate::canvas_tools::fanout(ui, self, pin.id, snarl);
        }
        self.record_pin_interaction_id(ui, CanvasPin::Output(pin.id), canvas_node, None);
        PinInfo::circle()
            .with_fill(fill.to_egui())
            .with_wire_color(self.output_wire_color(pin))
    }
}
