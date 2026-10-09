use super::*;
use crate::canvas::{SourceBlock, TargetBlock, source_blocks, target_blocks};
use egui_snarl::ui::SnarlWidget;
use egui_snarl::{InPinId, OutPinId};
use ir::{ScalarType, SchemaNode};
use mapping::NamedSource;

struct Fixture {
    graph: Graph,
    root_scope: Scope,
    source_blocks: Vec<SourceBlock>,
    target_blocks: Vec<TargetBlock>,
    source_paths: SourcePathCatalog,
    endpoint_scroll: crate::canvas_endpoints::EndpointScrollState,
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
    let source_paths = SourcePathCatalog::new(&source_schema, &[]);
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::Call {
            function: "concat".to_string(),
            args: vec![],
        },
    );
    let mut snarl = Snarl::new();
    let source = snarl.insert_node(egui::pos2(0.0, 0.0), CanvasNode::SourceBlock(0));
    let target = snarl.insert_node(egui::pos2(400.0, 0.0), CanvasNode::TargetBlock(0));
    let call = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(0));
    Fixture {
        graph,
        root_scope: Scope::default(),
        source_blocks: source_blocks(&source_schema),
        target_blocks: target_blocks(&target_schema),
        source_paths,
        endpoint_scroll: crate::canvas_endpoints::EndpointScrollState::default(),
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
            primary_root_authoring: false,
            extra_targets: &[],
            inactive_target_scopes: &[],
            project_references: Default::default(),
            source_blocks: &self.source_blocks,
            target_blocks: &self.target_blocks,
            source_x12: false,
            target_x12: false,
            source_paths: &self.source_paths,
            function_names: Default::default(),
            function_inputs: Default::default(),
            parameter_names: Default::default(),
            protected_output: None,
            function_output: None,
            requested_function_open: None,
            colors: crate::appearance::SemanticThemeColors::default(),
            wire_color_mode: crate::appearance::WireColorMode::Theme,
            endpoint_scroll: &mut self.endpoint_scroll,
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
        }
    }
}

#[test]
fn endpoint_labels_keep_the_field_name_when_compacting_deep_paths() {
    let label =
        compact_endpoint_label("Workbook/Worksheets/Regional Offices/Departments/PrimaryKey");

    assert_eq!(label, ".../Departments/PrimaryKey");
    assert!(label.chars().count() <= ENDPOINT_LABEL_CHAR_LIMIT);
}

#[test]
fn endpoint_labels_keep_short_paths_unchanged() {
    assert_eq!(compact_endpoint_label("Office/Name"), "Office/Name");
}

#[test]
fn wider_endpoints_reveal_more_of_deep_field_paths() {
    let path = "Interchange/Group/Message/LoopPO1/Product/PrimaryIdentifier";

    let compact = compact_endpoint_label_to(path, 24);
    let expanded = compact_endpoint_label_to(path, 52);

    assert!(expanded.chars().count() > compact.chars().count());
    assert!(path.ends_with(expanded.trim_start_matches("...")));
}

#[test]
fn minimap_focus_sets_zoom_and_centers_the_requested_graph_point() {
    let graph_point = egui::pos2(640.0, 360.0);
    let screen_point = egui::pos2(500.0, 400.0);
    let mut transform = egui::emath::TSTransform::from_scaling(0.2);

    apply_camera_focus(&mut transform, graph_point, screen_point, Some(1.0));

    assert_eq!(transform.scaling, 1.0);
    assert!((transform * graph_point - screen_point).length() < 0.001);
}

#[test]
fn long_constant_values_do_not_expand_node_titles() {
    let mut fx = fixture();
    let value = "transactionId followed by a long structured runtime payload".repeat(4);
    fx.graph.nodes.insert(
        7,
        Node::Const {
            value: ir::Value::String(value.clone()),
        },
    );

    let title = fx.viewer().title(&CanvasNode::Graph(7));

    assert!(!title.contains(&value));
    assert!(title.ends_with("..."));
    assert!(title.chars().count() <= 7 + 28);
}

#[test]
fn node_hover_emphasizes_every_incident_fanout_pin() {
    let source = SnarlNodeId(1);
    let hovered = SnarlNodeId(2);
    let unrelated = SnarlNodeId(3);
    let input = InPin {
        id: InPinId {
            node: hovered,
            input: 0,
        },
        remotes: vec![OutPinId {
            node: source,
            output: 0,
        }],
    };
    let single_output = OutPin {
        id: OutPinId {
            node: source,
            output: 0,
        },
        remotes: vec![input.id],
    };
    let fanout = OutPin {
        id: single_output.id,
        remotes: vec![
            input.id,
            InPinId {
                node: unrelated,
                input: 0,
            },
        ],
    };

    assert_eq!(input_wire_emphasis(None, &input), WireEmphasis::Normal);
    assert_eq!(
        input_wire_emphasis(Some(hovered), &input),
        WireEmphasis::Incident
    );
    assert_eq!(
        input_wire_emphasis(Some(source), &input),
        WireEmphasis::Incident
    );
    assert_eq!(
        input_wire_emphasis(Some(unrelated), &input),
        WireEmphasis::Unrelated
    );
    assert_eq!(
        output_wire_emphasis(Some(hovered), &single_output),
        WireEmphasis::Incident
    );
    assert_eq!(
        output_wire_emphasis(Some(hovered), &fanout),
        WireEmphasis::Incident
    );
    assert_eq!(
        output_wire_emphasis(Some(source), &fanout),
        WireEmphasis::Incident
    );
}

#[test]
fn recorded_pin_ids_match_snarl_drag_widgets() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);

    let context = egui::Context::default();
    crate::icons::install(&context);
    let _ = context.run_ui(Default::default(), |ui| {
        ui.set_min_size(egui::vec2(800.0, 600.0));
        let mut viewer = fx.viewer();
        SnarlWidget::new().show(&mut snarl, &mut viewer, ui);

        assert!(!viewer.pin_interaction_ids.is_empty());
        assert!(viewer.pin_interaction_ids.iter().all(|id| {
            ui.ctx()
                .read_response(*id)
                .is_some_and(|response| response.sense.senses_drag())
        }));
    });
}

#[test]
fn long_endpoint_paths_do_not_expand_the_source_node() {
    let mut fx = fixture();
    fx.source_blocks[0].leaves[0].label =
        "Workbook/Worksheets/Regional Offices/Departments/People/PrimaryKey".into();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let mut node_sizes = std::collections::BTreeMap::new();
    let mut endpoint_scroll = crate::canvas_endpoints::EndpointScrollState::default();

    let context = egui::Context::default();
    crate::icons::install(&context);
    let _ = context.run_ui(Default::default(), |ui| {
        ui.set_min_size(egui::vec2(800.0, 600.0));
        let mut viewer = GraphViewer {
            graph: &mut fx.graph,
            root_scope: &mut fx.root_scope,
            primary_root_authoring: false,
            extra_targets: &[],
            inactive_target_scopes: &[],
            project_references: Default::default(),
            source_blocks: &fx.source_blocks,
            target_blocks: &fx.target_blocks,
            source_x12: false,
            target_x12: false,
            source_paths: &fx.source_paths,
            function_names: Default::default(),
            function_inputs: Default::default(),
            parameter_names: Default::default(),
            protected_output: None,
            function_output: None,
            requested_function_open: None,
            colors: crate::appearance::SemanticThemeColors::default(),
            wire_color_mode: crate::appearance::WireColorMode::Theme,
            endpoint_scroll: &mut endpoint_scroll,
            value_map_wheel: None,
            endpoint_search_match: None,
            node_sizes: Some(&mut node_sizes),
            hovered_node: None,
            hovered_node_this_frame: None,
            camera_pan: egui::Vec2::ZERO,
            camera_focus: None,
            canvas_transform: None,
            pin_interaction_ids: Vec::new(),
            error: None,
        };
        SnarlWidget::new().show(&mut snarl, &mut viewer, ui);
    });

    let source_width = node_sizes
        .get(&CanvasNode::SourceBlock(0))
        .map_or(f32::INFINITY, |size| size.x);
    assert!(
        source_width <= 250.0,
        "source endpoint widened to {source_width}"
    );
}

