// Independently authored complete Project oracle for constructed-record dates.
use ir::{ItemCountRange, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, NodeId, Project, Scope, ScopeIteration};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub enum Shape {
    /// `Date` parses the current-parent `Raw` with `picture`; `Picked` parses
    /// `Alt` with `[D]/[M]/[Y]` only when `Choose` is true and else keeps `Note`.
    Source { nested: bool, picture: &'static str },
    /// `Date` parses `Day` from the required `payload` JSON parameter.
    Parser,
}

fn text(value: &str) -> Node {
    Node::Const {
        value: Value::String(value.into()),
    }
}

fn parse_date(value: NodeId, picture: NodeId) -> Node {
    Node::Call {
        function: "parse_date".into(),
        args: vec![value, picture],
    }
}

pub fn expected(shape: Shape) -> Project {
    let scalar = SchemaNode::scalar;
    let fields = vec![
        scalar("Choose", ScalarType::Bool),
        scalar("Raw", ScalarType::String),
        scalar("Alt", ScalarType::String),
        scalar("Note", ScalarType::String),
    ];
    let mut records = SchemaNode::group(
        "Records",
        vec![
            scalar("Text", ScalarType::String),
            scalar("Date", ScalarType::String),
            scalar("Picked", ScalarType::String),
        ],
    )
    .repeating()
    .with_item_count_range(ItemCountRange::new(1, Some(1)).unwrap())
    .unwrap();
    records.json_unique_items = true;
    let nested = matches!(shape, Shape::Source { nested: true, .. });
    let frame = nested.then(|| vec!["Rows".to_string()]);
    let field = |name: &str| Node::SourceField {
        path: vec![name.into()],
        frame: frame.clone(),
    };
    // Function IDs are reserved in declaration order before their inputs.
    let mut nodes = BTreeMap::from([(0, text("created"))]);
    let date = 2;
    let picked = match shape {
        Shape::Source { picture, .. } => {
            nodes.extend([
                (1, text(picture)),
                (2, parse_date(3, 1)),
                (3, field("Raw")),
                (4, text("[D]/[M]/[Y]")),
                (5, parse_date(6, 4)),
                (6, field("Alt")),
                (
                    7,
                    Node::If {
                        condition: 8,
                        then: 5,
                        else_: 9,
                    },
                ),
                (8, field("Choose")),
                (9, field("Note")),
            ]);
            7
        }
        Shape::Parser => {
            let payload = SchemaNode::group("Payload", vec![scalar("Day", ScalarType::String)]);
            nodes.extend([
                (1, text("[Y]-[M]-[D]")),
                (2, parse_date(6, 1)),
                (
                    3,
                    Node::RuntimeParameter {
                        name: "payload".into(),
                        ty: ScalarType::String,
                        preview: Some(r#"{"Day":"1999-12-31"}"#.into()),
                    },
                ),
                (4, text(&serde_json::to_string(&payload).unwrap())),
                (5, text(r#"["Day"]"#)),
                (
                    6,
                    Node::Call {
                        function: "json_parse_field".into(),
                        args: vec![3, 4, 5],
                    },
                ),
                (7, text("[D]/[M]/[Y]")),
                (8, parse_date(9, 7)),
                (9, field("Alt")),
                (
                    10,
                    Node::If {
                        condition: 11,
                        then: 8,
                        else_: 12,
                    },
                ),
                (11, field("Choose")),
                (12, field("Note")),
            ]);
            10
        }
    };
    let records_scope = Scope {
        target_field: "Records".into(),
        bindings: vec![
            Binding {
                target_field: "Text".into(),
                node: 0,
            },
            Binding {
                target_field: "Date".into(),
                node: date,
            },
            Binding {
                target_field: "Picked".into(),
                node: picked,
            },
        ],
        ..Default::default()
    };
    let (source, target, root) = if nested {
        (
            SchemaNode::group(
                "Source",
                vec![
                    SchemaNode::group("Rows", fields)
                        .repeating()
                        .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
                        .unwrap(),
                ],
            ),
            SchemaNode::group(
                "Report",
                vec![
                    SchemaNode::group("Batches", vec![records])
                        .repeating()
                        .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
                        .unwrap(),
                ],
            ),
            Scope {
                children: vec![Scope {
                    target_field: "Batches".into(),
                    iteration: ScopeIteration::Source(vec!["Rows".into()]),
                    children: vec![records_scope],
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
    } else {
        (
            SchemaNode::group("Source", fields),
            SchemaNode::group("Report", vec![records]),
            Scope {
                children: vec![records_scope],
                ..Default::default()
            },
        )
    };
    Project {
        source,
        target,
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
        graph: Graph { nodes },
        root,
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}
