use super::*;
use ir::SchemaNode;
use mapping::{Binding, Graph, Node, Scope};

fn project() -> Project {
    Project {
        source: SchemaNode::group("Input", vec![]),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::scalar("Correlation", ScalarType::String),
                SchemaNode::scalar("Control", ScalarType::Int),
                SchemaNode::scalar("Test", ScalarType::Bool),
                SchemaNode::scalar("Amount", ScalarType::Float),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    1,
                    Node::RuntimeParameter {
                        name: "correlation_id".into(),
                        ty: ScalarType::String,
                        preview: None,
                    },
                ),
                (
                    2,
                    Node::RuntimeParameter {
                        name: "control_number".into(),
                        ty: ScalarType::Int,
                        preview: None,
                    },
                ),
                (
                    3,
                    Node::RuntimeParameter {
                        name: "test_mode".into(),
                        ty: ScalarType::Bool,
                        preview: None,
                    },
                ),
                (
                    4,
                    Node::RuntimeParameter {
                        name: "amount".into(),
                        ty: ScalarType::Float,
                        preview: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Correlation".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Control".into(),
                    node: 2,
                },
                Binding {
                    target_field: "Test".into(),
                    node: 3,
                },
                Binding {
                    target_field: "Amount".into(),
                    node: 4,
                },
            ],
            ..Scope::default()
        },
    }
}

fn source() -> Instance {
    Instance::Group(vec![])
}

#[test]
fn typed_runtime_parameters_execute_with_bounded_scalar_coercion() {
    let project = project();
    assert!(validate(&project).is_empty());

    let mut parameters = RuntimeParameters::new();
    parameters
        .insert("correlation_id", Value::String("txn-17".into()))
        .unwrap();
    parameters
        .insert("control_number", Value::String("42".into()))
        .unwrap();
    parameters.insert("test_mode", Value::Bool(true)).unwrap();
    parameters.insert("amount", Value::Int(125)).unwrap();
    let execution =
        ExecutionContext::new(Path::new("mapping.ferrule")).with_parameters(&parameters);

    assert_eq!(
        run_with_context(&project, &source(), &execution).unwrap(),
        Instance::Group(vec![
            (
                "Correlation".into(),
                Instance::Scalar(Value::String("txn-17".into())),
            ),
            ("Control".into(), Instance::Scalar(Value::Int(42))),
            ("Test".into(), Instance::Scalar(Value::Bool(true))),
            ("Amount".into(), Instance::Scalar(Value::Float(125.0))),
        ])
    );
}

#[test]
fn missing_and_wrong_typed_parameters_are_distinct() {
    let project = project();
    let empty = RuntimeParameters::new();
    let execution = ExecutionContext::new(Path::new("mapping.ferrule")).with_parameters(&empty);
    assert_eq!(
        run_with_context(&project, &source(), &execution),
        Err(EngineError::MissingRuntimeParameter {
            node: 1,
            name: "correlation_id".into(),
        })
    );

    let mut parameters = RuntimeParameters::new();
    parameters
        .insert("correlation_id", Value::String("txn-17".into()))
        .unwrap();
    parameters
        .insert("control_number", Value::Bool(false))
        .unwrap();
    parameters.insert("test_mode", Value::Bool(true)).unwrap();
    parameters.insert("amount", Value::Int(125)).unwrap();
    let execution =
        ExecutionContext::new(Path::new("mapping.ferrule")).with_parameters(&parameters);
    assert_eq!(
        run_with_context(&project, &source(), &execution),
        Err(EngineError::RuntimeParameterType {
            node: 2,
            name: "control_number".into(),
            expected: ScalarType::Int,
            found: "bool",
        })
    );
}

