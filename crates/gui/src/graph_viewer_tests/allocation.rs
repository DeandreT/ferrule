use super::*;
use ir::Instance;
use mapping::{
    FailureIteration, FailureRule, FailureSelection, Project, ScopeIteration, SequenceExpr,
};

fn constant(value: &str) -> Node {
    Node::Const {
        value: Value::String(value.into()),
    }
}

fn live_fixture(maximum: NodeId) -> (Fixture, Snarl<CanvasNode>) {
    let mut fx = fixture();
    fx.graph.nodes = [
        (
            0,
            Node::Call {
                function: "upper".into(),
                args: vec![1],
            },
        ),
        (1, constant("keep")),
        (maximum, constant("unrelated")),
    ]
    .into_iter()
    .collect();
    fx.root_scope.bindings.push(Binding {
        target_field: "out".into(),
        node: 0,
    });
    let mut snarl = std::mem::take(&mut fx.snarl);
    let input = snarl.insert_node(egui::pos2(80.0, 60.0), CanvasNode::Graph(1));
    snarl.connect(
        OutPinId {
            node: input,
            output: 0,
        },
        InPinId {
            node: fx.call,
            input: 0,
        },
    );
    snarl.connect(
        OutPinId {
            node: fx.call,
            output: 0,
        },
        InPinId {
            node: fx.target,
            input: 0,
        },
    );
    (fx, snarl)
}

fn state(fx: &Fixture, snarl: &Snarl<CanvasNode>) -> (String, String, String) {
    (
        serde_json::to_string(&fx.graph).unwrap(),
        serde_json::to_string(&fx.root_scope).unwrap(),
        format!("{snarl:?}"),
    )
}

fn assert_no_placeholders(snarl: &Snarl<CanvasNode>) {
    assert!(
        !snarl
            .nodes()
            .any(|node| matches!(node, CanvasNode::Placeholder(_)))
    );
}

/// Execute a repaired clone containing the visible static output's reachable
/// graph. Deliberately malformed unused ownership fixtures remain untouched live.
fn assert_output(fx: &Fixture, expected: Option<&str>, repair_inputs: bool) {
    let root = Scope {
        bindings: fx.root_scope.bindings.clone(),
        ..Scope::default()
    };
    let mut graph = fx.graph.clone();
    let mut pending = root
        .bindings
        .iter()
        .map(|binding| binding.node)
        .collect::<Vec<_>>();
    pending.extend(root.filter);
    let mut reached = std::collections::BTreeSet::new();
    while let Some(id) = pending.pop() {
        if reached.insert(id) {
            pending.extend(graph.nodes.get(&id).map(node_inputs).unwrap_or_default());
        }
    }
    graph.nodes.retain(|id, _| reached.contains(id));
    if repair_inputs {
        for node in graph.nodes.values_mut() {
            if matches!(node, Node::Unconnected) {
                *node = constant("fixed");
            }
        }
    }
    let project = Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group("row", vec![SchemaNode::scalar("out", ScalarType::String)]),
        graph,
        root,
        ..crate::new_mapping::blank_project()
    };
    assert!(engine::validate(&project).is_empty());
    let result = engine::run(&project, &Instance::Group((Vec::new()).into())).unwrap();
    let expected = Instance::Group(
        (expected
            .map(|value| vec![("out".into(), Instance::Scalar(Value::String(value.into())))])
            .unwrap_or_default())
        .into(),
    );
    assert_eq!(result, expected);
}

#[test]
fn reservation_is_checked_append_only_and_keeps_its_original_cursor() {
    let mut graph = Graph::default();
    let owned = [0, 2, 3].into_iter().collect();
    let mut cursor = NodeIdReservation::new(&graph, &owned);
    assert_eq!(cursor.reserve(2).unwrap(), [1, 4]);
    assert_eq!(cursor.reserve(1).unwrap(), [5]);
    graph.nodes.insert(u32::MAX, constant("keep"));
    let empty = Default::default();
    let mut cursor = NodeIdReservation::new(&graph, &empty);
    assert!(cursor.reserve(0).unwrap().is_empty());
    assert_eq!(
        cursor.reserve(1).unwrap_err(),
        "mapping node IDs are exhausted"
    );
    graph.nodes.remove(&u32::MAX);
    graph.nodes.insert(u32::MAX - 2, constant("keep"));
    let mut cursor = NodeIdReservation::new(&graph, &empty);
    assert_eq!(cursor.reserve(2).unwrap(), [u32::MAX - 1, u32::MAX]);
    assert!(cursor.reserve(1).is_err());
    let mut cursor = NodeIdReservation::new(&graph, &empty);
    assert!(cursor.reserve(usize::MAX).is_err());
}

