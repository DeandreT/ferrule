use super::*;

#[path = "../../../mfd/tests/fixtures/enclosing-frame-value-maps/expected_project.rs"]
mod oracle;

#[test]
fn enclosing_frame_total_string_value_maps_compiled_complete_oracles() -> TestResult<()> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/enclosing-frame-value-maps");
    let imported = mfd::import(&directory.join("total-string.mfd"))?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(
        mapping::project_file::encode_pretty(&imported.project)?,
        mapping::project_file::encode_pretty(&oracle::expected_total_string_value_map_project())?,
    );
    assert!(engine::validate(&imported.project).is_empty());
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../mfd/tests/fixtures/enclosing-frame-value-maps/cases.json"
    ))?;
    json_text_boundaries::run_generated_boundary_cases(
        &imported.project,
        &cases,
        "enclosing_total_string_maps",
    )
}
