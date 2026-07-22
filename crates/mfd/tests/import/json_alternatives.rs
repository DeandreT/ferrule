use super::*;

use std::collections::BTreeMap;

use ir::{GroupAlternative, GroupAlternativeConstraint, GroupAlternativeConstraintValue};
use mapping::{Binding, Graph, Project, Scope, ScopeIteration};

#[test]
fn object_one_of_subtypes_import_execute_and_select_xml_types() {
    let imported = mfd::import(&fixture("json-alternatives.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let source_address = imported
        .project
        .source
        .child("Rows")
        .and_then(|row| row.child("address"))
        .unwrap();
    assert_eq!(source_address.alternatives().len(), 2);
    assert!(source_address.child("state").is_some());
    assert!(source_address.child("postcode").is_some());

    let target_address = imported
        .project
        .target
        .child("Row")
        .and_then(|row| row.child("Address"))
        .unwrap();
    assert_eq!(target_address.alternatives().len(), 3);
    assert!(
        target_address
            .alternatives()
            .iter()
            .any(|alternative| alternative.name.ends_with("}Address"))
    );
    let address_scope = imported.project.root.children[0]
        .children
        .iter()
        .find(|scope| scope.target_field == "Address")
        .unwrap();
    assert_eq!(
        address_scope
            .bindings
            .iter()
            .filter(|binding| binding.target_field == "name")
            .count(),
        1
    );

    let source =
        format_json::read(&fixture("json-alternatives.json"), &imported.project.source).unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 2);
    let domestic = rows[0].field("Address").unwrap();
    assert_eq!(scalar(domestic, "name"), Value::String("Ada".into()));
    assert_eq!(scalar(domestic, "state"), Value::String("WA".into()));
    assert_eq!(scalar(domestic, "postcode"), Value::Null);
    let international = rows[1].field("Address").unwrap();
    assert_eq!(scalar(international, "name"), Value::String("Lin".into()));
    assert_eq!(scalar(international, "state"), Value::Null);
    assert_eq!(
        scalar(international, "postcode"),
        Value::String("SW1".into())
    );

    let xml = format_xml::to_string(&imported.project.target, &output).unwrap();
    assert!(xml.contains("xsi:type=\"ft:Domestic\""), "{xml}");
    assert!(xml.contains("xsi:type=\"ft:International\""), "{xml}");
    assert!(xml.contains("<state>WA</state>"), "{xml}");
    assert!(xml.contains("<postcode>SW1</postcode>"), "{xml}");
}

#[test]
fn required_typed_const_json_alternatives_export_reimport_and_execute() {
    let event = SchemaNode::group(
        "Event",
        vec![
            SchemaNode::scalar("kind", ScalarType::Bool),
            SchemaNode::scalar("value", ScalarType::String),
        ],
    )
    .with_alternatives(vec![
        GroupAlternative {
            name: "active".into(),
            members: vec!["kind".into(), "value".into()],
            required: vec!["kind".into(), "value".into()],
            constraints: vec![GroupAlternativeConstraint {
                member: "kind".into(),
                value: GroupAlternativeConstraintValue::Bool(true),
            }],
        },
        GroupAlternative {
            name: "inactive".into(),
            members: vec!["kind".into(), "value".into()],
            required: vec!["kind".into(), "value".into()],
            constraints: vec![GroupAlternativeConstraint {
                member: "kind".into(),
                value: GroupAlternativeConstraintValue::Bool(false),
            }],
        },
    ])
    .unwrap()
    .repeating();
    let project = Project {
        source: SchemaNode::group("Events", vec![event]),
        target: SchemaNode::group(
            "Result",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Kind", ScalarType::Bool),
                        SchemaNode::scalar("Value", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: Some("events.json".into()),
        target_path: Some("result.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["kind".into()],
                        frame: Some(vec!["Event".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["value".into()],
                        frame: Some(vec!["Event".into()]),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Event".into()]),
                bindings: vec![
                    Binding {
                        target_field: "Kind".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Value".into(),
                        node: 1,
                    },
                ],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    };

    let temp = TempDir::new("json_const_alternatives");
    let design = temp.0.join("events.mfd");
    let warnings = mfd::export(&project, &design).unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let schema_text = std::fs::read_to_string(temp.0.join("events-source.schema.json")).unwrap();
    assert!(schema_text.contains(r#""const": true"#));
    assert!(schema_text.contains(r#""const": false"#));

    let reimported = mfd::import(&design).unwrap();
    assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
    assert!(engine::validate(&reimported.project).is_empty());
    assert_eq!(reimported.project.source, project.source);
    let source = format_json::from_str(
        r#"{"Event":[{"kind":true,"value":"one"},{"kind":false,"value":"two"}]}"#,
        &reimported.project.source,
    )
    .unwrap();
    let output = engine::run(&reimported.project, &source).unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(scalar(&rows[0], "Kind"), Value::Bool(true));
    assert_eq!(scalar(&rows[1], "Value"), Value::String("two".into()));
}
