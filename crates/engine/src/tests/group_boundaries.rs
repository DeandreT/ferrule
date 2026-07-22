use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{AggregateOp, Binding, Graph, Node, Project, Scope, ScopeIteration};

use super::{EngineError, run, validate};

#[derive(Clone, Copy)]
enum Mode {
    Adjacent,
    Ending,
}

fn row(kind: &str, value: &str, end: Value) -> Instance {
    Instance::Group(vec![
        ("Kind".into(), Instance::Scalar(Value::String(kind.into()))),
        (
            "Value".into(),
            Instance::Scalar(Value::String(value.into())),
        ),
        ("End".into(), Instance::Scalar(end)),
    ])
}

fn project(mode: Mode) -> Project {
    let rows = SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Kind", ScalarType::String),
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("End", ScalarType::Bool),
        ],
    )
    .repeating();
    let groups = SchemaNode::group(
        "Groups",
        vec![
            SchemaNode::scalar("First", ScalarType::String),
            SchemaNode::scalar("Joined", ScalarType::String),
            SchemaNode::scalar("Position", ScalarType::Int),
        ],
    )
    .repeating();
    let mut grouped = Scope {
        target_field: "Groups".into(),
        iteration: ScopeIteration::Source(vec!["Rows".into()]),
        bindings: vec![
            Binding {
                target_field: "First".into(),
                node: 1,
            },
            Binding {
                target_field: "Joined".into(),
                node: 3,
            },
            Binding {
                target_field: "Position".into(),
                node: 5,
            },
        ],
        ..Scope::default()
    };
    match mode {
        Mode::Adjacent => grouped.group_adjacent_by = Some(0),
        Mode::Ending => grouped.group_ending_with = Some(2),
    }

    Project {
        source: SchemaNode::group("Root", vec![rows]),
        target: SchemaNode::group("Target", vec![groups]),
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
                    0,
                    Node::SourceField {
                        path: vec!["Kind".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["End".into()],
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Aggregate {
                        function: AggregateOp::Join,
                        collection: vec!["Rows".into()],
                        value: vec!["Value".into()],
                        expression: None,
                        arg: Some(4),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::String(",".into()),
                    },
                ),
                (
                    5,
                    Node::Position {
                        collection: vec!["Rows".into()],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            children: vec![grouped],
            ..Scope::default()
        },
    }
}

fn source(rows: impl IntoIterator<Item = Instance>) -> Instance {
    Instance::Group(vec![(
        "Rows".into(),
        Instance::Repeated(rows.into_iter().collect()),
    )])
}

fn groups(output: &Instance) -> &[Instance] {
    output
        .field("Groups")
        .and_then(Instance::as_repeated)
        .unwrap()
}

fn assert_group(group: &Instance, first: &str, joined: &str, position: i64) {
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

#[test]
fn group_adjacent_by_keeps_separated_equal_keys_in_distinct_groups() {
    let input = source([
        row("A", "one", Value::Bool(false)),
        row("A", "two", Value::Bool(true)),
        row("B", "three", Value::Bool(false)),
        row("A", "four", Value::Bool(false)),
        row("A", "five", Value::Bool(true)),
    ]);

    let output = run(&project(Mode::Adjacent), &input).unwrap();
    let groups = groups(&output);
    assert_eq!(groups.len(), 3);
    assert_group(&groups[0], "one", "one,two", 1);
    assert_group(&groups[1], "three", "three", 2);
    assert_group(&groups[2], "four", "four,five", 3);
}

#[test]
fn group_ending_with_includes_boundaries_and_retains_trailing_items() {
    let input = source([
        row("A", "one", Value::Bool(false)),
        row("B", "two", Value::Bool(true)),
        row("C", "three", Value::Bool(true)),
        row("D", "four", Value::Bool(false)),
        row("E", "five", Value::Bool(false)),
    ]);

    let output = run(&project(Mode::Ending), &input).unwrap();
    let groups = groups(&output);
    assert_eq!(groups.len(), 3);
    assert_group(&groups[0], "one", "one,two", 1);
    assert_group(&groups[1], "three", "three", 2);
    assert_group(&groups[2], "four", "four,five", 3);
}

#[test]
fn group_ending_with_requires_a_boolean_predicate() {
    let mut project = project(Mode::Ending);
    project.root.children[0].group_ending_with = Some(0);
    let input = source([row("A", "one", Value::Bool(false))]);

    assert!(matches!(
        run(&project, &input),
        Err(EngineError::NotABool { node: 0, .. })
    ));
}

#[test]
fn adjacent_and_ending_group_modes_are_mutually_exclusive() {
    let mut project = project(Mode::Ending);
    project.root.children[0].group_adjacent_by = Some(0);
    let input = source([row("A", "one", Value::Bool(false))]);

    assert_eq!(
        run(&project, &input),
        Err(EngineError::ConflictingGroupingModes)
    );
    assert!(validate(&project).iter().any(|issue| {
        issue
            .message
            .contains("scope grouping modes are mutually exclusive")
    }));
}
