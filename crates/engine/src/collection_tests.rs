use super::*;
use ir::SchemaNode;
use mapping::Binding;

fn graph_from(nodes: Vec<(NodeId, Node)>) -> Graph {
    Graph {
        nodes: nodes.into_iter().collect(),
    }
}

fn dummy_schema() -> SchemaNode {
    SchemaNode::group("root", vec![])
}

#[test]
fn group_by_partitions_iterated_items() {
    use mapping::AggregateOp;
    let graph = graph_from(vec![
        (
            0,
            Node::Call {
                function: "substring_before".into(),
                args: vec![1, 2],
            },
        ),
        (
            1,
            Node::SourceField {
                frame: None,
                path: vec!["month".into()],
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String("-".into()),
            },
        ),
        (
            3,
            Node::Aggregate {
                function: AggregateOp::Avg,
                collection: vec!["Row".into()],
                value: vec!["temp".into()],
                expression: None,
                arg: None,
            },
        ),
        (
            4,
            Node::Aggregate {
                function: AggregateOp::Count,
                collection: vec![],
                value: vec![],
                expression: None,
                arg: None,
            },
        ),
    ]);
    let project = Project {
        source: dummy_schema(),
        target: dummy_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            source: None,
            sequence: None,
            filter: None,
            group_by: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            take: None,
            bindings: vec![],
            children: vec![Scope {
                target_field: "Year".into(),
                source: Some(vec!["Row".into()]),
                sequence: None,
                filter: None,
                group_by: Some(0),
                group_into_blocks: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![
                    Binding {
                        target_field: "Label".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "AvgTemp".into(),
                        node: 3,
                    },
                    Binding {
                        target_field: "Months".into(),
                        node: 4,
                    },
                ],
                children: vec![],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    let row = |month: &str, temp: f64| {
        Instance::Group(vec![
            (
                "month".into(),
                Instance::Scalar(Value::String(month.into())),
            ),
            ("temp".into(), Instance::Scalar(Value::Float(temp))),
        ])
    };
    let source = Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![
            row("2024-01", 2.0),
            row("2024-07", 22.0),
            row("2025-01", 4.0),
        ]),
    )]);

    let target = run(&project, &source).unwrap();
    let years = target
        .field("Year")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(years.len(), 2);
    assert_eq!(
        years[0].field("Label").and_then(Instance::as_scalar),
        Some(&Value::String("2024".into()))
    );
    assert_eq!(
        years[0].field("AvgTemp").and_then(Instance::as_scalar),
        Some(&Value::Float(12.0))
    );
    assert_eq!(
        years[0].field("Months").and_then(Instance::as_scalar),
        Some(&Value::Int(2))
    );
    assert_eq!(
        years[1].field("Label").and_then(Instance::as_scalar),
        Some(&Value::String("2025".into()))
    );
    assert_eq!(
        years[1].field("Months").and_then(Instance::as_scalar),
        Some(&Value::Int(1))
    );
}

