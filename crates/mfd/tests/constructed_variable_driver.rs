use std::collections::BTreeMap;
use std::path::PathBuf;

use ir::{ItemCountRange, ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/constructed-variable-drivers")
        .join(name)
}

fn expected(nested: bool) -> Project {
    let mut records = SchemaNode::group(
        "Records",
        vec![
            SchemaNode::scalar("Text", ScalarType::String),
            SchemaNode::scalar("Count", ScalarType::Int),
        ],
    )
    .repeating()
    .with_item_count_range(ItemCountRange::new(1, Some(1)).unwrap())
    .unwrap();
    records.json_unique_items = true;
    let mut source =
        SchemaNode::group("Source", vec![SchemaNode::scalar("Count", ScalarType::Int)]);
    let mut target = SchemaNode::group("Report", vec![records.clone()]);
    let mut root = Scope {
        children: vec![Scope {
            target_field: "Records".into(),
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
        }],
        ..Default::default()
    };
    if nested {
        source = SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Count", ScalarType::Int)])
                    .repeating()
                    .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
                    .unwrap(),
            ],
        );
        target = SchemaNode::group(
            "Report",
            vec![
                SchemaNode::group("Batches", vec![records])
                    .repeating()
                    .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
                    .unwrap(),
            ],
        );
        root = Scope {
            children: vec![Scope {
                target_field: "Batches".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: root.children,
                ..Default::default()
            }],
            ..Default::default()
        };
    }
    Project {
        source,
        target,
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
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    0,
                    Node::Const {
                        value: ir::Value::String("created".into()),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Count".into()],
                        frame: nested.then(|| vec!["Rows".into()]),
                    },
                ),
            ]),
        },
        root,
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

#[test]
fn constructed_variable_drivers_preserve_complete_project_and_parent_frame() {
    for (design, nested, catalog) in [
        (
            "edi.mfd",
            false,
            include_str!("fixtures/constructed-variable-drivers/cases.json"),
        ),
        (
            "json.mfd",
            false,
            include_str!("fixtures/constructed-variable-drivers/cases.json"),
        ),
        (
            "nested-edi.mfd",
            true,
            include_str!("fixtures/constructed-variable-drivers/nested-cases.json"),
        ),
    ] {
        let imported = mfd::import(&fixture(design)).unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(expected(nested)).unwrap(),
            "{design}"
        );
        assert!(engine::validate(&imported.project).is_empty());
        for case in serde_json::from_str::<Vec<serde_json::Value>>(catalog).unwrap() {
            let input =
                format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source);
            if case["reject"] == true {
                assert!(input.is_err(), "{design}: {case}");
                continue;
            }
            let value = engine::run(&imported.project, &input.unwrap()).unwrap();
            let actual: serde_json::Value = serde_json::from_str(
                &format_json::to_string(&imported.project.target, &value).unwrap(),
            )
            .unwrap();
            assert_eq!(actual, case["expected"], "{design}: {case}");
        }
    }
}

