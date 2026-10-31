//! Potential-context admission at the real property and wire controls.
use super::*;
use mapping::{AggregateOp, ScopeIteration, SequenceExpr};

pub(super) fn observed(app: &FerruleApp) -> serde_json::Value {
    let wires = [&app.main_canvas, &app.mapping_workspace.target_canvases[&0]].map(|canvas| {
        let mut wires = canvas
            .snarl
            .wires()
            .map(|(from, to)| {
                format!(
                    "{:?}:{}->{:?}:{}",
                    canvas.snarl[from.node], from.output, canvas.snarl[to.node], to.input
                )
            })
            .collect::<Vec<_>>();
        wires.sort();
        wires
    });
    serde_json::json!({"state": snapshot(app), "wires": wires,
        "undo": app.history.undo_len(), "redo": app.history.redo_len()})
}

fn legacy() -> Node {
    Node::Lookup {
        collection: vec!["Rows".into()],
        key: vec!["Code".into()],
        matches: 3,
        value: vec!["PrimaryValue".into()],
    }
}
fn owned() -> Node {
    Node::OwnedLookup(OwnedLookup {
        collection: source(SourceOwner::Primary, "Rows"),
        key: vec!["Code".into()],
        matches: 3,
        value: vec!["PrimaryValue".into()],
    })
}
fn string(value: &str) -> Node {
    Node::Const {
        value: Value::String(value.into()),
    }
}
fn call(input: NodeId) -> Node {
    Node::Call {
        function: "concat".into(),
        args: vec![input],
    }
}
fn binding(node: NodeId) -> Binding {
    Binding {
        target_field: "Result".into(),
        node,
    }
}

pub(super) fn run(app: &FerruleApp, input: &str) -> engine::ExecutionOutputs {
    assert!(
        engine::validate(&app.project).is_empty(),
        "{:?}",
        engine::validate(&app.project)
    );
    engine::check_source_ownership_capability(&app.project, None).unwrap();
    engine::run_outputs_with_sources_and_context(
        &app.project,
        &format_json::from_str(input, &app.project.source).unwrap(),
        vec![(
            "Ref".into(),
            format_json::from_str(NAMED, &app.project.extra_sources[0].schema).unwrap(),
        )],
        &engine::ExecutionContext::new(Path::new("lookup-contexts.json")),
    )
    .unwrap()
}
fn expected_string(value: &str) -> ir::Instance {
    ir::Instance::Group(
        vec![(
            "Result".into(),
            ir::Instance::Scalar(Value::String(value.into())),
        )]
        .into(),
    )
}

fn assert_source_needle_wire(app: &FerruleApp) {
    let snarl = &app.main_canvas.snarl;
    let incoming = snarl
        .wires()
        .filter(|(_, to)| snarl[to.node] == CanvasNode::Graph(7))
        .map(|(from, to)| (snarl[from.node], from.output, to.input))
        .collect::<Vec<_>>();
    assert_eq!(incoming, [(CanvasNode::SourceBlock(0), 0, 0)]);
}

fn install_root_needle(app: &mut FerruleApp) -> String {
    if let ir::SchemaKind::Group { children, .. } = &mut app.project.source.kind {
        children.insert(0, SchemaNode::scalar("Needle", ScalarType::String));
    }
    app.project.graph.nodes.insert(
        4,
        Node::SourceField {
            path: vec!["Needle".into()],
            frame: None,
        },
    );
    if let Some(Node::OwnedLookup(node)) = app.project.graph.nodes.get_mut(&7) {
        node.matches = 4;
    }
    let mut input: serde_json::Value = serde_json::from_str(INPUT).unwrap();
    input["Needle"] = serde_json::json!("hit");
    input.to_string()
}
pub(super) fn open_legacy(app: &mut FerruleApp, ctx: &egui::Context) {
    let output = settle(app, ctx, true);
    let title = visible(&output, "Rows")
        .into_iter()
        .min_by(|a, b| a.y.total_cmp(&b.y))
        .unwrap();
    frame(app, ctx, true, vec![egui::Event::PointerMoved(title)]);
    let output = settle(app, ctx, true);
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    let pos = visible(&output, &pencil)
        .into_iter()
        .min_by(|a, b| a.distance(title).total_cmp(&b.distance(title)))
        .unwrap();
    pointer(app, ctx, true, pos, egui::PointerButton::Primary);
    settle(app, ctx, true);
}

