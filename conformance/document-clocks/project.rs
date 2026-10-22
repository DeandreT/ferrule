use ir::{ScalarType, SchemaNode, Value};
use mapping::{AggregateOp, Binding, Graph, Node, Project, RuntimeValue, Scope, ScopeIteration};

pub fn project() -> Project {
    let call = |function: &str, args| Node::Call {
        function: function.into(),
        args,
    };
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar("Fail", ScalarType::Bool),
                SchemaNode::scalar("Mode", ScalarType::Int),
                SchemaNode::group(
                    "Samples",
                    vec![SchemaNode::scalar("Marker", ScalarType::String)],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Body", ScalarType::String)],
        ),
        source_path: Some("input-*.xml".into()),
        target_path: None,
        source_options: mapping::FormatOptions {
            xml_document: true,
            local_xml_file_set: true,
            ..Default::default()
        },
        target_options: mapping::FormatOptions {
            xml_document: true,
            ..Default::default()
        },
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
                    Node::SourceField {
                        path: vec!["Fail".into()],
                        frame: None,
                    },
                ),
                (2, Node::Raise { message: None }),
                (
                    3,
                    Node::If {
                        condition: 1,
                        then: 2,
                        else_: 0,
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: vec!["Mode".into()],
                        frame: None,
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (6, call("equal", vec![4, 5])),
                (
                    7,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (8, call("equal", vec![4, 7])),
                (
                    9,
                    Node::Const {
                        value: Value::Int(3),
                    },
                ),
                (10, call("equal", vec![4, 9])),
                (11, Node::SourceDocumentPath),
                (
                    12,
                    Node::Const {
                        value: Value::String("fixed-".into()),
                    },
                ),
                (13, call("concat", vec![0, 0, 11])),
                (
                    14,
                    Node::Aggregate {
                        function: AggregateOp::LastPresentText,
                        collection: vec!["Samples".into()],
                        value: vec![],
                        expression: Some(0),
                        arg: None,
                    },
                ),
                (15, call("concat", vec![14, 11])),
                (16, call("concat", vec![0, 11])),
                (17, call("concat", vec![12, 11])),
                (
                    18,
                    Node::If {
                        condition: 10,
                        then: 15,
                        else_: 17,
                    },
                ),
                (
                    19,
                    Node::If {
                        condition: 8,
                        then: 13,
                        else_: 18,
                    },
                ),
                (
                    20,
                    Node::If {
                        condition: 6,
                        then: 16,
                        else_: 19,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: vec![],
                output_path: 20,
            },
            bindings: vec![Binding {
                target_field: "Body".into(),
                node: 3,
            }],
            ..Default::default()
        },
    }
}
