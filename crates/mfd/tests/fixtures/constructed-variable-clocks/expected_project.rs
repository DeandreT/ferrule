// Independently authored complete Project oracle for constructed-record clocks.
use ir::{ItemCountRange, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, RuntimeValue, Scope, ScopeIteration};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub enum Shape {
    /// `Stamp`/`Picked` select a clock only when `Choose` is true.
    Conditional { nested: bool },
    /// `Stamp` and `Picked` read the clock unconditionally.
    Direct,
}

pub fn expected(shape: Shape) -> Project {
    let scalar = SchemaNode::scalar;
    let fields = vec![
        scalar("Choose", ScalarType::Bool),
        scalar("Note", ScalarType::String),
    ];
    let mut records = SchemaNode::group(
        "Records",
        vec![
            scalar("Text", ScalarType::String),
            scalar("Stamp", ScalarType::String),
            scalar("Picked", ScalarType::String),
        ],
    )
    .repeating()
    .with_item_count_range(ItemCountRange::new(1, Some(1)).unwrap())
    .unwrap();
    records.json_unique_items = true;
    let clock = || Node::RuntimeValue {
        value: RuntimeValue::CurrentDateTime,
    };
    let created = Node::Const {
        value: Value::String("created".into()),
    };
    let nested = matches!(shape, Shape::Conditional { nested: true });
    let frame = nested.then(|| vec!["Rows".to_string()]);
    let (nodes, stamp, picked) = match shape {
        Shape::Direct => (
            BTreeMap::from([(0, created), (1, clock()), (2, clock())]),
            1,
            2,
        ),
        // Function IDs are reserved in declaration order before their inputs;
        // the shared `Choose` and `Note` feeds lower once.
        Shape::Conditional { .. } => (
            BTreeMap::from([
                (0, created),
                (
                    1,
                    Node::If {
                        condition: 2,
                        then: 3,
                        else_: 4,
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["Choose".into()],
                        frame: frame.clone(),
                    },
                ),
                (3, clock()),
                (
                    4,
                    Node::SourceField {
                        path: vec!["Note".into()],
                        frame,
                    },
                ),
                (
                    5,
                    Node::If {
                        condition: 2,
                        then: 6,
                        else_: 4,
                    },
                ),
                (6, clock()),
            ]),
            1,
            5,
        ),
    };
    let records_scope = Scope {
        target_field: "Records".into(),
        bindings: vec![
            Binding {
                target_field: "Text".into(),
                node: 0,
            },
            Binding {
                target_field: "Stamp".into(),
                node: stamp,
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
