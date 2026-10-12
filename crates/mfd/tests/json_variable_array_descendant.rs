use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};
use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/json-variable-array-descendants/mapping.mfd")
}

#[test]
fn declared_array_descendant_preserves_complete_project_and_ordered_values() {
    let imported = mfd::import(&fixture()).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let fields = || {
        vec![
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Total", ScalarType::Int),
        ]
    };
    let expected = Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::group("Header", fields())]).repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Report",
            vec![SchemaNode::group("Results", fields()).repeating()],
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
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: std::collections::BTreeMap::from([
                (
                    0,
                    Node::SourceField {
                        path: vec!["Header".into(), "Label".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Header".into(), "Total".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Results".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                bindings: vec![
                    Binding {
                        target_field: "Label".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Total".into(),
                        node: 1,
                    },
                ],
                ..Default::default()
            }],
            ..Default::default()
        },
    };
    assert_eq!(
        serde_json::to_value(&imported.project).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(engine::validate(&imported.project).is_empty());
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/json-variable-array-descendants/cases.json"
    ))
    .unwrap();
    for case in cases {
        let source =
            format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source);
        if case.get("reject") == Some(&serde_json::Value::Bool(true)) {
            assert!(source.is_err(), "{case}");
            continue;
        }
        let value = engine::run(&imported.project, &source.unwrap()).unwrap();
        let actual: serde_json::Value = serde_json::from_str(
            &format_json::to_string(&imported.project.target, &value).unwrap(),
        )
        .unwrap();
        assert_eq!(actual, case["expected"], "{case}");
    }
}
