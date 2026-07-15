use super::*;
use crate::canvas::{source_leaves, target_leaves};
use egui_snarl::{InPinId, OutPinId};
use ir::{ScalarType, SchemaNode};

struct Fixture {
    graph: Graph,
    root_scope: Scope,
    source_leaves: Vec<SourceLeaf>,
    target_leaves: Vec<TargetLeaf>,
    source_paths: SourcePathCatalog,
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
    let source = snarl.insert_node(egui::pos2(0.0, 0.0), CanvasNode::Source);
    let target = snarl.insert_node(egui::pos2(400.0, 0.0), CanvasNode::Target);
    let call = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(0));
    Fixture {
        graph,
        root_scope: Scope::default(),
        source_leaves: source_leaves(&source_schema),
        target_leaves: target_leaves(&target_schema),
        source_paths,
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
            extra_targets: &[],
            source_leaves: &self.source_leaves,
            target_leaves: &self.target_leaves,
            source_paths: &self.source_paths,
            error: None,
        }
    }
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
    let source_leaves = source_leaves(&source_schema);
    let target_leaves = target_leaves(&target_schema);
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
    let mut snarl = Snarl::new();
    let source = snarl.insert_node(egui::pos2(0.0, 0.0), CanvasNode::Source);
    let call = snarl.insert_node(egui::pos2(200.0, 0.0), CanvasNode::Graph(0));
    let mut viewer = GraphViewer {
        graph: &mut graph,
        root_scope: &mut root_scope,
        extra_targets: &[],
        source_leaves: &source_leaves,
        target_leaves: &target_leaves,
        source_paths: &source_paths,
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
fn required_inputs_are_visible_wired_placeholders() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let pos = egui::pos2(600.0, 300.0);
    let (_, if_node) = fx
        .viewer()
        .insert_with_placeholders(&mut snarl, pos, 3, |inputs| Node::If {
            condition: inputs[0],
            then: inputs[1],
            else_: inputs[2],
        });

    let placeholders: Vec<_> = snarl
        .nodes_pos()
        .filter_map(|(pos, node)| match node {
            CanvasNode::Placeholder(id) => Some((*id, pos)),
            _ => None,
        })
        .collect();
    assert_eq!(placeholders.len(), 3);
    assert_eq!(
        snarl.wires().filter(|(_, to)| to.node == if_node).count(),
        3
    );
    for (input, (_, placeholder_pos)) in placeholders.iter().enumerate() {
        assert_eq!(
            *placeholder_pos,
            GraphViewer::placeholder_position(pos, input, 3)
        );
    }
    assert!(placeholders.iter().all(|(id, _)| matches!(
        fx.graph.nodes.get(id),
        Some(Node::Const { value: Value::Null })
    )));
}

#[test]
fn reconnect_and_disconnect_replace_placeholders_without_orphans() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let (placeholder, placeholder_node) = fx
        .viewer()
        .insert_placeholder(&mut snarl, egui::pos2(40.0, 80.0));
    let Node::Call { args, .. } = fx.graph.nodes.get_mut(&0).unwrap() else {
        panic!("fixture node should be a call");
    };
    args.push(placeholder);
    snarl.connect(
        OutPinId {
            node: placeholder_node,
            output: 0,
        },
        InPinId {
            node: fx.call,
            input: 0,
        },
    );

    let from = snarl.out_pin(OutPinId {
        node: fx.source,
        output: 0,
    });
    let to = snarl.in_pin(InPinId {
        node: fx.call,
        input: 0,
    });
    fx.viewer().connect(&from, &to, &mut snarl);
    assert!(!fx.graph.nodes.contains_key(&placeholder));
    assert!(
        !snarl
            .nodes()
            .any(|node| *node == CanvasNode::Placeholder(placeholder))
    );

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
    let placeholders: Vec<_> = snarl
        .nodes()
        .filter(|node| matches!(node, CanvasNode::Placeholder(_)))
        .collect();
    assert_eq!(placeholders.len(), 1);
    assert_eq!(snarl.wires().count(), 1);
    assert_eq!(fx.graph.nodes.len(), 2, "call plus one live placeholder");
}

#[test]
fn deleting_a_node_removes_its_owned_placeholders() {
    let mut fx = fixture();
    let mut snarl = std::mem::take(&mut fx.snarl);
    let (if_id, if_node) =
        fx.viewer()
            .insert_with_placeholders(&mut snarl, egui::pos2(600.0, 300.0), 3, |inputs| Node::If {
                condition: inputs[0],
                then: inputs[1],
                else_: inputs[2],
            });
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
    fx.target_leaves = vec![TargetLeaf {
        label: "Order/Address/b".into(),
        chain: vec!["Order".into(), "Address".into()],
        field: "b".into(),
    }];
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
    assert!(
        snarl
            .nodes()
            .all(|node| matches!(node, CanvasNode::Source | CanvasNode::Target))
    );
}

#[test]
fn batch_removal_uses_context_menu_reference_guards_and_placeholder_cleanup() {
    let mut fx = fixture();
    fx.graph.nodes.insert(1, Node::Const { value: Value::Null });
    let Some(Node::Call { args, .. }) = fx.graph.nodes.get_mut(&0) else {
        panic!("fixture call exists");
    };
    args.push(1);
    let mut snarl = std::mem::take(&mut fx.snarl);
    let placeholder = snarl.insert_node(egui::pos2(100.0, 100.0), CanvasNode::Placeholder(1));
    snarl.connect(
        OutPinId {
            node: placeholder,
            output: 0,
        },
        InPinId {
            node: fx.call,
            input: 0,
        },
    );
    fx.root_scope.bindings.push(Binding {
        target_field: "out".into(),
        node: 0,
    });
    let call = fx.call;

    assert_eq!(fx.viewer().remove_snarl_nodes(&[call], &mut snarl), 0);
    assert!(fx.graph.nodes.contains_key(&0));
    assert!(snarl.get_node(call).is_some());

    fx.root_scope.bindings.clear();
    assert_eq!(fx.viewer().remove_snarl_nodes(&[call], &mut snarl), 1);
    assert!(fx.graph.nodes.is_empty());
    assert!(snarl.get_node(call).is_none());
    assert!(snarl.get_node(placeholder).is_none());
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
    let count = GraphViewer::aggregate_node(AggregateOp::Count, None);
    assert_eq!(GraphViewer::input_count(&count), 0);

    let arg = fx.viewer().fresh_const();
    let join = GraphViewer::aggregate_node(AggregateOp::Join, Some(arg));
    assert_eq!(GraphViewer::input_count(&join), 1);
    let Node::Aggregate { arg: Some(arg), .. } = join else {
        panic!("join should get a separator placeholder");
    };
    assert!(matches!(fx.graph.nodes[&arg], Node::Const { .. }));

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
    fx.root_scope.group_starting_with = Some(1);
    fx.root_scope.group_into_blocks = Some(1);
    fx.root_scope.sort_by = Some(1);
    fx.root_scope.take = Some(1);

    assert_eq!(
        fx.viewer().references_to(1),
        vec![
            "graph node 0",
            "root scope binding out",
            "root scope group block size",
            "root scope group-by key",
            "root scope group-starting predicate",
            "root scope sort key",
            "root scope take count",
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
            take: Some(9),
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
        vec!["scope <dynamic child 1>/<dynamic child 1> take count"]
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
