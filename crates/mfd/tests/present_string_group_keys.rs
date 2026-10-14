use std::path::PathBuf;
use ir::{ScalarType, Value};

#[path = "fixtures/present-string-group-keys/expected_project.rs"]
mod oracle;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/present-string-group-keys").join(name)
}

#[test]
fn present_group_keys_match_complete_projects_typed_values_order_and_xml() {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!("fixtures/present-string-group-keys/cases.json")).unwrap();
    for (design, ty, literal) in [
        ("integer.mfd", ScalarType::Int, None), ("boolean.mfd", ScalarType::Bool, None), ("string.mfd", ScalarType::String, None),
        ("literal-integer.mfd", ScalarType::Int, Some(Value::Int(-5))),
        ("literal-boolean.mfd", ScalarType::Bool, Some(Value::Bool(false))),
        ("literal-string.mfd", ScalarType::String, Some(Value::String("sprout".into()))),
    ] {
        let expected = oracle::expected(ty, literal);
        let imported = mfd::import(&fixture(design)).unwrap();
        assert!(imported.warnings.is_empty(), "{design}: {:?}", imported.warnings);
        assert_eq!(mapping::project_file::encode_pretty(&imported.project).unwrap(), mapping::project_file::encode_pretty(&expected).unwrap(), "{design}");
        assert!(engine::validate(&imported.project).is_empty());
        let executable = mfd::import_with_profile(&fixture(design), &mfd::ImportOptions::default(), mfd::ImportProfile::Executable).unwrap();
        assert!(executable.imported.warnings.is_empty());
        assert_eq!(mapping::project_file::encode_pretty(&executable.imported.project).unwrap(), mapping::project_file::encode_pretty(&expected).unwrap());
        for case in cases.iter().filter(|case| case["design"] == design || case["design"] == "all") {
            let result = format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source);
            if let Some(error) = case.get("error") {
                let error = match error["kind"].as_str().unwrap() {
                    "required" => matches!(result, Err(format_json::JsonFormatError::MissingRequiredProperty { object, property }) if object == error["object"].as_str().unwrap() && property == error["property"].as_str().unwrap()),
                    "shape" => matches!(result, Err(format_json::JsonFormatError::Shape { name, expected, got }) if name == error["name"].as_str().unwrap() && got == error["got"].as_str().unwrap() && expected == match ty { ScalarType::String => "string", ScalarType::Int => "integer", ScalarType::Bool => "bool", _ => unreachable!() }),
                    _ => panic!("authored error kind"),
                };
                assert!(error, "{design}: {case}");
                continue;
            }
            let output = engine::run(&imported.project, &result.unwrap()).unwrap();
            let expected_value = format_json::from_str(&serde_json::to_string(&case["expected"]).unwrap(), &expected.target).unwrap();
            assert_eq!(output, expected_value, "{design}: {case}");
            let actual: serde_json::Value = serde_json::from_str(&format_json::to_string(&expected.target, &output).unwrap()).unwrap();
            assert_eq!(actual, case["expected"], "{design}: {case}");
            for group in output.field("Group").unwrap().as_repeated().unwrap() {
                assert!(matches!(group.field("Key").unwrap().as_scalar(), Some(Value::String(_))));
            }
            let xml = format_xml::to_string_with_options(&expected.target, &output, &format_xml::XmlWriteOptions { declaration: false, indent: false, ..Default::default() }).unwrap();
            assert_eq!(xml.as_bytes(), case["expected_xml"].as_str().unwrap().as_bytes(), "{design}: {case}");
        }
    }
}