#[test]
fn palette_units_reserve_all_inputs_before_any_visible_or_hidden_node_changes() {
    for (template, count) in [
        (NodeTemplate::Constant, 1),
        (NodeTemplate::HostInputDefault, 2),
        (NodeTemplate::If, 4),
        (NodeTemplate::Builtin("matches"), 3),
        (NodeTemplate::CollectionFind, 3),
        (NodeTemplate::Aggregate(AggregateOp::Join), 2),
    ] {
        for success in [false, true] {
            let maximum = if success {
                u32::MAX - count
            } else {
                u32::MAX - count + 1
            };
            let (mut fx, mut snarl) = live_fixture(maximum);
            let before = state(&fx, &snarl);
            let result =
                fx.viewer()
                    .insert_palette_node(&mut snarl, egui::pos2(250.0, 200.0), template);
            if success {
                let (created, canvas) = result.unwrap();
                assert_eq!(created, u32::MAX);
                assert_eq!(snarl[canvas], CanvasNode::Graph(created));
                assert_eq!(fx.graph.nodes.len(), 3 + count as usize);
                assert_eq!(snarl.wires().count(), 2);
                assert_no_placeholders(&snarl);
            } else {
                assert_eq!(result.unwrap_err(), "mapping node IDs are exhausted");
                assert_eq!(state(&fx, &snarl), before);
            }
            assert_output(&fx, Some("KEEP"), false);
        }
    }
}

fn item_sequence(item: NodeId) -> SequenceExpr {
    SequenceExpr::Tokenize {
        input: 1,
        delimiter: 20,
        item,
    }
}
fn item_scope(item: NodeId) -> Scope {
    Scope {
        iteration: ScopeIteration::Sequence(item_sequence(item)),
        ..Scope::default()
    }
}
fn named_scope(name: &str, item: NodeId) -> NamedTarget {
    NamedTarget {
        name: name.into(),
        path: None,
        schema: SchemaNode::group("Target", Vec::new()),
        options: Default::default(),
        root: item_scope(item),
    }
}

#[test]
fn all_generated_owners_reserve_missing_ids_while_function_bodies_stay_isolated() {
    for named in [false, true] {
        for isolated in [false, true] {
            let (mut fx, mut snarl) = live_fixture(20);
            fx.root_scope.children.push(item_scope(21));
            let concatenated = Scope {
                iteration: ScopeIteration::Concatenate(mapping::ScopeSequence::new(
                    item_scope(25),
                    Vec::new(),
                )),
                ..Scope::default()
            };
            fx.root_scope.children.push(concatenated);
            fx.root_scope.dynamic_children.push(mapping::DynamicChild {
                key: 1,
                scope: item_scope(26),
            });
            fx.graph.nodes.insert(
                3,
                Node::Const {
                    value: Value::Bool(true),
                },
            );
            fx.graph.nodes.insert(
                4,
                Node::SequenceExists {
                    sequence: item_sequence(27),
                    predicate: 3,
                },
            );
            let targets = [named_scope("Before", 23), named_scope("After", 24)];
            let primary = item_scope(22);
            let primary_targets = [
                named_scope("Middle", 22),
                targets[0].clone(),
                targets[1].clone(),
            ];
            let inactive = inactive_target_scopes(&primary, &targets[..1], &targets[1..]);
            let failures = [FailureRule {
                iteration: FailureIteration::Sequence {
                    sequence: item_sequence(28),
                },
                selection: FailureSelection::All,
                message: None,
            }];
            let metadata_before = (
                serde_json::to_string(&fx.root_scope).unwrap(),
                serde_json::to_string(&targets).unwrap(),
                serde_json::to_string(&primary).unwrap(),
                serde_json::to_string(&failures).unwrap(),
                serde_json::to_string(&fx.graph.nodes[&4]).unwrap(),
            );
            let mut output = 0;
            let mut viewer = fx.viewer();
            if named {
                viewer.inactive_target_scopes = &inactive;
            } else {
                viewer.extra_targets = &primary_targets;
            }
            viewer.project_references = ProjectGraphReferences::new(&failures, &[]);
            viewer.function_output = isolated.then_some(&mut output);
            let owned = viewer.owned_item_ids();
            if isolated {
                assert!(owned.is_empty());
            } else {
                assert_eq!(owned, (21..=28).collect());
            }
            let (created, _) = viewer
                .insert_palette_node(&mut snarl, egui::Pos2::ZERO, NodeTemplate::If)
                .unwrap();
            assert_eq!(created, if isolated { 24 } else { 32 });
            drop(viewer);
            assert_eq!(
                metadata_before,
                (
                    serde_json::to_string(&fx.root_scope).unwrap(),
                    serde_json::to_string(&targets).unwrap(),
                    serde_json::to_string(&primary).unwrap(),
                    serde_json::to_string(&failures).unwrap(),
                    serde_json::to_string(&fx.graph.nodes[&4]).unwrap()
                )
            );
            if !isolated {
                assert!((21..=28).all(|id| !fx.graph.nodes.contains_key(&id)));
            }
            assert_no_placeholders(&snarl);
            assert_output(&fx, Some("KEEP"), false);
        }
    }
}

