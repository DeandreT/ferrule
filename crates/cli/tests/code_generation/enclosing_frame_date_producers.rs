use super::*;

#[path = "../../../mfd/tests/fixtures/enclosing-frame-date-producers/expected_project.rs"]
mod oracle;

#[test]
fn enclosing_frame_date_producers_compiled_complete_values_typed_errors_and_xml() -> TestResult<()> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/enclosing-frame-date-producers");
    for (design, picture, cases, label) in [
        ("literal-picture.mfd", "[Y]-[M]-[D]", include_str!("../../../mfd/tests/fixtures/enclosing-frame-date-producers/cases.json"), "enclosing_parse_date"),
        ("malformed-picture.mfd", "[]-x", include_str!("../../../mfd/tests/fixtures/enclosing-frame-date-producers/picture-error-cases.json"), "enclosing_parse_date_bad_picture"),
        ("blank-picture.mfd", "", include_str!("../../../mfd/tests/fixtures/enclosing-frame-date-producers/picture-error-cases.json"), "enclosing_parse_date_blank_picture"),
    ] {
        let imported = mfd::import(&directory.join(design))?;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        let expected = oracle::expected(picture);
        assert_eq!(mapping::project_file::encode_pretty(&imported.project)?, mapping::project_file::encode_pretty(&expected)?);
        assert!(engine::validate(&imported.project).is_empty());
        let mut cases: Vec<serde_json::Value> = serde_json::from_str(cases)?;
        let source_schema = codegen::serialize_embedded_schema(&expected.source, codegen::MAX_EMBEDDED_JSON_SCHEMA_BYTES)?;
        let target_schema = codegen::serialize_embedded_schema(&expected.target, codegen::MAX_EMBEDDED_XML_SCHEMA_BYTES)?;
        for case in &mut cases {
            if case.get("expected_xml").is_some() {
                case["source_schema"] = serde_json::Value::String(source_schema.clone());
                case["target_schema"] = serde_json::Value::String(target_schema.clone());
            }
        }
        json_text_boundaries::run_generated_boundary_cases_with_harnesses(
            &imported.project, &cases, label,
            include_str!("fixtures/enclosing_date_producers_rust.rs.txt"),
            include_str!("fixtures/enclosing_date_producers_csharp.cs.txt"),
        )?;
    }
    Ok(())
}