#[test]
fn legacy_parent_inputs_and_unselected_shared_branches_cannot_convert_to_owned() {
    for kind in [
        "lookup-needle",
        "aggregate-argument",
        "dynamic-key",
        "shared-if-branch",
    ] {
        let mut app = fixture(MappingDocument::Main);
        app.project.graph.nodes.insert(7, legacy());
        let mut input = INPUT.to_owned();
        let expected = match kind {
            "aggregate-argument" => {
                app.project.graph.nodes.insert(
                    9,
                    Node::Aggregate {
                        function: AggregateOp::Join,
                        collection: vec!["Rows".into()],
                        value: vec!["PrimaryValue".into()],
                        expression: None,
                        arg: Some(7),
                    },
                );
                "primary-firstprimary-firstprimary-laterprimary-firstprimary-only"
            }
            "dynamic-key" => {
                let open = SchemaNode::group("Open", vec![])
                    .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
                    .unwrap();
                if let ir::SchemaKind::Group { children, .. } = &mut app.project.source.kind {
                    children.push(open);
                }
                let mut json: serde_json::Value = serde_json::from_str(INPUT).unwrap();
                json["Open"] = serde_json::json!({"primary-first": "dynamic-found"});
                input = json.to_string();
                app.project.graph.nodes.insert(
                    9,
                    Node::DynamicSourceField {
                        object: vec!["Open".into()],
                        frame: None,
                        key: 7,
                    },
                );
                "dynamic-found"
            }
            _ => {
                app.project.graph.nodes.insert(
                    9,
                    Node::Lookup {
                        collection: vec!["Rows".into()],
                        key: vec!["PrimaryValue".into()],
                        matches: 7,
                        value: vec!["PrimaryValue".into()],
                    },
                );
                "primary-first"
            }
        };
        let output = if kind == "shared-if-branch" {
            app.project.graph.nodes.insert(
                8,
                Node::Const {
                    value: Value::Bool(true),
                },
            );
            app.project.graph.nodes.insert(12, call(7));
            if let Some(Node::Lookup { matches, .. }) = app.project.graph.nodes.get_mut(&9) {
                *matches = 12;
            }
            app.project.graph.nodes.insert(
                10,
                Node::If {
                    condition: 8,
                    then: 7,
                    else_: 9,
                },
            );
            10
        } else {
            9
        };
        app.project.root.bindings = vec![binding(output)];
        app.project.extra_targets[0].root = app.project.root.clone();
        rebuild(&mut app, MappingDocument::Main);
        // Keep the edited inner Lookup visible independently of its consumers.
        for document in [MappingDocument::Main, MappingDocument::Target(0)] {
            let canvas = canvas_mut(&mut app, document);
            let nodes = canvas
                .snarl
                .node_ids()
                .map(|(id, node)| (id, *node))
                .collect::<Vec<_>>();
            for (id, node) in nodes {
                if let CanvasNode::Graph(node_id) = node
                    && ![3, 7].contains(&node_id)
                {
                    canvas.snarl.get_node_info_mut(id).unwrap().pos =
                        egui::pos2(920.0, 80.0 + node_id as f32 * 35.0);
                }
            }
        }
        let result = run(&app, &input);
        assert_eq!(result.primary, expected_string(expected), "{kind}");
        assert_eq!(result.extras[0].instance, result.primary);
        let mut counterfactual = app.project.clone();
        counterfactual.graph.nodes.insert(7, owned());
        assert!(
            matches!(
                engine::check_source_ownership_capability(&counterfactual, None),
                Err(engine::EngineError::UnsupportedOwnedContext { .. })
            ),
            "{kind}"
        );
        let ctx = context();
        open_legacy(&mut app, &ctx);
        app.mark_clean();
        app.rebase_history();
        let before = observed(&app);
        click_nth(&mut app, &ctx, true, "Use an exact input", 0);
        assert_eq!(observed(&app), before, "{kind}");
        assert!(matches!(
            app.project.graph.nodes[&7],
            Node::Lookup { matches: 3, .. }
        ));
        assert_eq!(run(&app, &input).primary, expected_string(expected));
    }
}