#[test]
fn runtime_parameter_sets_reject_ambiguous_and_unbounded_inputs() {
    let mut parameters = RuntimeParameters::new();
    assert_eq!(
        parameters.insert("", Value::Null),
        Err(RuntimeParameterError::EmptyName)
    );
    assert_eq!(
        parameters.insert("bad\0name", Value::Null),
        Err(RuntimeParameterError::NameContainsNul)
    );
    assert_eq!(
        parameters.insert(
            "x".repeat(mapping::MAX_RUNTIME_PARAMETER_NAME_BYTES + 1),
            Value::Null,
        ),
        Err(RuntimeParameterError::NameTooLong {
            limit: mapping::MAX_RUNTIME_PARAMETER_NAME_BYTES,
        })
    );
    parameters.insert("duplicate", Value::Int(1)).unwrap();
    assert_eq!(
        parameters.insert("duplicate", Value::Int(2)),
        Err(RuntimeParameterError::Duplicate {
            name: "duplicate".into(),
        })
    );
    assert_eq!(
        parameters.insert(
            "large",
            Value::String("x".repeat(MAX_RUNTIME_PARAMETER_STRING_BYTES + 1)),
        ),
        Err(RuntimeParameterError::StringTooLong {
            name: "large".into(),
            limit: MAX_RUNTIME_PARAMETER_STRING_BYTES,
        })
    );
}

#[test]
fn invalid_runtime_parameter_declarations_fail_validation() {
    for name in [
        String::new(),
        "bad\0name".into(),
        "x".repeat(mapping::MAX_RUNTIME_PARAMETER_NAME_BYTES + 1),
    ] {
        let mut project = project();
        project.graph.nodes.insert(
            1,
            Node::RuntimeParameter {
                name,
                ty: ScalarType::String,
                preview: None,
            },
        );
        assert_eq!(validate(&project).len(), 1);
    }
}

fn optional_project(default: Node) -> Project {
    let mut project = project();
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Control", ScalarType::Int)],
    );
    project.graph.nodes = [
        (1, default),
        (
            2,
            Node::RuntimeParameterDefault {
                name: "control_number".into(),
                ty: ScalarType::Int,
                default: 1,
                preview: None,
            },
        ),
    ]
    .into_iter()
    .collect();
    project.root.bindings = vec![Binding {
        target_field: "Control".into(),
        node: 2,
    }];
    project
}

#[test]
fn optional_runtime_parameter_uses_default_only_when_name_is_absent() {
    let project = optional_project(Node::Const {
        value: Value::String("7".into()),
    });
    assert!(validate(&project).is_empty());
    assert_eq!(
        run(&project, &source()).unwrap(),
        Instance::Group(vec![("Control".into(), Instance::Scalar(Value::Int(7)))])
    );

    let mut supplied = RuntimeParameters::new();
    supplied
        .insert("control_number", Value::String("42".into()))
        .unwrap();
    let context = ExecutionContext::new(Path::new("mapping.ferrule")).with_parameters(&supplied);
    assert_eq!(
        run_with_context(&project, &source(), &context).unwrap(),
        Instance::Group(vec![("Control".into(), Instance::Scalar(Value::Int(42)))])
    );

    let mut supplied_null = RuntimeParameters::new();
    supplied_null.insert("control_number", Value::Null).unwrap();
    let context =
        ExecutionContext::new(Path::new("mapping.ferrule")).with_parameters(&supplied_null);
    assert_eq!(
        run_with_context(&project, &source(), &context).unwrap(),
        Instance::Group(vec![("Control".into(), Instance::Scalar(Value::Null))]),
    );

    let mut wrong = RuntimeParameters::new();
    wrong.insert("control_number", Value::Bool(false)).unwrap();
    let context = ExecutionContext::new(Path::new("mapping.ferrule")).with_parameters(&wrong);
    assert_eq!(
        run_with_context(&project, &source(), &context),
        Err(EngineError::RuntimeParameterType {
            node: 2,
            name: "control_number".into(),
            expected: ScalarType::Int,
            found: "bool",
        })
    );
}

