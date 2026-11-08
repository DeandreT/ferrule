//! Canvas node titles and headers.

use super::*;

pub(super) fn complete_node_title(node: Option<&Node>, fallback: String) -> String {
    if let Some(Node::OwnedLookup(lookup)) = node {
        format!(
            "Lookup from {}\nCollection: {:?}\nKey: {:?}\nReturn field: {:?}\nReads exactly this input; a missing match returns Null",
            crate::source_ownership::owner_label(&lookup.collection.owner),
            lookup.collection.path,
            lookup.key,
            lookup.value,
        )
    } else {
        graph_node_presentation::complete_title(node, fallback)
    }
}

pub(super) fn has_node_properties(node: &Node) -> bool {
    matches!(node, Node::OwnedLookup(_) | Node::OwnedAggregate(_))
        || graph_node_presentation::has_properties(node)
}

fn show_owned_expression_header(
    ui: &mut Ui,
    node: SnarlNodeId,
    id: NodeId,
    label: &str,
    edit_label: &str,
) -> egui::Response {
    ui.horizontal(|ui| {
        let response = ui.strong(label);
        let edit = crate::ui_kit::icon_button(ui, lucide_icons::Icon::Pencil, edit_label, None);
        focus::record_control(ui, edit.id, node);
        let popup = crate::graph_draft_ui::property_canvas(ui).with(("node_properties", id));
        if edit.clicked() || crate::canvas_tools::take_edit(ui.ctx(), CanvasNode::Graph(id)) {
            ui.ctx().data_mut(|data| data.insert_temp(popup, true));
        }
        let anchor = crate::graph_draft_ui::property_canvas(ui).with(("node_edit_anchor", id));
        ui.ctx().data_mut(|data| {
            data.insert_temp(anchor, edit);
        });
        response
    })
    .inner
}

fn compact_graph_title(label: &str) -> String {
    label.to_owned()
}