#[test]
fn lookup_node_width_stabilizes_across_repaints() {
    let mut fx = fixture();
    let catalog = NamedSource {
        name: "Articles".to_string(),
        path: "Articles.xml".to_string(),
        schema: SchemaNode::group(
            "Articles",
            vec![
                SchemaNode::group(
                    "Article",
                    vec![
                        SchemaNode::scalar("Number", ScalarType::Int),
                        SchemaNode::scalar("Name", ScalarType::String),
                        SchemaNode::scalar("SinglePrice", ScalarType::Float),
                    ],
                )
                .repeating(),
            ],
        ),
        options: Default::default(),
        dynamic_path: None,
    };
    fx.source_paths =
        SourcePathCatalog::new(&SchemaNode::group("LineItems", Vec::new()), &[catalog]);
    fx.graph.nodes.insert(
        1,
        Node::SourceField {
            path: vec!["name".into()],
            frame: None,
        },
    );
    fx.graph.nodes.insert(
        2,
        Node::Lookup {
            collection: vec!["Articles".into(), "Article".into()],
            key: vec!["Number".into()],
            matches: 1,
            value: vec!["Name".into()],
        },
    );
    let mut snarl = std::mem::take(&mut fx.snarl);
    let lookup = snarl.insert_node(egui::pos2(250.0, 180.0), CanvasNode::Graph(2));
    snarl.connect(
        OutPinId {
            node: fx.source,
            output: 0,
        },
        InPinId {
            node: lookup,
            input: 0,
        },
    );
    snarl.connect(
        OutPinId {
            node: lookup,
            output: 0,
        },
        InPinId {
            node: fx.target,
            input: 0,
        },
    );
    let mut node_sizes = std::collections::BTreeMap::new();
    let mut endpoint_scroll = crate::canvas_endpoints::EndpointScrollState::default();
    let context = egui::Context::default();
    crate::icons::install(&context);
    context.set_zoom_factor(1.5);
    let mut sizes = Vec::new();
    let style = crate::appearance::EditorAppearance::default().to_snarl_style();
    let mut search = crate::canvas_search::CanvasSearchState::default();

    for _ in 0..24 {
        crate::theme::ThemeState::default().apply(&context);
        let _ = context.run_ui(Default::default(), |ui| {
            ui.set_min_size(egui::vec2(900.0, 700.0));
            egui::CentralPanel::default().show(ui, |ui| {
                let mut viewer = GraphViewer {
                    graph: &mut fx.graph,
                    root_scope: &mut fx.root_scope,
                    primary_root_authoring: false,
                    extra_targets: &[],
                    inactive_target_scopes: &[],
                    project_references: Default::default(),
                    source_blocks: &fx.source_blocks,
                    target_blocks: &fx.target_blocks,
                    source_x12: false,
                    target_x12: false,
                    source_paths: &fx.source_paths,
                    function_names: Default::default(),
                    function_inputs: Default::default(),
                    parameter_names: Default::default(),
                    protected_output: None,
                    function_output: None,
                    requested_function_open: None,
                    colors: crate::appearance::SemanticThemeColors::default(),
                    wire_color_mode: crate::appearance::WireColorMode::Theme,
                    endpoint_scroll: &mut endpoint_scroll,
                    value_map_wheel: None,
                    endpoint_search_match: None,
                    node_sizes: Some(&mut node_sizes),
                    hovered_node: None,
                    hovered_node_this_frame: None,
                    camera_pan: egui::Vec2::ZERO,
                    camera_focus: None,
                    canvas_transform: None,
                    pin_interaction_ids: Vec::new(),
                    error: None,
                };
                crate::canvas_keyboard::show(
                    &mut snarl,
                    &mut viewer,
                    &mut search,
                    crate::canvas_keyboard::CanvasOptions {
                        id_salt: egui::Id::new("endpoint-wheel-test"),
                        show_minimap: false,
                        view_generation: 0,
                        style,
                        focus: None,
                    },
                    ui,
                );
            });
        });
        if let Some(size) = node_sizes.get(&CanvasNode::Graph(2)) {
            sizes.push(*size);
        }
    }

    assert_eq!(sizes.len(), 24, "Lookup node was not rendered: {sizes:?}");
    assert!(
        sizes
            .iter()
            .all(|size| { (size.x - sizes[0].x).abs() < 0.5 && (size.y - sizes[0].y).abs() < 0.5 }),
        "Lookup node kept growing across repaints: {sizes:?}"
    );
    assert!(
        sizes[0].x < 400.0 && sizes[0].y < 240.0,
        "Lookup node settled at an excessive size: {sizes:?}"
    );
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
            matches!(n, Node::SourceField { path, .. } if path == &["name"]).then_some(*id)
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
fn sibling_repeating_source_pins_create_distinct_framed_fields() {
    let source_schema = SchemaNode::group(
        "root",
        vec![
            SchemaNode::group("A", vec![SchemaNode::scalar("Id", ScalarType::String)]).repeating(),
            SchemaNode::group("B", vec![SchemaNode::scalar("Id", ScalarType::String)]).repeating(),
        ],
    );
    let target_schema =
        SchemaNode::group("root", vec![SchemaNode::scalar("out", ScalarType::String)]);
    let source_blocks = source_blocks(&source_schema);
    let target_blocks = target_blocks(&target_schema);
    let source_paths = SourcePathCatalog::new(&source_schema, &[]);
    let mut graph = Graph::default();
    graph.nodes.insert(
        0,
        Node::Call {
            function: "concat".into(),
            args: vec![100, 101],
        },
    );
    let mut root_scope = Scope::default();
    let mut endpoint_scroll = crate::canvas_endpoints::EndpointScrollState::default();
    let mut snarl = Snarl::new();
    let source = snarl.insert_node(egui::pos2(0.0, 0.0), CanvasNode::SourceBlock(0));
    let call = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(0));
    let mut viewer = GraphViewer {
        graph: &mut graph,
        root_scope: &mut root_scope,
        primary_root_authoring: false,
        extra_targets: &[],
        inactive_target_scopes: &[],
        project_references: Default::default(),
        source_blocks: &source_blocks,
        target_blocks: &target_blocks,
        source_x12: false,
        target_x12: false,
        source_paths: &source_paths,
        function_names: Default::default(),
        function_inputs: Default::default(),
        parameter_names: Default::default(),
        protected_output: None,
        function_output: None,
        requested_function_open: None,
        colors: crate::appearance::SemanticThemeColors::default(),
        wire_color_mode: crate::appearance::WireColorMode::Theme,
        endpoint_scroll: &mut endpoint_scroll,
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

    for pin in 0..2 {
        let from = snarl.out_pin(OutPinId {
            node: source,
            output: pin,
        });
        let to = snarl.in_pin(InPinId {
            node: call,
            input: pin,
        });
        viewer.connect(&from, &to, &mut snarl);
    }

    let fields: std::collections::BTreeSet<_> = viewer
        .graph
        .nodes
        .values()
        .filter_map(|node| match node {
            Node::SourceField { frame, path } => Some((frame.clone(), path.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        fields,
        std::collections::BTreeSet::from([
            (Some(vec!["A".into()]), vec!["Id".into()]),
            (Some(vec!["B".into()]), vec!["Id".into()]),
        ])
    );
    let Some(Node::Call { args, .. }) = viewer.graph.nodes.get(&0) else {
        panic!("call node vanished");
    };
    assert_ne!(args[0], args[1]);
}

#[test]
fn required_inputs_stay_visually_empty() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let pos = egui::pos2(600.0, 300.0);
    let (if_id, _) = fx
        .viewer()
        .insert_with_unconnected_inputs(&mut snarl, pos, 3, |inputs| Node::If {
            condition: inputs[0],
            then: inputs[1],
            else_: inputs[2],
        })
        .unwrap();

    let inputs = node_inputs(&fx.graph.nodes[&if_id]);
    assert_eq!(inputs.len(), 3);
    assert!(
        inputs
            .iter()
            .all(|id| matches!(fx.graph.nodes.get(id), Some(Node::Unconnected)))
    );
    assert!(
        !snarl
            .nodes()
            .any(|node| matches!(node, CanvasNode::Placeholder(_)))
    );
    assert_eq!(snarl.wires().count(), 0);
}

#[test]
fn reconnect_and_disconnect_keep_the_input_pin_empty_without_visual_nodes() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let unconnected = fx.viewer().fresh_unconnected().expect("available node ID");
    let Node::Call { args, .. } = fx.graph.nodes.get_mut(&0).unwrap() else {
        panic!("fixture node should be a call");
    };
    args.push(unconnected);

    let from = snarl.out_pin(OutPinId {
        node: fx.source,
        output: 0,
    });
    let to = snarl.in_pin(InPinId {
        node: fx.call,
        input: 0,
    });
    fx.viewer().connect(&from, &to, &mut snarl);
    assert!(!fx.graph.nodes.contains_key(&unconnected));

    let source_field = fx
        .graph
        .nodes
        .iter()
        .find_map(|(&id, node)| matches!(node, Node::SourceField { .. }).then_some(id))
        .expect("source wire has a backing field");
    let from = snarl.out_pin(OutPinId {
        node: fx.source,
        output: 0,
    });
    let to = snarl.in_pin(InPinId {
        node: fx.call,
        input: 0,
    });
    fx.viewer().disconnect(&from, &to, &mut snarl);

    assert!(!fx.graph.nodes.contains_key(&source_field));
    let Some(Node::Call { args, .. }) = fx.graph.nodes.get(&0) else {
        panic!("fixture call exists");
    };
    assert!(matches!(
        fx.graph.nodes.get(&args[0]),
        Some(Node::Unconnected)
    ));
    assert_eq!(snarl.wires().count(), 0);
    assert_eq!(fx.graph.nodes.len(), 2, "call plus one hidden input value");
}

#[test]
fn deleting_a_node_removes_its_hidden_input_values() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let (if_id, if_node) = fx
        .viewer()
        .insert_with_unconnected_inputs(&mut snarl, egui::pos2(600.0, 300.0), 3, |inputs| {
            Node::If {
                condition: inputs[0],
                then: inputs[1],
                else_: inputs[2],
            }
        })
        .unwrap();
    fx.viewer().remove_graph_node(if_id, if_node, &mut snarl);

    assert_eq!(fx.graph.nodes.len(), 1, "only the fixture call remains");
    assert!(
        !snarl
            .nodes()
            .any(|node| matches!(node, CanvasNode::Placeholder(_)))
    );
    assert_eq!(snarl.wires().count(), 0);
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
fn binding_into_a_nested_target_creates_non_iterating_scope_chain() {
    let mut fx = fixture();
    fx.target_blocks = target_blocks(&SchemaNode::group(
        "root",
        vec![SchemaNode::group(
            "Order",
            vec![SchemaNode::group(
                "Address",
                vec![SchemaNode::scalar("b", ScalarType::Int)],
            )],
        )],
    ));
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
    assert!(viewer.error.is_none());
    assert_eq!(snarl.wires().count(), 1);
    assert!(fx.root_scope.bindings.is_empty());
    let order = &fx.root_scope.children[0];
    assert_eq!(order.target_field, "Order");
    assert!(!order.iterates());
    let address = &order.children[0];
    assert_eq!(address.target_field, "Address");
    assert!(!address.iterates());
    assert_eq!(address.bindings.len(), 1);
    assert_eq!(address.bindings[0].target_field, "b");
    assert!(matches!(
        fx.graph.nodes.get(&address.bindings[0].node),
        Some(Node::SourceField { path, .. }) if path == &["name"]
    ));
}

#[test]
fn rejected_source_connection_does_not_leak_a_source_field() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let from = snarl.out_pin(OutPinId {
        node: fx.source,
        output: 0,
    });
    let to = snarl.in_pin(InPinId {
        node: fx.target,
        input: 99,
    });
    let initial_nodes = fx.graph.nodes.len();

    let mut viewer = fx.viewer();
    viewer.connect(&from, &to, &mut snarl);

    assert!(
        viewer
            .error
            .as_deref()
            .is_some_and(|error| error.contains("target pin 99"))
    );
    assert_eq!(viewer.graph.nodes.len(), initial_nodes);
    assert!(
        !viewer
            .graph
            .nodes
            .values()
            .any(|node| matches!(node, Node::SourceField { .. }))
    );
    assert!(viewer.root_scope.bindings.is_empty());
    assert!(viewer.root_scope.children.is_empty());
    assert_eq!(snarl.wires().count(), 0);
}

#[test]
fn batch_removal_deletes_selected_dependency_chains_in_reference_order() {
    let mut fx = fixture();
    fx.graph.nodes.insert(
        1,
        Node::Call {
            function: "upper".into(),
            args: vec![0],
        },
    );
    let mut snarl = std::mem::take(&mut fx.snarl);
    let downstream = snarl.insert_node(egui::pos2(300.0, 100.0), CanvasNode::Graph(1));
    let call = fx.call;

    let removed = fx
        .viewer()
        .remove_snarl_nodes(&[call, downstream], &mut snarl);

    assert_eq!(removed, 2);
    assert!(fx.graph.nodes.is_empty());
    assert!(snarl.nodes().all(|node| matches!(
        node,
        CanvasNode::SourceBlock(0) | CanvasNode::TargetBlock(0)
    )));
}

#[test]
fn deleting_a_wired_node_leaves_downstream_input_empty() {
    let mut fx = fixture();
    fx.graph.nodes.insert(
        1,
        Node::Call {
            function: "upper".into(),
            args: vec![0],
        },
    );
    let mut snarl = std::mem::take(&mut fx.snarl);
    let downstream = snarl.insert_node(egui::pos2(300.0, 100.0), CanvasNode::Graph(1));
    snarl.connect(
        OutPinId {
            node: fx.call,
            output: 0,
        },
        InPinId {
            node: downstream,
            input: 0,
        },
    );

    let call = fx.call;
    assert!(fx.viewer().remove_graph_node(0, call, &mut snarl));

    let Some(Node::Call { args, .. }) = fx.graph.nodes.get(&1) else {
        panic!("downstream call remains");
    };
    assert!(matches!(
        fx.graph.nodes.get(&args[0]),
        Some(Node::Unconnected)
    ));
    assert_eq!(snarl.wires().count(), 0);
    assert_eq!(
        snarl.nodes().count(),
        3,
        "source, target, and downstream call"
    );
}

#[test]
fn connected_node_removal_clears_target_binding_and_hidden_inputs() {
    let mut fx = fixture();
    fx.graph.nodes.insert(1, Node::Unconnected);
    let Some(Node::Call { args, .. }) = fx.graph.nodes.get_mut(&0) else {
        panic!("fixture call exists");
    };
    args.push(1);
    let mut snarl = std::mem::take(&mut fx.snarl);
    fx.root_scope.bindings.push(Binding {
        target_field: "out".into(),
        node: 0,
    });
    let call = fx.call;

    assert_eq!(fx.viewer().remove_snarl_nodes(&[call], &mut snarl), 1);
    assert!(fx.graph.nodes.is_empty());
    assert!(fx.root_scope.bindings.is_empty());
    assert!(snarl.get_node(call).is_none());
}

#[test]
fn graph_connections_reject_invalid_inputs_and_cycles_atomically() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);

    // The fixture call has no inputs, so a source drag to pin zero must not
    // create its hidden SourceField before rejecting the pin.
    let source = snarl.out_pin(OutPinId {
        node: fx.source,
        output: 0,
    });
    let invalid = snarl.in_pin(InPinId {
        node: fx.call,
        input: 0,
    });
    fx.viewer().connect(&source, &invalid, &mut snarl);
    assert_eq!(fx.graph.nodes.len(), 1);
    assert_eq!(snarl.wires().count(), 0);

    let invalid_output = snarl.out_pin(OutPinId {
        node: fx.call,
        output: 1,
    });
    let target = snarl.in_pin(InPinId {
        node: fx.target,
        input: 0,
    });
    {
        let mut viewer = fx.viewer();
        viewer.connect(&invalid_output, &target, &mut snarl);
        assert!(
            viewer
                .error
                .as_deref()
                .is_some_and(|error| error.contains("output 1"))
        );
        assert!(viewer.root_scope.bindings.is_empty());
    }
    assert_eq!(snarl.wires().count(), 0);

    fx.graph.nodes.insert(2, Node::Const { value: Value::Null });
    fx.graph.nodes.insert(
        1,
        Node::Call {
            function: "concat".into(),
            args: vec![2],
        },
    );
    let Node::Call { args, .. } = fx.graph.nodes.get_mut(&0).unwrap() else {
        panic!("fixture node should be a call");
    };
    args.push(2);
    let second = snarl.insert_node(egui::pos2(300.0, 100.0), CanvasNode::Graph(1));

    let first_to_second = (
        snarl.out_pin(OutPinId {
            node: fx.call,
            output: 0,
        }),
        snarl.in_pin(InPinId {
            node: second,
            input: 0,
        }),
    );
    fx.viewer()
        .connect(&first_to_second.0, &first_to_second.1, &mut snarl);
    assert!(matches!(
        fx.graph.nodes.get(&1),
        Some(Node::Call { args, .. }) if args == &[0]
    ));
    assert_eq!(snarl.wires().count(), 1);

    let second_to_first = (
        snarl.out_pin(OutPinId {
            node: second,
            output: 0,
        }),
        snarl.in_pin(InPinId {
            node: fx.call,
            input: 0,
        }),
    );
    let call = fx.call;
    let mut viewer = fx.viewer();
    viewer.connect(&second_to_first.0, &second_to_first.1, &mut snarl);
    assert!(
        viewer
            .error
            .as_deref()
            .is_some_and(|error| error.contains("cycle"))
    );
    assert!(matches!(
        viewer.graph.nodes.get(&0),
        Some(Node::Call { args, .. }) if args == &[2]
    ));
    assert_eq!(snarl.wires().count(), 1);

    let self_connection = (
        snarl.out_pin(OutPinId {
            node: call,
            output: 0,
        }),
        snarl.in_pin(InPinId {
            node: call,
            input: 0,
        }),
    );
    viewer.connect(&self_connection.0, &self_connection.1, &mut snarl);
    assert!(
        viewer
            .error
            .as_deref()
            .is_some_and(|error| error.contains("cycle"))
    );
    assert!(matches!(
        viewer.graph.nodes.get(&0),
        Some(Node::Call { args, .. }) if args == &[2]
    ));
    assert_eq!(snarl.wires().count(), 1);
}

