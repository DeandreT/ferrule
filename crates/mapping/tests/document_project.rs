use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DocumentProject, GlobalFilename, Graph, Node, PrimaryFilenameError, PrimarySourcePlan,
    Project, Scope,
};

fn project(filename: Node) -> Project {
    Project {
        source: SchemaNode::group("Input", vec![]),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        source_path: Some("preview.csv".into()),
        target_path: Some("preview-output.csv".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (1, filename),
                (
                    2,
                    Node::Const {
                        value: Value::String("mapped".into()),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String("unused".into()),
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 2,
            }],
            ..Default::default()
        },
    }
}

fn wrap(filename: Node) -> DocumentProject {
    DocumentProject::new(project(filename), PrimarySourcePlan { filename: 1 })
}

#[test]
fn literal_filename_is_not_replaced_by_saved_preview_path() {
    let document = wrap(Node::Const {
        value: Value::String("./selected/../input-*.csv".into()),
    });
    assert_eq!(
        document.global_filename().unwrap(),
        GlobalFilename::Literal("./selected/../input-*.csv")
    );
    assert_eq!(
        document.project().source_path.as_deref(),
        Some("preview.csv")
    );
    assert_eq!(
        document.project().target_path.as_deref(),
        Some("preview-output.csv")
    );
}

#[test]
fn parameter_selection_does_not_use_preview_or_look_like_a_source_field() {
    let document = wrap(Node::RuntimeParameter {
        name: "InputFilename".into(),
        ty: ScalarType::String,
        preview: Some("preview-only.csv".into()),
    });
    assert_eq!(
        document.global_filename().unwrap(),
        GlobalFilename::Parameter("InputFilename")
    );
    let value = serde_json::to_value(&document).unwrap();
    let restored: DocumentProject = serde_json::from_value(value).unwrap();
    assert_eq!(
        restored.global_filename().unwrap(),
        GlobalFilename::Parameter("InputFilename")
    );
    assert!(
        matches!(restored.project().graph.nodes.get(&1), Some(Node::RuntimeParameter { preview: Some(value), .. }) if value == "preview-only.csv")
    );
}

#[test]
fn document_artifact_cannot_be_loaded_as_a_legacy_project() {
    let ordinary = project(Node::Const {
        value: Value::String("input.csv".into()),
    });
    let ordinary_json = serde_json::to_value(&ordinary).unwrap();
    let restored: Project = serde_json::from_value(ordinary_json.clone()).unwrap();
    assert_eq!(serde_json::to_value(restored).unwrap(), ordinary_json);
    let document = DocumentProject::new(ordinary, PrimarySourcePlan { filename: 1 });
    let value = serde_json::to_value(&document).unwrap();
    assert_eq!(value["format"], "ferrule_document_project_v1");
    assert!(serde_json::from_value::<Project>(value.clone()).is_err());
    assert!(serde_json::from_value::<DocumentProject>(ordinary_json).is_err());
    let mut unknown = value;
    unknown["ignored_acquisition"] = serde_json::json!(true);
    assert!(serde_json::from_value::<DocumentProject>(unknown).is_err());
}

#[test]
fn missing_nonstring_and_source_dependent_nodes_are_refused() {
    let missing = DocumentProject::new(
        project(Node::Unconnected),
        PrimarySourcePlan { filename: 999 },
    );
    assert_eq!(
        missing.global_filename(),
        Err(PrimaryFilenameError::MissingNode { node: 999 })
    );
    for node in [
        Node::Const { value: Value::Null },
        Node::RuntimeParameter {
            name: "Input".into(),
            ty: ScalarType::Int,
            preview: None,
        },
    ] {
        assert_eq!(
            wrap(node).global_filename(),
            Err(PrimaryFilenameError::NonString { node: 1 })
        );
    }
    for node in [
        Node::SourceField {
            path: vec!["Filename".into()],
            frame: None,
        },
        Node::SourceDocumentPath,
        Node::RuntimeParameterDefault {
            name: "Input".into(),
            ty: ScalarType::String,
            default: 2,
            preview: None,
        },
    ] {
        assert_eq!(
            wrap(node).global_filename(),
            Err(PrimaryFilenameError::UnsupportedExpression { node: 1 })
        );
    }
}

#[test]
fn parameter_name_uses_existing_byte_boundary_without_trimming() {
    for name in [
        String::new(),
        "bad\0name".into(),
        "x".repeat(mapping::MAX_RUNTIME_PARAMETER_NAME_BYTES + 1),
    ] {
        assert_eq!(
            wrap(Node::RuntimeParameter {
                name,
                ty: ScalarType::String,
                preview: None
            })
            .global_filename(),
            Err(PrimaryFilenameError::InvalidParameterName { node: 1 })
        );
    }
    for name in [
        " spaced ".to_string(),
        "é".repeat(mapping::MAX_RUNTIME_PARAMETER_NAME_BYTES / 2),
    ] {
        let document = wrap(Node::RuntimeParameter {
            name: name.clone(),
            ty: ScalarType::String,
            preview: None,
        });
        assert_eq!(
            document.global_filename().unwrap(),
            GlobalFilename::Parameter(&name)
        );
    }
}

#[test]
fn pruning_keeps_filename_dependencies_even_when_the_profile_refuses_them() {
    let mut source = project(Node::RuntimeParameterDefault {
        name: "Input".into(),
        ty: ScalarType::String,
        default: 4,
        preview: None,
    });
    source.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::String("default.csv".into()),
        },
    );
    let mut document = DocumentProject::new(source, PrimarySourcePlan { filename: 1 });
    document.prune_unreachable_nodes();
    assert_eq!(
        document
            .project()
            .graph
            .nodes
            .keys()
            .copied()
            .collect::<Vec<_>>(),
        vec![1, 2, 4]
    );
    assert_eq!(
        document.global_filename(),
        Err(PrimaryFilenameError::UnsupportedExpression { node: 1 })
    );
    let mut ordinary = project(Node::Const {
        value: Value::String("unused-input.csv".into()),
    });
    ordinary.prune_unreachable_nodes();
    assert_eq!(
        ordinary.graph.nodes.keys().copied().collect::<Vec<_>>(),
        vec![2]
    );
}
