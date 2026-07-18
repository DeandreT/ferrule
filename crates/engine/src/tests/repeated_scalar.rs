use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope};

use crate::{run, validate};

#[test]
fn repeated_scalar_bindings_concatenate_non_null_values_in_order() {
    let target = SchemaNode::group(
        "Result",
        vec![SchemaNode::scalar("line", ScalarType::String).repeating()],
    );
    let project = Project {
        source: SchemaNode::group("Source", Vec::new()),
        target,
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
                        value: Value::String("first".into()),
                    },
                ),
                (1, Node::Const { value: Value::Null }),
                (
                    2,
                    Node::Const {
                        value: Value::String("second".into()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "line".into(),
                    node: 0,
                },
                Binding {
                    target_field: "line".into(),
                    node: 1,
                },
                Binding {
                    target_field: "line".into(),
                    node: 2,
                },
            ],
            ..Scope::default()
        },
    };

    assert!(validate(&project).is_empty());
    let output = run(&project, &Instance::Group(Vec::new())).unwrap();
    assert_eq!(
        output.field("line"),
        Some(&Instance::Repeated(vec![
            Instance::Scalar(Value::String("first".into())),
            Instance::Scalar(Value::String("second".into())),
        ]))
    );
}

#[test]
fn duplicate_non_repeating_scalar_bindings_remain_invalid() {
    let mut project = Project {
        source: SchemaNode::group("Source", Vec::new()),
        target: SchemaNode::group(
            "Result",
            vec![SchemaNode::scalar("value", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        graph: Graph::default(),
        root: Scope::default(),
    };
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("value".into()),
        },
    );
    project.root.bindings = vec![
        Binding {
            target_field: "value".into(),
            node: 0,
        },
        Binding {
            target_field: "value".into(),
            node: 0,
        },
    ];

    assert!(validate(&project).iter().any(|issue| {
        issue
            .message
            .contains("target field `value` is bound more than once")
    }));
}
