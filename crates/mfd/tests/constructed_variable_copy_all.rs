use std::collections::BTreeMap;
use std::path::PathBuf;

use ir::{ItemCountRange, ScalarType, SchemaNode};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/constructed-variable-copy-all")
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
fn constructed_copy_all_preserves_complete_project_and_parent_frames() {
    for (design, nested, overrides) in [
        ("explicit-edi.mfd", false, false),
        ("json-object.mfd", false, false),
        ("json-array.mfd", false, false),
        ("edi-array.mfd", false, false),
        ("nested-edi-array.mfd", true, false),
        ("override-json.mfd", false, true),
        ("alias-json.mfd", false, false),
    ] {
        let path = fixture(design);
        let imported = mfd::import_with_options(
            &path,
            &mfd::ImportOptions::default()
                .with_package_root(path.parent().unwrap().parent().unwrap()),
        )
        .unwrap();
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        let mut oracle = expected(nested);
        if overrides {
            oracle.graph.nodes = BTreeMap::from([
                // Both literal functions reserve IDs before scalar projection;
                // the unused presence literal at ID 0 is subsequently pruned.
                (
                    2,
                    Node::SourceField {
                        path: vec!["Count".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: ir::Value::String("override".into()),
                    },
                ),
            ]);
            oracle.root.children[0].bindings = vec![
                Binding {
                    target_field: "Count".into(),
                    node: 2,
                },
                Binding {
                    target_field: "Text".into(),
                    node: 1,
                },
            ];
        }
        assert_eq!(
            serde_json::to_value(&imported.project).unwrap(),
            serde_json::to_value(&oracle).unwrap(),
            "{design}"
        );
        assert!(engine::validate(&imported.project).is_empty());
        let catalog = if nested {
            include_str!("fixtures/constructed-variable-drivers/nested-cases.json")
        } else {
            include_str!("fixtures/constructed-variable-drivers/cases.json")
        };
        for mut case in serde_json::from_str::<Vec<serde_json::Value>>(catalog).unwrap() {
            let input =
                format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source);
            if case["reject"] == true {
                assert!(input.is_err(), "{design}: {case}");
                continue;
            }
            if overrides {
                case["expected"]["Records"][0]["Text"] = "override".into();
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

fn import_control(
    design: &str,
    label: &str,
    variable: &serde_json::Value,
    target: &serde_json::Value,
) -> mfd::Imported {
    let directory = std::env::temp_dir().join(format!(
        "ferrule-variable-copy-all-{}-{label}",
        std::process::id()
    ));
    std::fs::create_dir(&directory).unwrap();
    let source = fixture("json-array.mfd")
        .parent()
        .unwrap()
        .join("../constructed-variable-drivers/source.schema.json");
    std::fs::copy(source, directory.join("source.schema.json")).unwrap();
    std::fs::write(
        directory.join("variable-array.schema.json"),
        serde_json::to_vec(variable).unwrap(),
    )
    .unwrap();
    std::fs::write(
        directory.join("target.schema.json"),
        serde_json::to_vec(target).unwrap(),
    )
    .unwrap();
    let path = directory.join("mapping.mfd");
    std::fs::write(
        &path,
        design.replace("../constructed-variable-drivers/", ""),
    )
    .unwrap();
    let result = mfd::import(&path).unwrap();
    std::fs::remove_dir_all(directory).unwrap();
    result
}

#[test]
fn constructed_copy_all_refuses_unproved_cardinality_shapes_and_facets() {
    let design = std::fs::read_to_string(fixture("json-array.mfd")).unwrap();
    let variable: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/constructed-variable-copy-all/variable-array.schema.json"
    ))
    .unwrap();
    let target: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/constructed-variable-drivers/target.schema.json"
    ))
    .unwrap();
    for label in [
        "source-minimum",
        "source-maximum",
        "target-minimum",
        "target-maximum",
        "contains",
        "facet",
        "nullable",
        "required",
        "repeated-child",
        "record-input",
        "ambiguous-input",
        "no-presence",
    ] {
        let mut variable = variable.clone();
        let mut target = target.clone();
        let mut design = design.clone();
        match label {
            "source-minimum" => {
                variable["properties"]["Stored"]["minItems"] = 2.into();
            }
            "source-maximum" => {
                variable["properties"]["Stored"]["maxItems"] = 0.into();
            }
            "target-minimum" => {
                target["properties"]["Records"]["minItems"] = 2.into();
                target["properties"]["Records"]["maxItems"] = 4.into();
            }
            "target-maximum" => {
                target["properties"]["Records"]["minItems"] = 0.into();
                target["properties"]["Records"]["maxItems"] = 0.into();
            }
            "contains" => {
                variable["properties"]["Stored"]["contains"] = serde_json::json!({"type":"object"});
            }
            "facet" => {
                variable["properties"]["Stored"]["items"]["properties"]["Count"]["minimum"] =
                    0.into();
            }
            "nullable" => {
                variable["properties"]["Stored"]["items"]["properties"]["Count"]["type"] =
                    serde_json::json!(["integer", "null"]);
            }
            "required" => {
                variable["properties"]["Stored"]["items"]["required"] =
                    serde_json::json!(["Count"]);
            }
            "repeated-child" => {
                variable["properties"]["Stored"]["items"]["properties"]["Count"] =
                    serde_json::json!({"type":"array","items":{"type":"integer"}});
            }
            "record-input" => {
                design = design.replace(
                    "<edge vertexkey=\"22\"/>",
                    "<edge vertexkey=\"22\"/><edge vertexkey=\"20\"/>",
                );
            }
            "ambiguous-input" => {
                design = design.replace("<entry name=\"integer\" inpkey=\"24\" outkey=\"25\"/>", "<entry name=\"integer\" inpkey=\"24\" outkey=\"25\"/><entry name=\"integer\" inpkey=\"26\"/>").replace("<edge vertexkey=\"24\"/>", "<edge vertexkey=\"24\"/><edge vertexkey=\"26\"/>");
            }
            "no-presence" => {
                design = design.replace(
                    "<vertex vertexkey=\"101\"><edges><edge vertexkey=\"22\"/></edges></vertex>",
                    "",
                );
            }
            _ => unreachable!(),
        }
        let imported = import_control(&design, label, &variable, &target);
        assert!(
            imported.warnings.iter().any(|warning| warning
                .contains("iteration into `Records` comes from an unsupported feed")
                || warning.contains("cannot construct repeating target `Records`")),
            "{label}: {:?}",
            imported.warnings
        );
        assert!(
            !imported
                .project
                .root
                .children
                .iter()
                .any(|scope| scope.target_field == "Records"),
            "{label}"
        );
    }
}
