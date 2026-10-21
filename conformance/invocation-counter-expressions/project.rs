//! Self-authored counter mapping: primary and named output share one key.
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, NamedTarget, Node, Project, Scope};

pub fn project() -> Project {
    let target = SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
    let root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 1,
        }],
        ..Default::default()
    };
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: target.clone(),
        source_path: Some("input.json".into()),
        target_path: Some("output.json".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: vec![NamedTarget {
            name: "audit".into(),
            path: Some("audit.json".into()),
            schema: target,
            options: Default::default(),
            root: root.clone(),
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        root,
        graph: Graph {
            nodes: [
                (
                    1,
                    Node::InvocationCounter {
                        key: "line".into(),
                        start: 2,
                        step: 3,
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
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
            ]
            .into(),
        },
    }
}

#[allow(dead_code)]
pub fn parameter_project() -> Project {
    let mut project = project();
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("A", ScalarType::Int),
            SchemaNode::scalar("B", ScalarType::Int),
        ],
    );
    project.root.bindings = vec![
        Binding {
            target_field: "A".into(),
            node: 1,
        },
        Binding {
            target_field: "B".into(),
            node: 1,
        },
    ];
    for (id, name) in [(2, "start"), (3, "step")] {
        project.graph.nodes.insert(
            id,
            Node::RuntimeParameter {
                name: name.into(),
                ty: ScalarType::Int,
                preview: None,
            },
        );
    }
    project
}

fn input_fields() -> Vec<SchemaNode> {
    vec![
        SchemaNode::scalar("start", ScalarType::Int),
        SchemaNode::scalar("step", ScalarType::Int),
        SchemaNode::scalar("restart", ScalarType::String),
    ]
}

#[allow(dead_code)]
pub fn source_project() -> Project {
    let mut project = parameter_project();
    project.source = SchemaNode::group("Input", input_fields());
    for (id, name) in [(2, "start"), (3, "step"), (4, "restart")] {
        project.graph.nodes.insert(
            id,
            Node::SourceField {
                path: vec![name.into()],
                frame: None,
            },
        );
    }
    if let Node::InvocationCounter { restart, .. } = project.graph.nodes.get_mut(&1).unwrap() {
        *restart = Some(4);
    }
    project
}

#[allow(dead_code)]
pub fn rows_project() -> Project {
    let mut project = source_project();
    project.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::group("Rows", input_fields()).repeating()],
    );
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Items", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    );
    project.extra_targets.clear();
    project.root = Scope {
        children: vec![Scope {
            target_field: "Items".into(),
            iteration: mapping::ScopeIteration::Source(vec!["Rows".into()]),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    project
}
