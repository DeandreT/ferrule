use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, RuntimeValue};

pub fn project(mut base: Project) -> Project {
    let field = |name: &str| Node::SourceField {
        path: vec![name.into()],
        frame: None,
    };
    let text = |value: &str| Node::Const {
        value: Value::String(value.into()),
    };
    let call = |function: &str, args| Node::Call {
        function: function.into(),
        args,
    };
    base.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Key", ScalarType::String),
                    SchemaNode::scalar("Marker", ScalarType::String),
                    SchemaNode::scalar("Skip", ScalarType::Bool),
                    SchemaNode::scalar("Invalid", ScalarType::Bool),
                    SchemaNode::scalar("Fail", ScalarType::Bool),
                ],
            )
            .repeating(),
        ],
    );
    base.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Selected", ScalarType::String)],
    );
    base.graph = Graph {
        nodes: [
            (0, field("Marker")),
            (
                1,
                Node::RuntimeValue {
                    value: RuntimeValue::CurrentDateTime,
                },
            ),
            (2, text("@")),
            (3, text(":")),
            (
                4,
                Node::Position {
                    collection: vec!["Rows".into()],
                },
            ),
            (5, call("string", vec![4])),
            (6, call("concat", vec![0, 2, 5, 3, 1])),
            (7, field("Skip")),
            (8, Node::Const { value: Value::Null }),
            (
                9,
                Node::If {
                    condition: 7,
                    then: 8,
                    else_: 6,
                },
            ),
            (10, field("Invalid")),
            (
                11,
                Node::Const {
                    value: Value::Int(1),
                },
            ),
            (
                12,
                Node::If {
                    condition: 10,
                    then: 11,
                    else_: 9,
                },
            ),
            (13, field("Fail")),
            (14, text("not-a-date")),
            (15, text("[Y]")),
            (16, call("format_datetime", vec![14, 15])),
            (
                17,
                Node::If {
                    condition: 13,
                    then: 16,
                    else_: 12,
                },
            ),
            (
                50,
                Node::DistinctTextMapLast {
                    collection: vec!["Rows".into()],
                    key: vec!["Key".into()],
                    expression: 17,
                },
            ),
            (60, text("out-")),
            (61, text(".txt")),
            (70, call("concat", vec![60, 50, 61])),
        ]
        .into(),
    };
    base.root.bindings = vec![Binding {
        target_field: "Selected".into(),
        node: 50,
    }];
    assert!(base.root.set_output_path(Some(70)));
    base
}