#[test]
fn legacy_string_identity_and_csv_compatibility_keep_complete_projects() {
    for (design, substring) in [("legacy-framed-string.mfd", false), ("legacy-substring-string.mfd", true)] {
        let expected = oracle::legacy_xml(substring);
        let imported = mfd::import(&fixture(design)).unwrap();
        assert!(imported.warnings.is_empty(), "{design}: {:?}", imported.warnings);
        assert_eq!(mapping::project_file::encode_pretty(&imported.project).unwrap(), mapping::project_file::encode_pretty(&expected).unwrap(), "{design}");
        let executable = mfd::import_with_profile(&fixture(design), &mfd::ImportOptions::default(), mfd::ImportProfile::Executable).unwrap();
        assert_eq!(mapping::project_file::encode_pretty(&executable.imported.project).unwrap(), mapping::project_file::encode_pretty(&expected).unwrap());
        let source = format_json::from_str(if substring { r#"{"Rows":[{"Key":"oak~1","Name":"elm"},{"Key":"ash~1","Name":"fir"},{"Key":"oak~2","Name":"yew"}]}"# } else { r#"{"Rows":[{"Key":"oak","Name":"elm"},{"Key":"ash","Name":"fir"},{"Key":"oak","Name":"yew"}]}"# }, &expected.source).unwrap();
        let output = engine::run(&imported.project, &source).unwrap();
        let expected_value = format_json::from_str(r#"{"Group":[{"Key":"oak","Member":[{"Name":"elm"},{"Name":"yew"}]},{"Key":"ash","Member":[{"Name":"fir"}]}]}"#, &expected.target).unwrap();
        assert_eq!(output, expected_value);
        assert_eq!(format_xml::to_string_with_options(&expected.target, &output, &format_xml::XmlWriteOptions { declaration: false, indent: false, ..Default::default() }).unwrap(), "<Summary><Group><Key>oak</Key><Member><Name>elm</Name></Member><Member><Name>yew</Name></Member></Group><Group><Key>ash</Key><Member><Name>fir</Name></Member></Group></Summary>");
    }
    let expected = oracle::legacy_csv();
    let imported = mfd::import(&fixture("legacy-csv-sparse-string.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(mapping::project_file::encode_pretty(&imported.project).unwrap(), mapping::project_file::encode_pretty(&expected).unwrap());
    let executable = mfd::import_with_profile(&fixture("legacy-csv-sparse-string.mfd"), &mfd::ImportOptions::default(), mfd::ImportProfile::Executable).unwrap();
    assert_eq!(mapping::project_file::encode_pretty(&executable.imported.project).unwrap(), mapping::project_file::encode_pretty(&expected).unwrap());
    // The compatibility oracle retains the engine's tagged Null. It makes no
    // assumption about the reference application's missing/empty adaptation.
    for value in [Value::String("sapling".into()), Value::Null] {
        let source = ir::Instance::Repeated(vec![ir::Instance::Group(vec![("Key".into(), ir::Instance::Scalar(value.clone()))].into())]);
        let output = engine::run(&imported.project, &source).unwrap();
        assert_eq!(output.field("Key").and_then(ir::Instance::as_scalar), Some(&value));
    }
}

#[test]
fn certified_legacy_filter_keeps_complete_string_grouping_project() {
    let imported = mfd::import(&fixture("legacy-filtered-string.mfd")).unwrap();
    let expected = oracle::legacy_filtered();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(mapping::project_file::encode_pretty(&imported.project).unwrap(), mapping::project_file::encode_pretty(&expected).unwrap());
    let source = format_json::from_str(r#"{"Rows":[{"Key":"oak","Name":"elm"},{"Key":"ash","Name":"fir"},{"Key":"oak","Name":"yew"}]}"#, &expected.source).unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let value = format_json::from_str(r#"{"Group":[{"Key":"oak","Member":[{"Name":"elm"},{"Name":"yew"}]}]}"#, &expected.target).unwrap();
    assert_eq!(output, value);
    assert_eq!(format_xml::to_string_with_options(&expected.target, &output, &format_xml::XmlWriteOptions { declaration: false, indent: false, ..Default::default() }).unwrap(), "<Summary><Group><Key>oak</Key><Member><Name>elm</Name></Member><Member><Name>yew</Name></Member></Group></Summary>");
    let executable = mfd::import_with_profile(&fixture("legacy-filtered-string.mfd"), &mfd::ImportOptions::default(), mfd::ImportProfile::Executable).unwrap();
    assert_eq!(mapping::project_file::encode_pretty(&executable.imported.project).unwrap(), mapping::project_file::encode_pretty(&expected).unwrap());
}

#[test]
fn positive_proof_refusals_preserve_complete_legacy_tagged_projects() {
    let design = std::fs::read_to_string(fixture("integer.mfd")).unwrap();
    let schema: serde_json::Value = serde_json::from_slice(&std::fs::read(fixture("integer.schema.json")).unwrap()).unwrap();
    for (index, ty, required) in [(0, ScalarType::Int, false), (1, ScalarType::Float, true)] {
        let directory = std::env::temp_dir().join(format!("ferrule_legacy_key_{}_{}", std::process::id(), index));
        std::fs::create_dir(&directory).unwrap();
        let mut schema = schema.clone();
        if required { schema["properties"]["Rows"]["items"]["properties"]["Key"]["type"] = serde_json::json!("number"); }
        else { schema["properties"]["Rows"]["items"]["required"] = serde_json::json!(["Name"]); }
        std::fs::write(directory.join("integer.schema.json"), serde_json::to_vec(&schema).unwrap()).unwrap();
        std::fs::write(directory.join("target.xsd"), std::fs::read(fixture("target.xsd")).unwrap()).unwrap();
        let path = directory.join("mapping.mfd");
        std::fs::write(&path, &design).unwrap();
        let imported = mfd::import(&path).unwrap();
        let expected = oracle::legacy_json(ty, required);
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert_eq!(mapping::project_file::encode_pretty(&imported.project).unwrap(), mapping::project_file::encode_pretty(&expected).unwrap());
        let executable = mfd::import_with_profile(&path, &mfd::ImportOptions::default(), mfd::ImportProfile::Executable).unwrap();
        assert_eq!(mapping::project_file::encode_pretty(&executable.imported.project).unwrap(), mapping::project_file::encode_pretty(&expected).unwrap());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
