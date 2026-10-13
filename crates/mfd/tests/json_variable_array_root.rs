use ir::{ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};
use std::path::PathBuf;

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/json-variable-array-roots/mapping.mfd")
}

#[test]
fn selected_array_root_preserves_complete_project_and_ordered_values() {
    let imported = mfd::import(&fixture()).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let records = |name| {
        let mut schema = SchemaNode::group(
            name,
            vec![
                SchemaNode::scalar("Label", ScalarType::String),
                SchemaNode::scalar("Total", ScalarType::Int),
            ],
        )
        .repeating()
        .with_item_count_range(ir::ItemCountRange::new(0, Some(3)).unwrap())
        .unwrap();
        schema.json_unique_items = true;
        schema
    };
    let expected = Project {
        source: SchemaNode::group("Source", vec![records("Rows")]),
        target: SchemaNode::group("Report", vec![records("Results")]),
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
                        path: vec!["Label".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Total".into()],
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
        "fixtures/json-variable-array-roots/cases.json"
    ))
    .unwrap();
    for case in cases {
        let source =
            format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source);
        if case["reject"] == true {
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

#[test]
fn selected_array_root_refuses_competing_drivers_and_descendant_overrides() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let original = std::fs::read_to_string(fixture()).unwrap();
    for (design, reason) in [
        (original.replace("<vertex vertexkey=\"10\">", "<vertex vertexkey=\"11\"><edges><edge vertexkey=\"22\"/></edges></vertex><vertex vertexkey=\"10\">"), "a selected array root cannot have connected descendant overrides"),
        (original.replace("inpkey=\"20\" outkey=\"21\"><entry name=\"item\" type=\"json-item\"", "inpkey=\"20\" outkey=\"21\"><entry name=\"item\" type=\"json-item\" inpkey=\"26\"").replace("<edge vertexkey=\"20\"/>", "<edge vertexkey=\"20\"/><edge vertexkey=\"26\"/>"), "a selected array root must have one explicit structural driver"),
        (original.replace("<edge vertexkey=\"20\"/>", ""), "a selected array root must have one explicit structural driver"),
        (original.replace("<entry name=\"Batches\" type=\"json-property\"/><entry name=\"array\"/>", "<entry name=\"Batches\" type=\"json-property\"/><entry name=\"array\"/><entry name=\"item\" type=\"json-item\"/><entry name=\"object\"/>"), "selected root is an array"),
    ] {
        let directory = std::env::temp_dir().join(format!("ferrule-json-array-root-refusal-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&directory).unwrap();
        for name in ["source.schema.json", "variable.schema.json", "target.schema.json"] {
            std::fs::copy(fixture().with_file_name(name), directory.join(name)).unwrap();
        }
        let path = directory.join("mapping.mfd");
        std::fs::write(&path, design).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert!(imported.warnings.iter().any(|warning| warning.contains(reason)), "{reason}: {:?}", imported.warnings);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
