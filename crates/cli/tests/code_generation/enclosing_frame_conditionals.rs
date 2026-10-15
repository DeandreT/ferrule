use super::*;

#[test]
fn enclosing_frame_constant_conditionals_compiled_complete_oracles() -> TestResult<()> {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../mfd/tests/fixtures/enclosing-frame-conditionals/cases.json"
    ))?;
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/enclosing-frame-conditionals");
    let imported = mfd::import(&directory.join("typed-predicate.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    json_text_boundaries::run_generated_boundary_cases(
        &imported.project,
        &cases,
        "enclosing_conditional_constants",
    )?;
    Ok(())
}
