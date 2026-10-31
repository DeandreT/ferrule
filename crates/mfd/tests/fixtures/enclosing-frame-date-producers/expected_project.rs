// Independently authored complete Project oracle for date parsing occurrences.
use ir::{ItemCountRange, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration, ScopeSequence};
use std::collections::BTreeMap;

pub fn expected(picture: &str) -> Project {
    let rows = SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("RawA", ScalarType::String),
            SchemaNode::scalar("RawB", ScalarType::String),
        ],
    )
    .repeating()
    .with_required_fields(vec!["RawA".into(), "RawB".into()])
    .unwrap()
    .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
    .unwrap();
    assert_eq!(rows.json_object_item_nullable, None);
    assert!(!rows.nullable && !rows.container_nullable && !rows.json_single_container_nullable());
    let mut date = SchemaNode::scalar("Date", ScalarType::String);
    date.xml_optional = true;
    let branch = |node| Scope {
        bindings: vec![Binding {
            target_field: "Date".into(),
            node,
        }],
        ..Default::default()
    };
    Project {
        source: SchemaNode::group("Source", vec![rows]),
        target: SchemaNode::group(
            "Result",
            vec![
                SchemaNode::group(
                    "Batch",
                    vec![SchemaNode::group("Record", vec![date]).repeating()],
                )
                .repeating(),
            ],
        ),
        source_path: Some("source.json".into()),
        target_path: Some("report.xml".into()),
        source_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        // Import derives the output schema-location hint from the target schema.
        target_options: FormatOptions {
            xml_document: true,
            xml_schema_hints: Some(ir::XmlSchemaHints {
                no_namespace_location: Some("target.xsd".into()),
                derivation: Some(ir::XmlSchemaHintDerivation {
                    schema_reference: "target.xsd".into(),
                }),
                ..Default::default()
            }),
            ..Default::default()
        },
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::Const {
                        value: Value::String(picture.into()),
                    },
                ),
                (
                    1,
                    Node::Call {
                        function: "parse_date".into(),
                        args: vec![2, 0],
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: vec!["RawA".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "parse_date".into(),
                        args: vec![4, 0],
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: vec!["RawB".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Batch".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: vec![Scope {
                    target_field: "Record".into(),
                    iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                        branch(1),
                        vec![branch(3)],
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
