use std::path::PathBuf;

#[path = "fixtures/enclosing-frame-value-maps/expected_project.rs"]
mod oracle;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/enclosing-frame-value-maps")
        .join(name)
}

#[test]
fn total_string_value_maps_preserve_complete_project_values_and_xml() {
    let imported = mfd::import(&fixture("total-string.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let expected = oracle::expected_total_string_value_map_project();
    let ir::SchemaKind::Group { children, .. } = &imported.project.source.kind else {
        panic!("source group")
    };
    let rows = &children[0];
    assert!(!rows.container_nullable);
    assert_eq!(rows.json_object_item_nullable, None);
    assert!(!rows.json_single_container_nullable());
    assert_eq!(
        mapping::project_file::encode_pretty(&imported.project).unwrap(),
        mapping::project_file::encode_pretty(&expected).unwrap(),
    );
    assert!(engine::validate(&imported.project).is_empty());
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/enclosing-frame-value-maps/cases.json"
    ))
    .unwrap();
    for case in cases {
        let source =
            format_json::from_str(case["input"].as_str().unwrap(), &imported.project.source)
                .unwrap();
        let output = engine::run(&imported.project, &source).unwrap();
        let value: serde_json::Value = serde_json::from_str(
            &format_json::to_string(&imported.project.target, &output).unwrap(),
        )
        .unwrap();
        assert_eq!(value, case["expected"], "{case}");
        let xml = format_xml::to_string_with_options(
            &imported.project.target,
            &output,
            &format_xml::XmlWriteOptions {
                declaration: false,
                indent: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            xml.as_bytes(),
            case["expected_xml"].as_str().unwrap().as_bytes(),
            "{case}"
        );
    }
}

#[test]
fn total_string_value_map_input_domain_preserves_schema_refusals() {
    let imported = mfd::import(&fixture("total-string.mfd")).unwrap();
    for input in [
        r#"{"Rows":[{}]}"#,
        r#"{"Rows":[{"Code":null}]}"#,
        r#"{"Rows":[null]}"#,
        r#"{"Rows":null}"#,
        r#"{"Rows":[{"Code":7}]}"#,
        r#"{"Rows":[{"Code":"A","extra":1}]}"#,
        r#"{"Rows":[{"Code":"A"},{"Code":"A"},{"Code":"A"},{"Code":"A"},{"Code":"A"}]}"#,
    ] {
        assert!(
            format_json::from_str(input, &imported.project.source).is_err(),
            "{input}"
        );
    }
}

fn write_control(
    index: usize,
    design: &str,
    source_schema: &serde_json::Value,
    target_schema: &str,
) -> PathBuf {
    let directory = std::env::temp_dir().join(format!(
        "ferrule_total_string_map_contract_{}_{}",
        std::process::id(),
        index
    ));
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(
        directory.join("source.schema.json"),
        serde_json::to_vec_pretty(source_schema).unwrap(),
    )
    .unwrap();
    std::fs::write(directory.join("target.xsd"), target_schema).unwrap();
    std::fs::write(directory.join("mapping.mfd"), design).unwrap();
    directory
}

#[test]
fn total_string_value_map_empty_table_and_enabled_empty_default_preserve_full_project() {
    let original = std::fs::read_to_string(fixture("total-string.mfd")).unwrap();
    let schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("source.schema.json")).unwrap()).unwrap();
    let target = std::fs::read_to_string(fixture("target.xsd")).unwrap();
    let start = original.find("<valuemapTable>").unwrap() + "<valuemapTable>".len();
    let end = original.find("</valuemapTable>").unwrap();
    let design = format!("{}{}", &original[..start], &original[end..])
        .replace("defaultValue=\"other\"", "defaultValue=\"\"");
    let directory = write_control(900, &design, &schema, &target);
    let imported = mfd::import(&directory.join("mapping.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let mut expected = oracle::expected_total_string_value_map_project();
    expected.graph.nodes.insert(
        3,
        mapping::Node::ValueMap {
            input: 4,
            input_type: Some(ir::ScalarType::String),
            table: Vec::new(),
            default: Some(ir::Value::String(String::new())),
        },
    );
    assert_eq!(
        mapping::project_file::encode_pretty(&imported.project).unwrap(),
        mapping::project_file::encode_pretty(&expected).unwrap()
    );
    let source = format_json::from_str(
        r#"{"Rows":[{"Code":"anything"}]}"#,
        &imported.project.source,
    )
    .unwrap();
    let output = engine::run(&imported.project, &source).unwrap();
    let value: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&imported.project.target, &output).unwrap())
            .unwrap();
    assert_eq!(
        value,
        serde_json::json!({"Batch":[{"Record":[{"Type":"literal","Note":"anchor"},{"Type":"mapped","Note":""}]}]})
    );
    let xml = format_xml::to_string_with_options(
        &imported.project.target,
        &output,
        &format_xml::XmlWriteOptions {
            declaration: false,
            indent: false,
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(
        xml,
        "<Result><Batch><Record><Type>literal</Type><Note>anchor</Note></Record><Record><Type>mapped</Type><Note></Note></Record></Batch></Result>"
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn total_string_value_map_unproved_raw_owners_defaults_and_frames_remain_refusals() {
    let original = std::fs::read_to_string(fixture("total-string.mfd")).unwrap();
    let schema: serde_json::Value =
        serde_json::from_slice(&std::fs::read(fixture("source.schema.json")).unwrap()).unwrap();
    let target = std::fs::read_to_string(fixture("target.xsd")).unwrap();
    let shadow_input = r#"<component name="constant" library="core" kind="2"><targets><datapoint key="11"/></targets><data><constant value="shadow" datatype="string"/></data></component>"#;
    let shadow_map = r#"<component name="constant" library="core" kind="2"><targets><datapoint key="104"/></targets><data><constant value="shadow" datatype="string"/></data></component>"#;
    let skipped_input = r#"<component name="unknown" library="unlowered" kind="999"><targets><datapoint key="11"/></targets></component>"#;
    let skipped_map = r#"<component name="unknown" library="unlowered" kind="999"><targets><datapoint key="104"/></targets></component>"#;
    let skipped_schema = r#"<component name="unknown" library="unlowered" kind="999"><data><root><entry name="Unused" outkey="11"/></root></data></component>"#;
    let designs = [
        (
            "disabled default",
            original.replace("defaultValueMode=\"custom\"", "defaultValueMode=\"none\""),
        ),
        (
            "missing default",
            original.replace(" defaultValue=\"other\"", ""),
        ),
        (
            "nonstrict raw input type",
            original.replace(
                "name=\"input\" type=\"string\"",
                "name=\"input\" type=\"unknown\"",
            ),
        ),
        (
            "nonstring result",
            original.replace(
                "name=\"result\" type=\"string\"",
                "name=\"result\" type=\"integer\"",
            ),
        ),
        (
            "missing table key",
            original.replace("from=\"A\" to=\"alpha\"", "to=\"alpha\""),
        ),
        (
            "hidden table subtree",
            original.replace(
                "<entry from=\"A\" to=\"alpha\"/>",
                "<hidden><entry from=\"A\" to=\"alpha\"/></hidden>",
            ),
        ),
        (
            "raw pin alias",
            original.replace(
                "pos=\"0\" key=\"111\"",
                "pos=\"0\" key=\"111\" alias=\"foreign\"",
            ),
        ),
        (
            "raw keyless input",
            original.replace("pos=\"0\" key=\"111\"", "pos=\"0\""),
        ),
        (
            "raw malformed input position",
            original.replace("pos=\"0\" key=\"111\"", "pos=\"bad\" key=\"111\""),
        ),
        (
            "raw hidden output",
            original.replace(
                "<datapoint key=\"104\"/>",
                "<datapoint pos=\"0\" key=\"999\"/><datapoint pos=\"0\" key=\"104\"/>",
            ),
        ),
        (
            "competing physical producer",
            original.replace("</children>", &format!("{shadow_input}</children>")),
        ),
        (
            "competing map producer",
            original.replace("</children>", &format!("{shadow_map}</children>")),
        ),
        (
            "skipped unknown input producer",
            original.replace("</children>", &format!("{skipped_input}</children>")),
        ),
        (
            "skipped unknown map producer",
            original.replace("</children>", &format!("{skipped_map}</children>")),
        ),
        (
            "skipped unknown schema producer",
            original.replace("</children>", &format!("{skipped_schema}</children>")),
        ),
        (
            "unused schema root producer",
            original.replace(
                "</root><json",
                "</root><root><entry name=\"Unused\" outkey=\"11\"/></root><json",
            ),
        ),
        (
            "computed input",
            original.replace(
                "<edge from=\"11\" to=\"111\"/>",
                "<edge from=\"101\" to=\"111\"/>",
            ),
        ),
        (
            "cyclic map input",
            original.replace(
                "<edge from=\"11\" to=\"111\"/>",
                "<edge from=\"104\" to=\"111\"/>",
            ),
        ),
        (
            "uniterated parent",
            original.replace("<edge from=\"10\" to=\"40\"/>", ""),
        ),
    ];
    for (index, (label, design)) in designs.into_iter().enumerate() {
        let directory = write_control(index, &design, &schema, &target);
        let mapping = directory.join("mapping.mfd");
        let imported = mfd::import(&mapping).unwrap();
        assert!(imported.warnings.iter().any(|warning| warning == "declared target clones in `Batch/Record` cannot prove complete enclosing-frame scalar occurrences; lowering skipped"), "{label}: {:?}", imported.warnings);
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
        std::fs::remove_dir_all(directory).unwrap();
    }
}
