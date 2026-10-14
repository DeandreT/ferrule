use std::collections::BTreeMap;

use ir::{ItemCountRange, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration, ScopeSequence};

// Every Project field and node identity is an independently authored expectation.
pub fn expected(mixed: bool) -> Project {
    let rows = SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Choose", ScalarType::Bool),
            SchemaNode::scalar("Then", ScalarType::String),
            SchemaNode::scalar("Else", ScalarType::String),
        ],
    )
    .repeating()
    .with_required_fields(vec!["Then".into(), "Else".into()])
    .unwrap()
    .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
    .unwrap();
    let mut text = SchemaNode::scalar("Text", ScalarType::String);
    text.xml_optional = true;
    let target = SchemaNode::group(
        "Result",
        vec![SchemaNode::group(
            "Batch",
            vec![SchemaNode::group("Record", vec![text]).repeating()],
        )
        .repeating()],
    );
    let field = |name: &str| Node::SourceField {
        path: vec![name.into()],
        frame: Some(vec!["Rows".into()]),
    };
    let nodes = if mixed {
        BTreeMap::from([
            (0, Node::Const { value: Value::String(String::new()) }),
            (1, Node::If { condition: 2, then: 3, else_: 0 }),
            (2, field("Choose")),
            (3, field("Then")),
            (4, Node::If { condition: 2, then: 0, else_: 3 }),
        ])
    } else {
        BTreeMap::from([
            (0, Node::If { condition: 1, then: 2, else_: 3 }),
            (1, field("Choose")),
            (2, field("Then")),
            (3, field("Else")),
            (4, Node::If { condition: 1, then: 3, else_: 2 }),
        ])
    };
    let branch = |node| Scope {
        bindings: vec![Binding { target_field: "Text".into(), node }],
        ..Default::default()
    };
    Project {
        source: SchemaNode::group(
            "Source",
            vec![rows, SchemaNode::scalar("GlobalChoose", ScalarType::Bool)],
        ),
        target,
        source_path: Some("source.json".into()),
        target_path: Some("report.xml".into()),
        source_options: FormatOptions { json_document: true, ..Default::default() },
        target_options: FormatOptions { xml_document: true, ..Default::default() },
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: "Batch".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: vec![Scope {
                    target_field: "Record".into(),
                    iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                        branch(if mixed { 1 } else { 0 }),
                        vec![branch(4)],
                    )),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}