#[test]
fn filter_removes_candidates_before_grouping() {
    let graph = graph_from(vec![
        (
            0,
            Node::SourceField {
                frame: None,
                path: vec!["category".into()],
            },
        ),
        (
            1,
            Node::SourceField {
                frame: None,
                path: vec!["label".into()],
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String("skip".into()),
            },
        ),
        (
            3,
            Node::Call {
                function: "not_equal".into(),
                args: vec![1, 2],
            },
        ),
    ]);
    let project = Project {
        source: dummy_schema(),
        target: dummy_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            source: None,
            sequence: None,
            filter: None,
            group_by: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            take: None,
            bindings: vec![],
            children: vec![Scope {
                target_field: "Row".into(),
                source: Some(vec!["Item".into()]),
                sequence: None,
                filter: Some(3),
                group_by: Some(0),
                group_into_blocks: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![
                    Binding {
                        target_field: "Category".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "FirstLabel".into(),
                        node: 1,
                    },
                ],
                children: vec![],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    let item = |category: &str, label: &str| {
        Instance::Group(vec![
            (
                "category".into(),
                Instance::Scalar(Value::String(category.into())),
            ),
            (
                "label".into(),
                Instance::Scalar(Value::String(label.into())),
            ),
        ])
    };
    let source = Instance::Group(vec![(
        "Item".into(),
        Instance::Repeated(vec![
            item("A", "skip"),
            item("B", "second"),
            item("A", "first"),
            item("B", "fourth"),
        ]),
    )]);

    let target = run(&project, &source).unwrap();
    let rows = target.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].field("Category").and_then(Instance::as_scalar),
        Some(&Value::String("B".into()))
    );
    assert_eq!(
        rows[0].field("FirstLabel").and_then(Instance::as_scalar),
        Some(&Value::String("second".into()))
    );
    assert_eq!(
        rows[1].field("Category").and_then(Instance::as_scalar),
        Some(&Value::String("A".into()))
    );
    assert_eq!(
        rows[1].field("FirstLabel").and_then(Instance::as_scalar),
        Some(&Value::String("first".into()))
    );
}

#[test]
fn grouped_nested_items_preserve_outer_iteration_frames() {
    use mapping::AggregateOp;

    let graph = graph_from(vec![
        (
            0,
            Node::SourceField {
                frame: Some(vec!["Order".into(), "Items".into(), "Item".into()]),
                path: vec!["Category".into()],
            },
        ),
        (
            1,
            Node::SourceField {
                frame: Some(vec!["Order".into()]),
                path: vec!["Id".into()],
            },
        ),
        (
            2,
            Node::Aggregate {
                function: AggregateOp::Count,
                collection: vec![],
                value: vec![],
                expression: None,
                arg: None,
            },
        ),
        (
            3,
            Node::Aggregate {
                function: AggregateOp::Count,
                collection: vec!["Item".into()],
                value: vec![],
                expression: None,
                arg: None,
            },
        ),
    ]);
    let project = Project {
        source: dummy_schema(),
        target: dummy_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            source: None,
            sequence: None,
            filter: None,
            group_by: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            take: None,
            bindings: vec![],
            children: vec![Scope {
                target_field: "OrderOut".into(),
                source: Some(vec!["Order".into()]),
                sequence: None,
                filter: None,
                group_by: None,
                group_into_blocks: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![],
                children: vec![Scope {
                    target_field: "CategoryOut".into(),
                    source: Some(vec!["Items".into(), "Item".into()]),
                    sequence: None,
                    filter: None,
                    group_by: Some(0),
                    group_into_blocks: None,
                    sort_by: None,
                    sort_descending: false,
                    take: None,
                    bindings: vec![
                        Binding {
                            target_field: "Category".into(),
                            node: 0,
                        },
                        Binding {
                            target_field: "OrderId".into(),
                            node: 1,
                        },
                        Binding {
                            target_field: "Members".into(),
                            node: 2,
                        },
                        Binding {
                            target_field: "NamedMembers".into(),
                            node: 3,
                        },
                    ],
                    children: vec![],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    let item = |category: &str| {
        Instance::Group(vec![(
            "Category".into(),
            Instance::Scalar(Value::String(category.into())),
        )])
    };
    let order = |id: &str, categories: &[&str]| {
        Instance::Group(vec![
            ("Id".into(), Instance::Scalar(Value::String(id.into()))),
            (
                "Item".into(),
                Instance::Repeated((0..5).map(|_| item("outer")).collect()),
            ),
            (
                "Items".into(),
                Instance::Group(vec![(
                    "Item".into(),
                    Instance::Repeated(categories.iter().map(|value| item(value)).collect()),
                )]),
            ),
        ])
    };
    let source = Instance::Group(vec![(
        "Order".into(),
        Instance::Repeated(vec![
            order("O-1", &["A", "A", "B"]),
            order("O-2", &["A", "C"]),
        ]),
    )]);

    let target = run(&project, &source).unwrap();
    let orders = target
        .field("OrderOut")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(orders.len(), 2);
    fn categories(order: &Instance) -> &[Instance] {
        order
            .field("CategoryOut")
            .and_then(Instance::as_repeated)
            .unwrap()
    }
    let first = categories(&orders[0]);
    assert_eq!(first.len(), 2);
    assert_eq!(
        first[0].field("Category").and_then(Instance::as_scalar),
        Some(&Value::String("A".into()))
    );
    assert_eq!(
        first[0].field("OrderId").and_then(Instance::as_scalar),
        Some(&Value::String("O-1".into()))
    );
    assert_eq!(
        first[0].field("Members").and_then(Instance::as_scalar),
        Some(&Value::Int(2))
    );
    assert_eq!(
        first[0].field("NamedMembers").and_then(Instance::as_scalar),
        Some(&Value::Int(2))
    );
    let second = categories(&orders[1]);
    assert_eq!(second.len(), 2);
    assert_eq!(
        second[0].field("OrderId").and_then(Instance::as_scalar),
        Some(&Value::String("O-2".into()))
    );
    assert_eq!(
        second[1].field("Category").and_then(Instance::as_scalar),
        Some(&Value::String("C".into()))
    );
    assert_eq!(
        second[1].field("Members").and_then(Instance::as_scalar),
        Some(&Value::Int(1))
    );
    assert_eq!(
        second[1]
            .field("NamedMembers")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(1))
    );
}

/// Aggregates reduce a repeating collection found by outward context
/// fallback: count/sum inside an iterating scope see the current
/// item's children, and join with a separator works over leaf values.
#[test]
fn aggregates_reduce_collections_in_context() {
    use mapping::AggregateOp;
    let graph = graph_from(vec![
        (
            0,
            Node::Aggregate {
                function: AggregateOp::Count,
                collection: vec!["Item".into()],
                value: vec![],
                expression: None,
                arg: None,
            },
        ),
        (
            1,
            Node::Aggregate {
                function: AggregateOp::Sum,
                collection: vec!["Item".into()],
                value: vec!["Price".into()],
                expression: None,
                arg: None,
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String(", ".into()),
            },
        ),
        (
            3,
            Node::Aggregate {
                function: AggregateOp::Join,
                collection: vec!["Order".into()],
                value: vec!["Id".into()],
                expression: None,
                arg: Some(2),
            },
        ),
        (
            4,
            Node::SourceField {
                frame: None,
                path: vec!["Price".into()],
            },
        ),
        (
            5,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            6,
            Node::Call {
                function: "multiply".into(),
                args: vec![4, 5],
            },
        ),
        (
            7,
            Node::Aggregate {
                function: AggregateOp::Sum,
                collection: vec!["Item".into()],
                value: vec![],
                expression: Some(6),
                arg: None,
            },
        ),
        (
            8,
            Node::Position {
                collection: vec!["Item".into()],
            },
        ),
        (
            9,
            Node::Aggregate {
                function: AggregateOp::Sum,
                collection: vec!["Item".into()],
                value: vec![],
                expression: Some(8),
                arg: None,
            },
        ),
    ]);
    let project = Project {
        source: dummy_schema(),
        target: dummy_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            source: None,
            sequence: None,
            filter: None,
            group_by: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            take: None,
            bindings: vec![Binding {
                target_field: "AllIds".into(),
                node: 3,
            }],
            children: vec![Scope {
                target_field: "Order".into(),
                source: Some(vec!["Order".into()]),
                sequence: None,
                filter: None,
                group_by: None,
                group_into_blocks: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![
                    Binding {
                        target_field: "ItemCount".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Total".into(),
                        node: 1,
                    },
                    Binding {
                        target_field: "ComputedTotal".into(),
                        node: 7,
                    },
                    Binding {
                        target_field: "PositionSum".into(),
                        node: 9,
                    },
                ],
                children: vec![],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    let item = |price: f64| {
        Instance::Group(vec![(
            "Price".into(),
            Instance::Scalar(Value::Float(price)),
        )])
    };
    let order = |id: &str, items: Vec<Instance>| {
        Instance::Group(vec![
            ("Id".into(), Instance::Scalar(Value::String(id.into()))),
            ("Item".into(), Instance::Repeated(items)),
        ])
    };
    let source = Instance::Group(vec![(
        "Order".into(),
        Instance::Repeated(vec![
            order("A", vec![item(1.5), item(2.5)]),
            order("B", vec![]),
        ]),
    )]);

    let target = run(&project, &source).unwrap();
    assert_eq!(
        target.field("AllIds").and_then(Instance::as_scalar),
        Some(&Value::String("A, B".into()))
    );
    let orders = target
        .field("Order")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(
        orders[0].field("ItemCount").and_then(Instance::as_scalar),
        Some(&Value::Int(2))
    );
    assert_eq!(
        orders[0].field("Total").and_then(Instance::as_scalar),
        Some(&Value::Float(4.0))
    );
    assert_eq!(
        orders[0]
            .field("ComputedTotal")
            .and_then(Instance::as_scalar),
        Some(&Value::Float(8.0))
    );
    assert_eq!(
        orders[0].field("PositionSum").and_then(Instance::as_scalar),
        Some(&Value::Int(3))
    );
    // An empty collection counts 0 and sums to 0.
    assert_eq!(
        orders[1].field("ItemCount").and_then(Instance::as_scalar),
        Some(&Value::Int(0))
    );
    assert_eq!(
        orders[1].field("Total").and_then(Instance::as_scalar),
        Some(&Value::Int(0))
    );
    assert_eq!(
        orders[1]
            .field("ComputedTotal")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(0))
    );
    assert_eq!(
        orders[1].field("PositionSum").and_then(Instance::as_scalar),
        Some(&Value::Int(0))
    );
}

/// The enrichment pattern: iterate the primary source's rows while a
/// `Lookup` node joins each row against a named extra source by key.
/// A key with no match resolves to `Null` rather than erroring.

#[test]
fn generated_sequences_reuse_nested_scope_controls_and_positions() {
    let graph = graph_from(vec![
        (
            0,
            Node::SourceField {
                path: vec!["Text".into()],
                frame: None,
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            2,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            3,
            Node::Call {
                function: "upper".into(),
                args: vec![2],
            },
        ),
        (4, Node::Position { collection: vec![] }),
        (
            5,
            Node::Const {
                value: Value::Int(2),
            },
        ),
    ]);
    let project = Project {
        source: dummy_schema(),
        target: dummy_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                source: Some(vec!["Row".into()]),
                children: vec![Scope {
                    target_field: "Token".into(),
                    sequence: Some(SequenceExpr::Tokenize {
                        input: 0,
                        delimiter: 1,
                        item: 2,
                    }),
                    take: Some(5),
                    bindings: vec![
                        Binding {
                            target_field: "Value".into(),
                            node: 3,
                        },
                        Binding {
                            target_field: "Position".into(),
                            node: 4,
                        },
                    ],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    let mut invalid = project.clone();
    invalid.root.children.push(Scope {
        target_field: "Duplicate".into(),
        source: Some(Vec::new()),
        sequence: Some(SequenceExpr::Tokenize {
            input: 0,
            delimiter: 1,
            item: 2,
        }),
        ..Scope::default()
    });
    invalid.root.children.push(Scope {
        target_field: "Missing".into(),
        sequence: Some(SequenceExpr::TokenizeByLength {
            input: 999,
            length: 998,
            item: 997,
        }),
        ..Scope::default()
    });
    invalid.root.children.push(Scope {
        target_field: "WrongItem".into(),
        sequence: Some(SequenceExpr::Tokenize {
            input: 0,
            delimiter: 1,
            item: 3,
        }),
        ..Scope::default()
    });
    let issues = validate(&invalid);
    assert!(issues.iter().any(|issue| {
        issue
            .message
            .contains("each generated sequence requires a unique item node")
    }));
    assert!(issues.iter().any(|issue| {
        issue
            .message
            .contains("source path and generated sequence are mutually exclusive")
    }));
    assert!(
        issues
            .iter()
            .any(|issue| issue.message == "sequence input references missing node 999")
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.message == "sequence item references missing node 997")
    );
    assert!(issues.iter().any(|issue| {
        issue.message == "sequence item must reference an unframed empty-path source field"
    }));
    let row = |text: &str| {
        Instance::Group(vec![(
            "Text".into(),
            Instance::Scalar(Value::String(text.into())),
        )])
    };
    let source = Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![row("a,b,c"), row("d,e")]),
    )]);

    let output = run(&project, &source).unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    let values = |row: &Instance| {
        row.field("Token")
            .and_then(Instance::as_repeated)
            .unwrap()
            .iter()
            .map(|token| {
                (
                    token.field("Value").and_then(Instance::as_scalar).cloned(),
                    token
                        .field("Position")
                        .and_then(Instance::as_scalar)
                        .cloned(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        values(&rows[0]),
        vec![
            (Some(Value::String("A".into())), Some(Value::Int(1))),
            (Some(Value::String("B".into())), Some(Value::Int(2))),
        ]
    );
    assert_eq!(
        values(&rows[1]),
        vec![
            (Some(Value::String("D".into())), Some(Value::Int(1))),
            (Some(Value::String("E".into())), Some(Value::Int(2))),
        ]
    );
}

#[test]
fn tokenizers_handle_empty_and_unicode_inputs() {
    assert_eq!(
        tokenize(Value::String(String::new()), Value::String(",".into())).unwrap(),
        vec![Value::String(String::new())]
    );
    assert_eq!(
        tokenize_by_length(Value::String("aé🙂z".into()), Value::Int(2)).unwrap(),
        vec![Value::String("aé".into()), Value::String("🙂z".into())]
    );
    assert!(matches!(
        tokenize_by_length(Value::String("abc".into()), Value::Int(0)),
        Err(EngineError::Function(
            functions::FunctionError::InvalidArgument { .. }
        ))
    ));

    let graph = graph_from(vec![
        (0, Node::Const { value: Value::Null }),
        (
            1,
            Node::Const {
                value: Value::String(",".into()),
            },
        ),
        (
            2,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ]);
    let project = Project {
        source: dummy_schema(),
        target: dummy_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph,
        root: Scope {
            sequence: Some(SequenceExpr::Tokenize {
                input: 0,
                delimiter: 1,
                item: 2,
            }),
            ..Scope::default()
        },
    };
    let output = run(&project, &Instance::Group(Vec::new())).unwrap();
    assert!(output.as_repeated().is_some_and(<[Instance]>::is_empty));
    let mut missing_parameter = project.clone();
    missing_parameter.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("abc".into()),
        },
    );
    missing_parameter
        .graph
        .nodes
        .insert(1, Node::Const { value: Value::Null });
    let output = run(&missing_parameter, &Instance::Group(Vec::new())).unwrap();
    assert!(output.as_repeated().is_some_and(<[Instance]>::is_empty));
}

#[test]
fn generated_integer_ranges_use_parent_context_defaults_and_positions() {
    assert_eq!(
        generate_sequence(None, Value::Int(3)).unwrap(),
        vec![Value::Int(1), Value::Int(2), Value::Int(3)]
    );
    assert_eq!(
        generate_sequence(Some(Value::Int(7)), Value::Int(7)).unwrap(),
        vec![Value::Int(7)]
    );
    assert!(
        generate_sequence(Some(Value::Int(4)), Value::Int(2))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        generate_sequence(Some(Value::Int(i64::MIN)), Value::Int(i64::MAX)),
        Err(EngineError::GeneratedSequenceTooLarge {
            requested: 1_u128 << 64,
            max: MAX_GENERATED_SEQUENCE_ITEMS,
        })
    );

    let graph = graph_from(vec![
        (
            0,
            Node::SourceField {
                path: vec!["From".into()],
                frame: None,
            },
        ),
        (
            1,
            Node::SourceField {
                path: vec!["To".into()],
                frame: None,
            },
        ),
        (
            2,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            3,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            4,
            Node::Position {
                collection: Vec::new(),
            },
        ),
        (
            5,
            Node::SourceField {
                path: vec!["Name".into()],
                frame: None,
            },
        ),
    ]);
    let project = Project {
        source: dummy_schema(),
        target: dummy_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                source: Some(vec!["Row".into()]),
                children: vec![
                    Scope {
                        target_field: "Bounded".into(),
                        sequence: Some(SequenceExpr::Generate {
                            from: Some(0),
                            to: 1,
                            item: 2,
                        }),
                        bindings: vec![
                            Binding {
                                target_field: "Value".into(),
                                node: 2,
                            },
                            Binding {
                                target_field: "Position".into(),
                                node: 4,
                            },
                            Binding {
                                target_field: "Parent".into(),
                                node: 5,
                            },
                        ],
                        ..Scope::default()
                    },
                    Scope {
                        target_field: "Default".into(),
                        sequence: Some(SequenceExpr::Generate {
                            from: None,
                            to: 1,
                            item: 3,
                        }),
                        bindings: vec![Binding {
                            target_field: "Value".into(),
                            node: 3,
                        }],
                        ..Scope::default()
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };
    let row = |name: &str, from: i64, to: i64| {
        Instance::Group(vec![
            ("Name".into(), Instance::Scalar(Value::String(name.into()))),
            ("From".into(), Instance::Scalar(Value::Int(from))),
            ("To".into(), Instance::Scalar(Value::Int(to))),
        ])
    };
    let source = Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![row("A", 2, 4), row("B", 4, 2), row("C", 7, 7)]),
    )]);

    let output = run(&project, &source).unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    let bounded = |row: &Instance| {
        row.field("Bounded")
            .and_then(Instance::as_repeated)
            .unwrap()
            .iter()
            .map(|item| {
                (
                    item.field("Value").and_then(Instance::as_scalar).cloned(),
                    item.field("Position")
                        .and_then(Instance::as_scalar)
                        .cloned(),
                    item.field("Parent").and_then(Instance::as_scalar).cloned(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(
        bounded(&rows[0]),
        vec![
            (
                Some(Value::Int(2)),
                Some(Value::Int(1)),
                Some(Value::String("A".into()))
            ),
            (
                Some(Value::Int(3)),
                Some(Value::Int(2)),
                Some(Value::String("A".into()))
            ),
            (
                Some(Value::Int(4)),
                Some(Value::Int(3)),
                Some(Value::String("A".into()))
            ),
        ]
    );
    assert!(bounded(&rows[1]).is_empty());
    assert_eq!(bounded(&rows[2]).len(), 1);
    assert_eq!(
        rows[1]
            .field("Default")
            .and_then(Instance::as_repeated)
            .map(<[Instance]>::len),
        Some(2)
    );

    let mut invalid = project;
    let Some(SequenceExpr::Generate { from, to, .. }) =
        &mut invalid.root.children[0].children[0].sequence
    else {
        unreachable!()
    };
    *from = Some(998);
    *to = 999;
    let issues = validate(&invalid);
    assert!(
        issues
            .iter()
            .any(|issue| issue.message == "sequence lower boundary references missing node 998")
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.message == "sequence upper boundary references missing node 999")
    );
}
