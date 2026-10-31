// Literal Project oracle for enclosing-frame String value maps.
// IDs and every Project field are independently authored expectations.
use ir::{ItemCountRange, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration, ScopeSequence};
use std::collections::BTreeMap;

pub fn expected_total_string_value_map_project() -> Project {
    let mut rows = SchemaNode::group("Rows", vec![SchemaNode::scalar("Code", ScalarType::String)])
        .repeating()
        .with_required_fields(vec!["Code".into()])
        .unwrap()
        .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
        .unwrap();
    rows.json_object_item_nullable = None;
    assert!(!rows.container_nullable && !rows.json_single_container_nullable());
    let scalar = |name| {
        let mut node = SchemaNode::scalar(name, ScalarType::String);
        node.xml_optional = true;
        node
    };
    let target = SchemaNode::group(
        "Result",
        vec![
            SchemaNode::group(
                "Batch",
                vec![SchemaNode::group("Record", vec![scalar("Type"), scalar("Note")]).repeating()],
            )
            .repeating(),
        ],
    );
    let table = vec![
        (Value::String("A".into()), Value::String("alpha".into())),
        (Value::String("B".into()), Value::String("beta".into())),
        (Value::String("D".into()), Value::String("first".into())),
        (Value::String("D".into()), Value::String("second".into())),
        (
            Value::String(String::new()),
            Value::String("empty-key".into()),
        ),
        (Value::String("E".into()), Value::String(String::new())),
        (Value::String("x&<".into()), Value::String("α&<".into())),
    ];
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::Const {
                    value: Value::String("literal".into()),
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String("anchor".into()),
                },
            ),
            (
                2,
                Node::Const {
                    value: Value::String("mapped".into()),
                },
            ),
            (
                4,
                Node::SourceField {
                    path: vec!["Code".into()],
                    frame: Some(vec!["Rows".into()]),
                },
            ),
            (
                3,
                Node::ValueMap {
                    input: 4,
                    input_type: Some(ScalarType::String),
                    table,
                    default: Some(Value::String("other".into())),
                },
            ),
        ]),
    };
    let branch = |kind, note| Scope {
        bindings: vec![
            Binding {
                target_field: "Type".into(),
                node: kind,
            },
            Binding {
                target_field: "Note".into(),
                node: note,
            },
        ],
        ..Default::default()
    };
    Project {
        source: SchemaNode::group("Source", vec![rows]),
        target,
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
        graph,
        root: Scope {
            children: vec![Scope {
                target_field: "Batch".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: vec![Scope {
                    target_field: "Record".into(),
                    iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                        branch(0, 1),
                        vec![branch(2, 3)],
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
