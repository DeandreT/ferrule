use super::*;
use mapping::SequenceExpr;

fn functions() -> crate::filter_map_editor::Functions {
    let corpus: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/design/fixtures/scalar-filter-map-v1.json"
    )))
    .unwrap();
    let case = corpus["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "identity-all")
        .unwrap();
    serde_json::from_value(case["user_functions"].clone()).unwrap()
}
fn snapshot(fx: &Fixture, snarl: &Snarl<CanvasNode>) -> String {
    let float_bits = fx
        .graph
        .nodes
        .iter()
        .filter_map(|(id, node)| match node {
            Node::Const {
                value: Value::Float(value),
            } => Some((*id, value.to_bits())),
            _ => None,
        })
        .collect::<Vec<_>>();
    format!(
        "exact graph Float bits={float_bits:?}\ncomplete graph={:#?}\nroot={:#?}\nsource={:#?}\ntarget={:#?}\nSnarl={snarl:#?}\nordered wires={:#?}",
        fx.graph,
        fx.root_scope,
        fx.source_blocks,
        fx.target_blocks,
        snarl.wires().collect::<Vec<_>>()
    )
}
fn feature_fixture(
    kind: usize,
) -> (
    Fixture,
    crate::filter_map_editor::Functions,
    SnarlNodeId,
    std::collections::BTreeMap<NodeId, SnarlNodeId>,
) {
    let mut fx = fixture();
    let functions = functions();
    let (sequence, nodes) =
        crate::filter_map_editor::create(&[41, 42, 43, 44], &functions).unwrap();
    fx.graph.nodes.extend(nodes);
    fx.graph.nodes.insert(
        45,
        Node::Const {
            value: Value::Int(1),
        },
    );
    fx.graph.nodes.insert(
        46,
        Node::Const {
            value: Value::Bool(true),
        },
    );
    let node = match kind {
        0 => Node::SequenceItemAt {
            sequence,
            index: 45,
        },
        1 => Node::SequenceExists {
            sequence,
            predicate: 46,
        },
        _ => Node::SequenceAggregate {
            sequence,
            function: AggregateOp::Sum,
            predicate: None,
            expression: Some(44),
            arg: None,
        },
    };
    fx.graph.nodes.insert(50, node);
    fx.graph.nodes.insert(
        60,
        Node::Call {
            function: "add".into(),
            args: vec![43, 45],
        },
    );
    let mut visible = std::collections::BTreeMap::new();
    for (index, id) in [41, 42, 43, 44, 45, 46, 50, 60].into_iter().enumerate() {
        visible.insert(
            id,
            fx.snarl.insert_node(
                egui::pos2(index as f32 * 50.0, 120.0),
                CanvasNode::Graph(id),
            ),
        );
    }
    // Match project canvas construction for both visible nodes with inputs.
    for node in [50, 60] {
        for (input, id) in node_inputs(&fx.graph.nodes[&node]).into_iter().enumerate() {
            fx.snarl.connect(
                OutPinId {
                    node: visible[&id],
                    output: 0,
                },
                InPinId {
                    node: visible[&node],
                    input,
                },
            );
        }
    }
    let consumer = visible[&50];
    (fx, functions, consumer, visible)
}

#[test]
fn filter_map_palette_creation_is_atomic_and_keeps_both_owned_items_distinct() {
    let mut fx = fixture();
    let functions = functions();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let before = snapshot(&fx, &snarl);
    let created = {
        let mut viewer = fx.viewer();
        viewer.project_references =
            ProjectGraphReferences::default().with_user_functions(&functions);
        viewer.insert_palette_node(
            &mut snarl,
            egui::pos2(300.0, 160.0),
            NodeTemplate::FilterMapItemAt,
        )
    };
    eprintln!(
        "palette creation original={created:#?}\nbefore={before}\nafter={}",
        snapshot(&fx, &snarl)
    );
    let (id, canvas) = created.unwrap();
    assert_eq!(id, 6);
    assert_eq!(snarl[canvas], CanvasNode::Graph(6));
    let Node::SequenceItemAt {
        sequence: SequenceExpr::FilterMapV1(value),
        index,
    } = &fx.graph.nodes[&id]
    else {
        panic!("item at")
    };
    assert_eq!(value.source.item(), 3);
    assert_eq!(value.item, 4);
    assert_eq!(*index, 5);
    assert_eq!(value.output_type, ScalarType::Int);
    assert!(value.captures.is_empty());
    let refused_before = snapshot(&fx, &snarl);
    let unavailable = fx.viewer().insert_palette_node(
        &mut snarl,
        egui::Pos2::ZERO,
        NodeTemplate::FilterMapItemAt,
    );
    eprintln!(
        "unavailable original={unavailable:#?}\ncomplete={}",
        snapshot(&fx, &snarl)
    );
    assert!(unavailable.is_err());
    assert_eq!(snapshot(&fx, &snarl), refused_before);
}

