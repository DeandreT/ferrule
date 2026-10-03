use super::*;

const ROOT_FIELD: NodeId = 1;
const ROOT_TYPE: NodeId = 2;
const ORDINARY: NodeId = 3;
const DERIVED_IF: NodeId = 4;
const DERIVED_CALL: NodeId = 5;

fn project() -> Project {
    let mut project = three_output_project(Node::Call {
        function: "concat".into(),
        args: vec![ORDINARY, ORDINARY],
    });
    project.source = crate::primary_root_authoring::test_schema();
    project.graph.nodes.extend([
        (
            ROOT_FIELD,
            Node::SourceRootField {
                path: vec!["Code".into()],
                required: false,
            },
        ),
        (
            ROOT_TYPE,
            Node::SourceRootXmlTypeEquals {
                canonical_expanded_type: "Base".into(),
            },
        ),
        (
            ORDINARY,
            Node::Const {
                value: Value::String("safe".into()),
            },
        ),
        (
            DERIVED_IF,
            Node::If {
                condition: ROOT_TYPE,
                then: ROOT_FIELD,
                else_: ORDINARY,
            },
        ),
        (
            DERIVED_CALL,
            Node::Call {
                function: "concat".into(),
                args: vec![DERIVED_IF, ORDINARY],
            },
        ),
    ]);
    assert!(engine::validate(&project).is_empty());
    project
}

fn connect_primary(
    project: &mut Project,
    from: &OutPin,
    to: &InPin,
    snarl: &mut Snarl<CanvasNode>,
) -> Option<String> {
    let source_blocks = source_blocks(&project.source);
    let target_blocks = target_blocks(&project.target);
    let source_paths = SourcePathCatalog::new(&project.source, &project.extra_sources);
    let mut endpoint_scroll = EndpointScrollState::default();
    let primary_root_authoring = crate::primary_root_authoring::available(project);
    let mut viewer = GraphViewer {
        graph: &mut project.graph,
        root_scope: &mut project.root,
        primary_root_authoring,
        extra_targets: &project.extra_targets,
        inactive_target_scopes: &[],
        project_references: ProjectGraphReferences::new(
            &project.failure_rules,
            &project.extra_sources,
        ),
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
        colors: SemanticThemeColors::default(),
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
    viewer.connect(from, to, snarl);
    viewer.error
}

#[test]
fn named_target_root_wires_are_rejected_without_displacing_existing_bindings() {
    for target_index in 0..2 {
        for expression in [ROOT_FIELD, ROOT_TYPE, DERIVED_IF, DERIVED_CALL] {
            let mut project = project();
            let before = mapping::project_file::encode_pretty(&project).unwrap();
            let mut snarl = Snarl::new();
            let root = snarl.insert_node(egui::Pos2::ZERO, CanvasNode::Graph(expression));
            let old = snarl.insert_node(egui::pos2(100.0, 0.0), CanvasNode::Graph(SHARED));
            let target = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::TargetBlock(0));
            let target_pin = InPinId {
                node: target,
                input: 0,
            };
            snarl.connect(
                OutPinId {
                    node: old,
                    output: 0,
                },
                target_pin,
            );
            let before_wires = snarl.wires().collect::<Vec<_>>();
            let before_nodes = snarl.nodes().copied().collect::<Vec<_>>();
            let from = snarl.out_pin(OutPinId {
                node: root,
                output: 0,
            });
            let to = snarl.in_pin(target_pin);
            edit_named_target(&mut project, target_index, |viewer| {
                viewer.connect(&from, &to, &mut snarl);
                assert!(
                    viewer
                        .error
                        .as_deref()
                        .is_some_and(|error| error.contains("flat primary target root"))
                );
            });
            assert_eq!(
                mapping::project_file::encode_pretty(&project).unwrap(),
                before
            );
            assert_eq!(snarl.wires().collect::<Vec<_>>(), before_wires);
            assert_eq!(snarl.nodes().copied().collect::<Vec<_>>(), before_nodes);
            assert!(engine::validate(&project).is_empty());
        }
    }
}

#[test]
fn named_canvas_root_input_wires_preserve_shared_graph_and_all_target_owners() {
    for target_index in 0..2 {
        for expression in [ROOT_FIELD, DERIVED_CALL] {
            let mut project = project();
            let before = mapping::project_file::encode_pretty(&project).unwrap();
            let mut snarl = Snarl::new();
            let root = snarl.insert_node(egui::Pos2::ZERO, CanvasNode::Graph(expression));
            let ordinary = snarl.insert_node(egui::pos2(0.0, 100.0), CanvasNode::Graph(ORDINARY));
            let call = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(SHARED));
            let input = InPinId {
                node: call,
                input: 0,
            };
            snarl.connect(
                OutPinId {
                    node: ordinary,
                    output: 0,
                },
                input,
            );
            let before_wires = snarl.wires().collect::<Vec<_>>();
            let from = snarl.out_pin(OutPinId {
                node: root,
                output: 0,
            });
            let to = snarl.in_pin(input);
            edit_named_target(&mut project, target_index, |viewer| {
                viewer.connect(&from, &to, &mut snarl);
                assert!(
                    viewer
                        .error
                        .as_deref()
                        .is_some_and(|error| error.contains("flat primary target root"))
                );
            });
            assert_eq!(
                mapping::project_file::encode_pretty(&project).unwrap(),
                before
            );
            assert_eq!(snarl.wires().collect::<Vec<_>>(), before_wires);
            assert!(engine::validate(&project).is_empty());
        }
    }
}

