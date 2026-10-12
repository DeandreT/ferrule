use std::collections::BTreeMap;
use std::path::PathBuf;

use ir::{ItemCountRange, ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/constructed-variable-aliases")
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
fn scalar_alias_drivers_preserve_complete_project_and_native_values() {
    for (name, nested) in [
        ("one.mfd", false),
        ("edi-alias.mfd", false),
        ("two.mfd", false),
        ("nested.mfd", true),
        ("limit.mfd", false),
    ] {
        let path = fixture(name);
        let imported = mfd::import_with_options(
            &path,
            &mfd::ImportOptions::default()
                .with_package_root(path.parent().unwrap().parent().unwrap()),
        )
        .unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{name}: {:?}",
            imported.warnings
        );
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(expected(nested)).unwrap(),
            "{name}"
        );
        assert!(engine::validate(&imported.project).is_empty());
        let catalog = if nested {
            include_str!("fixtures/constructed-variable-drivers/nested-cases.json")
        } else {
            include_str!("fixtures/constructed-variable-drivers/cases.json")
        };
        for case in serde_json::from_str::<Vec<serde_json::Value>>(catalog).unwrap() {
            let input =
                format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source);
            if case["reject"] == true {
                assert!(input.is_err(), "{name}: {case}");
            } else {
                let value = engine::run(&imported.project, &input.unwrap()).unwrap();
                let actual: serde_json::Value = serde_json::from_str(
                    &format_json::to_string(&imported.project.target, &value).unwrap(),
                )
                .unwrap();
                assert_eq!(actual, case["expected"], "{name}: {case}");
            }
        }
    }
}

fn import_control(design: &str, label: &str, schema: Option<serde_json::Value>) -> mfd::Imported {
    let directory = std::env::temp_dir().join(format!(
        "ferrule-scalar-alias-{}-{label}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let resources = fixture("one.mfd")
        .parent()
        .unwrap()
        .join("../constructed-variable-drivers");
    for entry in std::fs::read_dir(resources).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(entry.path(), directory.join(entry.file_name())).unwrap();
    }
    if let Some(schema) = schema {
        std::fs::write(
            directory.join("alias.schema.json"),
            serde_json::to_vec(&schema).unwrap(),
        )
        .unwrap();
    }
    let path = directory.join("mapping.mfd");
    std::fs::write(
        &path,
        design.replace("../constructed-variable-drivers/", ""),
    )
    .unwrap();
    let imported = mfd::import(&path).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    imported
}

#[test]
fn scalar_alias_drivers_refuse_ambiguous_controls_and_metadata_changes() {
    let one = std::fs::read_to_string(fixture("one.mfd")).unwrap();
    let nested = std::fs::read_to_string(fixture("nested.mfd")).unwrap();
    let variants = [
        ("ancestor", one.replace("<edge vertexkey=\"402\"/>", "<edge vertexkey=\"402\"/><edge vertexkey=\"400\"/>"), "Records"),
        ("duplicate-input", one.replace("inpkey=\"402\" outkey=\"403\"/>", "inpkey=\"402\" outkey=\"403\"/><entry name=\"integer\" inpkey=\"405\"/>").replace("<edge vertexkey=\"402\"/>", "<edge vertexkey=\"402\"/><edge vertexkey=\"405\"/>"), "Records"),
        ("control", one.replace("<entry name=\"document\"><entry name=\"object\" inpkey=\"400\">", "<entry name=\"compute-when\" inpkey=\"407\"/><entry name=\"document\"><entry name=\"object\" inpkey=\"400\">"), "Records"),
        ("foreign-owner", one.replace("name=\"alias0\" library=\"json\"", "name=\"alias0\" library=\"vendor\""), "Records"),
        ("physical-collision", one.replace("outkey=\"12\"", "outkey=\"403\""), "Records"),
        ("unframed", nested.replace("<vertex vertexkey=\"10\"><edges><edge vertexkey=\"40\"/></edges></vertex>", ""), "Batches/Records"),
    ];
    for (label, design, path) in variants {
        assert_ne!(
            design,
            if path == "Records" {
                one.clone()
            } else {
                nested.clone()
            },
            "{label}"
        );
        let imported = import_control(&design, label, None);
        assert!(
            imported.warnings.iter().any(|warning| warning
                == &format!("iteration into `{path}` comes from an unsupported feed; skipped")),
            "{label}: {:?}",
            imported.warnings
        );
    }
    for (label, change) in [
        ("minimum", serde_json::json!({"minimum":0})),
        ("nullable", serde_json::json!({"type":["integer","null"]})),
        ("allowed", serde_json::json!({"enum":[1,2]})),
        ("multiple-of", serde_json::json!({"multipleOf":2})),
    ] {
        let mut schema: serde_json::Value = serde_json::from_str(include_str!(
            "fixtures/constructed-variable-drivers/source.schema.json"
        ))
        .unwrap();
        for (key, value) in change.as_object().unwrap() {
            schema["properties"]["Count"][key] = value.clone();
        }
        let design = one.replace("<json schema=\"../constructed-variable-drivers/source.schema.json\"/></data></component>", "<json schema=\"alias.schema.json\"/></data></component>");
        let imported = import_control(&design, label, Some(schema));
        assert!(
            imported.warnings.iter().any(|warning| warning
                == "iteration into `Records` comes from an unsupported feed; skipped"),
            "{label}: {:?}",
            imported.warnings
        );
    }
}

#[test]
fn scalar_alias_driver_proof_stops_at_cycles_and_excessive_hops() {
    let one = std::fs::read_to_string(fixture("one.mfd")).unwrap();
    // These controls isolate the driver proof; do not ask the legacy scalar
    // binder to evaluate a malformed cyclic leaf as well.
    let binding = "<vertex vertexkey=\"25\"><edges><edge vertexkey=\"32\"/></edges></vertex>";
    let cycle = one
        .replace(
            "<vertex vertexkey=\"12\"><edges><edge vertexkey=\"402\"/></edges></vertex>",
            "<vertex vertexkey=\"403\"><edges><edge vertexkey=\"402\"/></edges></vertex>",
        )
        .replace(binding, "");
    let limit = std::fs::read_to_string(fixture("limit.mfd")).unwrap();
    let alias = "<component name=\"alias64\" library=\"json\" kind=\"31\"><data><root><entry name=\"document\"><entry name=\"object\"><entry name=\"Count\" type=\"json-property\"><entry name=\"integer\" inpkey=\"1042\" outkey=\"1043\"/></entry></entry></entry></root><parameter usageKind=\"variable\"><root><entry name=\"root\"/><entry name=\"object\"/></root></parameter><json schema=\"../constructed-variable-drivers/source.schema.json\"/></data></component>";
    let over = limit.replace("</children>", &format!("{alias}</children>")).replace("<vertex vertexkey=\"1033\"><edges><edge vertexkey=\"24\"/></edges></vertex>", "<vertex vertexkey=\"1033\"><edges><edge vertexkey=\"1042\"/></edges></vertex><vertex vertexkey=\"1043\"><edges><edge vertexkey=\"24\"/></edges></vertex>").replace(binding, "");
    for (label, design) in [("cycle", cycle), ("hop-limit", over)] {
        let imported = import_control(&design, label, None);
        assert!(
            imported.warnings.iter().any(|warning| warning
                == "iteration into `Records` comes from an unsupported feed; skipped"),
            "{label}: {:?}",
            imported.warnings
        );
    }
}
