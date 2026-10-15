use super::*;

#[test]
fn sparse_nested_child_compiled_complete_oracles() -> TestResult<()> {
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../mfd/tests/fixtures/sparse-nested-child/cases.json"
    ))?;
    let directory =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/sparse-nested-child");
    let imported = mfd::import(&directory.join("sparse.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    json_text_boundaries::run_generated_boundary_cases(
        &imported.project,
        &cases,
        "sparse_nested_child",
    )?;
    Ok(())
}
