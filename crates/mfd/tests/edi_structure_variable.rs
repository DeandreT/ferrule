use std::path::PathBuf;

use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/edi-structure-variables/mapping.mfd")
}

#[test]
fn selected_edi_variable_forwards_complete_values_without_physical_endpoints() {
    let imported = mfd::import(&fixture()).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let fields = || {
        vec![
            SchemaNode::scalar("Text", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
        ]
    };
    let expected = Project {
        source: SchemaNode::group("Source", fields()),
        target: SchemaNode::group("Report", fields()),
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
                        path: vec!["Text".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Count".into()],
                        frame: None,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Text".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Count".into(),
                    node: 1,
                },
            ],
            ..Default::default()
        },
    };
    assert_eq!(
        serde_json::to_value(&imported.project).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(engine::validate(&imported.project).is_empty());
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/edi-structure-variables/cases.json")).unwrap();
    for case in cases {
        let source =
            format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source);
        if case.get("reject") == Some(&serde_json::Value::Bool(true)) {
            assert!(source.is_err(), "{case}");
            continue;
        }
        let result = engine::run(&imported.project, &source.unwrap()).unwrap();
        let actual: serde_json::Value = serde_json::from_str(
            &format_json::to_string(&imported.project.target, &result).unwrap(),
        )
        .unwrap();
        assert_eq!(actual, case["expected"], "{case}");
    }
}

#[test]
fn invalid_edi_variable_selectors_retain_explicit_refusals() {
    let original = std::fs::read_to_string(fixture()).unwrap();
    let cases=[
        (original.replace("config=\"Envelope.Config\"","config=\"absent.Config\""),"requires a complete compiled configuration"),
        (original.replace("<entry name=\"Envelope\"/>","<entry name=\"Wrong\"/>"),"selection root does not match its compiled configuration"),
        (original.replace("<entry name=\"N1\"/>","<entry name=\"N9\"/>"),"selected child is absent from its compiled configuration"),
        (original.replace("<entry name=\"N1\"/>","<entry name=\"N1\"><entry name=\"Text\"/></entry>"),"selection must be one flat path of named entries"),
        (original.replace("</parameter>","<root><entry name=\"Envelope\"/></root></parameter>"),"variable must declare one selection root"),
        (original.replace("</parameter>","</parameter><parameter usageKind=\"variable\"/>"),"variable must have one declared parameter"),
        (original.replace("<component name=\"segment\" library=\"text\" kind=\"16\"><data><root>","<component name=\"segment\" library=\"text\" kind=\"16\"><data><root><entry name=\"compute-when\" inpkey=\"60\"/>"),"variable scope controls require a separate contract"),
    ];
    for (index, (design, reason)) in cases.into_iter().enumerate() {
        let directory = std::env::temp_dir().join(format!(
            "ferrule-edi-variable-refusal-{}-{index}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        for entry in std::fs::read_dir(fixture().parent().unwrap()).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                std::fs::copy(&path, directory.join(path.file_name().unwrap())).unwrap();
            }
        }
        let path = directory.join("mapping.mfd");
        std::fs::write(&path, design).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert!(
            imported
                .warnings
                .iter()
                .any(|warning| warning.contains(reason)),
            "{reason}: {:?}",
            imported.warnings
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
