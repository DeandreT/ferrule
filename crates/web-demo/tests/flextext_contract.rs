use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, FlexCommand, FlexLineEnding, FlexTextLayout, Node, Project};
use web_demo::runtime::{
    DataFormat, DataSide, RuntimeError, UnsupportedContractFamily, parse_source, run,
};

fn layout(root: &str) -> FlexTextLayout {
    FlexTextLayout::new(
        root,
        FlexCommand::Store {
            name: "name".into(),
            ty: ScalarType::String,
            trim: None,
        },
        FlexLineEnding::Lf,
        false,
    )
    .unwrap()
}

fn project() -> Project {
    let mut project = Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("name", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("name", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Default::default(),
        root: Default::default(),
    };
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["name".into()],
            frame: None,
        },
    );
    project.root.bindings.push(Binding {
        target_field: "name".into(),
        node: 0,
    });
    project
}

fn name(value: &str) -> Instance {
    Instance::Group(vec![("name".into(), Instance::Scalar(Value::String(value.into())))].into())
}

#[test]
fn native_store_has_independent_complete_value_and_byte_oracles() {
    let layout = layout("Target");
    assert_eq!(
        format_flextext::from_str("Ada", &layout.schema(), &layout).unwrap(),
        name("Ada")
    );
    assert_eq!(
        format_flextext::to_string(&layout.schema(), &name("Ada"), &layout)
            .unwrap()
            .as_bytes(),
        b"Ada"
    );
}

#[test]
fn target_store_cannot_be_silently_replaced_by_json() {
    let mut project = project();
    project.target_options.flextext = Some(layout("Target"));
    assert!(engine::validate(&project).is_empty());
    let saved = web_demo::project_document::to_json(&project).unwrap();
    let project = web_demo::project_document::parse_and_validate(&saved).unwrap();
    assert_eq!(project.target_options.flextext, Some(layout("Target")));
    assert_eq!(
        web_demo::project_document::to_json(&project).unwrap(),
        saved
    );
    let outcome = run(
        &project,
        r#"{"name":"Ada"}"#,
        DataFormat::Json,
        DataFormat::Json,
    );
    assert_eq!(
        outcome,
        Err(RuntimeError::UnsupportedBoundaryContract {
            side: DataSide::Target,
            family: UnsupportedContractFamily::FlexText,
        })
    );
}

#[test]
fn source_store_cannot_be_silently_replaced_by_json() {
    let mut project = project();
    let layout = layout("Source");
    project.source_options.flextext = Some(layout.clone());
    let input = r#"{"name":"Ada"}"#;
    assert!(engine::validate(&project).is_empty());
    assert_eq!(
        format_flextext::from_str(input, &project.source, &layout).unwrap(),
        name(input)
    );
    let outcome = parse_source(&project, input, DataFormat::Json);
    assert_eq!(
        outcome,
        Err(RuntimeError::UnsupportedBoundaryContract {
            side: DataSide::Source,
            family: UnsupportedContractFamily::FlexText,
        })
    );
    let native_source = format_flextext::from_str(input, &project.source, &layout).unwrap();
    let native_target = engine::run(&project, &native_source).unwrap();
    assert_eq!(
        format_json::to_string(&project.target, &native_target)
            .unwrap()
            .as_bytes(),
        b"{\n  \"name\": \"{\\\"name\\\":\\\"Ada\\\"}\"\n}\n"
    );
    assert_eq!(
        run(&project, input, DataFormat::Json, DataFormat::Json),
        Err(RuntimeError::UnsupportedBoundaryContract {
            side: DataSide::Source,
            family: UnsupportedContractFamily::FlexText,
        })
    );
}