#[test]
fn optional_runtime_parameter_skips_failing_default_for_supplied_value() {
    let project = optional_project(Node::RuntimeParameter {
        name: "missing_default".into(),
        ty: ScalarType::Int,
        preview: None,
    });
    assert!(validate(&project).is_empty());

    let mut supplied = RuntimeParameters::new();
    supplied.insert("control_number", Value::Int(9)).unwrap();
    let context = ExecutionContext::new(Path::new("mapping.ferrule")).with_parameters(&supplied);
    assert_eq!(
        run_with_context(&project, &source(), &context).unwrap(),
        Instance::Group(vec![("Control".into(), Instance::Scalar(Value::Int(9)))])
    );
    assert_eq!(
        run(&project, &source()),
        Err(EngineError::MissingRuntimeParameter {
            node: 1,
            name: "missing_default".into(),
        })
    );
}

fn preview_context() -> ExecutionContext<'static> {
    ExecutionContext::new(Path::new("mapping.ferrule")).with_purpose(ExecutionPurpose::Preview)
}

#[test]
fn preview_purpose_coerces_raw_values_and_normal_run_still_requires_host_inputs() {
    let mut project = project();
    for (id, lexical) in [(1, ""), (2, " 17 "), (3, "1"), (4, "5.5")] {
        let Node::RuntimeParameter { preview, .. } = project.graph.nodes.get_mut(&id).unwrap()
        else {
            panic!("host node");
        };
        *preview = Some(lexical.into());
    }
    assert!(validate(&project).is_empty());
    assert_eq!(
        ExecutionContext::new(Path::new("mapping.ferrule")).purpose(),
        ExecutionPurpose::Run
    );
    assert_eq!(
        ExecutionContext::with_main_mapping_file_path(Path::new("child"), Path::new("main"))
            .purpose(),
        ExecutionPurpose::Run
    );
    assert_eq!(preview_context().purpose(), ExecutionPurpose::Preview);
    assert!(matches!(
        run(&project, &source()),
        Err(EngineError::MissingRuntimeParameter { node: 1, .. })
    ));
    assert_eq!(
        run_with_context(&project, &source(), &preview_context()).unwrap(),
        Instance::Group(vec![
            (
                "Correlation".into(),
                Instance::Scalar(Value::String("".into()))
            ),
            ("Control".into(), Instance::Scalar(Value::Int(17))),
            ("Test".into(), Instance::Scalar(Value::Bool(true))),
            ("Amount".into(), Instance::Scalar(Value::Float(5.5))),
        ])
    );
}

#[test]
fn preview_skips_connected_default_but_supplied_host_null_and_errors_win() {
    let mut project = optional_project(Node::RuntimeParameter {
        name: "missing_default".into(),
        ty: ScalarType::Int,
        preview: None,
    });
    let Node::RuntimeParameterDefault { preview, .. } = project.graph.nodes.get_mut(&2).unwrap()
    else {
        panic!("optional node");
    };
    *preview = Some("17".into());
    assert!(matches!(
        run(&project, &source()),
        Err(EngineError::MissingRuntimeParameter { node: 1, .. })
    ));
    assert_eq!(
        run_with_context(&project, &source(), &preview_context())
            .unwrap()
            .field("Control")
            .unwrap()
            .as_scalar(),
        Some(&Value::Int(17))
    );
    for (supplied, expected) in [(Value::Int(9), Value::Int(9)), (Value::Null, Value::Null)] {
        let mut parameters = RuntimeParameters::new();
        parameters.insert("control_number", supplied).unwrap();
        let execution = preview_context().with_parameters(&parameters);
        assert_eq!(
            run_with_context(&project, &source(), &execution)
                .unwrap()
                .field("Control")
                .unwrap()
                .as_scalar(),
            Some(&expected)
        );
    }
    let mut parameters = RuntimeParameters::new();
    parameters
        .insert("control_number", Value::Bool(false))
        .unwrap();
    assert!(matches!(
        run_with_context(
            &project,
            &source(),
            &preview_context().with_parameters(&parameters)
        ),
        Err(EngineError::RuntimeParameterType {
            node: 2,
            found: "bool",
            ..
        })
    ));
}

