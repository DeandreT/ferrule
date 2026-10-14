use std::path::PathBuf;

#[path = "fixtures/enclosing-frame-string-branches/expected_project.rs"]
mod oracle;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/enclosing-frame-string-branches")
        .join(name)
}

#[test]
fn strict_string_branches_preserve_complete_projects_values_xml_and_bool_errors() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/enclosing-frame-string-branches/cases.json"
    ))
    .unwrap();
    for design in ["physical-branches", "mixed-branches"] {
        let path = fixture(&format!("{design}.mfd"));
        let imported = mfd::import(&path).unwrap();
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        let expected = oracle::expected(design == "mixed-branches");
        assert_eq!(
            mapping::project_file::encode_pretty(&imported.project).unwrap(),
            mapping::project_file::encode_pretty(&expected).unwrap(),
        );
        let executable = mfd::import_with_profile(
            &path,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )
        .unwrap();
        assert_eq!(
            mapping::project_file::encode_pretty(&executable.imported.project).unwrap(),
            mapping::project_file::encode_pretty(&expected).unwrap(),
        );
        assert!(engine::validate(&imported.project).is_empty());
        let ir::SchemaKind::Group { children, .. } = &imported.project.source.kind else {
            panic!("source group")
        };
        let rows = &children[0];
        assert!(!rows.nullable && !rows.container_nullable);
        assert_eq!(rows.json_object_item_nullable, None);
        assert!(!rows.json_single_container_nullable());
        assert_eq!(rows.required_fields(), &["Then".to_string(), "Else".to_string()]);
        for case in cases.iter().filter(|case| case["design"] == design) {
            let source = format_json::from_str(
                case["input"].as_str().unwrap(),
                &imported.project.source,
            );
            if case["reject"] == true {
                assert!(source.is_err(), "{case}");
                continue;
            }
            let result = engine::run(&imported.project, &source.unwrap());
            if let Some(error) = case.get("boolean_error") {
                assert!(matches!(
                    result,
                    Err(engine::EngineError::NotABool { node, found })
                        if u64::from(node) == error["node"].as_u64().unwrap()
                            && found == error["found"].as_str().unwrap()
                ), "{case}");
                continue;
            }
            let output = result.unwrap();
            let actual: serde_json::Value = serde_json::from_str(
                &format_json::to_string(&imported.project.target, &output).unwrap(),
            )
            .unwrap();
            assert_eq!(actual, case["expected"], "{case}");
            if let Some(xml) = case.get("expected_xml") {
                let actual = format_xml::to_string_with_options(
                    &imported.project.target,
                    &output,
                    &format_xml::XmlWriteOptions {
                        declaration: false,
                        indent: false,
                        ..Default::default()
                    },
                )
                .unwrap();
                assert_eq!(actual.as_bytes(), xml.as_str().unwrap().as_bytes(), "{case}");
            }
        }
    }
}

