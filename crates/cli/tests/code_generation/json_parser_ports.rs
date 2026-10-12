use super::*;

#[test]
fn json_parser_unused_ports_compiled_complete_oracles() -> TestResult<()> {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/json-parser-unused-ports/mapping.mfd");
    let imported = mfd::import(&fixture)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../mfd/tests/fixtures/json-parser-unused-ports/cases.json"
    ))?;
    json_text_boundaries::run_generated_boundary_cases(
        &imported.project,
        &cases,
        "json_parser_unused_ports",
    )
}
