use super::*;
use std::collections::BTreeMap;

use mapping::{
    AggregateOp, FilterMapCapture, FilterMapV1, FunctionId, FunctionParameter, FunctionParameterId,
    Project, SequenceExpr, UserFunction,
};

fn sequence(read_only: bool) -> SequenceExpr {
    let source = SequenceExpr::Generate {
        from: Some(1),
        to: 2,
        item: 10,
    };
    if !read_only {
        return source;
    }
    SequenceExpr::FilterMapV1(FilterMapV1 {
        source: Box::new(source),
        item: 11,
        predicate: FunctionId::new(100),
        mapper: FunctionId::new(101),
        output_type: ScalarType::Int,
        captures: vec![FilterMapCapture {
            node: 20,
            ty: ScalarType::Int,
        }],
    })
}

fn functions() -> BTreeMap<FunctionId, UserFunction> {
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
    let mut functions: BTreeMap<FunctionId, UserFunction> =
        serde_json::from_value(case["user_functions"].clone()).unwrap();
    for function in functions.values_mut() {
        function.parameters.push(FunctionParameter {
            id: FunctionParameterId::new(3),
            name: "capture".into(),
            ty: ScalarType::Int,
        });
    }
    functions
}

fn project(fx: &Fixture) -> Project {
    Project {
        source: SchemaNode::group(
            "row",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("age", ScalarType::Int),
            ],
        ),
        target: SchemaNode::group("row", vec![SchemaNode::scalar("out", ScalarType::String)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: functions(),
        graph: fx.graph.clone(),
        root: fx.root_scope.clone(),
    }
}

fn setup(kind: usize, read_only: bool) -> (Fixture, Snarl<CanvasNode>, Vec<(NodeId, SnarlNodeId)>) {
    let mut fx = fixture();
    let sequence = sequence(read_only);
    let output_item = sequence.item();
    let consumer = match kind {
        0 => Node::SequenceExists {
            sequence,
            predicate: 40,
        },
        1 => Node::SequenceItemAt {
            sequence,
            index: 41,
        },
        2 => Node::SequenceAggregate {
            function: AggregateOp::Join,
            sequence,
            predicate: Some(40),
            expression: Some(output_item),
            arg: Some(42),
        },
        _ => panic!("finite test consumer"),
    };
    fx.graph.nodes = [
        (0, consumer),
        (
            1,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            10,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            20,
            Node::Const {
                value: Value::Int(7),
            },
        ),
        (
            40,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            41,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            42,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            99,
            Node::Const {
                value: Value::Int(9),
            },
        ),
    ]
    .into();
    if read_only {
        fx.graph.nodes.insert(
            11,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        );
    }
    fx.root_scope.bindings.push(Binding {
        target_field: "out".into(),
        node: 0,
    });
    let slots = match kind {
        0 => vec![1, 2, 20, 40],
        1 => vec![1, 2, 20, 41],
        _ => vec![1, 2, 20, 40, output_item, 42],
    };
    let slots = if read_only {
        slots
    } else {
        match kind {
            0 => vec![1, 2, 40],
            1 => vec![1, 2, 41],
            _ => vec![1, 2, 40, output_item, 42],
        }
    };
    let mut snarl = std::mem::take(&mut fx.snarl);
    let nodes: Vec<_> = slots
        .iter()
        .enumerate()
        .map(|(index, &id)| {
            let node =
                snarl.insert_node(egui::pos2(60.0, 40.0 * index as f32), CanvasNode::Graph(id));
            snarl.connect(
                OutPinId { node, output: 0 },
                InPinId {
                    node: fx.call,
                    input: index,
                },
            );
            (id, node)
        })
        .collect();
    snarl.insert_node(egui::pos2(80.0, 400.0), CanvasNode::Graph(99));
    let actual = project(&fx).validate_filter_map_v1();
    eprintln!(
        "setup_project={}\nsetup_admission={actual:#?}",
        serde_json::to_string(&project(&fx)).unwrap()
    );
    assert_eq!(actual, Ok(()));
    assert_eq!(node_inputs(&fx.graph.nodes[&0]), slots);
    (fx, snarl, nodes)
}

fn state(fx: &Fixture, snarl: &Snarl<CanvasNode>) -> (String, String, String) {
    (
        serde_json::to_string(&project(fx)).unwrap(),
        format!("{snarl:?}"),
        format!(
            "{:?}",
            snarl.wires().collect::<std::collections::BTreeSet<_>>()
        ),
    )
}

#[test]
fn filter_map_disconnect_refuses_every_slot_before_any_allocation_or_wire_edit() {
    for kind in 0..3 {
        let (_, _, initial) = setup(kind, true);
        for slot in 0..initial.len() {
            let (mut fx, mut snarl, nodes) = setup(kind, true);
            let before = state(&fx, &snarl);
            let from = snarl.out_pin(OutPinId {
                node: nodes[slot].1,
                output: 0,
            });
            let to = snarl.in_pin(InPinId {
                node: fx.call,
                input: slot,
            });
            let error = {
                let mut viewer = fx.viewer();
                viewer.disconnect(&from, &to, &mut snarl);
                viewer.error
            };
            let after = state(&fx, &snarl);
            eprintln!(
                "kind={kind} slot={slot}\noriginal={before:#?}\nactual={after:#?}\nerror={error:#?}"
            );
            assert_eq!(
                error.as_deref(),
                Some(graph_sequence::READ_ONLY_INPUTS_MESSAGE)
            );
            assert_eq!(after, before);
        }
    }
}

#[test]
fn filter_map_reconnect_refuses_graph_and_physical_sources_without_allocating() {
    for kind in 0..3 {
        for physical in [false, true] {
            let (mut fx, mut snarl, _) = setup(kind, true);
            let before = state(&fx, &snarl);
            let alternative = snarl
                .node_ids()
                .find_map(|(id, node)| (*node == CanvasNode::Graph(99)).then_some(id))
                .unwrap();
            let from = snarl.out_pin(OutPinId {
                node: if physical { fx.source } else { alternative },
                output: 0,
            });
            let to = snarl.in_pin(InPinId {
                node: fx.call,
                input: 0,
            });
            let error = {
                let mut viewer = fx.viewer();
                viewer.connect(&from, &to, &mut snarl);
                viewer.error
            };
            let after = state(&fx, &snarl);
            eprintln!(
                "kind={kind} physical={physical}\noriginal={before:#?}\nactual={after:#?}\nerror={error:#?}"
            );
            assert_eq!(
                error.as_deref(),
                Some(graph_sequence::READ_ONLY_INPUTS_MESSAGE)
            );
            assert_eq!(after, before);
        }
    }
}

#[test]
fn filter_map_input_deletion_is_blocked_before_replacement_planning() {
    for kind in 0..3 {
        let (_, _, initial) = setup(kind, true);
        for slot in 0..initial.len() {
            for batch in [false, true] {
                let (mut fx, mut snarl, nodes) = setup(kind, true);
                let before = state(&fx, &snarl);
                let (removed, error) = {
                    let mut viewer = fx.viewer();
                    let removed = if batch {
                        viewer.remove_snarl_nodes(&[nodes[slot].1], &mut snarl)
                    } else {
                        usize::from(viewer.remove_graph_node(
                            nodes[slot].0,
                            nodes[slot].1,
                            &mut snarl,
                        ))
                    };
                    (removed, viewer.error)
                };
                let after = state(&fx, &snarl);
                eprintln!(
                    "kind={kind} slot={slot} batch={batch}\noriginal={before:#?}\nactual={after:#?}\nremoved={removed} error={error:#?}"
                );
                assert_eq!(removed, 0);
                assert!(error.is_some());
                assert_eq!(after, before);
            }
        }
    }
}

#[test]
fn ordinary_generator_disconnect_and_input_deletion_still_edit_the_input() {
    for delete in [false, true] {
        let (mut fx, mut snarl, nodes) = setup(1, false);
        let before = state(&fx, &snarl);
        let original_wire_count = snarl.wires().count();
        let from = snarl.out_pin(OutPinId {
            node: nodes[0].1,
            output: 0,
        });
        let to = snarl.in_pin(InPinId {
            node: fx.call,
            input: 0,
        });
        let (removed, error) = {
            let mut viewer = fx.viewer();
            let removed = if delete {
                viewer.remove_graph_node(nodes[0].0, nodes[0].1, &mut snarl)
            } else {
                viewer.disconnect(&from, &to, &mut snarl);
                false
            };
            (removed, viewer.error)
        };
        let after = state(&fx, &snarl);
        eprintln!(
            "legacy delete={delete}\noriginal={before:#?}\nactual={after:#?}\nremoved={removed} error={error:#?}"
        );
        assert_eq!(error, None);
        assert_eq!(removed, delete);
        let Node::SequenceItemAt {
            sequence:
                SequenceExpr::Generate {
                    from: Some(replacement),
                    to: 2,
                    item: 10,
                },
            index: 41,
        } = &fx.graph.nodes[&0]
        else {
            panic!("legacy descriptor changed unexpectedly");
        };
        assert!(matches!(
            fx.graph.nodes.get(replacement),
            Some(Node::Unconnected)
        ));
        assert_eq!(snarl.wires().count(), original_wire_count - 1);
        assert_eq!(fx.graph.nodes.contains_key(&1), !delete);
    }
}
