use ir::{ItemCountRange, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};
use std::collections::BTreeMap;

fn string(name: &str, nullable: bool) -> SchemaNode {
    let node = SchemaNode::scalar(name, ScalarType::String);
    if nullable {
        node.nullable().unwrap()
    } else {
        node
    }
}
fn rows(children: Vec<SchemaNode>, required: Vec<String>) -> SchemaNode {
    SchemaNode::group("Rows", children)
        .with_required_fields(required)
        .unwrap()
        .repeating()
        .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
        .unwrap()
}
pub fn expected(design: &str) -> Project {
    let payload = SchemaNode::group(
        "Payload",
        vec![
            SchemaNode::group("Header", vec![string("Replacement", false)]),
            SchemaNode::scalar("Tail", ScalarType::Bool),
        ],
    );
    let mut project = Project {
        source: SchemaNode::group("Source", vec![rows(vec![string("Value", true)], vec![])])
            .with_required_fields(vec!["Rows".into()])
            .unwrap(),
        target: SchemaNode::group(
            "Report",
            vec![rows(vec![string("Label", true)], vec!["Label".into()])],
        ),
        source_path: Some("source.json".into()),
        target_path: Some("report.json".into()),
        source_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::If {
                        condition: 6,
                        then: 1,
                        else_: 5,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
                (
                    2,
                    Node::RuntimeParameter {
                        name: "payload".into(),
                        ty: ScalarType::String,
                        preview: None,
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String(serde_json::to_string(&payload).unwrap()),
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::String(r#"["Header","Replacement"]"#.into()),
                    },
                ),
                (
                    5,
                    Node::Call {
                        function: "json_parse_field".into(),
                        args: vec![2, 3, 4],
                    },
                ),
                (
                    6,
                    Node::Call {
                        function: "exists".into(),
                        args: vec![1],
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                bindings: vec![Binding {
                    target_field: "Label".into(),
                    node: 0,
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    };
    if matches!(design, "physical" | "constant") {
        project.graph.nodes = BTreeMap::from([
            (
                0,
                Node::If {
                    condition: 3,
                    then: 1,
                    else_: 2,
                },
            ),
            (
                1,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: Some(vec!["Rows".into()]),
                },
            ),
            (
                2,
                if design == "physical" {
                    Node::SourceField {
                        path: vec!["Replacement".into()],
                        frame: Some(vec!["Rows".into()]),
                    }
                } else {
                    Node::Const {
                        value: Value::String("FALLBACK".into()),
                    }
                },
            ),
            (
                3,
                Node::Call {
                    function: "exists".into(),
                    args: vec![1],
                },
            ),
        ]);
        if design == "physical" {
            project.source = SchemaNode::group(
                "Source",
                vec![rows(
                    vec![string("Value", true), string("Replacement", true)],
                    vec![],
                )],
            )
            .with_required_fields(vec!["Rows".into()])
            .unwrap();
        }
    }
    project
}