#[test]
fn primary_canvas_cannot_add_root_input_to_a_named_target_dependency() {
    for expression in [ROOT_FIELD, DERIVED_CALL] {
        for receiver in [SHARED, 6] {
            let mut project = project();
            if receiver == 6 {
                project.graph.nodes.insert(
                    6,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![ORDINARY, ORDINARY],
                    },
                );
                project.graph.nodes.insert(
                    SHARED,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![6, ORDINARY],
                    },
                );
            }
            assert!(engine::validate(&project).is_empty());
            let before = mapping::project_file::encode_pretty(&project).unwrap();
            let mut snarl = Snarl::new();
            let root = snarl.insert_node(egui::Pos2::ZERO, CanvasNode::Graph(expression));
            let ordinary = snarl.insert_node(egui::pos2(0.0, 100.0), CanvasNode::Graph(ORDINARY));
            let call = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(receiver));
            let input = InPinId {
                node: call,
                input: 0,
            };
            snarl.connect(
                OutPinId {
                    node: ordinary,
                    output: 0,
                },
                input,
            );
            let before_wires = snarl.wires().collect::<Vec<_>>();
            let from = snarl.out_pin(OutPinId {
                node: root,
                output: 0,
            });
            let to = snarl.in_pin(input);
            let error = connect_primary(&mut project, &from, &to, &mut snarl).unwrap();
            assert!(error.contains("<target audit>"));
            assert!(error.contains("<target archive>"));
            assert_eq!(
                mapping::project_file::encode_pretty(&project).unwrap(),
                before
            );
            assert_eq!(snarl.wires().collect::<Vec<_>>(), before_wires);
            assert!(engine::validate(&project).is_empty());
        }
    }
}

#[test]
fn primary_root_and_derived_wires_remain_accepted_for_flat_primary_bindings() {
    for expression in [ROOT_FIELD, DERIVED_IF, DERIVED_CALL] {
        let mut project = project();
        let named_before = serde_json::to_value(&project.extra_targets).unwrap();
        let mut snarl = Snarl::new();
        let root = snarl.insert_node(egui::Pos2::ZERO, CanvasNode::Graph(expression));
        let target = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::TargetBlock(0));
        let from = snarl.out_pin(OutPinId {
            node: root,
            output: 0,
        });
        let to = snarl.in_pin(InPinId {
            node: target,
            input: 0,
        });
        assert_eq!(connect_primary(&mut project, &from, &to, &mut snarl), None);
        assert_eq!(project.root.bindings[0].node, expression);
        assert_eq!(snarl.wires().count(), 1);
        assert_eq!(
            serde_json::to_value(&project.extra_targets).unwrap(),
            named_before
        );
        assert!(engine::validate(&project).is_empty());
    }
}

#[test]
fn primary_graph_input_accepts_root_dependency_when_every_consumer_is_eligible() {
    let mut project = project();
    project.root.bindings[0].node = DERIVED_CALL;
    let mut snarl = Snarl::new();
    let root = snarl.insert_node(egui::Pos2::ZERO, CanvasNode::Graph(ROOT_FIELD));
    let derived = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(DERIVED_CALL));
    let from = snarl.out_pin(OutPinId {
        node: root,
        output: 0,
    });
    let to = snarl.in_pin(InPinId {
        node: derived,
        input: 1,
    });
    assert_eq!(connect_primary(&mut project, &from, &to, &mut snarl), None);
    assert!(
        matches!(&project.graph.nodes[&DERIVED_CALL], Node::Call { args, .. } if args == &[DERIVED_IF, ROOT_FIELD])
    );
    assert!(engine::validate(&project).is_empty());
}

#[test]
fn ordinary_named_target_wire_repairs_an_imported_invalid_root_binding() {
    let mut project = project();
    project.extra_targets[0].root.bindings[0].node = DERIVED_CALL;
    assert!(!engine::validate(&project).is_empty());
    let mut snarl = Snarl::new();
    let invalid = snarl.insert_node(egui::Pos2::ZERO, CanvasNode::Graph(DERIVED_CALL));
    let ordinary = snarl.insert_node(egui::pos2(0.0, 100.0), CanvasNode::Graph(ORDINARY));
    let target = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::TargetBlock(0));
    let input = InPinId {
        node: target,
        input: 0,
    };
    snarl.connect(
        OutPinId {
            node: invalid,
            output: 0,
        },
        input,
    );
    let from = snarl.out_pin(OutPinId {
        node: ordinary,
        output: 0,
    });
    let to = snarl.in_pin(input);
    edit_named_target(&mut project, 0, |viewer| {
        viewer.connect(&from, &to, &mut snarl);
        assert!(viewer.error.is_none());
    });
    assert_eq!(project.extra_targets[0].root.bindings[0].node, ORDINARY);
    assert_eq!(snarl.in_pin(input).remotes, vec![from.id]);
    assert!(project.graph.nodes.contains_key(&DERIVED_CALL));
    assert!(engine::validate(&project).is_empty());
}
