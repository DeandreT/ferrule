use super::*;
use ir::Instance;
use mapping::{Project, ScopeIteration, SequenceExpr};

fn rows() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Price", ScalarType::Int),
                    SchemaNode::scalar("Quantity", ScalarType::Int),
                ],
            )
            .repeating(),
        ],
    )
}

fn sum_fixture() -> (Fixture, Snarl<CanvasNode>) {
    let mut fx = fixture();
    fx.source_paths = SourcePathCatalog::new(&rows(), &[]);
    fx.source_blocks = source_blocks(&rows());
    fx.graph.nodes = [
        (
            0,
            Node::Aggregate {
                function: AggregateOp::Sum,
                collection: vec!["Rows".into()],
                value: vec!["Price".into()],
                expression: None,
                arg: None,
            },
        ),
        (
            1,
            Node::SourceField {
                path: vec!["Price".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
        (
            2,
            Node::SourceField {
                path: vec!["Quantity".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
        (
            3,
            Node::Call {
                function: "multiply".into(),
                args: vec![1, 2],
            },
        ),
        (
            4,
            Node::Call {
                function: "floor".into(),
                args: vec![3],
            },
        ),
    ]
    .into_iter()
    .collect();
    fx.root_scope.bindings = vec![Binding {
        target_field: "out".into(),
        node: 0,
    }];
    let mut snarl = std::mem::take(&mut fx.snarl);
    let expression = snarl.insert_node(egui::pos2(150.0, 200.0), CanvasNode::Graph(3));
    let other = snarl.insert_node(egui::pos2(400.0, 200.0), CanvasNode::Graph(4));
    snarl.connect(
        OutPinId {
            node: expression,
            output: 0,
        },
        InPinId {
            node: other,
            input: 0,
        },
    );
    (fx, snarl)
}

fn raw(fx: &Fixture, snarl: &Snarl<CanvasNode>) -> (serde_json::Value, String) {
    let state = (
        serde_json::json!({"graph": fx.graph, "scope": fx.root_scope}),
        format!("{snarl:?}"),
    );
    eprintln!("Computed aggregate original state={state:#?}");
    state
}

fn observed_labels(output: &egui::FullOutput, wanted: &str) -> Vec<egui::Pos2> {
    fn visit(
        shape: &egui::epaint::Shape,
        clip: egui::Rect,
        wanted: &str,
        positions: &mut Vec<egui::Pos2>,
    ) {
        match shape {
            egui::epaint::Shape::Text(text) if text.galley.text() == wanted => {
                let rect = text.visual_bounding_rect();
                if clip.contains(rect.center()) {
                    positions.push(rect.center());
                }
            }
            egui::epaint::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, clip, wanted, positions);
                }
            }
            _ => {}
        }
    }
    let mut positions = Vec::new();
    for shape in &output.shapes {
        visit(&shape.shape, shape.clip_rect, wanted, &mut positions);
    }
    positions
}

fn frame(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    enabled: bool,
    isolated: bool,
    events: Vec<egui::Event>,
) -> egui::FullOutput {
    let pin = snarl.out_pin(OutPinId {
        node: fx.call,
        output: 0,
    });
    let mut function_output = 0;
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1000.0, 700.0),
            )),
            time: Some(context.cumulative_frame_nr() as f64 / 10.0),
            events,
            ..Default::default()
        },
        |ui| {
            ui.add_enabled_ui(enabled, |ui| {
                let mut viewer = fx.viewer();
                if isolated {
                    viewer.function_output = Some(&mut function_output);
                }
                viewer.show_output(&pin, ui, snarl);
                eprintln!("Calculated properties original error={:?}", viewer.error);
            });
        },
    );
    raw(fx, snarl);
    output
}

fn settle(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    enabled: bool,
    isolated: bool,
) -> egui::FullOutput {
    let mut output = frame(fx, snarl, context, enabled, isolated, Vec::new());
    for _ in 0..3 {
        output = frame(fx, snarl, context, enabled, isolated, Vec::new());
    }
    output
}

fn click(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    enabled: bool,
    isolated: bool,
    pos: egui::Pos2,
) {
    frame(
        fx,
        snarl,
        context,
        enabled,
        isolated,
        vec![egui::Event::PointerMoved(pos)],
    );
    for pressed in [true, false] {
        frame(
            fx,
            snarl,
            context,
            enabled,
            isolated,
            vec![egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            }],
        );
    }
}

fn open(fx: &mut Fixture, snarl: &mut Snarl<CanvasNode>, context: &egui::Context, isolated: bool) {
    let output = settle(fx, snarl, context, true, isolated);
    let pencil = char::from(lucide_icons::Icon::Pencil).to_string();
    let positions = observed_labels(&output, &pencil);
    assert_eq!(positions.len(), 1, "one real aggregate pencil");
    click(fx, snarl, context, true, isolated, positions[0]);
    let output = settle(fx, snarl, context, true, isolated);
    assert_eq!(observed_labels(&output, "Calculate each value").len(), 1);
}

