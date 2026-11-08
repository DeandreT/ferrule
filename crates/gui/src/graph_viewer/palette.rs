//! Insertion of nodes chosen from the canvas palette.

use super::*;

impl GraphViewer<'_> {
    pub(super) fn insert_owned_lookup(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        pos: egui::Pos2,
    ) -> Result<(NodeId, SnarlNodeId), String> {
        if self.function_output.is_some()
            || !self
                .source_paths
                .lookup_binding_supported(self.root_scope, &[])
        {
            return Err(
                "Owned lookups require an ungrouped ordinary target context outside isolated functions."
                    .into(),
            );
        }
        let lookup = self
            .source_paths
            .first_owned_lookup(0)
            .ok_or_else(|| "No supported static input collection is available.".to_owned())?;
        self.plan_edit(
            |viewer| {
                viewer.insert_with_unconnected_inputs(snarl, pos, 1, |inputs| {
                    Node::OwnedLookup(mapping::OwnedLookup {
                        matches: inputs[0],
                        ..lookup
                    })
                })
            },
            |_, inserted| {
                let &(node, _) = inserted.as_ref().ok()?;
                Some(GraphAction::InsertNode {
                    node,
                    position: editor_ui::CanvasPosition { x: pos.x, y: pos.y },
                })
            },
        )
    }

    pub(super) fn insert_owned_aggregate(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        pos: egui::Pos2,
        op: AggregateOp,
    ) -> Result<(NodeId, SnarlNodeId), String> {
        if self.function_output.is_some()
            || !self
                .source_paths
                .lookup_binding_supported(self.root_scope, &[])
        {
            return Err("Owned aggregates require an ungrouped ordinary target context outside isolated functions.".into());
        }
        let aggregate = self
            .source_paths
            .first_owned_aggregate(op)
            .ok_or_else(|| "No supported static input collection is available.".to_owned())?;
        self.plan_edit(
            |viewer| {
                viewer.insert_with_unconnected_inputs(snarl, pos, 0, |_| {
                    Node::OwnedAggregate(aggregate)
                })
            },
            |_, inserted| {
                let &(node, _) = inserted.as_ref().ok()?;
                Some(GraphAction::InsertNode {
                    node,
                    position: editor_ui::CanvasPosition { x: pos.x, y: pos.y },
                })
            },
        )
    }

    pub(super) fn insert_palette_node(
        &mut self,
        snarl: &mut Snarl<CanvasNode>,
        pos: egui::Pos2,
        template: NodeTemplate,
    ) -> Result<(NodeId, SnarlNodeId), String> {
        match template {
            NodeTemplate::FilterMapItemAt
            | NodeTemplate::FilterMapExists
            | NodeTemplate::FilterMapSum => {
                if self.function_output.is_some() {
                    return Err("Filter/map authoring is unavailable in isolated functions.".into());
                }
                let functions = self
                    .project_references
                    .user_functions()
                    .ok_or_else(|| "Filter/map needs a project function library.".to_owned())?;
                let ids = self.reserve_node_ids(if template == NodeTemplate::FilterMapSum {
                    5
                } else {
                    6
                })?;
                let (sequence, nodes) = crate::filter_map_editor::create(&ids[..4], functions)?;
                let node = match template {
                    NodeTemplate::FilterMapExists => Node::SequenceExists {
                        sequence,
                        predicate: ids[4],
                    },
                    NodeTemplate::FilterMapSum => {
                        if !matches!(&sequence, mapping::SequenceExpr::FilterMapV1(value)
                            if matches!(value.output_type, ScalarType::Int | ScalarType::Float))
                        {
                            return Err(
                                "Filter/map sum needs an integer or number map function.".into()
                            );
                        }
                        Node::SequenceAggregate {
                            sequence,
                            function: AggregateOp::Sum,
                            predicate: None,
                            expression: Some(ids[3]),
                            arg: None,
                        }
                    }
                    _ => Node::SequenceItemAt {
                        sequence,
                        index: ids[4],
                    },
                };
                self.graph.nodes.extend(nodes);
                if template != NodeTemplate::FilterMapSum {
                    self.graph.nodes.insert(
                        ids[4],
                        Node::Const {
                            value: if template == NodeTemplate::FilterMapExists {
                                Value::Bool(true)
                            } else {
                                Value::Int(1)
                            },
                        },
                    );
                }
                let id = *ids
                    .last()
                    .ok_or_else(|| "mapping node IDs are exhausted".to_owned())?;
                let inputs = node_inputs(&node);
                self.graph.nodes.insert(id, node);
                let mut visible = std::collections::BTreeMap::new();
                for (index, &node) in ids.iter().enumerate() {
                    let offset = if node == id {
                        egui::Vec2::ZERO
                    } else {
                        egui::vec2(-220.0, index as f32 * 60.0)
                    };
                    visible.insert(
                        node,
                        snarl.insert_node(pos + offset, CanvasNode::Graph(node)),
                    );
                }
                for (input, source) in inputs.into_iter().enumerate() {
                    snarl.connect(
                        OutPinId {
                            node: visible[&source],
                            output: 0,
                        },
                        InPinId {
                            node: visible[&id],
                            input,
                        },
                    );
                }
                Ok((id, visible[&id]))
            }
            NodeTemplate::Constant => self.insert(snarl, pos, Node::Const { value: Value::Null }),
            NodeTemplate::SourceField => self.insert(
                snarl,
                pos,
                Node::SourceField {
                    path: Vec::new(),
                    frame: None,
                },
            ),
            NodeTemplate::SourceDocumentPath => {
                if self.function_output.is_some()
                    || !self.source_paths.primary_source_document_path_available()
                {
                    return Err("Source document paths require a primary local XML file set outside isolated functions.".into());
                }
                self.insert(snarl, pos, Node::SourceDocumentPath)
            }
            NodeTemplate::XmlSerialize => {
                if self.function_output.is_some() {
                    return Err(
                        "Source XML serialization is unavailable in isolated functions.".into(),
                    );
                }
                let (path, schema) =
                    self.source_paths
                        .first_xml_source_element()
                        .ok_or_else(|| {
                            "No supported primary source element is available.".to_owned()
                        })?;
                self.insert(
                    snarl,
                    pos,
                    Node::XmlSerialize {
                        path,
                        frame: None,
                        schema,
                        declaration: false,
                        indent: true,
                        namespace: None,
                    },
                )
            }
            NodeTemplate::DynamicSourceField => {
                if self.function_output.is_some() {
                    return Err("Source properties are unavailable in isolated functions.".into());
                }
                let object = self
                    .source_paths
                    .first_open_scalar_object()
                    .ok_or_else(|| {
                        "No supported open scalar source object is available.".to_owned()
                    })?;
                self.insert_with_unconnected_inputs(snarl, pos, 1, |inputs| {
                    Node::DynamicSourceField {
                        object,
                        frame: None,
                        key: inputs[0],
                    }
                })
            }
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
            NodeTemplate::Position => {
                let node = match self.source_paths.ownership().palette_position() {
                    Some(position) if self.function_output.is_none() => {
                        Node::OwnedPosition(position)
                    }
                    _ => Node::Position {
                        collection: Vec::new(),
                    },
                };
                self.insert(snarl, pos, node)
            }
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
            NodeTemplate::RuntimeValue(value) => {
                self.insert(snarl, pos, Node::RuntimeValue { value })
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

    pub(super) fn insert(
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

    pub(super) fn insert_with_unconnected_inputs(
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
}