#[test]
fn aggregate_argument_pins_match_the_operation() {
    let mut fx = fixture();
    let count = node_palette::aggregate_node(AggregateOp::Count, None);
    assert_eq!(GraphViewer::input_count(&count), 0);

    let arg = fx.viewer().fresh_unconnected().expect("available node ID");
    let join = node_palette::aggregate_node(AggregateOp::Join, Some(arg));
    assert_eq!(GraphViewer::input_count(&join), 1);
    let Node::Aggregate { arg: Some(arg), .. } = join else {
        panic!("join should get an unconnected separator");
    };
    assert!(matches!(fx.graph.nodes[&arg], Node::Unconnected));

    let computed = Node::Aggregate {
        function: AggregateOp::Sum,
        collection: vec!["rows".into()],
        value: vec![],
        expression: Some(0),
        arg: None,
    };
    assert_eq!(GraphViewer::input_count(&computed), 1);
    let computed_join = Node::Aggregate {
        function: AggregateOp::Join,
        collection: vec!["rows".into()],
        value: vec![],
        expression: Some(0),
        arg: Some(1),
    };
    assert_eq!(GraphViewer::input_count(&computed_join), 2);
}

#[test]
fn every_palette_template_creates_one_complete_atomic_node_unit() {
    fn expected_unconnected_inputs(template: NodeTemplate) -> usize {
        match template {
            NodeTemplate::If => 3,
            NodeTemplate::DynamicSourceField | NodeTemplate::ValueMap | NodeTemplate::Lookup => 1,
            NodeTemplate::HostInputDefault => 1,
            NodeTemplate::CollectionFind => 2,
            NodeTemplate::Aggregate(AggregateOp::Join | AggregateOp::ItemAt) => 1,
            NodeTemplate::Builtin(name) => functions::builtin(name)
                .map(|builtin| builtin.arity.minimum())
                .unwrap_or_default(),
            NodeTemplate::Constant
            | NodeTemplate::SourceField
            | NodeTemplate::SourceDocumentPath
            | NodeTemplate::XmlSerialize
            | NodeTemplate::SourceRootField
            | NodeTemplate::SourceRootXmlTypeEquals
            | NodeTemplate::Position
            | NodeTemplate::HostInput
            | NodeTemplate::RuntimeValue(_)
            | NodeTemplate::Raise
            | NodeTemplate::Aggregate(_) => 0,
        }
    }

    fn matches_template(template: NodeTemplate, node: &Node) -> bool {
        match (template, node) {
            (NodeTemplate::Constant, Node::Const { value: Value::Null })
            | (NodeTemplate::SourceField, Node::SourceField { .. })
            | (NodeTemplate::SourceDocumentPath, Node::SourceDocumentPath)
            | (
                NodeTemplate::XmlSerialize,
                Node::XmlSerialize {
                    frame: None,
                    declaration: false,
                    indent: true,
                    namespace: None,
                    ..
                },
            )
            | (NodeTemplate::DynamicSourceField, Node::DynamicSourceField { frame: None, .. })
            | (NodeTemplate::SourceRootField, Node::SourceRootField { .. })
            | (NodeTemplate::SourceRootXmlTypeEquals, Node::SourceRootXmlTypeEquals { .. })
            | (NodeTemplate::Position, Node::Position { .. })
            | (NodeTemplate::HostInput, Node::RuntimeParameter { .. })
            | (NodeTemplate::HostInputDefault, Node::RuntimeParameterDefault { .. })
            | (NodeTemplate::Raise, Node::Raise { message: None })
            | (NodeTemplate::If, Node::If { .. })
            | (NodeTemplate::ValueMap, Node::ValueMap { .. })
            | (NodeTemplate::Lookup, Node::Lookup { .. })
            | (NodeTemplate::CollectionFind, Node::CollectionFind { .. }) => true,
            (
                NodeTemplate::Builtin(expected),
                Node::Call {
                    function: actual, ..
                },
            ) => expected == actual,
            (NodeTemplate::RuntimeValue(expected), Node::RuntimeValue { value: actual }) => {
                expected == *actual
            }
            (
                NodeTemplate::Aggregate(expected),
                Node::Aggregate {
                    function: actual, ..
                },
            ) => expected == *actual,
            _ => false,
        }
    }

    for template in node_palette::templates().filter(|template| {
        !matches!(
            template,
            NodeTemplate::SourceRootField | NodeTemplate::SourceRootXmlTypeEquals
        )
    }) {
        let mut fx = fixture();
        if template == NodeTemplate::DynamicSourceField {
            fx.source_paths = SourcePathCatalog::new(
                &SchemaNode::group("object", Vec::new())
                    .with_dynamic_fields(SchemaNode::scalar("value", ScalarType::String))
                    .unwrap(),
                &[],
            );
        }
        if template == NodeTemplate::XmlSerialize {
            fx.source_paths = SourcePathCatalog::new(
                &SchemaNode::group(
                    "Input",
                    vec![SchemaNode::scalar("Value", ScalarType::String)],
                ),
                &[],
            );
        }
        if template == NodeTemplate::SourceDocumentPath {
            fx.source_paths =
                fx.source_paths
                    .with_primary_source_options(&mapping::FormatOptions {
                        local_xml_file_set: true,
                        ..Default::default()
                    });
        }
        let mut snarl = std::mem::take(&mut fx.snarl);
        let graph_before = fx.graph.nodes.len();
        let snarl_before = snarl.nodes().count();
        let wire_before = snarl.wires().count();
        let unconnected = expected_unconnected_inputs(template);

        let (created, created_snarl) = fx
            .viewer()
            .insert_palette_node(&mut snarl, egui::pos2(240.0, 160.0), template)
            .expect("available palette IDs");

        assert!(matches_template(template, &fx.graph.nodes[&created]));
        assert_eq!(snarl[created_snarl], CanvasNode::Graph(created));
        assert_eq!(fx.graph.nodes.len(), graph_before + unconnected + 1);
        assert_eq!(snarl.nodes().count(), snarl_before + 1);
        assert_eq!(snarl.wires().count(), wire_before);
        assert_eq!(
            fx.graph
                .nodes
                .values()
                .filter(|node| matches!(node, Node::Unconnected))
                .count(),
            unconnected
        );
    }
}

