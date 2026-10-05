//! Node property editors shown either in place or in a compact node's popover.

use super::*;

impl GraphViewer<'_> {
    pub(super) fn show_node_properties(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        snarl: &mut Snarl<CanvasNode>,
    ) {
        let Some(node_id) = Self::mapping_id(snarl[pin.id.node]) else {
            return;
        };
        let sequence_owners = if self.function_output.is_none()
            && matches!(
                self.graph.nodes.get(&node_id),
                Some(Node::SourceField { .. })
            ) {
            graph_references::sequence_item_owners(
                self.graph,
                self.root_scope,
                self.extra_targets,
                self.inactive_target_scopes,
                self.project_references,
                node_id,
            )
        } else {
            Vec::new()
        };
        let mut new_call_arg_needed = false;
        let mut call_function_changed = false;
        let mut remove_call_wire = None;
        let mut remove_aggregate_wire = None;
        let mut staged_node = self
            .graph
            .nodes
            .get(&node_id)
            .filter(|node| matches!(node, Node::Call { .. } | Node::Aggregate { .. }))
            .cloned();
        let node = if let Some(node) = staged_node.as_mut() {
            Some(node)
        } else {
            self.graph.nodes.get_mut(&node_id)
        };
        if let Some(node) = node {
            match node {
                Node::SourceField { path, frame } if !sequence_owners.is_empty() => {
                    graph_sequence_ownership::show(ui, &sequence_owners, path, frame.as_deref());
                }
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
                Node::SourceRootXmlTypeEquals {
                    canonical_expanded_type,
                } => {
                    ui.label("Primary source XML type");
                    ui.add_enabled_ui(self.primary_root_authoring, |ui| {
                        self.source_paths
                            .show_primary_root_type_picker(ui, canonical_expanded_type);
                    });
                }
                Node::SourceRootField { path, required } => {
                    ui.label("Primary source field");
                    ui.add_enabled_ui(self.primary_root_authoring, |ui| {
                        self.source_paths.show_primary_root_field_picker(ui, path);
                        ui.checkbox(required, "Require a value when read");
                    });
                    if *required {
                        ui.small("A missing value stops execution when this field is read.");
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
                Node::RuntimeParameter { name, ty, preview }
                | Node::RuntimeParameterDefault {
                    name, ty, preview, ..
                } => {
                    ui.horizontal(|ui| {
                        ui.label("name");
                        ui.add(
                            egui::TextEdit::singleline(name)
                                .hint_text("Name required")
                                .char_limit(mapping::MAX_RUNTIME_PARAMETER_NAME_BYTES),
                        );
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
                    let mut enabled = preview.is_some();
                    if ui.checkbox(&mut enabled, "Use preview value").changed() {
                        *preview = enabled.then(String::new);
                    }
                    if let Some(value) = preview {
                        ui.add(
                            egui::TextEdit::singleline(value)
                                .hint_text("Preview value")
                                .char_limit(engine::MAX_RUNTIME_PARAMETER_STRING_BYTES),
                        );
                        ui.weak("Used only in preview when no run value is supplied.");
                    }
                }
                Node::Call { function, args } => {
                    let previous_function = function.clone();
                    let selected = functions::builtin(function).map_or_else(
                        || function.clone(),
                        |builtin| builtin.display_name.to_owned(),
                    );
                    egui::ComboBox::from_id_salt(ui.id().with("builtin"))
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for builtin in functions::builtin_catalog().iter().filter(|builtin| {
                                builtin.exposure == functions::BuiltinExposure::Authoring
                            }) {
                                ui.selectable_value(
                                    function,
                                    builtin.native_name.to_owned(),
                                    builtin.display_name,
                                )
                                .on_hover_text(builtin.documentation);
                            }
                        });
                    call_function_changed = *function != previous_function;
                    if let Some(builtin) = functions::builtin(function) {
                        ui.weak(format!(
                            "{} input{} minimum",
                            builtin.arity.minimum(),
                            if builtin.arity.minimum() == 1 {
                                ""
                            } else {
                                "s"
                            }
                        ))
                        .on_hover_text(builtin.documentation);
                    }
                    let effective_count =
                        args.len() + call_missing_minimum_inputs(function, args.len());
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                call_can_add_argument(function, effective_count),
                                egui::Button::new("+arg").small(),
                            )
                            .clicked()
                        {
                            new_call_arg_needed = true;
                        }
                        if ui
                            .add_enabled(
                                call_can_remove_argument(function, effective_count),
                                egui::Button::new("-arg").small(),
                            )
                            .clicked()
                        {
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
                Node::ValueMap { .. } => {
                    ui.label("mapped value");
                }
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
                Node::SequenceAggregate {
                    function,
                    sequence,
                    predicate,
                    expression,
                    ..
                } => {
                    let op = format!("{function:?}").to_lowercase();
                    ui.label(format!(
                        "{op} {} {}",
                        graph_sequence::label(sequence),
                        match (predicate.is_some(), expression.is_some()) {
                            (true, true) => "filtered computed values",
                            (true, false) => "filtered items",
                            (false, true) => "computed values",
                            (false, false) => "items",
                        },
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
                    if previous != *function && !node_palette::aggregate_needs_arg(*function) {
                        remove_aggregate_wire = arg.take();
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
        let edit_committed = if let Some(node) = staged_node {
            match self.commit_node_property_edit(
                node_id,
                node,
                call_function_changed,
                new_call_arg_needed,
            ) {
                Ok(_) => true,
                Err(error) => {
                    self.error = Some(error);
                    false
                }
            }
        } else {
            true
        };
        if edit_committed && let Some((input_index, removed)) = remove_call_wire {
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
        if edit_committed && let Some(removed) = remove_aggregate_wire {
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
    }
}