#[test]
fn staged_function_and_aggregate_edits_preserve_prior_properties_on_exhaustion() {
    let (mut fx, snarl) = live_fixture(u32::MAX - 2);
    let before = state(&fx, &snarl);
    let edited = Node::Call {
        function: "matches".into(),
        args: Vec::new(),
    };
    assert!(
        fx.viewer()
            .commit_node_property_edit(0, edited.clone(), true, true)
            .is_err()
    );
    assert_eq!(
        state(&fx, &snarl),
        before,
        "two partial inputs must never be inserted"
    );
    assert_output(&fx, Some("KEEP"), false);
    assert_eq!(
        fx.viewer()
            .commit_node_property_edit(0, edited, true, false)
            .unwrap(),
        2
    );
    assert!(
        matches!(&fx.graph.nodes[&0], Node::Call { args, .. } if args == &[u32::MAX - 1, u32::MAX])
    );

    let (mut fx, snarl) = live_fixture(u32::MAX);
    fx.graph.nodes.insert(
        10,
        Node::Aggregate {
            function: AggregateOp::Count,
            collection: vec!["Rows".into()],
            value: vec!["Value".into()],
            expression: None,
            arg: None,
        },
    );
    let before = state(&fx, &snarl);
    let edited = Node::Aggregate {
        function: AggregateOp::Join,
        collection: vec!["Edited".into()],
        value: vec!["Changed".into()],
        expression: None,
        arg: None,
    };
    assert!(
        fx.viewer()
            .commit_node_property_edit(10, edited, false, false)
            .is_err()
    );
    assert_eq!(
        state(&fx, &snarl),
        before,
        "operation and earlier path edits are atomic"
    );
    assert_output(&fx, Some("KEEP"), false);
}

#[test]
fn input_disconnect_reserves_before_wires_and_keeps_replacement_hidden() {
    for success in [false, true] {
        let (mut fx, mut snarl) = live_fixture(if success { u32::MAX - 1 } else { u32::MAX });
        let before = state(&fx, &snarl);
        let to = snarl.in_pin(InPinId {
            node: fx.call,
            input: 0,
        });
        let from = snarl.out_pin(to.remotes[0]);
        let mut viewer = fx.viewer();
        viewer.disconnect(&from, &to, &mut snarl);
        let error = viewer.error.clone();
        drop(viewer);
        if success {
            assert!(error.is_none());
            assert!(matches!(&fx.graph.nodes[&0], Node::Call { args, .. } if args == &[u32::MAX]));
            assert!(matches!(fx.graph.nodes[&u32::MAX], Node::Unconnected));
            assert_eq!(snarl.wires().count(), 1);
            assert_no_placeholders(&snarl);
            assert_output(&fx, Some("FIXED"), true);
        } else {
            assert_eq!(error.as_deref(), Some("mapping node IDs are exhausted"));
            assert_eq!(state(&fx, &snarl), before);
            assert_output(&fx, Some("KEEP"), false);
        }
    }
    let (mut fx, mut snarl) = live_fixture(u32::MAX);
    let to = snarl.in_pin(InPinId {
        node: fx.target,
        input: 0,
    });
    let from = snarl.out_pin(to.remotes[0]);
    fx.viewer().disconnect(&from, &to, &mut snarl);
    assert!(
        fx.root_scope.bindings.is_empty(),
        "target disconnection allocates no IDs"
    );
    assert_output(&fx, None, false);
}

