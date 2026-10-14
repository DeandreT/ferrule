use std::path::PathBuf;

#[path = "fixtures/enclosing-frame-date-producers/expected_project.rs"]
mod oracle;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/enclosing-frame-date-producers").join(name)
}

#[test]
fn parse_date_occurrences_preserve_complete_project_values_errors_and_xml() {
    for (design, picture, cases) in [
        ("literal-picture.mfd", "[Y]-[M]-[D]", include_str!("fixtures/enclosing-frame-date-producers/cases.json")),
        ("malformed-picture.mfd", "[]-x", include_str!("fixtures/enclosing-frame-date-producers/picture-error-cases.json")),
        ("blank-picture.mfd", "", include_str!("fixtures/enclosing-frame-date-producers/picture-error-cases.json")),
    ] {
        let imported = mfd::import(&fixture(design)).unwrap();
        assert!(imported.warnings.is_empty(), "{design}: {:?}", imported.warnings);
        assert_eq!(mapping::project_file::encode_pretty(&imported.project).unwrap(), mapping::project_file::encode_pretty(&oracle::expected(picture)).unwrap());
        assert!(engine::validate(&imported.project).is_empty());
        let executable = mfd::import_with_profile(&fixture(design), &mfd::ImportOptions::default(), mfd::ImportProfile::Executable).unwrap();
        assert!(executable.imported.warnings.is_empty());
        assert_eq!(mapping::project_file::encode_pretty(&executable.imported.project).unwrap(), mapping::project_file::encode_pretty(&oracle::expected(picture)).unwrap());
        let ir::SchemaKind::Group { children, .. } = &imported.project.source.kind else { panic!("source group") };
        let rows = &children[0];
        assert_eq!(rows.json_object_item_nullable, None);
        assert!(!rows.nullable && !rows.container_nullable && !rows.json_single_container_nullable());
        let cases: Vec<serde_json::Value> = serde_json::from_str(cases).unwrap();
        for case in cases {
            let input = case["input"].as_str().unwrap();
            if case["reject"] == true {
                assert!(format_json::from_str(input, &imported.project.source).is_err(), "{case}");
                continue;
            }
            let source = format_json::from_str(input, &imported.project.source).unwrap();
            let result = engine::run(&imported.project, &source);
            if let Some(expected) = case.get("execution_error") {
                assert!(matches!(result, Err(engine::EngineError::Function(functions::FunctionError::InvalidArgument { function, message }))
                    if function == expected["function"].as_str().unwrap() && message == expected["message"].as_str().unwrap()), "{case}");
                continue;
            }
            let output = result.unwrap();
            let actual: serde_json::Value = serde_json::from_str(&format_json::to_string(&imported.project.target, &output).unwrap()).unwrap();
            assert_eq!(actual, case["expected"], "{case}");
            let xml = format_xml::to_string_with_options(&imported.project.target, &output, &format_xml::XmlWriteOptions { declaration: false, indent: false, ..Default::default() }).unwrap();
            assert_eq!(xml.as_bytes(), case["expected_xml"].as_str().unwrap().as_bytes(), "{case}");
        }
    }
}