#[test]
fn source_document_path_factory_defends_boundary_and_isolated_function_ownership() {
    for (file_set, isolated) in [(false, false), (false, true), (true, true)] {
        let mut fx = fixture();
        fx.source_paths = fx
            .source_paths
            .with_primary_source_options(&mapping::FormatOptions {
                local_xml_file_set: file_set,
                ..Default::default()
            });
        let before = serde_json::to_value(&fx.graph).unwrap();
        let scope_before = serde_json::to_value(&fx.root_scope).unwrap();
        let mut snarl = std::mem::take(&mut fx.snarl);
        let nodes = snarl.nodes().count();
        let wires = snarl.wires().count();
        let mut output = 0;
        let result = {
            let mut viewer = fx.viewer();
            if isolated {
                viewer.function_output = Some(&mut output);
            }
            viewer.insert_palette_node(
                &mut snarl,
                egui::pos2(240.0, 160.0),
                NodeTemplate::SourceDocumentPath,
            )
        };
        eprintln!(
            "Document-path original direct request: file_set={file_set}, isolated={isolated}, result={result:?}, graph={:?}",
            fx.graph
        );
        assert_eq!(result, Err("Source document paths require a primary local XML file set outside isolated functions.".into()));
        assert_eq!(serde_json::to_value(&fx.graph).unwrap(), before);
        assert_eq!(serde_json::to_value(&fx.root_scope).unwrap(), scope_before);
        assert_eq!(snarl.nodes().count(), nodes);
        assert_eq!(snarl.wires().count(), wires);
        assert_eq!(output, 0);
    }
}