#[test]
fn multi_consumer_removal_reserves_one_distinct_hidden_input_per_slot() {
    for success in [false, true] {
        let (mut fx, mut snarl) = live_fixture(if success { u32::MAX - 3 } else { u32::MAX - 2 });
        let first = snarl.insert_node(egui::pos2(300.0, 100.0), CanvasNode::Graph(2));
        let second = snarl.insert_node(egui::pos2(300.0, 180.0), CanvasNode::Graph(3));
        fx.graph.nodes.insert(
            2,
            Node::Call {
                function: "upper".into(),
                args: vec![0],
            },
        );
        fx.graph.nodes.insert(
            3,
            Node::Call {
                function: "concat".into(),
                args: vec![0, 0],
            },
        );
        for input in [
            InPinId {
                node: first,
                input: 0,
            },
            InPinId {
                node: second,
                input: 0,
            },
            InPinId {
                node: second,
                input: 1,
            },
        ] {
            snarl.connect(
                OutPinId {
                    node: fx.call,
                    output: 0,
                },
                input,
            );
        }
        let before = state(&fx, &snarl);
        let call = fx.call;
        let mut viewer = fx.viewer();
        assert_eq!(viewer.remove_graph_node(0, call, &mut snarl), success);
        let error = viewer.error.clone();
        drop(viewer);
        if success {
            assert!(error.is_none());
            assert!(fx.root_scope.bindings.is_empty());
            assert!(
                matches!(&fx.graph.nodes[&2], Node::Call { args, .. } if args == &[u32::MAX - 2])
            );
            assert!(
                matches!(&fx.graph.nodes[&3], Node::Call { args, .. } if args == &[u32::MAX - 1, u32::MAX])
            );
            assert_eq!(snarl.wires().count(), 0);
            assert_no_placeholders(&snarl);
            fx.root_scope.bindings.push(Binding {
                target_field: "out".into(),
                node: 2,
            });
            assert_output(&fx, Some("FIXED"), true);
        } else {
            assert_eq!(error.as_deref(), Some("mapping node IDs are exhausted"));
            assert_eq!(state(&fx, &snarl), before);
            assert_output(&fx, Some("KEEP"), false);
        }
    }
}

