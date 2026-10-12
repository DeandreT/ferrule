use super::*;

#[test]
fn json_structure_variable_compiled_complete_oracles() -> TestResult<()> {
    for (design, catalog, label) in [
        (
            "constant.mfd",
            include_str!(
                "../../../mfd/tests/fixtures/json-structure-variables/constant-cases.json"
            ),
            "json_variable_constant_record",
        ),
        (
            "mapping.mfd",
            include_str!("../../../mfd/tests/fixtures/json-structure-variables/cases.json"),
            "json_structure_variables",
        ),
        (
            "constructed.mfd",
            include_str!(
                "../../../mfd/tests/fixtures/json-structure-variables/constructed-cases.json"
            ),
            "json_variable_constructed_record",
        ),
    ] {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mfd/tests/fixtures/json-structure-variables")
            .join(design);
        let imported = mfd::import(&fixture)?;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        assert!(engine::validate(&imported.project).is_empty());
        assert!(imported.project.extra_sources.is_empty());
        assert!(imported.project.extra_targets.is_empty());
        let cases: Vec<serde_json::Value> = serde_json::from_str(catalog)?;
        json_text_boundaries::run_generated_boundary_cases(&imported.project, &cases, label)?;
    }
    Ok(())
}
