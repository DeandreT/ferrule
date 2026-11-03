//! Self-authored native Ferrule contract, NOT imported MFD auto-number behavior.
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, NamedTarget, Node, Project, Scope, ScopeIteration};

fn binding(name: &str, node: u32) -> Binding {
    Binding {
        target_field: name.into(),
        node,
    }
}

pub fn project() -> Project {
    let text = |name| SchemaNode::scalar(name, ScalarType::String);
    let tick = |name| SchemaNode::scalar(name, ScalarType::Int);
    Project {
        source: SchemaNode::group(
            "Input",
            vec![SchemaNode::group("Rows", vec![text("Key"), text("Label")]).repeating()],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Group",
                    vec![
                        text("Key"),
                        tick("GroupTick"),
                        SchemaNode::group("Member", vec![text("Label"), tick("MemberTick")])
                            .repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        graph: Graph {
            nodes: [
                (
                    1,
                    Node::InvocationCounter {
                        key: "run".into(),
                        start: 2,
                        step: 2,
                        restart: None,
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
                    Node::SourceField {
                        frame: Some(vec!["Rows".into()]),
                        path: vec!["Key".into()],
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        frame: None,
                        path: vec!["Label".into()],
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Group".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                group_by: Some(3),
                bindings: vec![binding("Key", 3), binding("GroupTick", 1)],
                children: vec![Scope {
                    target_field: "Member".into(),
                    iteration: ScopeIteration::Source(vec![]),
                    bindings: vec![binding("Label", 4), binding("MemberTick", 1)],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
        extra_targets: vec![NamedTarget {
            name: "audit".into(),
            path: Some("audit.json".into()),
            schema: SchemaNode::group("Audit", vec![tick("Tick")]),
            root: Scope {
                bindings: vec![binding("Tick", 1)],
                ..Default::default()
            },
            options: Default::default(),
        }],
        source_path: Some("input.json".into()),
        target_path: Some("output.json".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}