#[test]
fn selection_removal_is_atomic_on_later_exhaustion_and_retains_blocked_subset_behavior() {
    for blocked in [false, true] {
        for success in [false, true] {
            let needed = if blocked { 1 } else { 2 };
            let maximum = if success {
                u32::MAX - needed
            } else {
                u32::MAX - needed + 1
            };
            let (mut fx, mut snarl) = live_fixture(maximum);
            let first = snarl
                .node_ids()
                .find_map(|(id, node)| (*node == CanvasNode::Graph(1)).then_some(id))
                .unwrap();
            let second = snarl.insert_node(egui::pos2(80.0, 140.0), CanvasNode::Graph(5));
            let consumer = snarl.insert_node(egui::pos2(200.0, 140.0), CanvasNode::Graph(6));
            fx.graph.nodes.insert(
                5,
                if blocked {
                    Node::Const {
                        value: Value::Bool(true),
                    }
                } else {
                    constant("second")
                },
            );
            fx.graph.nodes.insert(
                6,
                Node::Call {
                    function: "concat".into(),
                    args: vec![5],
                },
            );
            snarl.connect(
                OutPinId {
                    node: second,
                    output: 0,
                },
                InPinId {
                    node: consumer,
                    input: 0,
                },
            );
            if blocked {
                fx.root_scope.filter = Some(5);
            }
            let selected = [first, second];
            let before = state(&fx, &snarl);
            let mut viewer = fx.viewer();
            let removed = viewer.remove_snarl_nodes(&selected, &mut snarl);
            let error = viewer.error.clone();
            drop(viewer);
            if success {
                assert_eq!(removed, needed as usize);
                assert_eq!(fx.root_scope.filter, blocked.then_some(5));
                assert_eq!(fx.graph.nodes.contains_key(&5), blocked);
                assert_eq!(snarl.get_node(second).is_some(), blocked);
                if blocked {
                    assert!(error.as_deref().unwrap().contains("node(s) 5"));
                } else {
                    assert!(error.is_none());
                }
                assert_no_placeholders(&snarl);
                assert_output(&fx, Some("FIXED"), true);
            } else {
                assert_eq!(removed, 0);
                assert_eq!(error.as_deref(), Some("mapping node IDs are exhausted"));
                assert_eq!(state(&fx, &snarl), before);
                assert!(selected.iter().all(|id| snarl.get_node(*id).is_some()));
                assert_output(&fx, Some("KEEP"), false);
            }
        }
    }
}

fn output_frame(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> (egui::FullOutput, Option<String>) {
    let call = fx.call;
    let pin = snarl.out_pin(OutPinId {
        node: call,
        output: 0,
    });
    let mut error = None;
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let mut viewer = fx.viewer();
            viewer.show_output(&pin, ui, snarl);
            error = viewer.error;
        },
    );
    (output, error)
}
fn text_position(output: &egui::FullOutput, label: &str) -> egui::Pos2 {
    fn find(shape: &egui::epaint::Shape, label: &str) -> Option<egui::Pos2> {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == label => {
                Some(text.pos + text.galley.size() * 0.5)
            }
            egui::epaint::Shape::Vec(shapes) => shapes.iter().find_map(|shape| find(shape, label)),
            _ => None,
        }
    }
    output
        .shapes
        .iter()
        .find_map(|shape| find(&shape.shape, label))
        .unwrap_or_else(|| panic!("missing rendered {label}"))
}

fn settled_output(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
) -> egui::FullOutput {
    let mut output = output_frame(fx, snarl, context, Vec::new()).0;
    for _ in 0..3 {
        output = output_frame(fx, snarl, context, Vec::new()).0;
    }
    output
}
fn click_output(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    position: egui::Pos2,
) -> (egui::FullOutput, Option<String>) {
    output_frame(
        fx,
        snarl,
        context,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    );
    output_frame(
        fx,
        snarl,
        context,
        vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            },
        ],
    )
}

#[test]
fn real_argument_button_reports_exhaustion_without_changing_the_wired_node() {
    for success in [false, true] {
        let (mut fx, mut snarl) = live_fixture(if success { u32::MAX - 1 } else { u32::MAX });
        fx.graph.nodes.insert(
            0,
            Node::Call {
                function: "concat".into(),
                args: vec![1],
            },
        );
        let context = egui::Context::default();
        crate::icons::install(&context);
        let output = settled_output(&mut fx, &mut snarl, &context);
        let position = text_position(&output, "+arg");
        let before = state(&fx, &snarl);
        let (_, error) = click_output(&mut fx, &mut snarl, &context, position);
        if success {
            assert!(error.is_none());
            assert!(
                matches!(&fx.graph.nodes[&0], Node::Call { args, .. } if args == &[1, u32::MAX])
            );
            assert_eq!(snarl.wires().count(), 2);
            assert_no_placeholders(&snarl);
            assert_output(&fx, Some("keepfixed"), true);
        } else {
            assert_eq!(error.as_deref(), Some("mapping node IDs are exhausted"));
            assert_eq!(state(&fx, &snarl), before);
            assert_output(&fx, Some("keep"), false);
        }
    }
}