#[test]
fn parse_date_unproved_pins_owners_operands_sources_and_targets_remain_refusals() {
    let original = std::fs::read_to_string(fixture("literal-picture.mfd")).unwrap();
    let source: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture("source.schema.json")).unwrap()).unwrap();
    let target = std::fs::read_to_string(fixture("target.xsd")).unwrap();
    let mut controls = Vec::new();
    for (label, from, to) in [
        ("missing source edge", "<edge from=\"11\" to=\"111\"/>", ""),
        ("missing picture edge", "<edge from=\"101\" to=\"112\"/>", ""),
        ("input gap", "pos=\"1\" key=\"112\"", "pos=\"2\" key=\"112\""),
        ("duplicate position", "pos=\"1\" key=\"112\"", "pos=\"0\" key=\"112\""),
        ("duplicate key", "pos=\"1\" key=\"112\"", "pos=\"1\" key=\"111\""),
        ("keyless pin", "pos=\"1\" key=\"112\"", "pos=\"1\""),
        ("output as input", "pos=\"1\" key=\"112\"", "pos=\"1\" key=\"102\""),
        ("extra source pin", "<datapoint pos=\"1\" key=\"112\"/>", "<datapoint pos=\"1\" key=\"112\"/><datapoint pos=\"2\" key=\"119\"/>"),
        ("duplicate source container", "<sources><datapoint pos=\"0\" key=\"111\"/>", "<sources/><sources><datapoint pos=\"0\" key=\"111\"/>"),
        ("hidden pin", "<datapoint pos=\"1\" key=\"112\"/>", "<datapoint pos=\"1\" key=\"112\"><hidden/></datapoint>"),
        ("wrong function library", "name=\"parse-date\" library=\"core\"", "name=\"parse-date\" library=\"xpath2\""),
        ("wrong raw kind", "name=\"parse-date\" library=\"core\" kind=\"5\"", "name=\"parse-date\" library=\"core\" kind=\"4\""),
        ("function alias", "name=\"parse-date\"", "name=\"parse_date\""),
        ("computed source", "<edge from=\"11\" to=\"111\"/>", "<edge from=\"101\" to=\"111\"/>"),
        ("physical picture", "<edge from=\"101\" to=\"112\"/>", "<edge from=\"12\" to=\"112\"/>"),
        ("nonordinary JSON lines", "<json schema=", "<json jsonlines=\"1\" schema="),
        ("source node rule", "<entry name=\"string\" outkey=\"11\"/>", "<entry name=\"string\" outkey=\"11\"><nodefunction name=\"unproved\"/></entry>"),
        ("missing enclosing driver", "<edge from=\"10\" to=\"40\"/>", ""),
    ] {
        assert!(original.contains(from), "{label}");
        controls.push((label.to_string(), original.replace(from, to), source.clone(), target.clone()));
    }
    for key in [10, 11, 12, 101, 102, 103] {
        for library in ["core", "unlowered"] {
            let shadow = format!(r#"<component name="unproved" library="{library}" kind="999"><targets><datapoint key="{key}"/></targets></component>"#);
            controls.push((format!("competing {library} producer {key}"), original.replace("</children>", &format!("{shadow}</children>")), source.clone(), target.clone()));
        }
    }
    for mode in 0..6 {
        let mut schema = source.clone();
        match mode {
            0 => schema["properties"]["Rows"]["items"]["required"] = serde_json::json!(["RawA"]),
            1 => schema["properties"]["Rows"]["items"]["properties"]["RawB"]["type"] = serde_json::json!(["string", "null"]),
            2 => schema["properties"]["Rows"]["items"]["type"] = serde_json::json!(["object", "null"]),
            3 => schema["properties"]["Rows"]["type"] = serde_json::json!(["array", "null"]),
            4 => schema["properties"]["Rows"]["items"]["properties"]["RawB"]["type"] = serde_json::json!(["string", "integer"]),
            _ => schema["properties"]["Rows"]["items"]["additionalProperties"] = serde_json::json!(true),
        }
        controls.push((format!("unproved schema domain {mode}"), original.clone(), schema, target.clone()));
    }
    for (label, replacement) in [("integer target", "type=\"xs:integer\""), ("repeating scalar target", "type=\"xs:string\" maxOccurs=\"unbounded\"")] {
        controls.push((label.into(), original.clone(), source.clone(), target.replace("type=\"xs:string\"", replacement)));
    }
    for (index, (label, design, source, target)) in controls.into_iter().enumerate() {
        let directory = std::env::temp_dir().join(format!("ferrule_parse_date_refusal_{}_{}", std::process::id(), index));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(directory.join("source.schema.json"), serde_json::to_vec_pretty(&source).unwrap()).unwrap();
        std::fs::write(directory.join("target.xsd"), target).unwrap();
        let mapping = directory.join("mapping.mfd");
        std::fs::write(&mapping, design).unwrap();
        let imported = mfd::import(&mapping).unwrap();
        assert!(imported.warnings.iter().any(|warning| warning == "declared target clones in `Batch/Record` cannot prove complete enclosing-frame scalar occurrences; lowering skipped"), "{label}: {:?}", imported.warnings);
        assert!(matches!(mfd::import_with_profile(&mapping, &mfd::ImportOptions::default(), mfd::ImportProfile::Executable), Err(mfd::MfdError::IncompatibleImport(_))), "{label}");
        std::fs::remove_dir_all(directory).unwrap();
    }
}
