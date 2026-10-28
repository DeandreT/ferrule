use super::*;

#[test]
fn explicit_xml_nil_preserves_cloned_typed_occurrences() -> TestResult<()> {
    let package = TempDir::new("xml_nil_cloned_feeds")?;
    let fixtures =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/optional_cloned_feeds");
    std::fs::copy(fixtures.join("mapping.mfd"), package.0.join("mapping.mfd"))?;
    std::fs::copy(
        fixtures.join("source-nil.xsd"),
        package.0.join("source.xsd"),
    )?;
    std::fs::copy(
        fixtures.join("target-nil-repeated.xsd"),
        package.0.join("target.xsd"),
    )?;
    let imported = mfd::import(&package.0.join("mapping.mfd"))?;
    assert!(imported.warnings.is_empty());
    assert!(engine::validate(&imported.project).is_empty());
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "../../../mfd/tests/fixtures/optional_cloned_feeds/nil_cases.json"
    ))?;
    assert_eq!(cases.len(), 5);
    json_text_boundaries::run_generated_boundary_cases_with_harnesses(
        &imported.project,
        &cases,
        "xml_nil_cloned_feeds",
        include_str!("xml_nil_cloned_feeds/Host.rs.txt"),
        include_str!("xml_nil_cloned_feeds/Host.cs.txt"),
    )
}
