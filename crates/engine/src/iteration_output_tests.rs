use std::collections::BTreeMap;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{Binding, Graph, IterationOutput, Node, Project, Scope};

use super::{EngineError, run, validate};

fn project() -> Project {
    let person = SchemaNode::group(
        "Person",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("Keep", ScalarType::Bool),
        ],
    )
    .repeating();
    let source = SchemaNode::group(
        "root",
        vec![SchemaNode::group("Department", vec![person]).repeating()],
    );
    let selected = SchemaNode::group(
        "Selected",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::scalar("DepartmentPosition", ScalarType::Int),
            SchemaNode::scalar("PersonPosition", ScalarType::Int),
        ],
    );
    let target = SchemaNode::group(
        "root",
        vec![
            SchemaNode::group(
                "Department",
                vec![SchemaNode::scalar("Position", ScalarType::Int), selected],
            )
            .repeating(),
        ],
    );
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::SourceField {
                    path: vec!["Name".into()],
                    frame: None,
                },
            ),
            (
                1,
                Node::SourceField {
                    path: vec!["Keep".into()],
                    frame: None,
                },
            ),
            (
                2,
                Node::Const {
                    value: Value::Int(1),
                },
            ),
            (
                3,
                Node::Position {
                    collection: vec!["Department".into()],
                },
            ),
            (
                4,
                Node::Position {
                    collection: vec!["Person".into()],
                },
            ),
        ]),
    };
    let selected_scope = Scope {
        target_field: "Selected".into(),
        source: Some(vec!["Person".into()]),
        filter: Some(1),
        take: Some(2),
        iteration_output: IterationOutput::First,
        bindings: vec![
            Binding {
                target_field: "Name".into(),
                node: 0,
            },
            Binding {
                target_field: "DepartmentPosition".into(),
                node: 3,
            },
            Binding {
                target_field: "PersonPosition".into(),
                node: 4,
            },
        ],
        ..Scope::default()
    };
    Project {
        source,
        target,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Department".into(),
                source: Some(vec!["Department".into()]),
                bindings: vec![Binding {
                    target_field: "Position".into(),
                    node: 3,
                }],
                children: vec![selected_scope],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn person(name: &str, keep: bool) -> Instance {
    Instance::Group(vec![
        ("Name".into(), Instance::Scalar(Value::String(name.into()))),
        ("Keep".into(), Instance::Scalar(Value::Bool(keep))),
    ])
}

fn department(people: Vec<Instance>) -> Instance {
    Instance::Group(vec![("Person".into(), Instance::Repeated(people))])
}

#[test]
fn first_output_applies_filter_and_take_and_preserves_nested_positions() {
    let project = project();
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let source = Instance::Group(vec![(
        "Department".into(),
        Instance::Repeated(vec![
            department(vec![
                person("discarded", false),
                person("first", true),
                person("not reached", true),
            ]),
            department(vec![person("also discarded", false)]),
        ]),
    )]);

    let output = run(&project, &source).unwrap();
    let departments = output
        .field("Department")
        .and_then(Instance::as_repeated)
        .unwrap();
    let selected = departments[0].field("Selected").unwrap();
    assert_eq!(
        selected.field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("first".into()))
    );
    assert_eq!(
        selected
            .field("DepartmentPosition")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(1))
    );
    assert_eq!(
        selected
            .field("PersonPosition")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(1))
    );
    assert_eq!(
        departments[1]
            .field("Position")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(2))
    );
    assert_eq!(
        departments[1].field("Selected"),
        Some(&Instance::Group(Vec::new()))
    );
}

#[test]
fn validation_rejects_invalid_first_output_cardinality() {
    let mut repeating_target = project();
    let SchemaKind::Group { children, .. } = &mut repeating_target.target.kind else {
        panic!("test target must be a group");
    };
    let Some(department) = children.iter_mut().find(|node| node.name == "Department") else {
        panic!("test target must contain Department");
    };
    let SchemaKind::Group { children, .. } = &mut department.kind else {
        panic!("Department must be a group");
    };
    let Some(selected) = children.iter_mut().find(|node| node.name == "Selected") else {
        panic!("Department must contain Selected");
    };
    selected.repeating = true;
    let issues = validate(&repeating_target);
    assert!(issues.iter().any(|issue| {
        issue
            .message
            .contains("first-item output requires a non-repeating target group schema")
    }));

    let mut without_iteration = project();
    without_iteration.root.iteration_output = IterationOutput::First;
    let issues = validate(&without_iteration);
    assert!(issues.iter().any(|issue| {
        issue
            .message
            .contains("first-item output requires an iterated source")
    }));
    assert_eq!(
        run(&without_iteration, &Instance::Group(Vec::new())),
        Err(EngineError::FirstOutputWithoutIteration)
    );
}

#[test]
fn first_output_does_not_evaluate_later_unused_bindings() {
    let project = Project {
        source: SchemaNode::group("row", vec![SchemaNode::scalar("Value", ScalarType::String)])
            .repeating(),
        target: SchemaNode::group("row", vec![SchemaNode::scalar("Value", ScalarType::String)]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::Call {
                        function: "upper".into(),
                        args: vec![0],
                    },
                ),
            ]),
        },
        root: Scope {
            source: Some(Vec::new()),
            iteration_output: IterationOutput::First,
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Scope::default()
        },
    };
    let source = Instance::Repeated(vec![
        Instance::Group(vec![(
            "Value".into(),
            Instance::Scalar(Value::String("first".into())),
        )]),
        Instance::Group(vec![("Value".into(), Instance::Scalar(Value::Int(2)))]),
    ]);

    let output = run(&project, &source).unwrap();
    assert_eq!(
        output.field("Value").and_then(Instance::as_scalar),
        Some(&Value::String("FIRST".into()))
    );
}