impl GraphViewer<'_> {
    pub(super) fn node_title(&mut self, node: &CanvasNode) -> String {
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
                    Some(Node::OwnedSourceField(field)) => compact_graph_title(&format!(
                        "Owned {}: {}",
                        crate::source_ownership::owner_label(&field.source.owner),
                        field.source.path.join("/")
                    )),
                    Some(Node::OwnedPosition(position)) => format!(
                        "Owned position: {}{}",
                        crate::source_ownership::owner_label(&position.owner),
                        position
                            .collection
                            .as_ref()
                            .map(|collection| format!(" {}", collection.join("/")))
                            .unwrap_or_default()
                    ),
                    Some(Node::OwnedRowDocumentPath(_)) => "Owned row document path".into(),
                    Some(Node::OwnedSourceDocumentPath(_)) => "Owned document path".into(),
                    Some(Node::OwnedLookup(lookup)) => {
                        format!(
                            "{}: Lookup {}",
                            crate::source_ownership::owner_label(&lookup.collection.owner),
                            lookup.collection.path.join("/")
                        )
                    }
                    Some(Node::OwnedCollectionFind(find)) => {
                        format!("Owned find: {}", find.collection.path.join("/"))
                    }
                    Some(Node::OwnedDynamicSourceField(_)) => "Owned dynamic field".into(),
                    Some(Node::OwnedXmlSerialize(_)) => "Owned XML serializer".into(),
                    Some(Node::OwnedAggregate(aggregate)) => format!(
                        "Owned {}: {}",
                        graph_node_presentation::aggregate_label(aggregate.function),
                        aggregate.collection.path.join("/")
                    ),
                    Some(Node::OwnedJoinField(field)) => format!(
                        "Owned join {} field: {}/{}",
                        field.join.get(),
                        field.collection.path.join("/"),
                        field.path.join("/")
                    ),
                    Some(Node::OwnedJoinSourcePosition(position)) => format!(
                        "Owned join {} position: {}",
                        position.join.get(),
                        position.collection.path.join("/")
                    ),
                    Some(Node::OwnedXmlMixedContent(mixed)) => {
                        format!("Owned XML mixed content: {}", mixed.source.path.join("/"))
                    }
                    Some(Node::OwnedJoinAggregate(aggregate)) => format!(
                        "Owned join {} {:?}",
                        aggregate.join.get(),
                        aggregate.function
                    ),
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
                        let (inputs, outputs) = graph_references::filter_map_item_ids(
                            self.graph,
                            self.root_scope,
                            self.extra_targets,
                            self.inactive_target_scopes,
                            self.project_references,
                        );
                        if inputs.contains(id) {
                            "Stage input".into()
                        } else if outputs.contains(id) {
                            "Mapped output".into()
                        } else {
                            format!("Generated item #{id}")
                        }
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
                    Some(Node::GeneratedPosition { item }) => {
                        format!("Source position: item {item}")
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
                    Some(node @ Node::RuntimeValue { .. }) => readable_node_label(node),
                    Some(Node::InvocationCounter { key, .. }) => {
                        format!("Invocation counter: {key}")
                    }
                    Some(Node::RuntimeParameter { name, ty, .. }) => {
                        let name = if name.is_empty() {
                            "<name required>"
                        } else {
                            name
                        };
                        format!(
                            "Run value: {name} ({})",
                            crate::value_editor::scalar_type_label(*ty)
                        )
                    }
                    Some(Node::RuntimeParameterDefault { name, ty, .. }) => {
                        let name = if name.is_empty() {
                            "<name required>"
                        } else {
                            name
                        };
                        format!(
                            "Run value: {name} ({}, optional)",
                            crate::value_editor::scalar_type_label(*ty)
                        )
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
                        let target = expression.map_or_else(
                            || path.join("/"),
                            |_| {
                                let collection = if collection.is_empty() {
                                    "<current>".to_owned()
                                } else {
                                    collection.join("/")
                                };
                                format!("{collection} (calculated values)")
                            },
                        );
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

    pub(super) fn show_node_header(
        &mut self,
        node: SnarlNodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        let canvas_node = snarl[node];
        // egui-snarl's collapse button reacts to the pointer only.
        let collapse_id = focus::first_widget_id(ui);
        if let Some(collapse) = ui
            .ctx()
            .read_response(collapse_id)
            .filter(|response| response.sense.senses_click())
        {
            let open = snarl.get_node_info(node).is_some_and(|info| info.open);
            let action = if open { "Collapse" } else { "Expand" };
            focus::name(&collapse, &format!("{action} {}", self.title(&canvas_node)));
            focus::record_control(ui, collapse_id, node);
            if focus::keyboard_activated(&collapse) {
                snarl.open_node(node, !open);
            }
        }
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
                            self.source_endpoint_hint(block, section),
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
        // Account for the nested node/header frame margins. Pin labels are
        // painted independently so right-to-left rows cannot grow sideways.
        let endpoint_content_width = endpoint_width
            .map(|width| (self.endpoint_scroll.width(canvas_node, width) - 32.0).max(0.0));
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
        let full_title = complete_node_title(graph_node, full_title);
        let header_hint = if matches!(
            canvas_node,
            CanvasNode::SourceBlock(_) | CanvasNode::TargetBlock(_)
        ) {
            Some(endpoint_hint.map_or_else(
                || full_title.clone(),
                |hint| format!("{full_title}\n{hint}"),
            ))
        } else {
            graph_node.map(|node| graph_node_presentation::hint(node, &full_title))
        };
        let show_title = |ui: &mut Ui| match canvas_node {
            CanvasNode::SourceBlock(_) => {
                graph_node_presentation::show_endpoint_header(ui, true, &full_title)
            }
            CanvasNode::TargetBlock(_) => {
                graph_node_presentation::show_endpoint_header(ui, false, &full_title)
            }
            CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => {
                if let Some(header) = compact_header {
                    let manual = crate::canvas_tools::manual_width(ui.ctx(), canvas_node);
                    if let Some(id) = Self::mapping_id(canvas_node)
                        && graph_node.is_some_and(has_node_properties)
                    {
                        let (anchor, popup) = crate::graph_draft_ui::node_controls(ui.ctx(), id)
                            .unwrap_or_else(|| {
                                (
                                    crate::graph_draft_ui::property_canvas(ui)
                                        .with(("node_edit_anchor", id)),
                                    crate::graph_draft_ui::property_canvas(ui)
                                        .with(("node_properties", id)),
                                )
                            });
                        let requested = crate::canvas_tools::take_edit(ui.ctx(), canvas_node);
                        let reveal = requested
                            || ui
                                .ctx()
                                .read_response(
                                    crate::graph_draft_ui::property_canvas(ui)
                                        .with(("snarl-node", node))
                                        .with("frame"),
                                )
                                .is_some_and(|response| response.contains_pointer())
                            || crate::canvas_tools::selected_node(ui.ctx(), canvas_node)
                            || crate::canvas_tools::focused_node(ui.ctx(), canvas_node)
                            || ui
                                .ctx()
                                .data(|data| data.get_temp::<bool>(popup).unwrap_or(false));
                        let header_scope =
                            crate::graph_draft_ui::node_container(ui.ctx(), id, "header");
                        let show = |ui: &mut Ui| {
                            graph_node_presentation::show_header_with_edit(
                                ui,
                                header,
                                &full_title,
                                manual,
                                reveal,
                                true,
                            )
                        };
                        let (response, edit) = if let Some(id) = header_scope {
                            ui.scope_builder(egui::UiBuilder::new().scope_id(id), show)
                                .inner
                        } else {
                            show(ui)
                        };
                        focus::record_control(ui, edit.id, node);
                        if edit.clicked() || requested {
                            ui.ctx().data_mut(|data| data.insert_temp(popup, true));
                            Self::mark_properties_open(ui.ctx());
                            ui.ctx().request_repaint();
                        }
                        ui.ctx().data_mut(|data| data.insert_temp(anchor, edit));
                        response
                    } else {
                        let width = manual.or_else(|| {
                            graph_node.map(|node| {
                                graph_node_presentation::node_width(
                                    ui,
                                    node,
                                    &full_title,
                                    is_output,
                                )
                            })
                        });
                        graph_node_presentation::show_header(ui, header, &full_title, width)
                    }
                } else if let (Some(id), Some(lookup @ Node::OwnedLookup(_))) =
                    (Self::mapping_id(canvas_node), graph_node)
                {
                    show_owned_expression_header(
                        ui,
                        node,
                        id,
                        &readable_node_label(lookup),
                        "Edit lookup",
                    )
                } else if let (Some(id), Some(aggregate @ Node::OwnedAggregate(_))) =
                    (Self::mapping_id(canvas_node), graph_node)
                {
                    show_owned_expression_header(
                        ui,
                        node,
                        id,
                        &readable_node_label(aggregate),
                        "Edit aggregate",
                    )
                } else {
                    ui.label(&full_title)
                }
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
                    egui::WidgetInfo::labeled(egui::Role::Button, true, "Open function mapping")
                });
                let open = open.on_hover_text("Open function mapping");
                if response.double_clicked() || open.clicked() {
                    self.requested_function_open = Some(function);
                }
                response
            })
            .inner
        } else if let Some(width) = endpoint_content_width {
            let total = match canvas_node {
                CanvasNode::SourceBlock(block) => self.source_blocks[block].leaves.len(),
                CanvasNode::TargetBlock(block) => self.target_blocks[block].leaves.len(),
                CanvasNode::Graph(_) | CanvasNode::Placeholder(_) => 0,
            };
            // egui-snarl has already placed its collapse button and drag space
            // on this row. Only the title row is indented past them; the
            // filter below keeps the full header width. Bounding a fresh
            // column also matters because set_max_width on the row itself
            // would move its cursor back over the collapse button.
            let left = ui.min_rect().left();
            let indent = ui.cursor().min.x - left;
            let column = egui::Rect::from_min_max(
                egui::pos2(left, ui.cursor().min.y),
                egui::pos2(left + indent + width, ui.max_rect().bottom()),
            );
            ui.scope_builder(
                egui::UiBuilder::new()
                    .max_rect(column)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
                |ui| {
                    ui.set_min_width(column.width());
                    ui.set_max_width(column.width());
                    let response = ui
                        .horizontal(|ui| {
                            ui.add_space(indent);
                            show_title(ui)
                        })
                        .inner;
                    graph_endpoint_view::show_filter(ui, canvas_node, total, self.endpoint_scroll);
                    response
                },
            )
            .inner
        } else {
            show_title(ui)
        };
        if graph_node.is_some_and(graph_node_presentation::is_inline)
            && let Some(output) = _outputs.first()
        {
            let _ = crate::canvas_tools::fanout(ui, self, output.id, snarl);
        }
        if let Some(hint) = header_hint {
            response.on_hover_text(hint);
        }
    }
}
