use ir::{ItemCountRange, ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/literal-group-drivers")
        .join(name)
}

fn one_group(name: &str, children: Vec<SchemaNode>) -> SchemaNode {
    let mut schema = SchemaNode::group(name, children)
        .repeating()
        .with_item_count_range(ItemCountRange::new(1, Some(1)).unwrap())
        .unwrap();
    schema.json_unique_items = true;
    schema
}

fn expected(design: &str) -> Project {
    let mut rows = SchemaNode::group(
        "Rows",
        vec![SchemaNode::scalar("Label", ScalarType::String)],
    )
    .repeating()
    .with_item_count_range(ItemCountRange::new(0, Some(3)).unwrap())
    .unwrap();
    rows.json_unique_items = true;
    let (target, graph, root) = match design {
        "root.mfd" => (
            SchemaNode::group(
                "Report",
                vec![
                    one_group("Once", vec![SchemaNode::scalar("Label", ScalarType::String)]),
                    one_group("Empty", vec![]),
                ],
            ),
            Graph {
                nodes: BTreeMap::from([(
                    1,
                    Node::Const {
                        value: ir::Value::String("created".into()),
                    },
                )]),
            },
            Scope {
                children: vec![
                    Scope {
                        target_field: "Once".into(),
                        bindings: vec![Binding {
                            target_field: "Label".into(),
                            node: 1,
                        }],
                        ..Default::default()
                    },
                    Scope {
                        target_field: "Empty".into(),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            },
        ),
        "nested.mfd" => (
            SchemaNode::group(
                "Report",
                vec![
                    SchemaNode::group(
                        "Results",
                        vec![
                            SchemaNode::scalar("Label", ScalarType::String),
                            one_group("Details", vec![SchemaNode::scalar("Value", ScalarType::String)]),
                            one_group("Empty", vec![]),
                        ],
                    )
                    .repeating()
                    .with_item_count_range(ItemCountRange::new(0, Some(3)).unwrap())
                    .unwrap(),
                ],
            ),
            Graph {
                nodes: BTreeMap::from([(
                    2,
                    Node::SourceField {
                        path: vec!["Label".into()],
                        frame: Some(vec!["Rows".into()]),
                    },
                )]),
            },
            Scope {
                children: vec![Scope {
                    target_field: "Results".into(),
                    iteration: ScopeIteration::Source(vec!["Rows".into()]),
                    bindings: vec![Binding {
                        target_field: "Label".into(),
                        node: 2,
                    }],
                    children: vec![
                        Scope {
                            target_field: "Details".into(),
                            bindings: vec![Binding {
                                target_field: "Value".into(),
                                node: 2,
                            }],
                            ..Default::default()
                        },
                        Scope {
                            target_field: "Empty".into(),
                            ..Default::default()
                        },
                    ],
                    ..Default::default()
                }],
                ..Default::default()
            },
        ),
        "array.mfd" => (one_group("Report", vec![]), Graph::default(), Scope::default()),
        _ => panic!("unrecognized authored fixture"),
    };
    Project {
        source: SchemaNode::group("Source", vec![rows]),
        target,
        source_path: Some("source.json".into()),
        target_path: Some("report.json".into()),
        source_options: FormatOptions { json_document: true, ..Default::default() },
        target_options: FormatOptions { json_document: true, ..Default::default() },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph,
        root,
    }
}

#[test]
fn literal_group_drivers_preserve_complete_project_and_parent_context() {
    for (design, catalog) in [
        ("root.mfd", include_str!("fixtures/literal-group-drivers/root-cases.json")),
        ("nested.mfd", include_str!("fixtures/literal-group-drivers/nested-cases.json")),
        ("array.mfd", include_str!("fixtures/literal-group-drivers/array-cases.json")),
    ] {
        let imported = mfd::import(&fixture(design)).unwrap();
        assert!(imported.warnings.is_empty(), "{design}: {:?}", imported.warnings);
        assert_eq!(serde_json::to_value(&imported.project).unwrap(), serde_json::to_value(expected(design)).unwrap(), "{design}");
        assert!(engine::validate(&imported.project).is_empty());
        let cases: Vec<serde_json::Value> = serde_json::from_str(catalog).unwrap();
        for case in cases {
            let source = format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source);
            if case["reject"] == true {
                assert!(source.is_err(), "{design}: {case}");
                continue;
            }
            let value = engine::run(&imported.project, &source.unwrap()).unwrap();
            let actual: serde_json::Value = serde_json::from_str(&format_json::to_string(&imported.project.target, &value).unwrap()).unwrap();
            assert_eq!(actual, case["expected"], "{design}: {case}");
        }
    }
}

#[test]
fn direct_literal_group_drivers_keep_invalid_constants_closed() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let original = std::fs::read_to_string(fixture("root.mfd")).unwrap();
    for (literal, datatype, accepted) in [
        ("", "string", true), ("雪", "string", true),
        ("false", "boolean", true), ("0", "boolean", true),
        ("0", "integer", true), ("-7", "int", true),
        ("9223372036854775807", "long", true),
        ("-1.25", "decimal", true), ("1.5", "double", true), ("0", "float", true),
        ("not-an-integer", "integer", false), ("9223372036854775808", "long", false),
        ("FALSE", "boolean", false), ("NaN", "double", false),
        ("inf", "float", false), ("value", "unrecognized", false),
    ] {
        let directory = std::env::temp_dir().join(format!("ferrule-literal-group-{}-{}", std::process::id(), NEXT.fetch_add(1, Ordering::Relaxed)));
        std::fs::create_dir(&directory).unwrap();
        for name in ["source.schema.json", "root.schema.json"] {
            std::fs::copy(fixture(name), directory.join(name)).unwrap();
        }
        let design = original.replace("value=\"false\" datatype=\"boolean\"", &format!("value=\"{literal}\" datatype=\"{datatype}\""));
        let path = directory.join("mapping.mfd");
        std::fs::write(&path, design).unwrap();
        let imported = mfd::import(&path).unwrap();
        if accepted {
            assert!(imported.warnings.is_empty(), "{datatype} {literal}: {:?}", imported.warnings);
            assert_eq!(serde_json::to_value(imported.project).unwrap(), serde_json::to_value(expected("root.mfd")).unwrap());
        } else {
            assert!(imported.warnings.iter().any(|warning| warning == "iteration into `Once` comes from an unsupported feed; skipped"), "{datatype} {literal}: {:?}", imported.warnings);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn literal_group_drivers_do_not_admit_other_components_or_sequence_controls() {
    let original = std::fs::read_to_string(fixture("root.mfd")).unwrap();
    let driver = "<component name=\"constant\" library=\"core\" kind=\"2\"><targets><datapoint pos=\"0\" key=\"101\"/></targets><data><constant value=\"false\" datatype=\"boolean\"/></data></component>";
    let concat = "<component name=\"concat\" library=\"core\" kind=\"5\"><sources><datapoint pos=\"0\" key=\"120\"/><datapoint pos=\"1\" key=\"121\"/></sources><targets><datapoint pos=\"0\" key=\"101\"/></targets></component>";
    let filter = "<component name=\"filter\" library=\"core\" kind=\"3\"><sources><datapoint pos=\"0\" key=\"120\"/><datapoint pos=\"1\" key=\"121\"/></sources><targets><datapoint pos=\"0\" key=\"110\"/></targets></component><component name=\"constant\" library=\"core\" kind=\"2\"><targets><datapoint pos=\"0\" key=\"105\"/></targets><data><constant value=\"true\" datatype=\"boolean\"/></data></component>";
    let extra_edges = |design: String, edges: &str| design.replace("</vertices>", &format!("{edges}</vertices>"));
    let variants = [
        original.replace(driver, &driver.replace("library=\"core\"", "library=\"vendor\"")),
        original.replace(driver, &driver.replace("kind=\"2\"", "kind=\"5\"")),
        original.replace(driver, &driver.replace("</targets>", "<datapoint pos=\"1\" key=\"104\"/></targets>")),
        extra_edges(original.replace(driver, &driver.replace("<targets>", "<sources><datapoint pos=\"0\" key=\"120\"/></sources><targets>")), "<vertex vertexkey=\"11\"><edges><edge vertexkey=\"120\"/></edges></vertex>"),
        extra_edges(original.replace(driver, concat), "<vertex vertexkey=\"102\"><edges><edge vertexkey=\"120\"/></edges></vertex><vertex vertexkey=\"103\"><edges><edge vertexkey=\"121\"/></edges></vertex>"),
        extra_edges(original.replace("</children>", &format!("{filter}</children>")).replace("<vertex vertexkey=\"101\"><edges><edge vertexkey=\"20\"/></edges></vertex>", "<vertex vertexkey=\"101\"><edges><edge vertexkey=\"120\"/></edges></vertex><vertex vertexkey=\"110\"><edges><edge vertexkey=\"20\"/></edges></vertex>"), "<vertex vertexkey=\"105\"><edges><edge vertexkey=\"121\"/></edges></vertex>"),
        original.replace("<entry name=\"array\" inpkey=\"20\"><entry name=\"item\" type=\"json-item\">", "<entry name=\"array\" inpkey=\"20\"><entry name=\"item\" type=\"json-item\" inpkey=\"23\">").replace("<edge vertexkey=\"20\"/>", "<edge vertexkey=\"20\"/><edge vertexkey=\"23\"/>"),
    ];
    for (index, design) in variants.into_iter().enumerate() {
        assert_ne!(design, original);
        let directory = std::env::temp_dir().join(format!("ferrule-literal-group-control-{}-{index}", std::process::id()));
        std::fs::create_dir(&directory).unwrap();
        for name in ["source.schema.json", "root.schema.json"] {
            std::fs::copy(fixture(name), directory.join(name)).unwrap();
        }
        let path = directory.join("mapping.mfd");
        std::fs::write(&path, design).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert!(imported.warnings.iter().any(|warning| warning == "iteration into `Once` comes from an unsupported feed; skipped"), "control {index}: {:?}", imported.warnings);
        std::fs::remove_dir_all(directory).unwrap();
    }
}
