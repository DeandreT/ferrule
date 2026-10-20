use std::path::Path;

use engine::{
    EngineError, ExecutionContext, ExecutionPurpose, PrimaryFilenameEvaluationError,
    RuntimeParameters, evaluate_primary_filename,
};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DocumentProject, Graph, Node, PrimaryFilenameError, PrimarySourcePlan, Project, Scope,
};

fn document(filename: Node) -> DocumentProject {
    DocumentProject::new(
        Project {
            source: SchemaNode::group("Input", vec![]),
            target: SchemaNode::group(
                "Output",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
            source_path: Some("never-load-preview.csv".into()),
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: vec![],
            extra_targets: vec![],
            failure_rules: vec![],
            user_functions: Default::default(),
            graph: Graph {
                nodes: [
                    (17, filename),
                    (
                        99,
                        Node::RuntimeParameter {
                            name: "unrelated_missing_parameter".into(),
                            ty: ScalarType::String,
                            preview: None,
                        },
                    ),
                ]
                .into(),
            },
            root: Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 99,
                }],
                ..Default::default()
            },
        },
        PrimarySourcePlan { filename: 17 },
    )
}

fn parameter(name: &str) -> DocumentProject {
    document(Node::RuntimeParameter {
        name: name.into(),
        ty: ScalarType::String,
        preview: Some("never-use-parameter-preview.csv".into()),
    })
}

#[test]
fn literal_text_is_exact_and_does_not_run_the_mapping() {
    let execution = ExecutionContext::new(Path::new("mapping.ferrule"));
    for text in [
        " ./input-*.csv ",
        "",
        "https://example.invalid/data",
        "a\0b",
    ] {
        let document = document(Node::Const {
            value: Value::String(text.into()),
        });
        assert_eq!(
            evaluate_primary_filename(&document, &execution),
            Ok(text.into())
        );
    }
}

#[test]
fn host_parameter_uses_normal_scalar_coercion_without_trimming_text() {
    for (value, expected) in [
        (
            Value::String(" ./selected-*.csv ".into()),
            " ./selected-*.csv ",
        ),
        (Value::String("".into()), ""),
        (Value::Int(42), "42"),
        (Value::Bool(false), "false"),
        (Value::Float(2.5), "2.5"),
    ] {
        let mut parameters = RuntimeParameters::new();
        parameters.insert(" input-é ", value).unwrap();
        let execution =
            ExecutionContext::new(Path::new("mapping.ferrule")).with_parameters(&parameters);
        assert_eq!(
            evaluate_primary_filename(&parameter(" input-é "), &execution),
            Ok(expected.into())
        );
    }
}

#[test]
fn previews_never_supply_missing_bindings_even_in_preview_purpose() {
    let document = parameter("input");
    for execution in [
        ExecutionContext::new(Path::new("mapping.ferrule")),
        ExecutionContext::new(Path::new("mapping.ferrule")).with_purpose(ExecutionPurpose::Preview),
    ] {
        assert_eq!(
            evaluate_primary_filename(&document, &execution),
            Err(PrimaryFilenameEvaluationError::Parameter(
                EngineError::MissingRuntimeParameter {
                    node: 17,
                    name: "input".into()
                }
            ))
        );
    }
}

#[test]
fn explicit_nulls_are_not_absent_parameters_or_preview_fallbacks() {
    for value in [Value::Null, Value::json_null(), Value::xml_nil()] {
        let found = value.type_name();
        let mut parameters = RuntimeParameters::new();
        parameters.insert("input", value).unwrap();
        let execution = ExecutionContext::new(Path::new("mapping.ferrule"))
            .with_parameters(&parameters)
            .with_purpose(ExecutionPurpose::Preview);
        assert_eq!(
            evaluate_primary_filename(&parameter("input"), &execution),
            Err(PrimaryFilenameEvaluationError::NonStringResult { node: 17, found })
        );
    }
}

#[test]
fn incompatible_parameter_retains_exact_type_error_metadata() {
    let mut parameters = RuntimeParameters::new();
    parameters.insert("input", Value::Float(f64::NAN)).unwrap();
    let execution =
        ExecutionContext::new(Path::new("mapping.ferrule")).with_parameters(&parameters);
    assert_eq!(
        evaluate_primary_filename(&parameter("input"), &execution),
        Err(PrimaryFilenameEvaluationError::Parameter(
            EngineError::RuntimeParameterType {
                node: 17,
                name: "input".into(),
                expected: ScalarType::String,
                found: "float",
            }
        ))
    );
}

#[test]
fn invalid_or_unsupported_plan_fails_before_resolving_host_parameters() {
    let execution = ExecutionContext::new(Path::new("mapping.ferrule"));
    for (node, expected) in [
        (
            Node::Const { value: Value::Null },
            PrimaryFilenameError::NonString { node: 17 },
        ),
        (
            Node::RuntimeParameter {
                name: "input".into(),
                ty: ScalarType::Int,
                preview: None,
            },
            PrimaryFilenameError::NonString { node: 17 },
        ),
        (
            Node::RuntimeParameter {
                name: "".into(),
                ty: ScalarType::String,
                preview: None,
            },
            PrimaryFilenameError::InvalidParameterName { node: 17 },
        ),
        (
            Node::SourceField {
                path: vec!["input".into()],
                frame: None,
            },
            PrimaryFilenameError::UnsupportedExpression { node: 17 },
        ),
        (
            Node::SourceDocumentPath,
            PrimaryFilenameError::UnsupportedExpression { node: 17 },
        ),
        (
            Node::RuntimeParameterDefault {
                name: "input".into(),
                ty: ScalarType::String,
                default: 99,
                preview: None,
            },
            PrimaryFilenameError::UnsupportedExpression { node: 17 },
        ),
    ] {
        assert_eq!(
            evaluate_primary_filename(&document(node), &execution),
            Err(PrimaryFilenameEvaluationError::Plan(expected))
        );
    }
    let mapping = document(Node::Unconnected);
    let missing = DocumentProject::new(
        mapping.project().clone(),
        PrimarySourcePlan { filename: 555 },
    );
    assert_eq!(
        evaluate_primary_filename(&missing, &execution),
        Err(PrimaryFilenameEvaluationError::Plan(
            PrimaryFilenameError::MissingNode { node: 555 }
        ))
    );
}
