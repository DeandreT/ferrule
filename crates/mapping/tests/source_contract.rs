use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    ClosedPrimarySourcePlan, ExecutionContract, Graph, Project, Scope,
    SourceContractReason as Reason,
};

fn text(name: &str) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::String)
}
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(key, value)| (key.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn scalar(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            text("Root"),
            SchemaNode::group(
                "Row",
                vec![
                    text("Text"),
                    SchemaNode::group("Cell", vec![text("Text")]).repeating(),
                ],
            )
            .repeating(),
        ],
    )
}

fn project() -> Project {
    Project {
        execution_contract: Some(ExecutionContract::ClosedPrimarySourceV1),
        source: schema(),
        target: SchemaNode::group("Output", vec![]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    }
}

#[test]
fn closed_shape_requires_explicit_repetition_without_cloning_or_coercing_scalars() {
    let plan = ClosedPrimarySourcePlan::compile(&schema()).unwrap();
    let row = |cell| group(vec![("Text", scalar("OUTER")), ("Cell", cell)]);
    let input = |row| {
        group(vec![
            ("Root", scalar("ROOT")),
            ("Row", Instance::Repeated(vec![row])),
        ])
    };
    plan.validate_input(&input(row(Instance::Repeated(vec![]))), 0, 0)
        .unwrap();
    plan.validate_input(
        &input(row(Instance::Repeated(vec![group(vec![(
            "Text",
            scalar("INNER"),
        )])]))),
        0,
        0,
    )
    .unwrap();
    for (bad, reason) in [
        (group(vec![]), Reason::MissingRepeatedSlot),
        (
            input(group(vec![("Text", scalar("OUTER"))])),
            Reason::MissingRepeatedSlot,
        ),
        (
            input(row(Instance::Scalar(Value::Null))),
            Reason::WrongShape,
        ),
        (
            input(row(Instance::Repeated(vec![Instance::Scalar(Value::Null)]))),
            Reason::WrongShape,
        ),
        (
            input(row(Instance::Repeated(vec![Instance::Repeated(vec![])]))),
            Reason::WrongShape,
        ),
        (
            input(group(vec![
                ("Cell", Instance::Repeated(vec![])),
                ("Injected", scalar("HIDDEN")),
            ])),
            Reason::UnexpectedField,
        ),
        (
            input(group(vec![
                ("Cell", Instance::Repeated(vec![])),
                ("Text", scalar("A")),
                ("Text", scalar("B")),
            ])),
            Reason::DuplicateField,
        ),
        (
            input(group(vec![
                ("Cell", Instance::Repeated(vec![])),
                ("Text", Instance::Scalar(Value::Int(1))),
            ])),
            Reason::WrongScalarDomain,
        ),
        (Instance::Repeated(vec![]), Reason::WrongShape),
        (Instance::MappedSequence(vec![]), Reason::WrongShape),
        (Instance::DocumentSet(vec![]), Reason::WrongShape),
    ] {
        let error = plan.validate_input(&bad, 0, 0).unwrap_err();
        assert_eq!(error.reason, reason);
        assert!(!error.to_string().contains("HIDDEN"));
    }
    assert_eq!(
        plan.validate_input(&Instance::Scalar(Value::Null), 1, 0)
            .unwrap_err()
            .reason,
        Reason::UnexpectedBinding
    );
    assert_eq!(
        plan.validate_input(&Instance::Scalar(Value::Null), 0, 1)
            .unwrap_err()
            .reason,
        Reason::UnexpectedParameter
    );
}

#[test]
fn root_and_pin_owners_use_concrete_local_frame_labels() {
    let plan = ClosedPrimarySourcePlan::compile(&schema()).unwrap();
    let row = vec!["Row".into()];
    let cell = vec!["Row".into(), "Cell".into()];
    let active = vec![row.clone(), cell.clone()];
    plan.validate_read(&["Root".into()], None, &active).unwrap();
    plan.validate_read(&["Text".into()], Some(&row), &active)
        .unwrap();
    plan.validate_read(&["Text".into()], Some(&cell), &active)
        .unwrap();
    assert!(
        plan.validate_read(&["Text".into()], Some(&cell), std::slice::from_ref(&row))
            .is_err()
    );
    assert!(plan.validate_read(&["Text".into()], None, &active).is_err());
    assert!(
        plan.validate_driver(&[], &["Row".into(), "Cell".into()])
            .is_err()
    );
    assert!(plan.validate_driver(&[row], &[]).is_err());
    let renamed = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Row",
                vec![
                    text("Text"),
                    SchemaNode::group("Row", vec![text("Text")]).repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let plan = ClosedPrimarySourcePlan::compile(&renamed).unwrap();
    assert_eq!(
        plan.validate_read(
            &["Text".into()],
            Some(&["Row".into()]),
            &[vec!["Row".into()], vec!["Row".into(), "Row".into()]]
        )
        .unwrap_err()
        .reason,
        Reason::UnprovedOwner
    );
}

#[test]
fn strict_enum_and_presence_based_saved_capabilities_refuse_downgrades() {
    let project = project();
    let text = mapping::project_file::encode_pretty(&project).unwrap();
    let mut envelope: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(envelope["__ferrule_file"]["version"], 3);
    assert_eq!(
        envelope["__ferrule_file"]["capabilities"],
        serde_json::json!([mapping::PRIMARY_SOURCE_SAFETY_GUARD_V1_CAPABILITY])
    );
    let reopened = mapping::project_file::decode_str(&text).unwrap();
    assert_eq!(reopened.execution_contract, project.execution_contract);
    assert!(mapping::project_file::decode_str(&serde_json::to_string(&project).unwrap()).is_err());
    envelope["__ferrule_file"]["version"] = 2.into();
    envelope["__ferrule_file"]
        .as_object_mut()
        .unwrap()
        .remove("capabilities");
    assert!(mapping::project_file::decode_str(&envelope.to_string()).is_err());
    let mut envelope: serde_json::Value = serde_json::from_str(&text).unwrap();
    envelope["document"]
        .as_object_mut()
        .unwrap()
        .remove("execution_contract");
    assert!(mapping::project_file::decode_str(&envelope.to_string()).is_err());
    for contract in [
        serde_json::json!({"kind":"closed_primary_source_v2"}),
        serde_json::json!({"kind":"closed_primary_source_v1","ignored":true}),
    ] {
        assert!(serde_json::from_value::<ExecutionContract>(contract).is_err());
    }
}

#[test]
fn embedded_pipeline_and_document_projects_retain_the_guard_capability() {
    let pipeline = mapping::Pipeline {
        main_mapping_path: None,
        stages: vec![mapping::PipelineStage {
            id: "guarded".into(),
            mapping_path: None,
            project: project(),
            source: mapping::PipelineInput::Host {
                name: "input".into(),
            },
            extra_sources: vec![],
        }],
    };
    let text = mapping::pipeline_file::encode_pretty(&pipeline).unwrap();
    let reopened = mapping::pipeline_file::decode_str(&text).unwrap();
    assert_eq!(
        reopened.stages[0].project.execution_contract,
        Some(ExecutionContract::ClosedPrimarySourceV1)
    );
    let mut envelope: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(envelope["__ferrule_file"]["version"], 3);
    envelope["__ferrule_file"]["capabilities"] =
        serde_json::json!([mapping::SQLITE_READ_V1_CAPABILITY]);
    assert!(mapping::pipeline_file::decode_str(&envelope.to_string()).is_err());
    let document = mapping::DocumentProject::new(project(), mapping::PrimarySourcePlan::new(0));
    let text = mapping::document_project_file::encode_pretty(&document).unwrap();
    let reopened = mapping::document_project_file::decode_str(&text).unwrap();
    assert_eq!(
        reopened.project().execution_contract,
        Some(ExecutionContract::ClosedPrimarySourceV1)
    );
    let mut envelope: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        envelope["__ferrule_file"]["capabilities"],
        serde_json::json!([mapping::PRIMARY_SOURCE_SAFETY_GUARD_V1_CAPABILITY])
    );
    envelope["__ferrule_file"]["capabilities"] = serde_json::json!(["not-a-supported-guard"]);
    assert!(mapping::document_project_file::decode_str(&envelope.to_string()).is_err());
}

#[test]
fn schema_walk_refuses_ambiguous_virtual_and_deep_shapes() {
    for source in [
        SchemaNode::group("Input", vec![text("A"), text("A")]),
        SchemaNode::group("Input", vec![text("#text")]),
        text("Input"),
        SchemaNode::group("Input", vec![text("A").repeating()]),
        SchemaNode::group("Input", vec![]).repeating(),
    ] {
        assert!(ClosedPrimarySourcePlan::compile(&source).is_err());
    }
    let mut source = text("Leaf");
    for _ in 0..=mapping::MAX_SOURCE_CONTRACT_DEPTH {
        source = SchemaNode::group("G", vec![source]);
    }
    assert_eq!(
        ClosedPrimarySourcePlan::compile(&source)
            .unwrap_err()
            .reason,
        Reason::ResourceLimit
    );
}

#[test]
fn scalar_domains_and_optional_singleton_presence_are_checked_without_coercion() {
    let mut nullable = text("Text");
    nullable.nillable = true;
    let schema = SchemaNode::group(
        "Input",
        vec![nullable, SchemaNode::group("Optional", vec![text("Value")])],
    );
    let plan = ClosedPrimarySourcePlan::compile(&schema).unwrap();
    for value in [
        Value::Null,
        Value::xml_nil(),
        Value::String("unchanged".into()),
    ] {
        plan.validate_input(&group(vec![("Text", Instance::Scalar(value))]), 0, 0)
            .unwrap();
    }
    plan.validate_input(&group(vec![]), 0, 0).unwrap();
    for value in [
        Value::Int(1),
        Value::Float(1.0),
        Value::Bool(true),
        Value::json_null(),
        Value::decimal(ir::Decimal96::parse_exact("1").unwrap()),
    ] {
        assert_eq!(
            plan.validate_input(&group(vec![("Text", Instance::Scalar(value))]), 0, 0)
                .unwrap_err()
                .reason,
            Reason::WrongScalarDomain
        );
    }
    for optional in [
        Instance::Scalar(Value::Null),
        Instance::Repeated(vec![]),
        Instance::MappedSequence(vec![]),
        Instance::DocumentSet(vec![]),
    ] {
        assert_eq!(
            plan.validate_input(&group(vec![("Optional", optional)]), 0, 0)
                .unwrap_err()
                .reason,
            Reason::WrongShape
        );
    }
    plan.validate_input(&group(vec![("Optional", group(vec![]))]), 0, 0)
        .unwrap();
}