#[test]
fn real_function_selector_preserves_old_function_when_its_minimum_input_cannot_fit() {
    for success in [false, true] {
        let (mut fx, mut snarl) = live_fixture(if success { u32::MAX - 1 } else { u32::MAX });
        fx.graph.nodes.insert(
            0,
            Node::Call {
                function: "concat".into(),
                args: Vec::new(),
            },
        );
        fx.graph.nodes.insert(
            2,
            Node::Call {
                function: "upper".into(),
                args: vec![1],
            },
        );
        let keeper = snarl.insert_node(egui::pos2(300.0, 200.0), CanvasNode::Graph(2));
        let old_input = snarl.in_pin(InPinId {
            node: fx.call,
            input: 0,
        });
        snarl.disconnect(old_input.remotes[0], old_input.id);
        snarl.connect(
            old_input.remotes[0],
            InPinId {
                node: keeper,
                input: 0,
            },
        );
        let target = InPinId {
            node: fx.target,
            input: 0,
        };
        for remote in snarl.in_pin(target).remotes {
            snarl.disconnect(remote, target);
        }
        snarl.connect(
            OutPinId {
                node: keeper,
                output: 0,
            },
            target,
        );
        fx.root_scope.bindings[0].node = 2;
        let context = egui::Context::default();
        crate::icons::install(&context);
        let output = settled_output(&mut fx, &mut snarl, &context);
        let before = state(&fx, &snarl);
        click_output(
            &mut fx,
            &mut snarl,
            &context,
            text_position(&output, "Concat"),
        );
        let menu = settled_output(&mut fx, &mut snarl, &context);
        let position = text_position(&menu, "Uppercase");
        let (_, error) = click_output(&mut fx, &mut snarl, &context, position);
        if success {
            assert!(error.is_none());
            assert!(
                matches!(&fx.graph.nodes[&0], Node::Call { function, args } if function == "upper" && args == &[u32::MAX])
            );
            assert_no_placeholders(&snarl);
            let from = snarl.out_pin(OutPinId {
                node: fx.call,
                output: 0,
            });
            let to = snarl.in_pin(InPinId {
                node: fx.target,
                input: 0,
            });
            fx.viewer().connect(&from, &to, &mut snarl);
            assert_output(&fx, Some("FIXED"), true);
        } else {
            assert_eq!(error.as_deref(), Some("mapping node IDs are exhausted"));
            assert_eq!(state(&fx, &snarl), before);
            assert_output(&fx, Some("KEEP"), false);
        }
    }
}

fn palette_frame(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    events: Vec<egui::Event>,
) -> Option<String> {
    let mut error = None;
    let _ = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 700.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            let mut viewer = fx.viewer();
            viewer.show_graph_menu(egui::pos2(250.0, 200.0), ui, snarl);
            error = viewer.error;
        },
    );
    error
}