#[test]
fn unproved_string_branch_owners_schema_options_and_types_remain_refusals() {
    let original = std::fs::read_to_string(fixture("physical-branches.mfd")).unwrap();
    let schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("source.schema.json")).unwrap()).unwrap();
    let competing = r#"<component name="unknown" library="unlowered" kind="999"><targets><datapoint key="12"/></targets></component>"#;
    let controls = [
        ("optional branch", original.clone()),
        ("nullable branch", original.clone()),
        ("nonstring branch", original.clone()),
        ("nullable item", original.clone()),
        ("nullable container", original.clone()),
        ("optional branch ancestor", original.replace(
            "<entry name=\"Then\" type=\"json-property\"><entry name=\"string\" outkey=\"12\"/></entry>",
            "<entry name=\"Container\" type=\"json-property\"><entry name=\"object\"><entry name=\"Then\" type=\"json-property\"><entry name=\"string\" outkey=\"12\"/></entry></entry></entry>",
        )),
        ("nonstring target", original.clone()),
        ("foreign predicate", original.replace("<edge from=\"11\" to=\"111\"/>", "<edge from=\"19\" to=\"111\"/>")),
        ("nonstring feed", original.replace("<edge from=\"12\" to=\"112\"/>", "<edge from=\"11\" to=\"112\"/>")),
        ("recursive branch", original.replace("<edge from=\"13\" to=\"123\"/>", "<edge from=\"101\" to=\"123\"/>")),
        ("missing branch", original.replace("<edge from=\"13\" to=\"123\"/>", "")),
        ("malformed pins", original.replacen("pos=\"2\" key=\"123\"", "pos=\"1\" key=\"123\"", 1)),
        ("competing producer", original.replace("</children>", &format!("{competing}</children>"))),
        ("competing frame driver", original.replace("</children>", &format!("{}</children>", competing.replace("key=\"12\"", "key=\"10\"")))),
        ("competing predicate", original.replace("</children>", &format!("{}</children>", competing.replace("key=\"12\"", "key=\"11\"")))),
        ("competing conditional", original.replace("</children>", &format!("{}</children>", competing.replace("key=\"12\"", "key=\"101\"")))),
        ("hidden schema branch producer", original.replacen(
            "</root><json schema=", "</root><root><entry name=\"Unused\" outkey=\"12\"/></root><json schema=", 1,
        )),
        ("wrong library", original.replacen("name=\"if-else\" library=\"core\"", "name=\"if-else\" library=\"other\"", 1)),
        ("json lines", original.replace("<json schema=", "<json jsonlines=\"1\" schema=")),
        ("unresolved schema", original.replace("schema=\"source.schema.json\"", "schema=\"missing.schema.json\"")),
        ("absent schema", original.replace(" schema=\"source.schema.json\"", "")),
        ("uniterated parent", original.replace("<edge from=\"10\" to=\"40\"/>", "")),
    ];
    for (index, (label, design)) in controls.into_iter().enumerate() {
        let directory = std::env::temp_dir().join(format!(
            "ferrule_string_branch_refusal_{}_{}", std::process::id(), index
        ));
        std::fs::create_dir(&directory).unwrap();
        let target = std::fs::read_to_string(fixture("target.xsd")).unwrap();
        std::fs::write(directory.join("target.xsd"), if label == "nonstring target" {
            target.replace("type=\"xs:string\"", "type=\"xs:integer\"")
        } else {
            target
        }).unwrap();
        let mut source = schema.clone();
        let item = &mut source["properties"]["Rows"]["items"];
        match label {
            "optional branch" => item["required"] = serde_json::json!(["Else"]),
            "nullable branch" => item["properties"]["Then"]["type"] = serde_json::json!(["string", "null"]),
            "nonstring branch" => item["properties"]["Then"]["type"] = serde_json::json!("integer"),
            "nullable item" => item["type"] = serde_json::json!(["object", "null"]),
            "nullable container" => source["properties"]["Rows"]["type"] = serde_json::json!(["array", "null"]),
            "optional branch ancestor" => {
                item["properties"].as_object_mut().unwrap().remove("Then");
                item["required"] = serde_json::json!(["Else"]);
                item["properties"]["Container"] = serde_json::json!({
                    "type": "object", "required": ["Then"],
                    "properties": {"Then": {"type": "string"}},
                });
            }
            _ => {}
        }
        std::fs::write(
            directory.join("source.schema.json"),
            serde_json::to_vec_pretty(&source).unwrap(),
        )
        .unwrap();
        let mapping = directory.join("mapping.mfd");
        std::fs::write(&mapping, design).unwrap();
        let imported = mfd::import(&mapping).unwrap();
        assert!(imported.warnings.iter().any(|warning| warning ==
            "declared target clones in `Batch/Record` cannot prove complete enclosing-frame scalar occurrences; lowering skipped"
        ), "{label}: {:?}", imported.warnings);
        assert!(matches!(
            mfd::import_with_profile(
                &mapping,
                &mfd::ImportOptions::default(),
                mfd::ImportProfile::Executable,
            ),
            Err(mfd::MfdError::IncompatibleImport(_))
        ), "{label}");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
