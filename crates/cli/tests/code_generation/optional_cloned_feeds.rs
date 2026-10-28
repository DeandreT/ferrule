use super::*;

#[test]
fn optional_cloned_feeds_keep_complete_typed_and_json_results() -> TestResult<()> {
    let package = TempDir::new("optional_cloned_feeds")?;
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/optional_cloned_feeds");
    for name in ["mapping.mfd", "source.xsd"] {
        std::fs::copy(fixtures.join(name), package.0.join(name))?;
    }
    std::fs::copy(
        fixtures.join("target-repeated.xsd"),
        package.0.join("target.xsd"),
    )?;
    let imported = mfd::import(&package.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty());
    assert!(engine::validate(&imported.project).is_empty());
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../mfd/tests/fixtures/optional_cloned_feeds/cases.json"
    ))?;
    assert_eq!(cases.len(), 4);
    json_text_boundaries::run_generated_boundary_cases_with_harnesses(
        &imported.project,
        &cases,
        "optional_cloned_feeds",
        include_str!("optional_cloned_feeds/Host.rs.txt"),
        include_str!("optional_cloned_feeds/Host.cs.txt"),
    )
}
