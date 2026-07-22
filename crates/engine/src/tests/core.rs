use super::*;
use ir::SchemaNode;
use mapping::{Binding, SequenceExpr, SequenceWindow};

fn graph_from(nodes: Vec<(NodeId, Node)>) -> Graph {
    Graph {
        nodes: nodes.into_iter().collect(),
    }
}

fn dummy_schema() -> SchemaNode {
    SchemaNode::group("root", vec![])
}

#[test]
fn copy_current_source_preserves_the_complete_nested_group() {
    let schema = SchemaNode::group(
        "root",
        vec![
            SchemaNode::scalar("id", ScalarType::Int),
            SchemaNode::group(
                "items",
                vec![SchemaNode::scalar("name", ScalarType::String)],
            )
            .repeating(),
        ],
    );
    let source = Instance::Group(vec![
        ("id".into(), Instance::Scalar(Value::Int(7))),
        (
            "items".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![(
                    "name".into(),
                    Instance::Scalar(Value::String("first".into())),
                )]),
                Instance::Group(vec![(
                    "name".into(),
                    Instance::Scalar(Value::String("second".into())),
                )]),
            ]),
        ),
    ]);
    let project = Project {
        source: schema.clone(),
        target: schema,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: Graph::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    };

    let issues = validate(&project);
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(run(&project, &source).unwrap(), source);
}

fn runtime_project() -> Project {
    Project {
        source: dummy_schema(),
        target: dummy_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: graph_from(vec![
            (
                0,
                Node::RuntimeValue {
                    value: RuntimeValue::MappingFilePath,
                },
            ),
            (
                1,
                Node::RuntimeValue {
                    value: RuntimeValue::MainMappingFilePath,
                },
            ),
        ]),
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "mapping".into(),
                    node: 0,
                },
                Binding {
                    target_field: "main".into(),
                    node: 1,
                },
            ],
            ..Scope::default()
        },
    }
}

#[test]
fn runtime_values_require_an_explicit_execution_context() {
    let error = run(&runtime_project(), &Instance::Group(Vec::new())).unwrap_err();
    assert_eq!(
        error,
        EngineError::MissingRuntimeValue(RuntimeValue::MappingFilePath)
    );
}

#[test]
fn runtime_values_distinguish_active_and_main_mapping_paths() {
    let project = runtime_project();
    let source = Instance::Group(Vec::new());
    let execution = ExecutionContext::with_main_mapping_file_path(
        Path::new("/maps/library.ferrule.json"),
        Path::new("/maps/main.ferrule.json"),
    );
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("mapping").and_then(Instance::as_scalar),
        Some(&Value::String("/maps/library.ferrule.json".into()))
    );
    assert_eq!(
        output.field("main").and_then(Instance::as_scalar),
        Some(&Value::String("/maps/main.ferrule.json".into()))
    );
}

#[test]
fn current_datetime_is_stable_and_explicitly_supplied() {
    let mut project = runtime_project();
    project.graph.nodes.insert(
        2,
        Node::RuntimeValue {
            value: RuntimeValue::CurrentDateTime,
        },
    );
    project.root.bindings = vec![Binding {
        target_field: "now".into(),
        node: 2,
    }];
    let source = Instance::Group(Vec::new());
    let without_clock = ExecutionContext::new(Path::new("/maps/main.ferrule.json"));
    assert_eq!(
        run_with_context(&project, &source, &without_clock),
        Err(EngineError::MissingRuntimeValue(
            RuntimeValue::CurrentDateTime
        ))
    );

    let execution = without_clock.with_current_datetime("2026-07-12T11:45:30.25-07:00");
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("now").and_then(Instance::as_scalar),
        Some(&Value::String("2026-07-12T11:45:30.25-07:00".into()))
    );
}

