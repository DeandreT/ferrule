use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, Graph, Node, Project, Scope, ScopeConstruction, ScopeIteration,
};

pub fn project() -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Order", ScalarType::String),
                        SchemaNode::scalar("Item", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::scalar("Item", ScalarType::String),
                SchemaNode::scalar("Line", ScalarType::Int),
            ],
        ),
        source_path: None,
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
                    1,
                    Node::SourceField {
                        path: vec!["Order".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Item".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    4,
                    Node::InvocationCounter {
                        key: "rows".into(),
                        start: 3,
                        step: 3,
                        restart: None,
                    },
                ),
                (
                    5,
                    Node::Aggregate {
                        function: AggregateOp::LastPresentText,
                        collection: vec!["Rows".into()],
                        value: vec!["Order".into()],
                        expression: None,
                        arg: None,
                    },
                ),
                (
                    6,
                    Node::Const {
                        value: Value::String(".txt".into()),
                    },
                ),
                (
                    7,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![5, 6],
                    },
                ),
                (
                    8,
                    Node::Aggregate {
                        function: AggregateOp::LastDistinctText,
                        collection: vec![],
                        value: vec!["Item".into()],
                        expression: None,
                        arg: None,
                    },
                ),
                (
                    9,
                    Node::Call {
                        function: "exists".into(),
                        args: vec![2],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: vec!["Rows".into()],
                output_path: 7,
            },
            construction: ScopeConstruction::DocumentRows,
            group_by: Some(1),
            children: vec![Scope {
                iteration: ScopeIteration::Source(vec![]),
                filter: Some(9),
                group_by: Some(2),
                bindings: vec![
                    Binding {
                        target_field: "Item".into(),
                        node: 8,
                    },
                    Binding {
                        target_field: "Line".into(),
                        node: 4,
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        },
    }
}
