use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, Node, Project};

pub fn project(mut project: Project) -> Project {
    // Keep a real fallible body arm without adding Raise to this profile.
    project.graph.nodes.insert(
        2,
        Node::Call {
            function: "format_datetime".into(),
            args: vec![21, 22],
        },
    );
    project.graph.nodes.insert(
        21,
        Node::Const {
            value: Value::String("not-a-date".into()),
        },
    );
    project.graph.nodes.insert(
        22,
        Node::Const {
            value: Value::String("[Y]".into()),
        },
    );
    project.graph.nodes.insert(
        30,
        Node::Const {
            value: Value::Int(1),
        },
    );
    project.graph.nodes.insert(
        31,
        Node::InvocationCounter {
            key: "body-clock-counter".into(),
            start: 30,
            step: 30,
            restart: None,
        },
    );
    project.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::scalar("Body", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
            SchemaNode::scalar("Repeated", ScalarType::String),
            SchemaNode::scalar("Fold", ScalarType::String),
        ],
    );
    project.root.bindings.extend([
        Binding {
            target_field: "Repeated".into(),
            node: 0,
        },
        Binding {
            target_field: "Fold".into(),
            node: 14,
        },
    ]);
    project.root.bindings.insert(
        1,
        Binding {
            target_field: "Count".into(),
            node: 31,
        },
    );
    project
}

pub fn named(mut project: Project) -> Project {
    project.extra_targets.push(mapping::NamedTarget {
        name: "selected".into(),
        path: None,
        schema: project.target.clone(),
        options: Default::default(),
        root: project.root.clone(),
    });
    project.root.bindings[0].node = 2;
    project
}