#[test]
fn host_input_palette_requires_a_name_and_keeps_optional_default_hidden() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let (required, required_snarl) = fx
        .viewer()
        .insert_palette_node(
            &mut snarl,
            egui::pos2(240.0, 160.0),
            NodeTemplate::HostInput,
        )
        .expect("available palette IDs");
    let (optional, optional_snarl) = fx
        .viewer()
        .insert_palette_node(
            &mut snarl,
            egui::pos2(340.0, 160.0),
            NodeTemplate::HostInputDefault,
        )
        .expect("available palette IDs");

    assert!(matches!(
        &fx.graph.nodes[&required],
        Node::RuntimeParameter { name, ty: ScalarType::String, .. } if name.is_empty()
    ));
    let Node::RuntimeParameterDefault {
        name, ty, default, ..
    } = &fx.graph.nodes[&optional]
    else {
        panic!("optional host input was not created");
    };
    assert!(name.is_empty());
    assert_eq!(*ty, ScalarType::String);
    assert!(matches!(
        fx.graph.nodes.get(default),
        Some(Node::Unconnected)
    ));
    assert_eq!(snarl.wires().count(), 0);
    assert_eq!(snarl.nodes().count(), 5, "the default has no canvas node");
    assert!(
        fx.viewer()
            .title(&CanvasNode::Graph(required))
            .contains("<name required>")
    );
    assert!(
        fx.viewer()
            .title(&CanvasNode::Graph(optional))
            .contains("<name required>")
    );

    let from = snarl.out_pin(OutPinId {
        node: required_snarl,
        output: 0,
    });
    let to = snarl.in_pin(InPinId {
        node: optional_snarl,
        input: 0,
    });
    fx.viewer().connect(&from, &to, &mut snarl);
    assert!(matches!(
        fx.graph.nodes.get(&optional),
        Some(Node::RuntimeParameterDefault { default, .. }) if *default == required
    ));
    assert_eq!(snarl.wires().count(), 1);
}