fn switch(
    fx: &mut Fixture,
    snarl: &mut Snarl<CanvasNode>,
    context: &egui::Context,
    enabled: bool,
    isolated: bool,
) {
    let output = settle(fx, snarl, context, enabled, isolated);
    let positions = observed_labels(&output, "Calculate each value");
    assert_eq!(positions.len(), 1);
    click(fx, snarl, context, enabled, isolated, positions[0]);
    settle(fx, snarl, context, enabled, isolated);
}

fn assert_sum(fx: &Fixture, expected: i64) {
    let source = Instance::Group(
        vec![(
            "Rows".into(),
            Instance::Repeated(
                [(3, 2), (5, 4), (7, 1)]
                    .into_iter()
                    .map(|(price, quantity)| {
                        Instance::Group(
                            vec![
                                ("Price".into(), Instance::Scalar(Value::Int(price))),
                                ("Quantity".into(), Instance::Scalar(Value::Int(quantity))),
                            ]
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )]
        .into(),
    );
    let project = Project {
        source: rows(),
        target: SchemaNode::group("Output", vec![SchemaNode::scalar("out", ScalarType::Int)]),
        graph: fx.graph.clone(),
        root: fx.root_scope.clone(),
        ..crate::new_mapping::blank_project()
    };
    let validation = engine::validate(&project);
    let output = engine::run(&project, &source);
    eprintln!(
        "Computed aggregate full original project={project:#?}; source={source:#?}; validation={validation:#?}; output={output:#?}"
    );
    assert!(validation.is_empty());
    assert_eq!(
        output.unwrap(),
        Instance::Group(vec![("out".into(), Instance::Scalar(Value::Int(expected)))].into())
    );
}

#[test]
fn calculated_sum_real_pencil_switch_and_connection_preserve_field_and_shared_expression() {
    let (mut fx, mut snarl) = sum_fixture();
    let context = egui::Context::default();
    crate::icons::install(&context);
    assert_sum(&fx, 15);
    open(&mut fx, &mut snarl, &context, false);
    let before_view = raw(&fx, &snarl);
    settle(&mut fx, &mut snarl, &context, true, false);
    assert_eq!(raw(&fx, &snarl), before_view, "viewing is not an edit");
    switch(&mut fx, &mut snarl, &context, true, false);
    let Node::Aggregate {
        expression: Some(empty),
        ref value,
        ..
    } = fx.graph.nodes[&0]
    else {
        panic!("calculated value input")
    };
    assert_eq!(value, &["Price"]);
    assert!(matches!(fx.graph.nodes[&empty], Node::Unconnected));
    assert!(
        !snarl.nodes().any(
            |node| *node == CanvasNode::Graph(empty) || *node == CanvasNode::Placeholder(empty)
        )
    );
    let expression = snarl
        .node_ids()
        .find_map(|(id, node)| (*node == CanvasNode::Graph(3)).then_some(id))
        .unwrap();
    let from = snarl.out_pin(OutPinId {
        node: expression,
        output: 0,
    });
    let to = snarl.in_pin(InPinId {
        node: fx.call,
        input: 0,
    });
    fx.viewer().connect(&from, &to, &mut snarl);
    raw(&fx, &snarl);
    assert!(!fx.graph.nodes.contains_key(&empty));
    assert_sum(&fx, 33);
    let imported = raw(&fx, &snarl);
    settle(&mut fx, &mut snarl, &context, true, false);
    assert_eq!(raw(&fx, &snarl), imported);
    switch(&mut fx, &mut snarl, &context, true, false);
    assert!(
        matches!(&fx.graph.nodes[&0], Node::Aggregate { expression: None, value, .. } if value == &["Price"])
    );
    assert!(
        matches!(&fx.graph.nodes[&3], Node::Call { function, args } if function == "multiply" && args == &[1, 2])
    );
    assert!(matches!(&fx.graph.nodes[&4], Node::Call { args, .. } if args == &[3]));
    assert_eq!(
        snarl.wires().count(),
        1,
        "the other expression consumer stays wired"
    );
    assert_sum(&fx, 15);
}

#[test]
fn calculated_aggregate_atomic_final_ids_and_reserved_owner_ids_preserve_complete_prior_state() {
    for available in [0_u32, 1, 2] {
        let (mut fx, snarl) = sum_fixture();
        fx.graph.nodes.insert(
            u32::MAX - available,
            Node::Const {
                value: Value::String("capacity".into()),
            },
        );
        let before = raw(&fx, &snarl);
        let mut edited = fx.graph.nodes[&0].clone();
        if let Node::Aggregate { function, .. } = &mut edited {
            *function = AggregateOp::Join;
        }
        let result = fx.viewer().commit_aggregate_property_edit(0, edited, true);
        eprintln!("Dual-input reservation original result={result:#?}");
        if available == 2 {
            assert_eq!(result.unwrap(), 2);
            assert!(
                matches!(&fx.graph.nodes[&0], Node::Aggregate { expression: Some(value), arg: Some(argument), .. }
                if *value == u32::MAX - 1 && *argument == u32::MAX)
            );
        } else {
            assert!(result.is_err());
            assert_eq!(raw(&fx, &snarl), before);
        }
    }
    let (mut fx, mut snarl) = sum_fixture();
    fx.graph.nodes.insert(
        u32::MAX - 1,
        Node::Const {
            value: Value::Int(0),
        },
    );
    let edited = fx.graph.nodes[&0].clone();
    assert_eq!(
        fx.viewer()
            .commit_aggregate_property_edit(0, edited, true)
            .unwrap(),
        1
    );
    let before = raw(&fx, &snarl);
    let edited = fx.graph.nodes[&0].clone();
    assert_eq!(
        fx.viewer()
            .commit_aggregate_property_edit(0, edited, false)
            .unwrap(),
        0
    );
    let aggregate = fx.call;
    fx.viewer()
        .migrate_aggregate_mode_wires(aggregate, Some(u32::MAX), None, &mut snarl);
    assert!(!fx.graph.nodes.contains_key(&u32::MAX));
    assert_ne!(raw(&fx, &snarl), before);

    let (mut fx, snarl) = sum_fixture();
    fx.root_scope.children.push(Scope {
        iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
            from: None,
            to: 2,
            item: 5,
        }),
        ..Scope::default()
    });
    let edited = fx.graph.nodes[&0].clone();
    assert_eq!(
        fx.viewer()
            .commit_aggregate_property_edit(0, edited, true)
            .unwrap(),
        1
    );
    raw(&fx, &snarl);
    assert!(matches!(
        &fx.graph.nodes[&0],
        Node::Aggregate {
            expression: Some(6),
            ..
        }
    ));
    assert!(
        !fx.graph.nodes.contains_key(&5),
        "missing owned generated item ID was not reused"
    );
}

#[test]
fn calculated_aggregate_open_popup_locks_and_isolated_functions_refuse_without_edits() {
    for isolated in [false, true] {
        let (mut fx, mut snarl) = sum_fixture();
        let context = egui::Context::default();
        crate::icons::install(&context);
        open(&mut fx, &mut snarl, &context, isolated);
        let before = raw(&fx, &snarl);
        switch(&mut fx, &mut snarl, &context, isolated, isolated);
        assert_eq!(raw(&fx, &snarl), before);
        if isolated {
            let edited = fx.graph.nodes[&0].clone();
            let mut output = 0;
            let mut viewer = fx.viewer();
            viewer.function_output = Some(&mut output);
            let result = viewer.commit_aggregate_property_edit(0, edited, true);
            eprintln!("Isolated aggregate refusal={result:#?}");
            assert!(result.unwrap_err().contains("isolated"));
            assert_eq!(raw(&fx, &snarl), before);
        }
    }
}

#[test]
fn calculated_count_and_item_at_keep_eager_late_errors_and_empty_results() {
    for function in [AggregateOp::Count, AggregateOp::ItemAt] {
        let (mut fx, snarl) = sum_fixture();
        fx.graph.nodes.insert(
            3,
            Node::Call {
                function: "divide".into(),
                args: vec![1, 2],
            },
        );
        fx.graph.nodes.insert(
            8,
            Node::Const {
                value: Value::Int(1),
            },
        );
        fx.graph.nodes.insert(
            0,
            Node::Aggregate {
                function,
                collection: vec!["Rows".into()],
                value: vec!["Price".into()],
                expression: Some(3),
                arg: (function == AggregateOp::ItemAt).then_some(8),
            },
        );
        raw(&fx, &snarl);
        let project = Project {
            source: rows(),
            target: SchemaNode::group("Output", vec![SchemaNode::scalar("out", ScalarType::Int)]),
            graph: fx.graph.clone(),
            root: fx.root_scope.clone(),
            ..crate::new_mapping::blank_project()
        };
        let source = Instance::Group(
            vec![(
                "Rows".into(),
                Instance::Repeated(
                    [(10, 2), (20, 0)]
                        .into_iter()
                        .map(|(price, quantity)| {
                            Instance::Group(
                                vec![
                                    ("Price".into(), Instance::Scalar(Value::Int(price))),
                                    ("Quantity".into(), Instance::Scalar(Value::Int(quantity))),
                                ]
                                .into(),
                            )
                        })
                        .collect(),
                ),
            )]
            .into(),
        );
        let validation = engine::validate(&project);
        let late = engine::run(&project, &source);
        let empty = engine::run(
            &project,
            &Instance::Group(vec![("Rows".into(), Instance::Repeated(Vec::new()))].into()),
        );
        eprintln!(
            "Eager aggregate full original project={project:#?}; input={source:#?}; validation={validation:#?}; late={late:#?}; empty={empty:#?}"
        );
        assert!(validation.is_empty());
        assert!(matches!(
            late,
            Err(engine::EngineError::Function(
                functions::FunctionError::DivideByZero
            ))
        ));
        assert_eq!(
            empty.unwrap(),
            Instance::Group(
                vec![(
                    "out".into(),
                    Instance::Scalar(if function == AggregateOp::Count {
                        Value::Int(0)
                    } else {
                        Value::Null
                    })
                )]
                .into()
            )
        );
    }
}
