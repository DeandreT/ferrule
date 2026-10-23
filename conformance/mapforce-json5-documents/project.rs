use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, RuntimeValue, Scope, ScopeIteration};

pub fn project(named: bool) -> Project {
    let mut project = Project {
        source: SchemaNode::group(
            "Rows",
            vec![
                SchemaNode::scalar("Order", ScalarType::String),
                SchemaNode::scalar("Qty", ScalarType::String),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::scalar("Time", ScalarType::String),
                SchemaNode::group("Lines", vec![SchemaNode::scalar("Qty", ScalarType::Float)])
                    .repeating(),
            ],
        ),
        source_path: Some("unused-preview.csv".into()),
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::RuntimeValue {
                        value: RuntimeValue::CurrentDateTime,
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("one document".into()),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Qty".into()],
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "decimal_from_nullable_text".into(),
                        args: vec![2],
                    },
                ),
                (
                    5,
                    Node::SourceField {
                        path: vec!["Order".into()],
                        frame: None,
                    },
                ),
                (
                    6,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![0, 5],
                    },
                ),
                (
                    7,
                    Node::DistinctTextMapLast {
                        collection: vec![],
                        key: vec!["Order".into()],
                        expression: 6,
                    },
                ),
                (
                    90,
                    Node::RuntimeParameter {
                        name: "Input".into(),
                        ty: ScalarType::String,
                        preview: Some("unused-parameter-preview.csv".into()),
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: vec![],
                output_path: 7,
            },
            group_by: Some(1),
            bindings: vec![Binding {
                target_field: "Time".into(),
                node: 0,
            }],
            children: vec![Scope {
                target_field: "Lines".into(),
                iteration: ScopeIteration::Source(vec![]),
                bindings: vec![Binding {
                    target_field: "Qty".into(),
                    node: 3,
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    };
    if named {
        project.graph.nodes.insert(
            91,
            Node::Const {
                value: Value::String("not-a-number".into()),
            },
        );
        project.graph.nodes.insert(
            92,
            Node::Call {
                function: "decimal_from_nullable_text".into(),
                args: vec![91],
            },
        );
        project.extra_targets.push(mapping::NamedTarget {
            name: "selected".into(),
            path: None,
            schema: project.target.clone(),
            options: Default::default(),
            root: project.root.clone(),
        });
        project.root.bindings[0].node = 92; // The unselected primary fails if evaluated.
    }
    project
}