#[test]
fn constructed_variable_driver_preserves_optional_xml_attribute_input() {
    let imported = mfd::import(&fixture("xml.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut project = expected(false);
    project.source = SchemaNode::group(
        "Source",
        vec![SchemaNode::scalar("Count", ScalarType::Int).attribute()],
    );
    project.source_path = Some("source.xml".into());
    project.source_options = FormatOptions {
        xml_document: true,
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(&imported.project).unwrap(),
        serde_json::to_value(&project).unwrap()
    );
    assert!(engine::validate(&imported.project).is_empty());
    for (text, expected) in [
        (
            "<Source/>",
            serde_json::json!({"Records":[{"Text":"created"}]}),
        ),
        (
            "<Source Count=\"0\"/>",
            serde_json::json!({"Records":[{"Text":"created","Count":0}]}),
        ),
        (
            "<Source Count=\"7\"/>",
            serde_json::json!({"Records":[{"Text":"created","Count":7}]}),
        ),
    ] {
        let input = format_xml::from_str(text, &imported.project.source).unwrap();
        let value = engine::run(&imported.project, &input).unwrap();
        let actual: serde_json::Value = serde_json::from_str(
            &format_json::to_string(&imported.project.target, &value).unwrap(),
        )
        .unwrap();
        assert_eq!(actual, expected, "{text}");
    }
    for invalid in ["<Source", "<Source Count=\"invalid\"/>"] {
        assert!(format_xml::from_str(invalid, &imported.project.source).is_err());
    }
}

#[test]
fn constructed_variable_drivers_refuse_unproved_presence_and_context() {
    let original = std::fs::read_to_string(fixture("edi.mfd")).unwrap();
    let nested = std::fs::read_to_string(fixture("nested-edi.mfd")).unwrap();
    let parent_edge = "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"40\"/></edges></vertex>";
    let filter = "<component name=\"filter\" library=\"core\" kind=\"3\"><sources><datapoint pos=\"0\" key=\"120\"/><datapoint pos=\"1\" key=\"121\"/></sources><targets><datapoint pos=\"0\" key=\"110\"/></targets></component><component name=\"constant\" library=\"core\" kind=\"2\"><targets><datapoint pos=\"0\" key=\"105\"/></targets><data><constant value=\"true\" datatype=\"boolean\"/></data></component>";
    let filtered = nested.replace("</children>", &format!("{filter}</children>")).replace(parent_edge, "<vertex vertexkey=\"10\"><edges><edge vertexkey=\"120\"/></edges></vertex><vertex vertexkey=\"105\"><edges><edge vertexkey=\"121\"/></edges></vertex><vertex vertexkey=\"110\"><edges><edge vertexkey=\"40\"/></edges></vertex>");
    let variants = [
        ("optional source does not establish presence", original.replace("<vertex vertexkey=\"101\"><edges><edge vertexkey=\"22\"/></edges></vertex>", ""), "Records"),
        ("malformed literal", original.replace("value=\"created\" datatype=\"string\"", "value=\"NaN\" datatype=\"double\""), "Records"),
        ("foreign constant", original.replace("name=\"constant\" library=\"core\"", "name=\"constant\" library=\"vendor\""), "Records"),
        ("constant input pin", original.replace("<targets><datapoint pos=\"0\" key=\"101\"/>", "<sources><datapoint pos=\"0\" key=\"120\"/></sources><targets><datapoint pos=\"0\" key=\"101\"/>"), "Records"),
        ("constant extra output", original.replace("key=\"101\"/></targets>", "key=\"101\"/><datapoint pos=\"1\" key=\"102\"/></targets>"), "Records"),
        ("connected record input", original.replace("<edge vertexkey=\"22\"/>", "<edge vertexkey=\"22\"/><edge vertexkey=\"20\"/>"), "Records"),
        ("competing scalar aliases", original.replace("<entry name=\"Text\" inpkey=\"22\" outkey=\"23\"/>", "<entry name=\"Text\" inpkey=\"22\" outkey=\"23\"/><entry name=\"Text\" inpkey=\"26\"/>").replace("<edge vertexkey=\"22\"/>", "<edge vertexkey=\"22\"/><edge vertexkey=\"26\"/>"), "Records"),
        ("competing target connectors", original.replace("<entry name=\"array\" inpkey=\"30\"><entry name=\"item\" type=\"json-item\">", "<entry name=\"array\" inpkey=\"30\"><entry name=\"item\" type=\"json-item\" inpkey=\"34\">").replace("<edge vertexkey=\"30\"/>", "<edge vertexkey=\"30\"/><edge vertexkey=\"34\"/>"), "Records"),
        ("copy-all", original.replace("</graph>", "</graph><connections><edge from=\"21\" to=\"30\"><data type=\"2\"/></edge></connections>"), "Records"),
        ("unframed collection", nested.replace(parent_edge, ""), "Batches/Records"),
        ("controlled parent", filtered, "Batches/Records"),
    ];
    for (index, (label, design, target_path)) in variants.into_iter().enumerate() {
        assert_ne!(
            design.as_str(),
            if target_path == "Records" {
                original.as_str()
            } else {
                nested.as_str()
            },
            "{label}"
        );
        let directory = std::env::temp_dir().join(format!(
            "ferrule-constructed-variable-{}-{index}",
            std::process::id()
        ));
        std::fs::create_dir(&directory).unwrap();
        for entry in std::fs::read_dir(fixture("edi.mfd").parent().unwrap()).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), directory.join(entry.file_name())).unwrap();
        }
        let path = directory.join("mapping.mfd");
        std::fs::write(&path, design).unwrap();
        let imported = mfd::import(&path).unwrap();
        assert!(
            imported.warnings.iter().any(|warning| warning
                == &format!(
                    "iteration into `{target_path}` comes from an unsupported feed; skipped"
                )),
            "{label}: {:?}",
            imported.warnings
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}

#[test]
fn constructed_variable_driver_does_not_borrow_a_sibling_frame() {
    let original = std::fs::read_to_string(fixture("nested-edi.mfd")).unwrap();
    let document = roxmltree::Document::parse(&original).unwrap();
    let report = document
        .descendants()
        .find(|node| node.has_tag_name("component") && node.attribute("name") == Some("report"))
        .unwrap();
    let object = report
        .descendants()
        .find(|node| node.has_tag_name("entry") && node.attribute("name") == Some("object"))
        .unwrap();
    let mut design = original.clone();
    design.insert_str(object.range().end - "</entry>".len(), "<entry name=\"Sibling\" type=\"json-property\"><entry name=\"array\" inpkey=\"60\"><entry name=\"item\" type=\"json-item\"><entry name=\"object\"/></entry></entry></entry>");
    design = design.replace("<edge vertexkey=\"40\"/>", "<edge vertexkey=\"60\"/>");
    let directory = std::env::temp_dir().join(format!(
        "ferrule-variable-sibling-frame-{}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    for entry in std::fs::read_dir(fixture("edi.mfd").parent().unwrap()).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), directory.join(entry.file_name())).unwrap();
    }
    let mut schema: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(directory.join("nested-target.schema.json")).unwrap(),
    )
    .unwrap();
    schema["properties"]["Sibling"] = serde_json::json!({"type":"array","items":{"type":"object","properties":{},"additionalProperties":false}});
    std::fs::write(
        directory.join("nested-target.schema.json"),
        serde_json::to_vec(&schema).unwrap(),
    )
    .unwrap();
    std::fs::write(directory.join("mapping.mfd"), design).unwrap();
    let imported = mfd::import(&directory.join("mapping.mfd")).unwrap();
    assert!(
        imported.warnings.iter().any(|warning| warning
            == "iteration into `Batches/Records` comes from an unsupported feed; skipped"),
        "{:?}",
        imported.warnings
    );
    assert!(imported.project.root.children.iter().any(|scope| scope.target_field == "Sibling" && matches!(&scope.iteration, ScopeIteration::Source(path) if path == &["Rows".to_string()])));
    std::fs::remove_dir_all(directory).unwrap();
}