#[test]
fn builtin_selector_reconciliation_adds_minimum_inputs_without_removing_excess() {
    let mut fx = fixture();
    if let Some(Node::Call { function, args }) = fx.graph.nodes.get_mut(&0) {
        *function = "matches".to_owned();
        args.clear();
    }
    let edited = fx.graph.nodes[&0].clone();
    let added = fx
        .viewer()
        .commit_node_property_edit(0, edited, true, false)
        .expect("available input IDs");
    assert_eq!(added, 2);
    let Some(Node::Call { args, .. }) = fx.graph.nodes.get(&0) else {
        panic!("fixture call is missing");
    };
    assert_eq!(args.len(), 2);
    assert!(
        args.iter()
            .all(|input| matches!(fx.graph.nodes.get(input), Some(Node::Unconnected)))
    );

    fx.graph.nodes.insert(
        20,
        Node::Const {
            value: Value::String("first".to_owned()),
        },
    );
    fx.graph.nodes.insert(
        21,
        Node::Const {
            value: Value::String("second".to_owned()),
        },
    );
    fx.graph.nodes.insert(
        22,
        Node::Const {
            value: Value::String("excess".to_owned()),
        },
    );
    if let Some(Node::Call { function, args }) = fx.graph.nodes.get_mut(&0) {
        *function = "upper".to_owned();
        *args = vec![20, 21, 22];
    }
    let edited = fx.graph.nodes[&0].clone();
    assert_eq!(
        fx.viewer()
            .commit_node_property_edit(0, edited, true, false)
            .expect("no input IDs needed"),
        0
    );
    let Some(Node::Call { args, .. }) = fx.graph.nodes.get(&0) else {
        panic!("fixture call is missing");
    };
    assert_eq!(args, &[20, 21, 22]);
}

#[test]
fn builtin_argument_controls_enforce_catalog_bounds_and_allow_excess_repair() {
    assert!(call_can_add_argument("matches", 2));
    assert!(!call_can_add_argument("matches", 3));
    assert!(!call_can_add_argument("matches", 4));
    assert!(!call_can_remove_argument("matches", 2));
    assert!(call_can_remove_argument("matches", 4));

    assert!(call_can_add_argument("add", 2));
    assert!(!call_can_remove_argument("add", 2));
    assert!(call_can_remove_argument("add", 3));

    assert!(call_can_add_argument("unknown_imported_function", 0));
    assert!(!call_can_remove_argument("unknown_imported_function", 0));
    assert!(call_can_remove_argument("unknown_imported_function", 1));
}

#[test]
fn variadic_parameter_labels_repeat_only_the_final_step_group() {
    let name = |function, index| builtin_parameter(function, index).map(|parameter| parameter.name);

    assert_eq!(name("add", 0), Some("left"));
    assert_eq!(name("add", 1), Some("right"));
    assert_eq!(name("add", 2), Some("right"));
    assert_eq!(name("datetime_add", 0), Some("value"));
    assert_eq!(name("datetime_add", 1), Some("duration"));
    assert_eq!(name("datetime_add", 4), Some("duration"));

    assert_eq!(name("json_serialize_object", 0), Some("path"));
    assert_eq!(name("json_serialize_object", 1), Some("scalar_type"));
    assert_eq!(name("json_serialize_object", 2), Some("value"));
    assert_eq!(name("json_serialize_object", 3), Some("path"));
    assert_eq!(name("json_serialize_object", 4), Some("scalar_type"));
    assert_eq!(name("json_serialize_object", 5), Some("value"));
    assert_eq!(name("upper", 1), None);
}

#[test]
fn sequence_exists_exposes_sequence_inputs_then_predicate() {
    let mut fx = fixture();
    fx.graph.nodes.insert(
        10,
        Node::SequenceExists {
            sequence: mapping::SequenceExpr::Tokenize {
                input: 1,
                delimiter: 2,
                item: 3,
            },
            predicate: 4,
        },
    );
    assert_eq!(GraphViewer::input_count(&fx.graph.nodes[&10]), 3);
    {
        let mut viewer = fx.viewer();
        assert_eq!(viewer.input_at(10, 0), Some(1));
        assert_eq!(viewer.input_at(10, 1), Some(2));
        assert_eq!(viewer.input_at(10, 2), Some(4));
        viewer.set_input(10, 0, 5);
        viewer.set_input(10, 2, 6);
    }
    let Node::SequenceExists {
        sequence,
        predicate,
    } = &fx.graph.nodes[&10]
    else {
        panic!("test node should remain sequence-exists");
    };
    assert_eq!(sequence.inputs(), vec![5, 2]);
    assert_eq!(*predicate, 6);

    let generated = Node::SequenceExists {
        sequence: mapping::SequenceExpr::Generate {
            from: None,
            to: 7,
            item: 8,
        },
        predicate: 9,
    };
    assert_eq!(GraphViewer::input_count(&generated), 2);

    let mut bounded = Node::SequenceExists {
        sequence: mapping::SequenceExpr::Generate {
            from: Some(7),
            to: 8,
            item: 9,
        },
        predicate: 4,
    };
    assert_eq!(GraphViewer::input_count(&bounded), 3);
    fx.graph.nodes.insert(11, bounded);
    {
        let mut viewer = fx.viewer();
        assert_eq!(viewer.input_at(11, 0), Some(7));
        assert_eq!(viewer.input_at(11, 1), Some(8));
        assert_eq!(viewer.input_at(11, 2), Some(4));
        viewer.set_input(11, 0, 5);
        viewer.set_input(11, 1, 6);
        viewer.set_input(11, 2, 10);
    }
    bounded = fx.graph.nodes.remove(&11).unwrap();
    let Node::SequenceExists {
        sequence,
        predicate,
    } = bounded
    else {
        panic!("test node should remain sequence-exists");
    };
    assert_eq!(sequence.inputs(), vec![5, 6]);
    assert_eq!(predicate, 10);
}

#[test]
fn sequence_exists_item_is_protected_from_deletion() {
    let mut fx = fixture();
    fx.graph.nodes.insert(
        10,
        Node::SequenceExists {
            sequence: mapping::SequenceExpr::Generate {
                from: None,
                to: 0,
                item: 3,
            },
            predicate: 0,
        },
    );
    fx.graph.nodes.insert(
        3,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );

    assert_eq!(
        fx.viewer().references_to(3),
        vec!["graph node 10 sequence item"]
    );
}