#[test]
fn evaluates_a_function_call_over_source_fields() {
    let graph = graph_from(vec![
        (
            0,
            Node::SourceField {
                frame: None,
                path: vec!["first".into()],
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String(" ".into()),
            },
        ),
        (
            2,
            Node::SourceField {
                frame: None,
                path: vec!["last".into()],
            },
        ),
        (
            3,
            Node::Call {
                function: "concat".into(),
                args: vec![0, 1, 2],
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
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            iteration: mapping::ScopeIteration::None,
            construction: ScopeConstruction::Constructed,
            filter: None,
            post_group_filter: None,
            group_by: None,
            group_adjacent_by: None,
            group_starting_with: None,
            group_ending_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_then_by: Vec::new(),
            sort_filter_order: Default::default(),
            windows: Vec::new(),
            iteration_output: Default::default(),
            bindings: vec![Binding {
                target_field: "full_name".into(),
                node: 3,
            }],
            dynamic_bindings: Vec::new(),
            children: vec![],
            dynamic_children: Vec::new(),
            merge_dynamic_fields: false,
        },
    };
    let source = Instance::Group(vec![
        (
            "first".into(),
            Instance::Scalar(Value::String("Jane".into())),
        ),
        ("last".into(), Instance::Scalar(Value::String("Doe".into()))),
    ]);

    let target = run(&project, &source).unwrap();
    assert_eq!(
        target.field("full_name").and_then(Instance::as_scalar),
        Some(&Value::String("Jane Doe".into()))
    );
}

#[test]
fn scalar_bindings_follow_repeating_target_shape() {
    let target = SchemaNode::group(
        "root",
        vec![SchemaNode::scalar("tag", ScalarType::String).repeating()],
    );
    let mut project = Project {
        source: dummy_schema(),
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph: graph_from(vec![(
            0,
            Node::SourceField {
                frame: None,
                path: vec!["tag".into()],
            },
        )]),
        root: Scope {
            bindings: vec![Binding {
                target_field: "tag".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    };

    let source = Instance::Group(vec![(
        "tag".into(),
        Instance::Scalar(Value::String("reference".into())),
    )]);
    assert_eq!(
        run(&project, &source),
        Ok(Instance::Group(vec![(
            "tag".into(),
            Instance::Repeated(vec![Instance::Scalar(Value::String("reference".into()))]),
        )]))
    );

    project
        .graph
        .nodes
        .insert(0, Node::Const { value: Value::Null });
    assert_eq!(
        run(&project, &Instance::Group(Vec::new())),
        Ok(Instance::Group(vec![(
            "tag".into(),
            Instance::Repeated(Vec::new()),
        )]))
    );
}

#[test]
fn missing_source_field_is_reported() {
    let graph = graph_from(vec![(
        0,
        Node::SourceField {
            frame: None,
            path: vec!["missing".into()],
        },
    )]);
    let project = Project {
        source: dummy_schema(),
        target: dummy_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            iteration: mapping::ScopeIteration::None,
            construction: ScopeConstruction::Constructed,
            filter: None,
            post_group_filter: None,
            group_by: None,
            group_adjacent_by: None,
            group_starting_with: None,
            group_ending_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_then_by: Vec::new(),
            sort_filter_order: Default::default(),
            windows: Vec::new(),
            iteration_output: Default::default(),
            bindings: vec![Binding {
                target_field: "out".into(),
                node: 0,
            }],
            dynamic_bindings: Vec::new(),
            children: vec![],
            dynamic_children: Vec::new(),
            merge_dynamic_fields: false,
        },
    };
    let err = run(&project, &Instance::Group(vec![])).unwrap_err();
    assert_eq!(err, EngineError::MissingSourceField("missing".to_string()));
}

#[test]
fn self_referential_node_is_a_cycle() {
    let graph = graph_from(vec![(
        0,
        Node::Call {
            function: "concat".into(),
            args: vec![0],
        },
    )]);
    let project = Project {
        source: dummy_schema(),
        target: dummy_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            iteration: mapping::ScopeIteration::None,
            construction: ScopeConstruction::Constructed,
            filter: None,
            post_group_filter: None,
            group_by: None,
            group_adjacent_by: None,
            group_starting_with: None,
            group_ending_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_then_by: Vec::new(),
            sort_filter_order: Default::default(),
            windows: Vec::new(),
            iteration_output: Default::default(),
            bindings: vec![Binding {
                target_field: "out".into(),
                node: 0,
            }],
            dynamic_bindings: Vec::new(),
            children: vec![],
            dynamic_children: Vec::new(),
            merge_dynamic_fields: false,
        },
    };
    let err = run(&project, &Instance::Group(vec![])).unwrap_err();
    assert_eq!(err, EngineError::Cycle(0));
}

/// The "hard part" this milestone is about: a nested repeating source
/// (Order -> Item) flattened into a single repeating target level, with
/// an Order-level field ("cust") broadcast into every produced row --
/// this is the shape of a real-world nested join.
#[test]
fn nested_repetition_flattens_with_broadcast_from_enclosing_scope() {
    let graph = graph_from(vec![
        (
            0,
            Node::SourceField {
                frame: None,
                path: vec!["cust".into()],
            },
        ),
        (
            1,
            Node::SourceField {
                frame: None,
                path: vec!["item_id".into()],
            },
        ),
        (
            2,
            Node::Position {
                collection: vec!["orders".into()],
            },
        ),
        (
            3,
            Node::Position {
                collection: vec!["items".into()],
            },
        ),
        (
            4,
            Node::SourceField {
                frame: None,
                path: vec!["keep".into()],
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
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            iteration: mapping::ScopeIteration::Source(vec!["orders".into(), "items".into()]),
            construction: ScopeConstruction::Constructed,
            filter: Some(4),
            post_group_filter: None,
            group_by: None,
            group_adjacent_by: None,
            group_starting_with: None,
            group_ending_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_then_by: Vec::new(),
            sort_filter_order: Default::default(),
            windows: Vec::new(),
            iteration_output: Default::default(),
            bindings: vec![
                Binding {
                    target_field: "cust".into(),
                    node: 0,
                },
                Binding {
                    target_field: "item_id".into(),
                    node: 1,
                },
                Binding {
                    target_field: "order_position".into(),
                    node: 2,
                },
                Binding {
                    target_field: "item_position".into(),
                    node: 3,
                },
            ],
            dynamic_bindings: Vec::new(),
            children: vec![],
            dynamic_children: Vec::new(),
            merge_dynamic_fields: false,
        },
    };

    let item = |id: &str, keep: bool| {
        Instance::Group(vec![
            ("item_id".into(), Instance::Scalar(Value::String(id.into()))),
            ("keep".into(), Instance::Scalar(Value::Bool(keep))),
        ])
    };
    let order = |cust: &str, items: Vec<Instance>| {
        Instance::Group(vec![
            ("cust".into(), Instance::Scalar(Value::String(cust.into()))),
            ("items".into(), Instance::Repeated(items)),
        ])
    };
    let source = Instance::Group(vec![(
        "orders".into(),
        Instance::Repeated(vec![
            order(
                "Jane",
                vec![item("A", false), item("B", true), item("C", true)],
            ),
            order("John", vec![item("D", false), item("E", true)]),
        ]),
    )]);

    let target = run(&project, &source).unwrap();
    let rows = target.as_repeated().unwrap();
    assert_eq!(rows.len(), 3);

    let row = |i: usize| &rows[i];
    let cust = |i: usize| row(i).field("cust").and_then(Instance::as_scalar).cloned();
    let item_id = |i: usize| {
        row(i)
            .field("item_id")
            .and_then(Instance::as_scalar)
            .cloned()
    };
    let position =
        |i: usize, field: &str| row(i).field(field).and_then(Instance::as_scalar).cloned();

    assert_eq!(cust(0), Some(Value::String("Jane".into())));
    assert_eq!(item_id(0), Some(Value::String("B".into())));
    assert_eq!(position(0, "order_position"), Some(Value::Int(1)));
    assert_eq!(position(0, "item_position"), Some(Value::Int(1)));
    assert_eq!(cust(1), Some(Value::String("Jane".into())));
    assert_eq!(item_id(1), Some(Value::String("C".into())));
    assert_eq!(position(1, "order_position"), Some(Value::Int(1)));
    assert_eq!(position(1, "item_position"), Some(Value::Int(2)));
    assert_eq!(cust(2), Some(Value::String("John".into())));
    assert_eq!(item_id(2), Some(Value::String("E".into())));
    assert_eq!(position(2, "order_position"), Some(Value::Int(2)));
    assert_eq!(position(2, "item_position"), Some(Value::Int(3)));
}

#[test]
fn if_only_evaluates_the_taken_branch() {
    let graph = graph_from(vec![
        (
            0,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("then".into()),
            },
        ),
        // A self-referential "else" branch would cycle if it were ever
        // evaluated -- this proves `If` short-circuits.
        (
            2,
            Node::Call {
                function: "concat".into(),
                args: vec![2],
            },
        ),
        (
            3,
            Node::If {
                condition: 0,
                then: 1,
                else_: 2,
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
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            iteration: mapping::ScopeIteration::None,
            construction: ScopeConstruction::Constructed,
            filter: None,
            post_group_filter: None,
            group_by: None,
            group_adjacent_by: None,
            group_starting_with: None,
            group_ending_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_then_by: Vec::new(),
            sort_filter_order: Default::default(),
            windows: Vec::new(),
            iteration_output: Default::default(),
            bindings: vec![Binding {
                target_field: "out".into(),
                node: 3,
            }],
            dynamic_bindings: Vec::new(),
            children: vec![],
            dynamic_children: Vec::new(),
            merge_dynamic_fields: false,
        },
    };
    let target = run(&project, &Instance::Group(vec![])).unwrap();
    assert_eq!(
        target.field("out").and_then(Instance::as_scalar),
        Some(&Value::String("then".into()))
    );
}

#[test]
fn value_map_falls_back_to_default_on_miss() {
    let graph = graph_from(vec![
        (
            0,
            Node::Const {
                value: Value::String("ZZ".into()),
            },
        ),
        (
            1,
            Node::ValueMap {
                input: 0,
                input_type: None,
                table: vec![(
                    Value::String("BD".into()),
                    Value::String("Balance Due".into()),
                )],
                default: Some(Value::String("Original".into())),
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
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            iteration: mapping::ScopeIteration::None,
            construction: ScopeConstruction::Constructed,
            filter: None,
            post_group_filter: None,
            group_by: None,
            group_adjacent_by: None,
            group_starting_with: None,
            group_ending_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_then_by: Vec::new(),
            sort_filter_order: Default::default(),
            windows: Vec::new(),
            iteration_output: Default::default(),
            bindings: vec![Binding {
                target_field: "out".into(),
                node: 1,
            }],
            dynamic_bindings: Vec::new(),
            children: vec![],
            dynamic_children: Vec::new(),
            merge_dynamic_fields: false,
        },
    };
    let target = run(&project, &Instance::Group(vec![])).unwrap();
    assert_eq!(
        target.field("out").and_then(Instance::as_scalar),
        Some(&Value::String("Original".into()))
    );
}

#[test]
fn value_map_without_a_default_returns_null_on_miss() {
    let graph = graph_from(vec![
        (
            0,
            Node::Const {
                value: Value::String("missing".into()),
            },
        ),
        (
            1,
            Node::ValueMap {
                input: 0,
                input_type: None,
                table: vec![(Value::String("known".into()), Value::String("value".into()))],
                default: None,
            },
        ),
    ]);
    let project = Project {
        source: dummy_schema(),
        target: SchemaNode::group(
            "target",
            vec![SchemaNode::scalar("out", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            bindings: vec![Binding {
                target_field: "out".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    };

    let target = run(&project, &Instance::Group(vec![])).unwrap();

    assert_eq!(
        target.field("out").and_then(Instance::as_scalar),
        Some(&Value::Null)
    );
}

#[test]
fn value_map_coerces_input_to_its_declared_type() {
    let graph = graph_from(vec![
        (
            0,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            1,
            Node::ValueMap {
                input: 0,
                input_type: Some(ScalarType::String),
                table: vec![(Value::String("1".into()), Value::String("January".into()))],
                default: None,
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
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            iteration: mapping::ScopeIteration::None,
            construction: ScopeConstruction::Constructed,
            filter: None,
            post_group_filter: None,
            group_by: None,
            group_adjacent_by: None,
            group_starting_with: None,
            group_ending_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_then_by: Vec::new(),
            sort_filter_order: Default::default(),
            windows: Vec::new(),
            iteration_output: Default::default(),
            bindings: vec![Binding {
                target_field: "out".into(),
                node: 1,
            }],
            dynamic_bindings: Vec::new(),
            children: vec![],
            dynamic_children: Vec::new(),
            merge_dynamic_fields: false,
        },
    };

    let target = run(&project, &Instance::Group(vec![])).unwrap();
    assert_eq!(
        target.field("out").and_then(Instance::as_scalar),
        Some(&Value::String("January".into()))
    );
}

#[test]
fn scope_filter_drops_items_that_fail_the_predicate() {
    let graph = graph_from(vec![
        (
            0,
            Node::SourceField {
                frame: None,
                path: vec!["age".into()],
            },
        ),
        (
            1,
            Node::Const {
                value: Value::Int(18),
            },
        ),
        (
            2,
            Node::Call {
                function: "greater_or_equal".into(),
                args: vec![0, 1],
            },
        ),
        (
            3,
            Node::Position {
                collection: Vec::new(),
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
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            iteration: mapping::ScopeIteration::Source(vec![]),
            construction: ScopeConstruction::Constructed,
            filter: Some(2),
            post_group_filter: None,
            group_by: None,
            group_adjacent_by: None,
            group_starting_with: None,
            group_ending_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_then_by: Vec::new(),
            sort_filter_order: Default::default(),
            windows: Vec::new(),
            iteration_output: Default::default(),
            bindings: vec![
                Binding {
                    target_field: "age".into(),
                    node: 0,
                },
                Binding {
                    target_field: "position".into(),
                    node: 3,
                },
            ],
            dynamic_bindings: Vec::new(),
            children: vec![],
            dynamic_children: Vec::new(),
            merge_dynamic_fields: false,
        },
    };
    let person =
        |age: i64| Instance::Group(vec![("age".into(), Instance::Scalar(Value::Int(age)))]);
    let source = Instance::Repeated(vec![person(29), person(17), person(41)]);

    let target = run(&project, &source).unwrap();
    let ages: Vec<_> = target
        .as_repeated()
        .unwrap()
        .iter()
        .map(|row| row.field("age").and_then(Instance::as_scalar).cloned())
        .collect();
    assert_eq!(ages, vec![Some(Value::Int(29)), Some(Value::Int(41))]);
    let positions: Vec<_> = target
        .as_repeated()
        .unwrap()
        .iter()
        .map(|row| row.field("position").and_then(Instance::as_scalar).cloned())
        .collect();
    assert_eq!(positions, vec![Some(Value::Int(1)), Some(Value::Int(2))]);
}

#[test]
fn scope_sort_and_first_window_are_stable_and_reindex_positions() {
    let graph = graph_from(vec![
        (
            0,
            Node::SourceField {
                frame: None,
                path: vec!["score".into()],
            },
        ),
        (
            1,
            Node::SourceField {
                frame: None,
                path: vec!["name".into()],
            },
        ),
        (
            2,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            3,
            Node::Position {
                collection: Vec::new(),
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
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            iteration: mapping::ScopeIteration::Source(Vec::new()),
            sort_by: Some(0),
            sort_descending: true,
            windows: vec![SequenceWindow::First { count: 2 }],
            bindings: vec![
                Binding {
                    target_field: "name".into(),
                    node: 1,
                },
                Binding {
                    target_field: "position".into(),
                    node: 3,
                },
            ],
            ..Scope::default()
        },
    };
    let row = |name: &str, score: i64| {
        Instance::Group(vec![
            ("name".into(), Instance::Scalar(Value::String(name.into()))),
            ("score".into(), Instance::Scalar(Value::Int(score))),
        ])
    };
    let source = Instance::Repeated(vec![
        row("low", 1),
        row("first-high", 5),
        row("second-high", 5),
        row("middle", 3),
    ]);

    let target = run(&project, &source).unwrap();
    let rows = target.as_repeated().unwrap();
    let values: Vec<_> = rows
        .iter()
        .map(|row| {
            (
                row.field("name").and_then(Instance::as_scalar).cloned(),
                row.field("position").and_then(Instance::as_scalar).cloned(),
            )
        })
        .collect();
    assert_eq!(
        values,
        vec![
            (
                Some(Value::String("first-high".into())),
                Some(Value::Int(1))
            ),
            (
                Some(Value::String("second-high".into())),
                Some(Value::Int(2))
            ),
        ]
    );
}

#[test]
fn scope_sort_uses_mixed_direction_tie_breakers() {
    let graph = graph_from(vec![
        (
            0,
            Node::SourceField {
                frame: None,
                path: vec!["score".into()],
            },
        ),
        (
            1,
            Node::SourceField {
                frame: None,
                path: vec!["last".into()],
            },
        ),
        (
            2,
            Node::SourceField {
                frame: None,
                path: vec!["first".into()],
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
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            iteration: mapping::ScopeIteration::Source(Vec::new()),
            sort_by: Some(0),
            sort_descending: true,
            sort_then_by: vec![
                mapping::SortKey {
                    node: 1,
                    descending: false,
                },
                mapping::SortKey {
                    node: 2,
                    descending: false,
                },
            ],
            bindings: vec![Binding {
                target_field: "first".into(),
                node: 2,
            }],
            ..Scope::default()
        },
    };
    let row = |first: &str, last: &str, score: i64| {
        Instance::Group(vec![
            (
                "first".into(),
                Instance::Scalar(Value::String(first.into())),
            ),
            ("last".into(), Instance::Scalar(Value::String(last.into()))),
            ("score".into(), Instance::Scalar(Value::Int(score))),
        ])
    };
    let source = Instance::Repeated(vec![
        row("Susan", "Schmitt", 2),
        row("Alex", "Martin", 2),
        row("Fred", "Landis", 2),
        row("Joe", "Martin", 2),
        row("Lower", "Able", 1),
    ]);

    let output = run(&project, &source).unwrap();
    let names = output
        .as_repeated()
        .unwrap()
        .iter()
        .filter_map(|row| row.field("first").and_then(Instance::as_scalar))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        [
            Value::String("Fred".into()),
            Value::String("Alex".into()),
            Value::String("Joe".into()),
            Value::String("Susan".into()),
            Value::String("Lower".into()),
        ]
    );
}

#[test]
fn scope_can_filter_in_source_order_before_sorting_survivors() {
    let graph = graph_from(vec![
        (
            0,
            Node::SourceField {
                frame: None,
                path: vec!["score".into()],
            },
        ),
        (
            1,
            Node::SourceField {
                frame: None,
                path: vec!["name".into()],
            },
        ),
        (
            2,
            Node::Position {
                collection: Vec::new(),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            4,
            Node::Call {
                function: "less_or_equal".into(),
                args: vec![2, 3],
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
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            iteration: mapping::ScopeIteration::Source(Vec::new()),
            filter: Some(4),
            post_group_filter: None,
            sort_by: Some(0),
            sort_descending: true,
            sort_filter_order: mapping::SortFilterOrder::FilterThenSort,
            bindings: vec![Binding {
                target_field: "name".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    };
    let row = |name: &str, score: i64| {
        Instance::Group(vec![
            ("name".into(), Instance::Scalar(Value::String(name.into()))),
            ("score".into(), Instance::Scalar(Value::Int(score))),
        ])
    };
    let source = Instance::Repeated(vec![row("low", 1), row("high", 5), row("middle", 3)]);

    let target = run(&project, &source).unwrap();
    let names = target
        .as_repeated()
        .unwrap()
        .iter()
        .map(|row| row.field("name").and_then(Instance::as_scalar).cloned())
        .collect::<Vec<_>>();
    assert_eq!(
        names,
        vec![
            Some(Value::String("high".into())),
            Some(Value::String("low".into()))
        ]
    );
}

#[test]
fn sort_order_places_null_first_for_ascending_and_last_when_reversed() {
    assert_eq!(
        value_ordering(&Value::Null, &Value::String("value".into())),
        Some(std::cmp::Ordering::Less)
    );
    assert_eq!(
        value_ordering(&Value::String("value".into()), &Value::Null)
            .map(std::cmp::Ordering::reverse),
        Some(std::cmp::Ordering::Less)
    );
}

/// A field path crossing a repeating element that no scope iterates
/// reads the first item (the visual-mapper convention for wiring a
/// repeating source into a singular target).
#[test]
fn uniterated_repeating_elements_resolve_to_their_first_item() {
    let graph = graph_from(vec![(
        0,
        Node::SourceField {
            frame: None,
            path: vec!["Address".into(), "city".into()],
        },
    )]);
    let project = Project {
        source: dummy_schema(),
        target: dummy_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            iteration: mapping::ScopeIteration::None,
            construction: ScopeConstruction::Constructed,
            filter: None,
            post_group_filter: None,
            group_by: None,
            group_adjacent_by: None,
            group_starting_with: None,
            group_ending_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_then_by: Vec::new(),
            sort_filter_order: Default::default(),
            windows: Vec::new(),
            iteration_output: Default::default(),
            bindings: vec![Binding {
                target_field: "City".into(),
                node: 0,
            }],
            dynamic_bindings: Vec::new(),
            children: vec![],
            dynamic_children: Vec::new(),
            merge_dynamic_fields: false,
        },
    };
    let address = |city: &str| {
        Instance::Group(vec![(
            "city".into(),
            Instance::Scalar(Value::String(city.into())),
        )])
    };
    let source = Instance::Group(vec![(
        "Address".into(),
        Instance::Repeated(vec![address("Vienna"), address("Boston")]),
    )]);

    let target = run(&project, &source).unwrap();
    assert_eq!(
        target.field("City").and_then(Instance::as_scalar),
        Some(&Value::String("Vienna".into()))
    );
}

/// A grouped scope produces one target item per distinct key (in
/// first-seen order); inside it, bindings read the first member and
/// aggregates reduce the group -- whether addressed as `[]` or by the
/// collection's own name (the group shadows the ungrouped data).

#[test]
fn lookup_joins_rows_against_an_extra_source() {
    let graph = graph_from(vec![
        (
            0,
            Node::SourceField {
                frame: None,
                path: vec!["customer_id".into()],
            },
        ),
        (
            1,
            Node::Lookup {
                collection: vec!["customers".into()],
                key: vec!["id".into()],
                matches: 0,
                value: vec!["name".into()],
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
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            iteration: mapping::ScopeIteration::Source(vec![]),
            construction: ScopeConstruction::Constructed,
            filter: None,
            post_group_filter: None,
            group_by: None,
            group_adjacent_by: None,
            group_starting_with: None,
            group_ending_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_then_by: Vec::new(),
            sort_filter_order: Default::default(),
            windows: Vec::new(),
            iteration_output: Default::default(),
            bindings: vec![
                Binding {
                    target_field: "customer_id".into(),
                    node: 0,
                },
                Binding {
                    target_field: "customer_name".into(),
                    node: 1,
                },
            ],
            dynamic_bindings: Vec::new(),
            children: vec![],
            dynamic_children: Vec::new(),
            merge_dynamic_fields: false,
        },
    };

    let order = |cid: i64| {
        Instance::Group(vec![(
            "customer_id".into(),
            Instance::Scalar(Value::Int(cid)),
        )])
    };
    let customer = |id: i64, name: &str| {
        Instance::Group(vec![
            ("id".into(), Instance::Scalar(Value::Int(id))),
            ("name".into(), Instance::Scalar(Value::String(name.into()))),
        ])
    };
    let source = Instance::Repeated(vec![order(2), order(1), order(99)]);
    let customers = Instance::Repeated(vec![customer(1, "Jane"), customer(2, "John")]);

    let target =
        run_with_sources(&project, &source, vec![("customers".into(), customers)]).unwrap();
    let names: Vec<_> = target
        .as_repeated()
        .unwrap()
        .iter()
        .map(|row| {
            row.field("customer_name")
                .and_then(Instance::as_scalar)
                .cloned()
        })
        .collect();
    assert_eq!(
        names,
        vec![
            Some(Value::String("John".into())),
            Some(Value::String("Jane".into())),
            Some(Value::Null),
        ]
    );
}

#[test]
fn collection_find_evaluates_composite_predicates_and_values_per_item() {
    let person_field = |path: &[&str]| Node::SourceField {
        frame: Some(vec!["departments".into(), "people".into()]),
        path: path.iter().map(|segment| (*segment).to_string()).collect(),
    };
    let graph = graph_from(vec![
        (
            0,
            Node::SourceField {
                frame: Some(vec!["departments".into()]),
                path: vec!["office".into()],
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("HQ".into()),
            },
        ),
        (
            2,
            Node::Call {
                function: "equal".into(),
                args: vec![0, 1],
            },
        ),
        (3, person_field(&["first"])),
        (
            4,
            Node::Const {
                value: Value::String("Ada".into()),
            },
        ),
        (
            5,
            Node::Call {
                function: "equal".into(),
                args: vec![3, 4],
            },
        ),
        (
            6,
            Node::Call {
                function: "and".into(),
                args: vec![2, 5],
            },
        ),
        (7, person_field(&["title"])),
        (8, person_field(&["email"])),
        (
            9,
            Node::Call {
                function: "concat".into(),
                args: vec![7, 8],
            },
        ),
        (
            10,
            Node::CollectionFind {
                collection: vec!["departments".into(), "people".into()],
                predicate: 6,
                value: 9,
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
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            bindings: vec![Binding {
                target_field: "details".into(),
                node: 10,
            }],
            ..Scope::default()
        },
    };
    let person = |first: &str, title: &str, email: &str| {
        Instance::Group(vec![
            (
                "first".into(),
                Instance::Scalar(Value::String(first.into())),
            ),
            (
                "title".into(),
                Instance::Scalar(Value::String(title.into())),
            ),
            (
                "email".into(),
                Instance::Scalar(Value::String(email.into())),
            ),
        ])
    };
    let department = |office: &str, people: Vec<Instance>| {
        Instance::Group(vec![
            (
                "office".into(),
                Instance::Scalar(Value::String(office.into())),
            ),
            ("people".into(), Instance::Repeated(people)),
        ])
    };
    let source = Instance::Group(vec![(
        "departments".into(),
        Instance::Repeated(vec![
            department(
                "Remote",
                vec![person("Ada", "Wrong: ", "remote@example.test")],
            ),
            department(
                "HQ",
                vec![
                    person("Grace", "Director: ", "grace@example.test"),
                    person("Ada", "Engineer: ", "ada@example.test"),
                ],
            ),
        ]),
    )]);

    let output = run(&project, &source).unwrap();
    assert_eq!(
        output.field("details").and_then(Instance::as_scalar),
        Some(&Value::String("Engineer: ada@example.test".to_string()))
    );
}

/// A scope can iterate a named extra source directly: its path falls
/// back outward past the primary source to the extras frame.
#[test]
fn scope_source_path_reaches_an_extra_source() {
    let graph = graph_from(vec![(
        0,
        Node::SourceField {
            frame: None,
            path: vec!["name".into()],
        },
    )]);
    let project = Project {
        source: dummy_schema(),
        target: dummy_schema(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            target_field: String::new(),
            iteration: mapping::ScopeIteration::Source(vec!["customers".into()]),
            construction: ScopeConstruction::Constructed,
            filter: None,
            post_group_filter: None,
            group_by: None,
            group_adjacent_by: None,
            group_starting_with: None,
            group_ending_with: None,
            group_into_blocks: None,
            sort_by: None,
            sort_descending: false,
            sort_then_by: Vec::new(),
            sort_filter_order: Default::default(),
            windows: Vec::new(),
            iteration_output: Default::default(),
            bindings: vec![Binding {
                target_field: "name".into(),
                node: 0,
            }],
            dynamic_bindings: Vec::new(),
            children: vec![],
            dynamic_children: Vec::new(),
            merge_dynamic_fields: false,
        },
    };

    let customers = Instance::Repeated(vec![Instance::Group(vec![(
        "name".into(),
        Instance::Scalar(Value::String("Jane".into())),
    )])]);
    let source = Instance::Group(vec![]);

    let target =
        run_with_sources(&project, &source, vec![("customers".into(), customers)]).unwrap();
    assert_eq!(target.as_repeated().map(<[Instance]>::len), Some(1));
}

#[test]
fn recursive_collect_populates_a_repeating_scalar_target() {
    let project = recursive_collect_project();
    let issues = validate(&project);
    assert!(issues.is_empty(), "{issues:?}");
    let source = directory(
        "root",
        &["top.txt"],
        vec![directory("child", &["nested.txt"], Vec::new())],
    );

    assert_eq!(
        run(&project, &source),
        Ok(Instance::Group(vec![(
            "File".into(),
            Instance::Repeated(vec![
                Instance::Scalar(Value::String("\\root\\top.txt".into())),
                Instance::Scalar(Value::String("\\root\\child\\nested.txt".into())),
            ]),
        )]))
    );
}

#[test]
fn recursive_collect_has_a_typed_depth_limit() {
    let project = recursive_collect_project();
    let mut source = directory("leaf", &[], Vec::new());
    for depth in 0..=super::sequence::MAX_RECURSIVE_SEQUENCE_DEPTH {
        source = directory(&format!("level-{depth}"), &[], vec![source]);
    }

    assert_eq!(
        run(&project, &source),
        Err(EngineError::RecursiveSequenceDepth {
            limit: super::sequence::MAX_RECURSIVE_SEQUENCE_DEPTH,
        })
    );
}

fn recursive_collect_project() -> Project {
    let source = SchemaNode::group(
        "directory",
        vec![
            SchemaNode::scalar("name", ScalarType::String),
            SchemaNode::group("file", vec![SchemaNode::scalar("name", ScalarType::String)])
                .repeating(),
            SchemaNode::recursive_group("directory", "directory").repeating(),
        ],
    );
    let target = SchemaNode::group(
        "FileList",
        vec![SchemaNode::scalar("File", ScalarType::String).repeating()],
    );
    let graph = graph_from(vec![
        (
            0,
            Node::Const {
                value: Value::String(String::new()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("\\".into()),
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
    Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "File".into(),
                iteration: mapping::ScopeIteration::Sequence(SequenceExpr::RecursiveCollect {
                    collection: Vec::new(),
                    children: vec!["directory".into()],
                    descent_value: vec!["name".into()],
                    values: vec!["file".into()],
                    value: vec!["name".into()],
                    prefix: 0,
                    separator: 1,
                    item: 2,
                }),
                construction: ScopeConstruction::Scalar { value: 2 },
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn directory(name: &str, files: &[&str], directories: Vec<Instance>) -> Instance {
    Instance::Group(vec![
        (
            "name".into(),
            Instance::Scalar(Value::String(name.to_string())),
        ),
        (
            "file".into(),
            Instance::Repeated(
                files
                    .iter()
                    .map(|name| {
                        Instance::Group(vec![(
                            "name".into(),
                            Instance::Scalar(Value::String((*name).to_string())),
                        )])
                    })
                    .collect(),
            ),
        ),
        ("directory".into(), Instance::Repeated(directories)),
    ])
}
