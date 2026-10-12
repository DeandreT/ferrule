use super::*;

#[test]
fn edi_structure_variable_compiled_complete_oracles() -> TestResult<()> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/edi-structure-variables/mapping.mfd");
    let imported = mfd::import(&fixture)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert!(imported.project.extra_sources.is_empty());
    assert!(imported.project.extra_targets.is_empty());
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../mfd/tests/fixtures/edi-structure-variables/cases.json"
    ))?;
    json_text_boundaries::run_generated_boundary_cases(
        &imported.project,
        &cases,
        "edi_structure_variables",
    )
}