#[test]
fn real_palette_keyboard_action_is_atomic_and_surfaces_allocation_error() {
    for success in [false, true] {
        let (mut fx, mut snarl) = live_fixture(if success { u32::MAX - 4 } else { u32::MAX - 3 });
        let context = egui::Context::default();
        crate::icons::install(&context);
        for _ in 0..4 {
            palette_frame(&mut fx, &mut snarl, &context, Vec::new());
        }
        palette_frame(
            &mut fx,
            &mut snarl,
            &context,
            vec![egui::Event::Text("condition then else".into())],
        );
        let before = state(&fx, &snarl);
        let error = palette_frame(
            &mut fx,
            &mut snarl,
            &context,
            vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        if success {
            assert!(error.is_none());
            assert!(
                matches!(&fx.graph.nodes[&u32::MAX], Node::If { condition, then, else_ }
                if (*condition, *then, *else_) == (u32::MAX - 3, u32::MAX - 2, u32::MAX - 1))
            );
            assert_eq!(snarl.nodes().count(), 5);
            assert_eq!(snarl.wires().count(), 2);
            assert_no_placeholders(&snarl);
        } else {
            assert_eq!(error.as_deref(), Some("mapping node IDs are exhausted"));
            assert_eq!(state(&fx, &snarl), before);
        }
        assert_output(&fx, Some("KEEP"), false);
    }
}

fn rows_schema() -> SchemaNode {
    SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    )
}
fn assert_aggregate_output(fx: &Fixture, joined: bool) {
    let mut graph = fx.graph.clone();
    if joined {
        for node in graph.nodes.values_mut() {
            if matches!(node, Node::Unconnected) {
                *node = constant(",");
            }
        }
    }
    let domain = ir::ScalarTypeSet::new([ScalarType::Int, ScalarType::String]).unwrap();
    let project = Project {
        source: rows_schema(),
        target: SchemaNode::group("row", vec![SchemaNode::scalar_union("out", domain)]),
        graph,
        root: fx.root_scope.clone(),
        ..crate::new_mapping::blank_project()
    };
    assert!(engine::validate(&project).is_empty());
    let source = Instance::Group(
        (vec![(
            "Rows".into(),
            Instance::Repeated(
                ["a", "b"]
                    .into_iter()
                    .map(|value| {
                        Instance::Group(
                            (vec![(
                                "Value".into(),
                                Instance::Scalar(Value::String(value.into())),
                            )])
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )])
        .into(),
    );
    let expected = if joined {
        Value::String("a,b".into())
    } else {
        Value::Int(2)
    };
    assert_eq!(
        engine::run(&project, &source).unwrap(),
        Instance::Group((vec![("out".into(), Instance::Scalar(expected))]).into())
    );
}

#[test]
fn real_aggregate_selector_preserves_operation_paths_and_wires_on_exhaustion() {
    for success in [false, true] {
        let mut fx = fixture();
        fx.graph.nodes = [
            (
                0,
                Node::Aggregate {
                    function: AggregateOp::Count,
                    collection: vec!["Rows".into()],
                    value: vec!["Value".into()],
                    expression: None,
                    arg: None,
                },
            ),
            (
                if success { u32::MAX - 1 } else { u32::MAX },
                constant("unrelated"),
            ),
        ]
        .into_iter()
        .collect();
        fx.root_scope.bindings.push(Binding {
            target_field: "out".into(),
            node: 0,
        });
        fx.source_paths = SourcePathCatalog::new(&rows_schema(), &[]);
        let mut snarl = std::mem::take(&mut fx.snarl);
        snarl.connect(
            OutPinId {
                node: fx.call,
                output: 0,
            },
            InPinId {
                node: fx.target,
                input: 0,
            },
        );
        assert_aggregate_output(&fx, false);
        let context = egui::Context::default();
        crate::icons::install(&context);
        let output = settled_output(&mut fx, &mut snarl, &context);
        let before = state(&fx, &snarl);
        click_output(
            &mut fx,
            &mut snarl,
            &context,
            text_position(&output, "Count"),
        );
        let menu = settled_output(&mut fx, &mut snarl, &context);
        let (_, error) = click_output(
            &mut fx,
            &mut snarl,
            &context,
            text_position(&menu, "String join"),
        );
        if success {
            assert!(error.is_none());
            assert!(
                matches!(&fx.graph.nodes[&0], Node::Aggregate { function: AggregateOp::Join,
                collection, value, arg: Some(arg), expression: None }
                if collection == &["Rows"] && value == &["Value"] && *arg == u32::MAX)
            );
            assert_eq!(snarl.wires().count(), 1);
            assert_no_placeholders(&snarl);
            assert_aggregate_output(&fx, true);
        } else {
            assert_eq!(error.as_deref(), Some("mapping node IDs are exhausted"));
            assert_eq!(state(&fx, &snarl), before);
            assert_aggregate_output(&fx, false);
        }
    }
}

fn assert_mixed_output(fx: &Fixture, expected: &str, repair_inputs: bool) {
    let mut graph = fx.graph.clone();
    if repair_inputs {
        for node in graph.nodes.values_mut() {
            if matches!(node, Node::Unconnected) {
                *node = constant("fixed");
            }
        }
    }
    let project = Project {
        source: SchemaNode::group(
            "Description",
            vec![
                SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
                SchemaNode::scalar("Bold", ScalarType::String).repeating(),
                SchemaNode::scalar("Italic", ScalarType::String).repeating(),
                SchemaNode::scalar("Plain", ScalarType::String).repeating(),
            ],
        ),
        target: SchemaNode::group("row", vec![SchemaNode::scalar("out", ScalarType::String)]),
        graph,
        root: fx.root_scope.clone(),
        ..crate::new_mapping::blank_project()
    };
    assert!(engine::validate(&project).is_empty());
    let input = cli::PayloadDocument::new(
        std::path::Path::new("input.xml"),
        b"<Description>pre<Bold>B</Bold>|<Italic>I</Italic><Plain>raw</Plain>post</Description>",
    )
    .unwrap();
    let output = cli::run_project_value_payloads(
        &project,
        std::path::Path::new("/tmp/ferrule-allocation-mixed/project.json"),
        &cli::PayloadRunOptions::new(input).with_output_path(std::path::Path::new("result.json")),
    )
    .unwrap();
    assert_eq!(output.artifacts.len(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.artifacts[0].bytes).unwrap(),
        serde_json::json!({ "out": expected })
    );
}

#[test]
fn mixed_content_disconnect_and_removal_update_exact_slots_or_preserve_the_whole_action() {
    for remove in [false, true] {
        let needed = if remove { 2 } else { 1 };
        for maximum in [20, u32::MAX - needed, u32::MAX - needed + 1] {
            let success = maximum != u32::MAX - needed + 1;
            let (mut fx, mut snarl) = live_fixture(maximum);
            fx.graph.nodes.insert(
                0,
                Node::XmlMixedContent {
                    path: Vec::new(),
                    frame: None,
                    replacements: vec![
                        mapping::XmlMixedContentReplacement {
                            element: "Bold".into(),
                            collection: vec!["Bold".into()],
                            expression: 1,
                        },
                        mapping::XmlMixedContentReplacement {
                            element: "Italic".into(),
                            collection: vec!["Italic".into()],
                            expression: 1,
                        },
                    ],
                },
            );
            let expression = snarl
                .node_ids()
                .find_map(|(id, node)| (*node == CanvasNode::Graph(1)).then_some(id))
                .unwrap();
            let from = OutPinId {
                node: expression,
                output: 0,
            };
            let to = InPinId {
                node: fx.call,
                input: 1,
            };
            snarl.connect(from, to);
            let before = state(&fx, &snarl);
            let metadata = |node: &Node| match node {
                Node::XmlMixedContent {
                    path,
                    frame,
                    replacements,
                } => (
                    path.clone(),
                    frame.clone(),
                    replacements
                        .iter()
                        .map(|replacement| {
                            (replacement.element.clone(), replacement.collection.clone())
                        })
                        .collect::<Vec<_>>(),
                ),
                _ => panic!("mixed content node expected"),
            };
            let before_metadata = metadata(&fx.graph.nodes[&0]);
            assert_mixed_output(&fx, "prekeep|keeprawpost", false);
            let mut viewer = fx.viewer();
            if remove {
                assert_eq!(viewer.remove_graph_node(1, expression, &mut snarl), success);
            } else {
                viewer.disconnect(&snarl.out_pin(from), &snarl.in_pin(to), &mut snarl);
            }
            let error = viewer.error.clone();
            drop(viewer);
            assert_eq!(metadata(&fx.graph.nodes[&0]), before_metadata);
            if success {
                assert!(error.is_none());
                let replacements = match &fx.graph.nodes[&0] {
                    Node::XmlMixedContent { replacements, .. } => replacements,
                    _ => unreachable!(),
                };
                assert_eq!(
                    replacements[0].expression,
                    if remove { maximum + 1 } else { 1 }
                );
                assert_eq!(replacements[1].expression, maximum + needed);
                assert_eq!(snarl.wires().count(), if remove { 1 } else { 2 });
                assert_eq!(fx.graph.nodes.contains_key(&1), !remove);
                assert_no_placeholders(&snarl);
                assert_mixed_output(
                    &fx,
                    if remove {
                        "prefixed|fixedrawpost"
                    } else {
                        "prekeep|fixedrawpost"
                    },
                    true,
                );
            } else {
                assert_eq!(error.as_deref(), Some("mapping node IDs are exhausted"));
                assert_eq!(state(&fx, &snarl), before);
                assert_mixed_output(&fx, "prekeep|keeprawpost", false);
            }
        }
    }
}
