use std::path::PathBuf;

use ir::SchemaNode;
use mapping::{Binding, Graph, Node, Project, Scope};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/json-structure-variables/mapping.mfd")
}

#[test]
fn selected_json_variable_forwards_complete_values_without_physical_endpoints() {
    let imported = mfd::import(&fixture()).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut expected = Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar("Code", ir::ScalarType::String),
                SchemaNode::scalar("Quantity", ir::ScalarType::Int),
            ],
        ),
        target: SchemaNode::group(
            "Report",
            vec![
                SchemaNode::scalar("Code", ir::ScalarType::String),
                SchemaNode::scalar("Quantity", ir::ScalarType::Int),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    };
    expected.source_path = Some("source.json".into());
    expected.target_path = Some("report.json".into());
    expected.source_options.json_document = true;
    expected.target_options.json_document = true;
    expected.graph = Graph {
        nodes: std::collections::BTreeMap::from([
            (
                0,
                Node::SourceField {
                    path: vec!["Code".into()],
                    frame: None,
                },
            ),
            (
                1,
                Node::SourceField {
                    path: vec!["Quantity".into()],
                    frame: None,
                },
            ),
        ]),
    };
    expected.root = Scope {
        bindings: vec![
            Binding {
                target_field: "Code".into(),
                node: 0,
            },
            Binding {
                target_field: "Quantity".into(),
                node: 1,
            },
        ],
        ..Scope::default()
    };
    assert_eq!(
        serde_json::to_value(&imported.project).unwrap(),
        serde_json::to_value(&expected).unwrap()
    );
    assert!(engine::validate(&imported.project).is_empty());
    let cases: Vec<serde_json::Value> =
        serde_json::from_str(include_str!("fixtures/json-structure-variables/cases.json")).unwrap();
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
fn constructed_json_variable_preserves_complete_record_and_missing_fields() {
    let path = fixture().with_file_name("constructed.mfd");
    let imported = mfd::import(&path).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(imported.project.extra_sources.is_empty());
    assert!(imported.project.extra_targets.is_empty());
    assert!(engine::validate(&imported.project).is_empty());
    let expected_root = Scope {
        bindings: vec![
            Binding {
                target_field: "Code".into(),
                node: 0,
            },
            Binding {
                target_field: "Quantity".into(),
                node: 1,
            },
        ],
        children: vec![Scope {
            target_field: "Stored".into(),
            bindings: vec![
                Binding {
                    target_field: "Code".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Quantity".into(),
                    node: 1,
                },
            ],
            ..Scope::default()
        }],
        ..Scope::default()
    };
    assert_eq!(
        serde_json::to_value(&imported.project.root).unwrap(),
        serde_json::to_value(expected_root).unwrap()
    );
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/json-structure-variables/constructed-cases.json"
    ))
    .unwrap();
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
fn invalid_variable_selectors_and_record_ownership_retain_refusals() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let original = std::fs::read_to_string(fixture()).unwrap();
    let constructed = std::fs::read_to_string(fixture().with_file_name("constructed.mfd")).unwrap();
    let selected = "<entry name=\"Stored\" type=\"json-property\"/>";
    let cases=[
        (constructed.replace("constructed-source.schema.json","source.schema.json"),"record requires a non-null constant or required non-null JSON scalar input to establish its presence"),
        (original.replace(selected,"<entry name=\"Absent\" type=\"json-property\"/>"),"selected property is absent from its schema"),
        (original.replace(selected,"<entry name=\"Stored\"/>"),"selection has an unsupported structural view"),
        (original.replace(selected,"<entry name=\"Stored\" type=\"json-property\"><entry name=\"Code\"/></entry>"),"selection must be one flat declared path"),
        (original.replace("</parameter>","<root><entry name=\"root\"/></root></parameter>"),"selection contains multiple roots"),
        (original.replace("<entry name=\"document\"><entry name=\"object\" inpkey=\"20\"","<entry name=\"compute-when\" inpkey=\"60\"><entry name=\"object\" inpkey=\"20\""),"compute-when and parent-context controls require a separate scope contract"),
        (constructed.replace("<entry name=\"integer\" inpkey=\"24\" outkey=\"25\"/>","<entry name=\"integer\" inpkey=\"24\" outkey=\"25\"/><entry name=\"integer\" inpkey=\"26\"/>").replace("<edge vertexkey=\"24\"/>","<edge vertexkey=\"24\"/><edge vertexkey=\"26\"/>"),"record has multiple declared inputs for one scalar field"),
        (constructed.replace("<connections>","<connections><edge from=\"10\" to=\"20\"/>"),"record has a connected structural or unsupported descendant input"),
    ];
    for (design, reason) in cases {
        let directory = std::env::temp_dir().join(format!(
            "ferrule-json-variable-refusal-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        for entry in std::fs::read_dir(fixture().parent().unwrap()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "json") {
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

#[test]
fn selected_arrays_and_repeating_record_inputs_are_not_silently_scalarized() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    for selected_array in [true, false] {
        let directory = std::env::temp_dir().join(format!(
            "ferrule-json-variable-array-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&directory).unwrap();
        for entry in std::fs::read_dir(fixture().parent().unwrap()).unwrap() {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|ext| ext == "json") {
                std::fs::copy(&path, directory.join(path.file_name().unwrap())).unwrap();
            }
        }
        let (design, reason) = if selected_array {
            let path = directory.join("variable.schema.json");
            let mut schema: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            let record = schema["properties"]["Stored"].take();
            schema["properties"]["Stored"] = serde_json::json!({"type":"array","items":record});
            std::fs::write(&path, serde_json::to_vec(&schema).unwrap()).unwrap();
            (
                std::fs::read_to_string(fixture()).unwrap().replace(
                    "<entry name=\"Stored\" type=\"json-property\"/><entry name=\"object\"/>",
                    "<entry name=\"Stored\" type=\"json-property\"/>",
                ),
                "selected root is an array",
            )
        } else {
            let path = directory.join("constructed-source.schema.json");
            let record: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            std::fs::write(
                &path,
                serde_json::to_vec(
                    &serde_json::json!({"title":"Source","type":"array","items":record}),
                )
                .unwrap(),
            )
            .unwrap();
            (
                std::fs::read_to_string(fixture().with_file_name("constructed.mfd")).unwrap(),
                "record input crosses a repeating or unsupported source boundary",
            )
        };
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

#[test]
fn constructed_child_record_does_not_discard_a_connected_ancestor() {
    let directory = std::env::temp_dir().join(format!(
        "ferrule-json-variable-ancestor-{}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    for entry in std::fs::read_dir(fixture().parent().unwrap()).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "json") {
            std::fs::copy(&path, directory.join(path.file_name().unwrap())).unwrap();
        }
    }
    let path = directory.join("variable.schema.json");
    let mut schema: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let record = schema["properties"]["Stored"].take();
    schema["properties"]["Stored"] = serde_json::json!({"type":"object","properties":{"Nested":record},"additionalProperties":false});
    std::fs::write(path, serde_json::to_vec(&schema).unwrap()).unwrap();
    let design=std::fs::read_to_string(fixture().with_file_name("constructed.mfd")).unwrap()
        .replace("<entry name=\"object\" inpkey=\"20\" outkey=\"21\">","<entry name=\"object\" inpkey=\"20\"><entry name=\"Nested\" type=\"json-property\"><entry name=\"object\" outkey=\"21\">")
        .replace("</root><parameter usageKind=\"variable\">","</entry></entry></root><parameter usageKind=\"variable\">")
        .replace("<connections>","<connections><edge from=\"10\" to=\"20\"/>");
    let path = directory.join("mapping.mfd");
    std::fs::write(&path, design).unwrap();
    let imported = mfd::import(&path).unwrap();
    assert!(
        imported
            .warnings
            .iter()
            .any(|warning| warning.contains("record has a connected structural ancestor input")),
        "{:?}",
        imported.warnings
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn constant_input_establishes_record_presence_without_optional_source_values() {
    let imported = mfd::import(&fixture().with_file_name("constant.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(imported.project.extra_sources.is_empty());
    assert!(imported.project.extra_targets.is_empty());
    let expected = Graph {
        nodes: std::collections::BTreeMap::from([
            (
                0,
                Node::Const {
                    value: ir::Value::String("fixed".into()),
                },
            ),
            (
                1,
                Node::SourceField {
                    path: vec!["Quantity".into()],
                    frame: None,
                },
            ),
        ]),
    };
    assert_eq!(
        serde_json::to_value(&imported.project.graph).unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/json-structure-variables/constant-cases.json"
    ))
    .unwrap();
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
fn nullable_inputs_do_not_establish_a_constructed_record() {
    for (label, schema, input) in [
        (
            "scalar",
            r#"{"title":"Source","type":"object","properties":{"Code":{"type":["string","null"]},"Quantity":{"type":"integer"}},"required":["Code"],"additionalProperties":false}"#,
            r#"{"Code":null}"#,
        ),
        (
            "container",
            r#"{"title":"Source","type":["object","null"],"properties":{"Code":{"type":"string"},"Quantity":{"type":"integer"}},"required":["Code"],"additionalProperties":false}"#,
            "null",
        ),
    ] {
        let directory = std::env::temp_dir().join(format!(
            "ferrule-json-variable-null-{label}-{}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        for entry in std::fs::read_dir(fixture().parent().unwrap()).unwrap() {
            let path = entry.unwrap().path();
            if path.is_file() {
                std::fs::copy(&path, directory.join(path.file_name().unwrap())).unwrap();
            }
        }
        std::fs::write(directory.join("constructed-source.schema.json"), schema).unwrap();
        let imported = mfd::import(&directory.join("constructed.mfd")).unwrap();
        assert!(
            format_json::from_str(input, &imported.project.source).is_ok(),
            "the independent null input must be valid"
        );
        assert!(imported.warnings.iter().any(|warning|warning.contains("record requires a non-null constant or required non-null JSON scalar input to establish its presence")),"{:?}",imported.warnings);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