#[test]
fn join_owned_nodes_are_read_only_zero_input_nodes() {
    let mut fx = fixture();
    fx.graph.nodes.insert(
        10,
        Node::JoinField {
            join: mapping::JoinId::new(24),
            collection: vec!["products".into()],
            path: vec!["name".into()],
        },
    );
    fx.graph.nodes.insert(
        11,
        Node::JoinPosition {
            join: mapping::JoinId::new(24),
        },
    );

    assert_eq!(GraphViewer::input_count(&fx.graph.nodes[&10]), 0);
    assert_eq!(GraphViewer::input_count(&fx.graph.nodes[&11]), 0);
    assert!(node_inputs(&fx.graph.nodes[&10]).is_empty());
    assert!(node_inputs(&fx.graph.nodes[&11]).is_empty());
    assert_eq!(
        fx.viewer().title(&CanvasNode::Graph(10)),
        "Join field #24: products/name"
    );
    assert_eq!(
        fx.viewer().title(&CanvasNode::Graph(11)),
        "Join position #24"
    );
}

#[test]
fn referenced_nodes_report_graph_and_scope_consumers() {
    let mut fx = fixture();
    fx.graph.nodes.insert(1, Node::Const { value: Value::Null });
    let Node::Call { args, .. } = fx.graph.nodes.get_mut(&0).unwrap() else {
        panic!("fixture node should be a call");
    };
    args.push(1);
    fx.root_scope.bindings.push(Binding {
        target_field: "out".into(),
        node: 1,
    });
    fx.root_scope.group_by = Some(1);
    fx.root_scope.group_adjacent_by = Some(1);
    fx.root_scope.group_starting_with = Some(1);
    fx.root_scope.group_ending_with = Some(1);
    fx.root_scope.group_into_blocks = Some(1);
    fx.root_scope.sort_by = Some(1);
    fx.root_scope.windows = vec![mapping::SequenceWindow::First { count: 1 }];

    assert_eq!(
        fx.viewer().references_to(1),
        vec![
            "graph node 0",
            "root scope binding out",
            "root scope group block size",
            "root scope group-adjacent key",
            "root scope group-by key",
            "root scope group-ending predicate",
            "root scope group-starting predicate",
            "root scope sequence window 1",
            "root scope sort key",
        ]
    );
}

#[test]
fn dynamic_scope_references_are_protected_recursively() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let protected = snarl.insert_node(egui::pos2(500.0, 0.0), CanvasNode::Graph(1));
    for id in 1..=9 {
        fx.graph
            .nodes
            .insert(id, Node::Const { value: Value::Null });
    }
    fx.root_scope
        .dynamic_bindings
        .push(mapping::DynamicBinding { key: 1, value: 2 });

    let mut computed_scope = Scope {
        filter: Some(4),
        ..Scope::default()
    };
    computed_scope.bindings.push(Binding {
        target_field: "nested".into(),
        node: 5,
    });
    computed_scope
        .dynamic_bindings
        .push(mapping::DynamicBinding { key: 6, value: 7 });
    computed_scope.dynamic_children.push(mapping::DynamicChild {
        key: 8,
        scope: Scope {
            windows: vec![mapping::SequenceWindow::Last { count: 9 }],
            ..Scope::default()
        },
    });
    fx.root_scope.dynamic_children.push(mapping::DynamicChild {
        key: 3,
        scope: computed_scope,
    });

    let mut viewer = fx.viewer();
    assert_eq!(
        viewer.references_to(1),
        vec!["root scope dynamic binding 1 key"]
    );
    assert_eq!(
        viewer.references_to(2),
        vec!["root scope dynamic binding 1 value"]
    );
    assert_eq!(
        viewer.references_to(3),
        vec!["root scope dynamic child 1 key"]
    );
    assert_eq!(
        viewer.references_to(4),
        vec!["scope <dynamic child 1> filter"]
    );
    assert_eq!(
        viewer.references_to(5),
        vec!["scope <dynamic child 1> binding nested"]
    );
    assert_eq!(
        viewer.references_to(6),
        vec!["scope <dynamic child 1> dynamic binding 1 key"]
    );
    assert_eq!(
        viewer.references_to(7),
        vec!["scope <dynamic child 1> dynamic binding 1 value"]
    );
    assert_eq!(
        viewer.references_to(8),
        vec!["scope <dynamic child 1> dynamic child 1 key"]
    );
    assert_eq!(
        viewer.references_to(9),
        vec!["scope <dynamic child 1>/<dynamic child 1> sequence window 1"]
    );
    assert!(!viewer.remove_graph_node(1, protected, &mut snarl));
    assert!(viewer.graph.nodes.contains_key(&1));
    assert!(snarl.nodes().any(|node| *node == CanvasNode::Graph(1)));
    assert!(
        viewer
            .error
            .as_deref()
            .is_some_and(|error| error.contains("root scope dynamic binding 1 key"))
    );
}

#[test]
fn generated_sequence_nodes_are_protected_from_deletion() {
    let mut fx = fixture();
    fx.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::Int(1),
        },
    );
    fx.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Int(3),
        },
    );
    fx.graph.nodes.insert(
        3,
        Node::SourceField {
            path: Vec::new(),
            frame: None,
        },
    );
    fx.root_scope
        .set_sequence(Some(mapping::SequenceExpr::Generate {
            from: Some(1),
            to: 2,
            item: 3,
        }));

    assert_eq!(
        fx.viewer().references_to(1),
        vec!["root scope sequence input"]
    );
    assert_eq!(
        fx.viewer().references_to(2),
        vec!["root scope sequence input"]
    );
    assert_eq!(
        fx.viewer().references_to(3),
        vec!["root scope sequence item"]
    );
}

#[test]
fn adjacency_tree_root_node_is_protected_from_deletion() {
    let mut fx = fixture();
    fx.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("Root".into()),
        },
    );
    fx.root_scope.construction = mapping::ScopeConstruction::AdjacencyTree {
        plan: mapping::AdjacencyTreePlan::new(
            vec!["row".into()],
            vec!["name".into()],
            vec!["base".into()],
            "name".into(),
            "children".into(),
            Some(1),
        )
        .unwrap(),
    };

    assert_eq!(
        fx.viewer().references_to(1),
        vec!["root scope adjacency-tree root"]
    );
}

#[path = "graph_viewer_tests/allocation.rs"]
mod allocation_tests;

#[test]
fn primary_root_primitives_are_zero_input_visible_graph_nodes() {
    let mut fx = fixture();
    for (id, node) in [
        (
            10,
            Node::SourceRootXmlTypeEquals {
                canonical_expanded_type: "Derived".into(),
            },
        ),
        (
            11,
            Node::SourceRootField {
                path: vec!["Code".into()],
                required: false,
            },
        ),
    ] {
        assert_eq!(GraphViewer::input_count(&node), 0);
        assert!(node_inputs(&node).is_empty());
        fx.graph.nodes.insert(id, node);
        assert_eq!(fx.viewer().input_at(id, 0), None);
        assert_eq!(fx.viewer().input_at(id, 1), None);
    }
    let mut viewer = fx.viewer();
    assert_eq!(
        viewer.title(&CanvasNode::Graph(10)),
        "Primary XML annotation equality"
    );
    assert_eq!(viewer.title(&CanvasNode::Graph(11)), "Primary field: Code");
}