pub(super) fn connection_fixture(kind: &str) -> FerruleApp {
    let mut app = fixture(MappingDocument::Main);
    app.project.graph.nodes.remove(&7);
    app.project.graph.nodes.insert(10, call(3));
    app.project.graph.nodes.insert(
        11,
        Node::If {
            condition: 30,
            then: 3,
            else_: 3,
        },
    );
    app.project.graph.nodes.insert(
        20,
        Node::SourceField {
            path: vec![],
            frame: None,
        },
    );
    app.project.graph.nodes.insert(
        21,
        Node::Const {
            value: Value::Int(1),
        },
    );
    app.project.graph.nodes.insert(
        30,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    app.project.graph.nodes.insert(31, string("extra"));
    let value_schema = || {
        SchemaNode::group(
            "Value",
            vec![SchemaNode::scalar("Result", ScalarType::String)],
        )
    };
    let mut rejected_schema = value_schema();
    rejected_schema.name = "Rejected".into();
    let mut rejected = Scope {
        target_field: "Rejected".into(),
        bindings: vec![binding(3)],
        ..Default::default()
    };
    match kind {
        "generated" => {
            rejected_schema.repeating = true;
            rejected.iteration = ScopeIteration::Sequence(SequenceExpr::Generate {
                item: 20,
                from: Some(21),
                to: 21,
            });
        }
        "concatenate" => {
            rejected_schema.repeating = true;
            rejected.bindings.clear();
            rejected.iteration = ScopeIteration::Concatenate(mapping::ScopeSequence::new(
                Scope {
                    bindings: vec![binding(3)],
                    ..Default::default()
                },
                vec![],
            ));
        }
        "dynamic" => {
            rejected_schema = rejected_schema
                .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
                .unwrap();
            rejected
                .dynamic_bindings
                .push(mapping::DynamicBinding { key: 31, value: 3 });
        }
        "presence" => {
            rejected_schema.repeating = true;
            rejected.iteration = ScopeIteration::OptionalPresence { condition: 30 };
        }
        _ => panic!("context"),
    }
    let mut allowed = value_schema();
    allowed.name = "Allowed".into();
    app.project.target = SchemaNode::group("Output", vec![allowed, rejected_schema]);
    app.project.root = Scope {
        children: vec![
            Scope {
                target_field: "Allowed".into(),
                bindings: vec![binding(3)],
                ..Default::default()
            },
            rejected,
        ],
        ..Default::default()
    };
    app.project.extra_targets[0].schema = app.project.target.clone();
    app.project.extra_targets[0].root = app.project.root.clone();
    if let ScopeIteration::Sequence(SequenceExpr::Generate { item, .. }) =
        &mut app.project.extra_targets[0].root.children[1].iteration
    {
        *item = 40;
        app.project.graph.nodes.insert(
            40,
            Node::SourceField {
                path: vec![],
                frame: None,
            },
        );
    }
    rebuild(&mut app, MappingDocument::Main);
    app
}
fn target_block(app: &FerruleApp, field: &str) -> CanvasNode {
    CanvasNode::TargetBlock(
        target_blocks(&app.project.target)
            .iter()
            .position(|block| block.chain == [field])
            .unwrap(),
    )
}
fn rejected_binding(project: &mut Project, node: NodeId) {
    let scope = &mut project.root.children[1];
    if let Some(segments) = scope.concatenated_mut() {
        segments.iter_mut().next().unwrap().bindings = vec![binding(node)];
    } else {
        scope.bindings = vec![binding(node)];
    }
}
fn create_wired_lookup(app: &mut FerruleApp, ctx: &egui::Context) -> NodeId {
    settle(app, ctx, true);
    pointer(
        app,
        ctx,
        true,
        egui::pos2(1100.0, 850.0),
        egui::PointerButton::Secondary,
    );
    click_nth(app, ctx, true, "Lookup from one input", 0);
    let id = app
        .project
        .graph
        .nodes
        .iter()
        .find_map(|(&id, node)| matches!(node, Node::OwnedLookup(_)).then_some(id))
        .unwrap();
    connect_needle(app, id);
    id
}

#[test]
fn newly_created_lookup_connections_check_the_receiving_scope_and_preserve_valid_siblings() {
    for kind in ["generated", "concatenate", "dynamic", "presence"] {
        for route in ["direct", "call", "if"] {
            let mut app = connection_fixture(kind);
            let ctx = context();
            let lookup = create_wired_lookup(&mut app, &ctx);
            let output = match route {
                "call" => {
                    wire_action(
                        &mut app,
                        CanvasNode::Graph(lookup),
                        0,
                        CanvasNode::Graph(10),
                        0,
                        false,
                    )
                    .unwrap();
                    10
                }
                "if" => {
                    wire_action(
                        &mut app,
                        CanvasNode::Graph(lookup),
                        0,
                        CanvasNode::Graph(11),
                        1,
                        false,
                    )
                    .unwrap();
                    11
                }
                _ => lookup,
            };
            run(&app, INPUT);
            let rejected = target_block(&app, "Rejected");
            let before = observed(&app);
            let error = wire_action(&mut app, CanvasNode::Graph(output), 0, rejected, 0, false)
                .unwrap_err();
            assert!(error.contains("Owned Lookup"), "{kind}/{route}: {error}");
            assert_eq!(observed(&app), before, "{kind}/{route}");
            let mut counterfactual = app.project.clone();
            rejected_binding(&mut counterfactual, output);
            assert!(
                matches!(
                    engine::check_source_ownership_capability(&counterfactual, None),
                    Err(engine::EngineError::UnsupportedOwnedContext { .. })
                ),
                "{kind}/{route}"
            );
            let allowed = target_block(&app, "Allowed");
            wire_action(&mut app, CanvasNode::Graph(output), 0, allowed, 0, false).unwrap();
            let result = run(&app, INPUT);
            assert_eq!(
                result.primary.field("Allowed").unwrap(),
                &expected_string("hit")
            );
            // The default explicit collection is Rows and its initial return is Code.
            let Some(HistoryRecord::Command(_)) = app.history.next_undo() else {
                panic!("typed connection")
            };
        }
    }
}

#[test]
fn shared_call_and_if_inputs_cannot_introduce_a_lookup_into_existing_closed_branches() {
    for kind in ["generated", "concatenate", "dynamic", "presence"] {
        for (consumer, input) in [(10, 0), (11, 1), (11, 2)] {
            let mut app = connection_fixture(kind);
            rejected_binding(&mut app.project, consumer);
            app.project.root.children[0].bindings = vec![binding(consumer)];
            rebuild(&mut app, MappingDocument::Main);
            let ctx = context();
            let lookup = create_wired_lookup(&mut app, &ctx);
            run(&app, INPUT);
            let before = observed(&app);
            assert!(
                wire_action(
                    &mut app,
                    CanvasNode::Graph(lookup),
                    0,
                    CanvasNode::Graph(consumer),
                    input,
                    false
                )
                .is_err()
            );
            assert_eq!(observed(&app), before, "{kind}/{consumer}/{input}");
            run(&app, INPUT);
        }
    }
}

#[test]
fn readonly_lookup_needle_connect_and_disconnect_preserve_orphan_fields_and_history() {
    let mut app = fixture(MappingDocument::Main);
    let input = install_root_needle(&mut app);
    assert_eq!(run(&app, &input).primary, expected_string("primary-first"));
    app.project.graph.nodes.insert(
        9,
        Node::Aggregate {
            function: AggregateOp::Join,
            collection: vec!["Rows".into()],
            value: vec![],
            expression: Some(7),
            arg: Some(3),
        },
    );
    app.project.root.bindings = vec![binding(9)];
    app.project.extra_targets[0].root = app.project.root.clone();
    rebuild(&mut app, MappingDocument::Main);
    assert_source_needle_wire(&app);
    assert!(matches!(
        engine::check_source_ownership_capability(&app.project, None),
        Err(engine::EngineError::UnsupportedOwnedContext { .. })
    ));
    let ctx = context();
    open_lookup(&mut app, &ctx);
    app.mark_clean();
    app.rebase_history();
    let before = observed(&app);
    click_nth(&mut app, &ctx, true, "Primary input", 0);
    assert_eq!(observed(&app), before);
    assert!(
        wire_action(
            &mut app,
            CanvasNode::Graph(3),
            0,
            CanvasNode::Graph(7),
            0,
            false
        )
        .is_err()
    );
    assert_eq!(observed(&app), before);
    assert!(
        wire_action(
            &mut app,
            CanvasNode::SourceBlock(0),
            0,
            CanvasNode::Graph(7),
            0,
            true
        )
        .is_err()
    );
    assert_eq!(observed(&app), before);
    assert!(app.project.graph.nodes.contains_key(&4));
    app.project.graph.nodes.remove(&9);
    app.project.root.bindings = vec![binding(7)];
    app.project.extra_targets[0].root = app.project.root.clone();
    rebuild(&mut app, MappingDocument::Main);
    wire_action(
        &mut app,
        CanvasNode::Graph(3),
        0,
        CanvasNode::Graph(7),
        0,
        false,
    )
    .unwrap();
    assert_eq!(lookup(&app).matches, 3);
    assert!(!app.project.graph.nodes.contains_key(&4));
    assert_eq!(run(&app, INPUT).primary, expected_string("primary-first"));
}

fn actual_pin(
    app: &FerruleApp,
    ctx: &egui::Context,
    wanted: CanvasNode,
    output: bool,
    index: usize,
) -> egui::Pos2 {
    let snarl = &app.main_canvas.snarl;
    let id = snarl
        .node_ids()
        .find_map(|(id, node)| (*node == wanted).then_some(id))
        .unwrap();
    let ids = crate::canvas_keyboard::current_pin_interaction_ids(
        ctx,
        app.embedded_canvas_id(egui::Id::unique("main_mapping_canvas")),
        app.main_canvas.view_generation,
    );
    let responses = ids
        .into_iter()
        .filter_map(|id| ctx.read_response(id))
        .collect::<Vec<_>>();
    let bounds = responses
        .iter()
        .find_map(|pin| ctx.read_response(pin.layer_id.id.with(("snarl-node", id)).with("frame")))
        .unwrap();
    let bounds = ctx
        .layer_transform_to_global(bounds.layer_id)
        .unwrap_or_default()
        * bounds.rect;
    let mut pins = responses
        .into_iter()
        .filter_map(|response| {
            let point = ctx
                .layer_transform_to_global(response.layer_id)
                .unwrap_or_default()
                * response.rect.center();
            let edge = if output {
                bounds.right()
            } else {
                bounds.left()
            };
            (response.sense.senses_drag()
                && bounds.expand(16.0).contains(point)
                && (point.x - edge).abs() < 20.0)
                .then_some(point)
        })
        .collect::<Vec<_>>();
    pins.sort_by(|a, b| a.y.total_cmp(&b.y));
    pins[index]
}

#[test]
fn locked_native_needle_drag_and_wire_disconnect_are_noops() {
    let mut app = fixture(MappingDocument::Main);
    let input = install_root_needle(&mut app);
    rebuild(&mut app, MappingDocument::Main);
    assert_source_needle_wire(&app);
    let ctx = context();
    settle(&mut app, &ctx, true);
    let from = actual_pin(&app, &ctx, CanvasNode::Graph(3), true, 0);
    let to = actual_pin(&app, &ctx, CanvasNode::Graph(7), false, 0);
    let source = actual_pin(&app, &ctx, CanvasNode::SourceBlock(0), true, 0);
    app.mark_clean();
    app.rebase_history();
    let before = observed(&app);
    for (point, pressed) in [
        (from, true),
        (from.lerp(to, 0.5), true),
        (to, true),
        (to, false),
    ] {
        frame(
            &mut app,
            &ctx,
            false,
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
    }
    pointer(
        &mut app,
        &ctx,
        false,
        source.lerp(to, 0.5),
        egui::PointerButton::Secondary,
    );
    assert_eq!(observed(&app), before);
    assert!(app.project.graph.nodes.contains_key(&4));
    assert_eq!(lookup(&app).matches, 4);
    assert_eq!(run(&app, &input).primary, expected_string("primary-first"));
}
