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
                    },
                ),
                (
                    2,
                    Node::RuntimeParameter {
                        name: "control_number".into(),
                        ty: ScalarType::Int,
                    },
                ),
                (
                    3,
                    Node::RuntimeParameter {
                        name: "test_mode".into(),
                        ty: ScalarType::Bool,
                    },
                ),
                (
                    4,
                    Node::RuntimeParameter {
                        name: "amount".into(),
                        ty: ScalarType::Float,
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
