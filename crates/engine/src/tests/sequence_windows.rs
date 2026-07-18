use super::*;
use ir::{ScalarType, SchemaNode};
use mapping::{Binding, ScopeIteration, SequenceExpr, SequenceWindow};

fn window_project(windows: Vec<SequenceWindow>) -> Project {
    Project {
        source: SchemaNode::group("source", Vec::new()),
        target: SchemaNode::group(
            "target",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                    .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::Int(8),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::Int(3),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::Int(5),
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
                    from: None,
                    to: 0,
                    item: 1,
                }),
                windows,
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 1,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn values(windows: Vec<SequenceWindow>) -> Vec<i64> {
    let project = window_project(windows);
    assert!(validate(&project).is_empty(), "{:?}", validate(&project));
    let output = run(&project, &Instance::Group(Vec::new())).unwrap();
    output
        .field("Rows")
        .and_then(Instance::as_repeated)
        .unwrap()
        .iter()
        .map(|row| {
            let Some(Value::Int(value)) = row.field("Value").and_then(Instance::as_scalar) else {
                panic!("window output should contain an integer Value")
            };
            *value
        })
        .collect()
}

#[test]
fn every_sequence_window_uses_one_based_bounded_semantics() {
    assert_eq!(
        values(vec![SequenceWindow::SkipFirst { count: 2 }]),
        [3, 4, 5, 6, 7, 8]
    );
    assert_eq!(values(vec![SequenceWindow::First { count: 2 }]), [1, 2]);
    assert_eq!(
        values(vec![SequenceWindow::From { position: 3 }]),
        [3, 4, 5, 6, 7, 8]
    );
    assert_eq!(
        values(vec![SequenceWindow::FromTo { first: 3, last: 4 }]),
        [3, 4, 5]
    );
    assert_eq!(values(vec![SequenceWindow::Last { count: 2 }]), [7, 8]);
}

#[test]
fn chained_sequence_windows_preserve_declaration_order() {
    assert_eq!(
        values(vec![
            SequenceWindow::SkipFirst { count: 2 },
            SequenceWindow::First { count: 3 },
        ]),
        [3, 4, 5]
    );
    assert_eq!(
        values(vec![
            SequenceWindow::First { count: 3 },
            SequenceWindow::SkipFirst { count: 2 },
        ]),
        [3]
    );
}

#[test]
fn sequence_window_bounds_must_reference_graph_nodes() {
    let project = window_project(vec![SequenceWindow::Last { count: 99 }]);
    assert!(validate(&project).iter().any(|issue| {
        issue
            .to_string()
            .contains("sequence window 1 references missing bound node 99")
    }));
}
