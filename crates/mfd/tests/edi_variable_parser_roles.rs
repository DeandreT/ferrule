use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope};
use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/edi-variable-parser-fields/mapping.mfd")
}

#[test]
fn configured_parser_prefix_payload_preserves_complete_project_and_values() {
    let imported = mfd::import(&fixture()).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let fields = || {
        vec![
            SchemaNode::scalar("Text", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
            SchemaNode::scalar("ParserErrors_Code", ScalarType::String),
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
                (
                    2,
                    Node::SourceField {
                        path: vec!["ParserErrors_Code".into()],
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
                Binding {
                    target_field: "ParserErrors_Code".into(),
                    node: 2,
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
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/edi-variable-parser-fields/cases.json"
    ))
    .unwrap();
    for case in cases {
        let source =
            format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source);
        if case["reject"] == true {
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
fn unconfigured_parser_error_interfaces_retain_a_variable_refusal() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let original = std::fs::read_to_string(fixture()).unwrap();
    let unknown_role = original.replace(
        "<entry name=\"Count\" inpkey=\"24\" outkey=\"25\"/>",
        "<entry name=\"Count\" inpkey=\"24\" outkey=\"25\"/><entry name=\"ParserErrors_N1\"><entry name=\"Detail\" outkey=\"99\"/></entry>",
    );
    let unknown_descendant = original.replace(
        "<entry name=\"ParserErrors_Code\" inpkey=\"26\" outkey=\"27\"/>",
        "<entry name=\"ParserErrors_Code\" inpkey=\"26\" outkey=\"27\"><entry name=\"Detail\" outkey=\"99\"/></entry>",
    );
    for changed in [unknown_role, unknown_descendant] {
        assert_ne!(changed, original);
        let directory = std::env::temp_dir().join(format!(
            "ferrule-edi-parser-role-refusal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        for entry in std::fs::read_dir(fixture().parent().unwrap()).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                std::fs::copy(&path, directory.join(path.file_name().unwrap())).unwrap();
            }
        }
        let path = directory.join("mapping.mfd");
        std::fs::write(&path, changed).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert!(imported.warnings.iter().any(|warning| {
            warning.contains("has parser-error ports absent from its compiled variable schema")
        }));
        std::fs::remove_dir_all(directory).unwrap();
    }
}