#[test]
fn malformed_or_empty_numeric_preview_is_a_typed_error_without_default_fallback() {
    let mut project = optional_project(Node::Const {
        value: Value::Int(7),
    });
    for lexical in ["not an integer", "", "999999999999999999999"] {
        let Node::RuntimeParameterDefault { preview, .. } =
            project.graph.nodes.get_mut(&2).unwrap()
        else {
            panic!("optional node");
        };
        *preview = Some(lexical.into());
        assert_eq!(
            run(&project, &source())
                .unwrap()
                .field("Control")
                .unwrap()
                .as_scalar(),
            Some(&Value::Int(7))
        );
        assert!(matches!(
            run_with_context(&project, &source(), &preview_context()),
            Err(EngineError::RuntimeParameterType {
                node: 2,
                found: "string",
                ..
            })
        ));
    }
    let Node::RuntimeParameterDefault { preview, .. } = project.graph.nodes.get_mut(&2).unwrap()
    else {
        panic!("optional node");
    };
    *preview = None;
    assert_eq!(
        run_with_context(&project, &source(), &preview_context())
            .unwrap()
            .field("Control")
            .unwrap()
            .as_scalar(),
        Some(&Value::Int(7))
    );
}

#[test]
fn previews_are_local_to_same_named_nodes_until_a_host_value_is_supplied() {
    let mut project = project();
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("First", ScalarType::Int),
            SchemaNode::scalar("Second", ScalarType::Int),
        ],
    );
    project.graph.nodes = [
        (
            1,
            Node::RuntimeParameter {
                name: "same".into(),
                ty: ScalarType::Int,
                preview: Some("1".into()),
            },
        ),
        (
            2,
            Node::RuntimeParameter {
                name: "same".into(),
                ty: ScalarType::Int,
                preview: Some("2".into()),
            },
        ),
    ]
    .into_iter()
    .collect();
    project.root.bindings = vec![
        Binding {
            target_field: "First".into(),
            node: 1,
        },
        Binding {
            target_field: "Second".into(),
            node: 2,
        },
    ];
    let output = run_with_context(&project, &source(), &preview_context()).unwrap();
    assert_eq!(
        output.field("First").unwrap().as_scalar(),
        Some(&Value::Int(1))
    );
    assert_eq!(
        output.field("Second").unwrap().as_scalar(),
        Some(&Value::Int(2))
    );
    let mut parameters = RuntimeParameters::new();
    parameters.insert("same", Value::Int(9)).unwrap();
    let output = run_with_context(
        &project,
        &source(),
        &preview_context().with_parameters(&parameters),
    )
    .unwrap();
    for field in ["First", "Second"] {
        assert_eq!(
            output.field(field).unwrap().as_scalar(),
            Some(&Value::Int(9))
        );
    }
}

#[test]
fn oversized_preview_fails_only_when_preview_execution_uses_it() {
    let mut project = optional_project(Node::Const {
        value: Value::Int(7),
    });
    let Node::RuntimeParameterDefault { preview, .. } = project.graph.nodes.get_mut(&2).unwrap()
    else {
        panic!("optional node");
    };
    *preview = Some("X".repeat(MAX_RUNTIME_PARAMETER_STRING_BYTES + 1));
    assert!(validate(&project).is_empty());
    assert!(run(&project, &source()).is_ok());
    assert!(matches!(
        run_with_context(&project, &source(), &preview_context()),
        Err(EngineError::RuntimeParameterPreviewTooLong { node: 2, .. })
    ));
    let mut parameters = RuntimeParameters::new();
    parameters.insert("control_number", Value::Null).unwrap();
    assert!(
        run_with_context(
            &project,
            &source(),
            &preview_context().with_parameters(&parameters)
        )
        .is_ok()
    );
}
