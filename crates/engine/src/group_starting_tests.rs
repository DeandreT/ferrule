use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{AggregateOp, Binding, Graph, Node, Project, Scope};

use super::{EngineError, run};

fn row(kind: &str, value: &str, keep: bool) -> Instance {
    Instance::Group(vec![
        ("Kind".into(), Instance::Scalar(Value::String(kind.into()))),
        (
            "Value".into(),
            Instance::Scalar(Value::String(value.into())),
        ),
        ("Keep".into(), Instance::Scalar(Value::Bool(keep))),
    ])
}

fn project() -> Project {
    let row_schema = SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Kind", ScalarType::String),
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Keep", ScalarType::Bool),
        ],
    )
    .repeating();
    let group_schema = SchemaNode::group(
        "Group",
        vec![
            SchemaNode::scalar("First", ScalarType::String),
            SchemaNode::scalar("Joined", ScalarType::String),
            SchemaNode::scalar("Position", ScalarType::Int),
        ],
    )
    .repeating();
    Project {
        source: SchemaNode::group("Root", vec![row_schema]),
        target: SchemaNode::group("Target", vec![group_schema]),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::SourceField {
                        path: vec!["Kind".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("H".into()),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "equal".into(),
                        args: vec![0, 1],
                    },
                ),
                (
                    3,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (
                    4,
                    Node::Aggregate {
                        function: AggregateOp::Join,
                        collection: vec!["Rows".into()],
                        value: vec!["Value".into()],
                        expression: None,
                        arg: Some(5),
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::String(",".into()),
                    },
                ),
                (
                    6,
                    Node::Position {
                        collection: vec!["Rows".into()],
                    },
                ),
                (
                    7,
                    Node::SourceField {
                        path: vec!["Keep".into()],
                        frame: None,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Group".into(),
                source: Some(vec!["Rows".into()]),
                filter: Some(7),
                group_starting_with: Some(2),
                bindings: vec![
                    Binding {
                        target_field: "First".into(),
                        node: 3,
                    },
                    Binding {
                        target_field: "Joined".into(),
                        node: 4,
                    },
                    Binding {
                        target_field: "Position".into(),
                        node: 6,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

#[test]
fn group_starting_with_partitions_filtered_items_in_source_order() {
    let source = Instance::Group(vec![(
        "Rows".into(),
        Instance::Repeated(vec![
            row("L", "A", true),
            row("H", "B", false),
            row("L", "C", true),
            row("H", "D", true),
            row("H", "E", true),
            row("L", "F", true),
        ]),
    )]);
    let output = run(&project(), &source).unwrap();
    let groups = output
        .field("Group")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(groups.len(), 3);
    for (group, first, joined, position) in [
        (&groups[0], "A", "A,C", 1),
        (&groups[1], "D", "D", 2),
        (&groups[2], "E", "E,F", 3),
    ] {
        assert_eq!(
            group.field("First").and_then(Instance::as_scalar),
            Some(&Value::String(first.into()))
        );
        assert_eq!(
            group.field("Joined").and_then(Instance::as_scalar),
            Some(&Value::String(joined.into()))
        );
        assert_eq!(
            group.field("Position").and_then(Instance::as_scalar),
            Some(&Value::Int(position))
        );
    }
}

#[test]
fn group_starting_with_requires_a_boolean_predicate() {
    let mut project = project();
    project.root.children[0].group_starting_with = Some(0);
    let source = Instance::Group(vec![(
        "Rows".into(),
        Instance::Repeated(vec![row("H", "A", true)]),
    )]);
    assert!(matches!(
        run(&project, &source),
        Err(EngineError::NotABool { node: 0, .. })
    ));
}
