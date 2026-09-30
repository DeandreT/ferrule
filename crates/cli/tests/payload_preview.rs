use std::collections::BTreeMap;
use std::path::Path;

use engine::{EngineError, ExecutionPurpose, RuntimeParameters};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};

fn project() -> Project {
    let options = FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    };
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ScalarType::Int)]),
        source_path: None,
        target_path: None,
        source_options: options.clone(),
        target_options: options,
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::Const {
                        value: Value::Int(7),
                    },
                ),
                (
                    1,
                    Node::RuntimeParameterDefault {
                        name: "choice".into(),
                        ty: ScalarType::Int,
                        default: 0,
                        preview: Some("11".into()),
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    }
}

fn execute(
    project: &Project,
    purpose: Option<ExecutionPurpose>,
    parameters: Option<&RuntimeParameters>,
) -> anyhow::Result<serde_json::Value> {
    let source = cli::PayloadDocument::new(Path::new("input.json"), b"{}")?;
    let mut options =
        cli::PayloadRunOptions::new(source).with_output_path(Path::new("output.json"));
    if let Some(purpose) = purpose {
        options = options.with_execution_purpose(purpose);
    }
    if let Some(parameters) = parameters {
        options = options.with_runtime_parameters(parameters);
    }
    let outcome =
        cli::run_project_value_payloads(project, Path::new("preview-project.json"), &options)?;
    Ok(serde_json::from_slice(&outcome.artifacts[0].bytes)?)
}

#[test]
fn payload_preview_is_explicit_and_host_values_override_saved_previews() -> anyhow::Result<()> {
    let project = project();
    assert_eq!(
        execute(&project, None, None)?,
        serde_json::json!({"Value": 7})
    );
    assert_eq!(
        execute(&project, Some(ExecutionPurpose::Preview), None)?,
        serde_json::json!({"Value": 11})
    );

    let mut parameters = RuntimeParameters::new();
    parameters.insert("choice", Value::String("5".into()))?;
    for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
        assert_eq!(
            execute(&project, Some(purpose), Some(&parameters))?,
            serde_json::json!({"Value": 5})
        );
    }
    let mut null_parameters = RuntimeParameters::new();
    null_parameters.insert("choice", Value::Null)?;
    assert_eq!(
        execute(
            &project,
            Some(ExecutionPurpose::Preview),
            Some(&null_parameters)
        )?,
        serde_json::json!({})
    );
    Ok(())
}

#[test]
fn payload_required_and_malformed_preview_values_preserve_typed_errors() -> anyhow::Result<()> {
    let mut project = project();
    project.graph.nodes.insert(
        1,
        Node::RuntimeParameter {
            name: "choice".into(),
            ty: ScalarType::Int,
            preview: Some("11".into()),
        },
    );
    let error = execute(&project, None, None).unwrap_err();
    assert!(
        matches!(error.downcast_ref::<EngineError>(), Some(EngineError::MissingRuntimeParameter { node: 1, name }) if name == "choice")
    );
    assert_eq!(
        execute(&project, Some(ExecutionPurpose::Preview), None)?,
        serde_json::json!({"Value": 11})
    );

    project.graph.nodes.insert(
        1,
        Node::RuntimeParameterDefault {
            name: "choice".into(),
            ty: ScalarType::Int,
            default: 0,
            preview: Some("not an integer".into()),
        },
    );
    let error = execute(&project, Some(ExecutionPurpose::Preview), None).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<EngineError>(),
        Some(EngineError::RuntimeParameterType { node: 1, .. })
    ));
    assert_eq!(
        execute(&project, None, None)?,
        serde_json::json!({"Value": 7})
    );
    Ok(())
}