#[test]
fn filter_map_pointer_pin_connections_keep_private_inputs_out_of_all_downstream_contexts() {
    for kind in 0..3 {
        let (mut fx, functions, consumer, visible) = feature_fixture(kind);
        let mut snarl = std::mem::take(&mut fx.snarl);
        for (id, input) in [(43, 0), (44, 0), (43, 2), (60, 2)] {
            let before = snapshot(&fx, &snarl);
            let from = snarl.out_pin(OutPinId {
                node: visible[&id],
                output: 0,
            });
            let to = snarl.in_pin(InPinId {
                node: consumer,
                input,
            });
            let error = {
                let mut viewer = fx.viewer();
                viewer.project_references =
                    ProjectGraphReferences::default().with_user_functions(&functions);
                viewer.connect(&from, &to, &mut snarl);
                viewer.error
            };
            let after = snapshot(&fx, &snarl);
            eprintln!(
                "native pin-connect kind={kind} source={id} input={input} error={error:#?}\nbefore={before}\nafter={after}"
            );
            assert!(error.is_some());
            assert_eq!(after, before);
        }
        // ItemAt uses Empty context; Exists and the aggregate item expression
        // inherit only the mapped output, never the raw stage-input owner.
        let before = snapshot(&fx, &snarl);
        let from = snarl.out_pin(OutPinId {
            node: visible[&44],
            output: 0,
        });
        let to = snarl.in_pin(InPinId {
            node: consumer,
            input: 2,
        });
        let error = {
            let mut viewer = fx.viewer();
            viewer.project_references =
                ProjectGraphReferences::default().with_user_functions(&functions);
            viewer.connect(&from, &to, &mut snarl);
            viewer.error
        };
        eprintln!(
            "output context kind={kind} error={error:#?}\nbefore={before}\nafter={}",
            snapshot(&fx, &snarl)
        );
        if kind == 0 {
            assert!(error.is_some());
            assert_eq!(snapshot(&fx, &snarl), before);
        } else {
            assert!(error.is_none());
            assert_eq!(fx.viewer().input_at(50, 2), Some(44));
        }
        let ordinary_before = snapshot(&fx, &snarl);
        let raw = snarl.out_pin(OutPinId {
            node: visible[&43],
            output: 0,
        });
        let ordinary = snarl.in_pin(InPinId {
            node: fx.call,
            input: 0,
        });
        // Give the ordinary node an existing input, before taking its baseline.
        if let Node::Call { args, .. } = fx.graph.nodes.get_mut(&0).unwrap() {
            args.push(45);
        }
        let ordinary_baseline = snapshot(&fx, &snarl);
        let error = {
            let mut viewer = fx.viewer();
            viewer.project_references =
                ProjectGraphReferences::default().with_user_functions(&functions);
            viewer.connect(&raw, &ordinary, &mut snarl);
            viewer.error
        };
        eprintln!(
            "ordinary input baseline change={ordinary_before}\nraw refusal={error:#?}\nafter={}",
            snapshot(&fx, &snarl)
        );
        assert!(error.is_some());
        assert_eq!(snapshot(&fx, &snarl), ordinary_baseline);
    }
}

#[test]
fn filter_map_disconnect_and_blocked_delete_preserve_graph_wire_ownership() {
    let (mut fx, functions, consumer, visible) = feature_fixture(0);
    let mut snarl = std::mem::take(&mut fx.snarl);
    let before = snapshot(&fx, &snarl);
    let removed = {
        let mut viewer = fx.viewer();
        viewer.project_references =
            ProjectGraphReferences::default().with_user_functions(&functions);
        viewer.remove_snarl_nodes(&[visible[&41]], &mut snarl)
    };
    eprintln!(
        "blocked delete count={removed}\nbefore={before}\nafter={}",
        snapshot(&fx, &snarl)
    );
    assert_eq!(removed, 0);
    assert_eq!(snapshot(&fx, &snarl), before);
    let from = snarl.out_pin(OutPinId {
        node: visible[&41],
        output: 0,
    });
    let to = snarl.in_pin(InPinId {
        node: consumer,
        input: 0,
    });
    let error = {
        let mut viewer = fx.viewer();
        viewer.project_references =
            ProjectGraphReferences::default().with_user_functions(&functions);
        viewer.disconnect(&from, &to, &mut snarl);
        viewer.error
    };
    eprintln!(
        "disconnect original error={error:#?}\ncomplete after={}",
        snapshot(&fx, &snarl)
    );
    assert!(error.is_none());
    assert!(matches!(fx.graph.nodes.get(&61), Some(Node::Unconnected)));
    assert_eq!(fx.viewer().input_at(50, 0), Some(61));
    assert!(
        !snarl
            .wires()
            .any(|(_, input)| input.node == consumer && input.input == 0)
    );
    let SequenceExpr::FilterMapV1(value) =
        crate::filter_map_editor::sequence(&fx.graph.nodes[&50]).unwrap()
    else {
        panic!("filter/map")
    };
    assert_eq!(value.source.item(), 43);
    assert_eq!(value.item, 44);
}

