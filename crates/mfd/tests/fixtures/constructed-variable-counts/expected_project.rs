// Independently authored complete Project oracle for constructed-record counts.
use ir::{ItemCountRange, ScalarType, SchemaNode, Value};
use mapping::{AggregateOp, Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
pub struct Shape {
    /// `Batches` iterates `Groups`, and each record counts that group's `Rows`.
    pub nested: bool,
    /// A Copy-All record lowers its fields in record order after one pruned
    /// node; field-by-field records lower the count before the literal.
    pub copy_all: bool,
}

fn rows() -> SchemaNode {
    let mut rows = SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Label", ScalarType::String)
                .nullable()
                .unwrap(),
            SchemaNode::scalar("Total", ScalarType::Int),
        ],
    )
    .repeating()
    .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
    .unwrap();
    rows.json_object_item_nullable = Some(true);
    rows
}

pub fn expected(shape: Shape) -> Project {
    let mut records = SchemaNode::group(
        "Records",
        vec![
            SchemaNode::scalar("Text", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
        ],
    )
    .repeating()
    .with_item_count_range(ItemCountRange::new(1, Some(1)).unwrap())
    .unwrap();
    records.json_unique_items = true;
    let created = Node::Const {
        value: Value::String("created".into()),
    };
    // The collection is relative to the enclosing `Groups` frame when nested.
    let count = Node::Aggregate {
        function: AggregateOp::Count,
        collection: vec!["Rows".into()],
        value: vec![],
        expression: None,
        arg: None,
    };
    let (text, counted) = if shape.copy_all { (1, 2) } else { (1, 0) };
    let nodes = BTreeMap::from([(text, created), (counted, count)]);
    let records_scope = Scope {
        target_field: "Records".into(),
        bindings: vec![
            Binding {
                target_field: "Text".into(),
                node: text,
            },
            Binding {
                target_field: "Count".into(),
                node: counted,
            },
        ],
        ..Default::default()
    };
    let (source, target, root) = if shape.nested {
        (
            SchemaNode::group(
                "Source",
                vec![
                    SchemaNode::group(
                        "Groups",
                        vec![SchemaNode::scalar("Name", ScalarType::String), rows()],
                    )
                    .repeating()
                    .with_item_count_range(ItemCountRange::new(0, Some(3)).unwrap())
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
                    iteration: ScopeIteration::Source(vec!["Groups".into()]),
                    children: vec![records_scope],
                    ..Default::default()
                }],
                ..Default::default()
            },
        )
    } else {
        (
            SchemaNode::group("Source", vec![rows()]),
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