#[test]
fn root_palette_creation_is_valid_atomic_and_rejected_on_other_canvases() {
    for template in [
        NodeTemplate::SourceRootField,
        NodeTemplate::SourceRootXmlTypeEquals,
    ] {
        let mut fx = fixture();
        fx.source_paths =
            SourcePathCatalog::new(&crate::primary_root_authoring::test_schema(), &[]);
        let mut snarl = std::mem::take(&mut fx.snarl);
        let before = serde_json::to_value(&fx.graph).unwrap();
        assert!(
            fx.viewer()
                .insert_palette_node(&mut snarl, egui::pos2(1.0, 1.0), template)
                .is_err()
        );
        assert_eq!(serde_json::to_value(&fx.graph).unwrap(), before);
        let count = fx.graph.nodes.len();
        let (id, snarl_id) = {
            let mut viewer = fx.viewer();
            viewer.primary_root_authoring = true;
            viewer
                .insert_palette_node(&mut snarl, egui::pos2(1.0, 1.0), template)
                .unwrap()
        };
        assert_eq!(fx.graph.nodes.len(), count + 1);
        assert_eq!(snarl[snarl_id], CanvasNode::Graph(id));
        assert_eq!(GraphViewer::input_count(&fx.graph.nodes[&id]), 0);
        assert!(node_inputs(&fx.graph.nodes[&id]).is_empty());
        match &fx.graph.nodes[&id] {
            Node::SourceRootField { path, required } => {
                assert_eq!(path, &["Code"]);
                assert!(!required);
            }
            Node::SourceRootXmlTypeEquals {
                canonical_expanded_type,
            } => assert_eq!(canonical_expanded_type, "Base"),
            other => panic!("wrong created root primitive: {other:?}"),
        }
        let serialized = serde_json::to_vec(&fx.graph).unwrap();
        let restored: Graph = serde_json::from_slice(&serialized).unwrap();
        assert_eq!(serde_json::to_vec(&restored).unwrap(), serialized);
    }
}

fn root_editor_frame(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    allowed: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let pin = snarl.out_pin(OutPinId {
        node: fx.call,
        output: 0,
    });
    context.run_ui(
        egui::RawInput {
            time: Some(context.cumulative_frame_nr() as f64 / 10.0),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 900.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let mut viewer = fx.viewer();
            viewer.primary_root_authoring = allowed;
            viewer.show_output(&pin, ui, snarl);
        },
    )
}
fn root_editor_click(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    allowed: bool,
    label: &str,
    last: bool,
) {
    fn collect(shape: &egui::epaint::Shape, label: &str, out: &mut Vec<egui::Pos2>) {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                out.push(text.visual_bounding_rect().center())
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    collect(shape, label, out);
                }
            }
            _ => {}
        }
    }
    let mut output = root_editor_frame(fx, snarl, context, allowed, Vec::new());
    for _ in 0..3 {
        output = root_editor_frame(fx, snarl, context, allowed, Vec::new());
    }
    let mut positions = Vec::new();
    for shape in &output.shapes {
        collect(&shape.shape, label, &mut positions);
    }
    if positions.is_empty() {
        let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
        let mut editors = Vec::new();
        for shape in &output.shapes {
            collect(&shape.shape, &pencil, &mut editors);
        }
        let edit = *editors.first().unwrap_or_else(|| {
            panic!(
                "missing root editor for {label:?}; shapes={:?}",
                output.shapes
            )
        });
        for pressed in [true, false] {
            let _ = root_editor_frame(
                fx,
                snarl,
                context,
                allowed,
                vec![
                    egui::Event::PointerMoved(edit),
                    egui::Event::PointerButton {
                        pos: edit,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
        for _ in 0..3 {
            output = root_editor_frame(fx, snarl, context, allowed, Vec::new());
        }
        for shape in &output.shapes {
            collect(&shape.shape, label, &mut positions);
        }
    }
    let pos = *if last {
        positions.last()
    } else {
        positions.first()
    }
    .unwrap_or_else(|| panic!("missing root control {label:?}"));
    for pressed in [true, false] {
        let _ = root_editor_frame(
            fx,
            snarl,
            context,
            allowed,
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
}

#[test]
fn pointer_root_editor_changes_path_required_and_exact_namespaced_type() {
    let mut fx = fixture();
    fx.source_paths = SourcePathCatalog::new(&crate::primary_root_authoring::test_schema(), &[]);
    fx.graph.nodes.insert(
        0,
        Node::SourceRootField {
            path: vec!["Code".into()],
            required: false,
        },
    );
    let mut snarl = std::mem::take(&mut fx.snarl);
    let context = egui::Context::default();
    crate::icons::install(&context);
    root_editor_click(&mut fx, &mut snarl, &context, true, "Code", false);
    root_editor_click(&mut fx, &mut snarl, &context, true, "Extra", true);
    root_editor_click(
        &mut fx,
        &mut snarl,
        &context,
        true,
        "Require a value when read",
        false,
    );
    assert!(
        matches!(&fx.graph.nodes[&0], Node::SourceRootField { path, required: true } if path == &["Extra"])
    );
    fx.graph.nodes.insert(
        0,
        Node::SourceRootXmlTypeEquals {
            canonical_expanded_type: "Base".into(),
        },
    );
    root_editor_click(&mut fx, &mut snarl, &context, true, "Base", false);
    root_editor_click(
        &mut fx,
        &mut snarl,
        &context,
        true,
        "{urn:root}Derived",
        true,
    );
    assert!(
        matches!(&fx.graph.nodes[&0], Node::SourceRootXmlTypeEquals { canonical_expanded_type } if canonical_expanded_type == "{urn:root}Derived")
    );
    assert!(snarl.wires().next().is_none());
}

#[test]
fn locked_root_editor_preserves_invalid_imported_path_and_policy() {
    let mut fx = fixture();
    fx.source_paths = SourcePathCatalog::new(&crate::primary_root_authoring::test_schema(), &[]);
    fx.graph.nodes.insert(
        0,
        Node::SourceRootField {
            path: vec!["LegacyMissing".into()],
            required: false,
        },
    );
    let before = serde_json::to_value(&fx.graph).unwrap();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let context = egui::Context::default();
    crate::icons::install(&context);
    root_editor_click(
        &mut fx,
        &mut snarl,
        &context,
        false,
        "Require a value when read",
        false,
    );
    assert_eq!(serde_json::to_value(&fx.graph).unwrap(), before);
    let _ = root_editor_frame(&mut fx, &mut snarl, &context, true, Vec::new());
    assert_eq!(serde_json::to_value(&fx.graph).unwrap(), before);
}

#[path = "graph_viewer_tests/compact_nodes.rs"]
mod compact_node_tests;

#[path = "graph_viewer_tests/compact_primary_root_nodes.rs"]
mod compact_primary_root_node_tests;

#[path = "graph_viewer_tests/compact_dynamic_property_nodes.rs"]
mod compact_dynamic_property_node_tests;

#[path = "graph_viewer_tests/compact_xml_mixed_content_nodes.rs"]
mod compact_xml_mixed_content_node_tests;

#[path = "graph_viewer_tests/compact_value_map_nodes.rs"]
mod compact_value_map_node_tests;

#[path = "graph_viewer_tests/compact_raise_nodes.rs"]
mod compact_raise_node_tests;

#[path = "graph_viewer_tests/compact_xml_pin_identities.rs"]
mod compact_xml_pin_identity_tests;

#[path = "graph_viewer_tests/computed_aggregates.rs"]
mod computed_aggregates;

#[path = "graph_viewer_tests/filter_map_guards.rs"]
mod filter_map_guard_tests;