#[test]
fn three_compact_sequence_nodes_expose_properties_and_plain_position_headers() {
    let context = egui::Context::default();
    crate::icons::install(&context);
    for kind in 0..3 {
        let (mut fx, functions, consumer, _) = feature_fixture(kind);
        let mut snarl = std::mem::take(&mut fx.snarl);
        let pin = snarl.out_pin(OutPinId {
            node: consumer,
            output: 0,
        });
        let before = snapshot(&fx, &snarl);
        crate::filter_map_editor::observations::begin();
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1200.0, 900.0),
                )),
                ..Default::default()
            },
            |ui| {
                let mut viewer = fx.viewer();
                viewer.project_references =
                    ProjectGraphReferences::default().with_user_functions(&functions);
                viewer.show_node_properties(&pin, ui, &mut snarl);
            },
        );
        let controls = crate::filter_map_editor::observations::take();
        eprintln!(
            "properties kind={kind} controls={controls:#?}\ncomplete shapes={:#?}\nbefore={before}\nafter={}",
            output.shapes,
            snapshot(&fx, &snarl)
        );
        assert!(graph_node_presentation::has_properties(
            &fx.graph.nodes[&50]
        ));
        assert!(
            controls
                .iter()
                .any(|control| control.name == "Filter stage")
        );
        assert!(controls.iter().any(|control| control.name == "Map stage"));
        assert_eq!(snapshot(&fx, &snarl), before);
        for (id, expected) in [(43, "Stage input"), (44, "Mapped output")] {
            let canvas = snarl
                .node_ids()
                .find(|(_, node)| **node == CanvasNode::Graph(id))
                .unwrap()
                .0;
            let actual = fx.viewer().title(&CanvasNode::Graph(id));
            eprintln!("owned header original={actual:?} canvas={canvas:?}");
            assert_eq!(actual, expected);
        }
    }
}

#[test]
fn any_and_sum_palette_units_match_frozen_complete_scalar_expectations() {
    for (template, expected_id, expected) in [
        (NodeTemplate::FilterMapExists, 6, Value::Bool(true)),
        (NodeTemplate::FilterMapSum, 5, Value::Int(6)),
    ] {
        let mut fx = fixture();
        let functions = functions();
        let mut snarl = std::mem::take(&mut fx.snarl);
        let before = snapshot(&fx, &snarl);
        let created = {
            let mut viewer = fx.viewer();
            viewer.project_references =
                ProjectGraphReferences::default().with_user_functions(&functions);
            viewer.insert_palette_node(&mut snarl, egui::pos2(300.0, 160.0), template)
        };
        eprintln!(
            "frozen constructor original={created:#?}\nbefore={before}\nafter={}",
            snapshot(&fx, &snarl)
        );
        let (id, _) = created.unwrap();
        assert_eq!(id, expected_id);
        let ty = if template == NodeTemplate::FilterMapExists {
            ScalarType::Bool
        } else {
            ScalarType::Int
        };
        let mut project = crate::new_mapping::blank_project();
        project.source = SchemaNode::group("Input", Vec::new());
        project.target = SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ty)]);
        project.graph = fx.graph.clone();
        project.user_functions = functions;
        project.root = Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: id,
            }],
            ..Default::default()
        };
        let actual = engine::run(&project, &ir::Instance::Group(Vec::new().into()));
        eprintln!("whole palette project={project:#?}\nfull actual native={actual:#?}");
        match &actual {
            Ok(value) => {
                crate::filter_map_editor::observations::retain_instance(value, "palette-target")
            }
            Err(error) => crate::filter_map_editor::observations::retain_error(error),
        }
        assert_eq!(
            actual.unwrap(),
            ir::Instance::Group(vec![("Value".into(), ir::Instance::Scalar(expected))].into())
        );
    }
}

#[test]
fn sum_without_a_numeric_stage_refuses_before_any_mapping_or_canvas_mutation() {
    let mut fx = fixture();
    let mut functions = functions();
    functions.remove(&FunctionId::new(101));
    let mut snarl = std::mem::take(&mut fx.snarl);
    let before = snapshot(&fx, &snarl);
    let actual = {
        let mut viewer = fx.viewer();
        viewer.project_references =
            ProjectGraphReferences::default().with_user_functions(&functions);
        viewer.insert_palette_node(&mut snarl, egui::Pos2::ZERO, NodeTemplate::FilterMapSum)
    };
    eprintln!(
        "non-numeric stage refusal original={actual:#?}\nbefore={before}\nafter={}",
        snapshot(&fx, &snarl)
    );
    assert_eq!(
        actual.unwrap_err(),
        "Filter/map sum needs an integer or number map function."
    );
    assert_eq!(snapshot(&fx, &snarl), before);
}
