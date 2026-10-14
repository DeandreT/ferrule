use std::collections::BTreeMap;
use std::path::PathBuf;

use ir::{ItemCountRange, ScalarType, SchemaNode, Value};
use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration, ScopeSequence};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/enclosing-frame-conditionals")
        .join(name)
}

fn expected() -> Project {
    let scalar = SchemaNode::scalar;
    let rows = SchemaNode::group(
        "Rows",
        vec![
            scalar("Count", ScalarType::Int),
            scalar("Note", ScalarType::String),
            scalar("Choose", ScalarType::Bool),
        ],
    )
    .repeating()
    .with_item_count_range(ItemCountRange::new(0, Some(4)).unwrap())
    .unwrap();
    let optional = |name, ty| {
        let mut node = scalar(name, ty);
        node.xml_optional = true;
        node
    };
    let target = SchemaNode::group(
        "Result",
        vec![
            SchemaNode::group(
                "Batch",
                vec![
                    SchemaNode::group(
                        "Record",
                        vec![
                            optional("Type", ScalarType::String),
                            optional("Count", ScalarType::Int),
                            optional("Note", ScalarType::String),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    let mut nodes = BTreeMap::from([
        (
            0,
            Node::Const {
                value: Value::String("left-true".into()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("right-true".into()),
            },
        ),
        (
            2,
            Node::Const {
                value: Value::String(String::new()),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::String("right-false".into()),
            },
        ),
        (
            4,
            Node::If {
                condition: 5,
                then: 0,
                else_: 2,
            },
        ),
        (
            5,
            Node::SourceField {
                path: vec!["Choose".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
        (
            6,
            Node::If {
                condition: 5,
                then: 1,
                else_: 3,
            },
        ),
    ]);
    for (node, name) in [(7, "Count"), (8, "Note")] {
        nodes.insert(
            node,
            Node::SourceField {
                path: vec![name.into()],
                frame: Some(vec!["Rows".into()]),
            },
        );
    }
    let branch = |conditional| Scope {
        bindings: vec![
            Binding {
                target_field: "Type".into(),
                node: conditional,
            },
            Binding {
                target_field: "Count".into(),
                node: 7,
            },
            Binding {
                target_field: "Note".into(),
                node: 8,
            },
        ],
        ..Default::default()
    };
    Project {
        source: SchemaNode::group(
            "Source",
            vec![rows, scalar("GlobalChoose", ScalarType::Bool)],
        ),
        target,
        source_path: Some("source.json".into()),
        target_path: Some("report.xml".into()),
        source_options: FormatOptions {
            json_document: true,
            ..Default::default()
        },
        target_options: FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        graph: Graph { nodes },
        root: Scope {
            children: vec![Scope {
                target_field: "Batch".into(),
                iteration: ScopeIteration::Source(vec!["Rows".into()]),
                children: vec![Scope {
                    target_field: "Record".into(),
                    iteration: ScopeIteration::Concatenate(ScopeSequence::new(
                        branch(4),
                        vec![branch(6)],
                    )),
                    ..Default::default()
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
    }
}

#[test]
fn same_frame_constant_conditionals_preserve_complete_project_values_and_errors() {
    let imported = mfd::import(&fixture("typed-predicate.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        mapping::project_file::encode_pretty(&imported.project).unwrap(),
        mapping::project_file::encode_pretty(&expected()).unwrap(),
    );
    assert!(engine::validate(&imported.project).is_empty());
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/enclosing-frame-conditionals/cases.json"
    ))
    .unwrap();
    for case in cases {
        let source =
            format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source);
        if case["reject"] == true {
            assert!(source.is_err(), "{case}");
        } else if let Some(error) = case.get("boolean_error") {
            assert!(
                matches!(
                    engine::run(&imported.project, &source.unwrap()),
                    Err(engine::EngineError::NotABool { node, found })
                        if u64::from(node) == error["node"].as_u64().unwrap()
                            && found == error["found"].as_str().unwrap()
                ),
                "{case}"
            );
        } else {
            let actual = engine::run(&imported.project, &source.unwrap()).unwrap();
            let actual: serde_json::Value = serde_json::from_str(
                &format_json::to_string(&imported.project.target, &actual).unwrap(),
            )
            .unwrap();
            assert_eq!(actual, case["expected"], "{case}");
        }
    }
}

#[test]
fn unproved_conditional_operands_frames_and_assignments_remain_refusals() {
    let original = std::fs::read_to_string(fixture("typed-predicate.mfd")).unwrap();
    let duplicate_predicate = r#"<component name="constant" library="core" kind="2"><targets><datapoint key="15"/></targets><data><constant value="true" datatype="boolean"/></data></component>"#;
    let duplicate_arm = r#"<component name="constant" library="core" kind="2"><targets><datapoint key="103"/></targets><data><constant value="other" datatype="string"/></data></component>"#;
    let controls = [
        (
            "foreign predicate frame",
            original.replace(
                "<edge from=\"15\" to=\"111\"/>",
                "<edge from=\"19\" to=\"111\"/>",
            ),
        ),
        (
            "missing conditional arm",
            original.replace("<edge from=\"103\" to=\"113\"/>", ""),
        ),
        (
            "nonnull arm type mismatch",
            original.replace(
                "value=\"\" datatype=\"string\"",
                "value=\"0\" datatype=\"integer\"",
            ),
        ),
        (
            "unsupported nullable arm",
            original.replace(
                "value=\"\" datatype=\"string\"",
                "value=\"null\" datatype=\"null\"",
            ),
        ),
        (
            "competing predicate producers",
            original.replace("</children>", &format!("{duplicate_predicate}</children>")),
        ),
        (
            "competing arm producers",
            original.replace("</children>", &format!("{duplicate_arm}</children>")),
        ),
        (
            "malformed conditional raw pins",
            original.replacen("pos=\"2\" key=\"113\"", "pos=\"1\" key=\"113\"", 1),
        ),
        (
            "hidden conditional output",
            original.replace(
                "<datapoint key=\"105\"/>",
                "<datapoint pos=\"0\" key=\"999\"/><datapoint pos=\"0\" key=\"105\"/>",
            ),
        ),
        (
            "constant predicate is not a physical frame operand",
            original.replace(
                "<edge from=\"15\" to=\"111\"/>",
                "<edge from=\"103\" to=\"111\"/>",
            ),
        ),
        (
            "uniterated parent",
            original.replace("<edge from=\"10\" to=\"40\"/>", ""),
        ),
        (
            "nonboolean predicate",
            original.replace(
                "<entry name=\"boolean\" outkey=\"15\"/>",
                "<entry name=\"string\" outkey=\"15\"/>",
            ),
        ),
        ("nullable predicate", original.clone()),
    ];
    for (index, (label, design)) in controls.into_iter().enumerate() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_enclosing_conditional_refusal_{}_{}",
            std::process::id(),
            index
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::copy(fixture("target.xsd"), path.join("target.xsd")).unwrap();
        let mut source: serde_json::Value =
            serde_json::from_slice(&std::fs::read(fixture("source.schema.json")).unwrap()).unwrap();
        if label == "nonboolean predicate" {
            source["properties"]["Rows"]["items"]["properties"]["Choose"]["type"] =
                serde_json::json!("string");
        } else if label == "nullable predicate" {
            source["properties"]["Rows"]["items"]["properties"]["Choose"]["type"] =
                serde_json::json!(["boolean", "null"]);
        }
        std::fs::write(
            path.join("source.schema.json"),
            serde_json::to_vec_pretty(&source).unwrap(),
        )
        .unwrap();
        let mapping = path.join("mapping.mfd");
        std::fs::write(&mapping, design).unwrap();
        let imported = mfd::import(&mapping).unwrap();
        assert!(imported.warnings.iter().any(|warning| warning ==
            "declared target clones in `Batch/Record` cannot prove complete enclosing-frame scalar occurrences; lowering skipped"
        ), "{label}: {:?}", imported.warnings);
        assert!(
            matches!(
                mfd::import_with_profile(
                    &mapping,
                    &mfd::ImportOptions::default(),
                    mfd::ImportProfile::Executable
                ),
                Err(mfd::MfdError::IncompatibleImport(_))
            ),
            "{label}"
        );
        std::fs::remove_dir_all(path).unwrap();
    }
}

#[test]
fn mixed_nullable_object_items_do_not_prove_enclosing_clone_frames() {
    let directory = std::env::temp_dir().join(format!(
        "ferrule_enclosing_conditional_nullable_items_{}",
        std::process::id(),
    ));
    std::fs::create_dir(&directory).unwrap();
    std::fs::copy(fixture("target.xsd"), directory.join("target.xsd")).unwrap();
    let mut schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("source.schema.json")).unwrap()).unwrap();
    schema["properties"]["Rows"]["items"]["type"] = serde_json::json!(["object", "null"]);
    std::fs::write(
        directory.join("source.schema.json"),
        serde_json::to_vec_pretty(&schema).unwrap(),
    )
    .unwrap();
    let mapping = directory.join("mapping.mfd");
    std::fs::copy(fixture("typed-predicate.mfd"), &mapping).unwrap();
    let imported = mfd::import(&mapping).unwrap();
    let mut expected_source = expected().source;
    let ir::SchemaKind::Group { children, .. } = &mut expected_source.kind else {
        panic!("source group");
    };
    let rows = children
        .iter_mut()
        .find(|child| child.name == "Rows")
        .unwrap();
    rows.json_object_item_nullable = Some(true);
    assert!(!rows.container_nullable);
    assert!(rows.json_single_container_nullable());
    assert_eq!(
        serde_json::to_value(&imported.project.source).unwrap(),
        serde_json::to_value(&expected_source).unwrap()
    );
    let expected_input = serde_json::json!({"Rows":[null,{"Count":0,"Note":"proof","Choose":true},null],"GlobalChoose":false});
    let parsed =
        format_json::from_str(&expected_input.to_string(), &imported.project.source).unwrap();
    let roundtrip: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&imported.project.source, &parsed).unwrap())
            .unwrap();
    assert_eq!(roundtrip, expected_input);
    assert!(imported.warnings.iter().any(|warning| warning ==
        "declared target clones in `Batch/Record` cannot prove complete enclosing-frame scalar occurrences; lowering skipped"
    ), "{:?}", imported.warnings);
    assert!(matches!(
        mfd::import_with_profile(
            &mapping,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        ),
        Err(mfd::MfdError::IncompatibleImport(_))
    ));
    std::fs::remove_dir_all(directory).unwrap();
}
