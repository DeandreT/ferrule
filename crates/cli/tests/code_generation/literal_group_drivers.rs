use super::*;

#[test]
fn literal_group_drivers_compiled_complete_oracles() -> TestResult<()> {
    for (design, catalog, label) in [
        (
            "root.mfd",
            include_str!("../../../mfd/tests/fixtures/literal-group-drivers/root-cases.json"),
            "literal_group_root",
        ),
        (
            "nested.mfd",
            include_str!("../../../mfd/tests/fixtures/literal-group-drivers/nested-cases.json"),
            "literal_group_nested",
        ),
        (
            "array.mfd",
            include_str!("../../../mfd/tests/fixtures/literal-group-drivers/array-cases.json"),
            "literal_group_array",
        ),
        (
            "cloned.mfd",
            include_str!("../../../mfd/tests/fixtures/literal-group-drivers/cloned-cases.json"),
            "literal_group_cloned",
        ),
    ] {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mfd/tests/fixtures/literal-group-drivers")
            .join(design);
        let imported = mfd::import(&fixture)?;
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert!(engine::validate(&imported.project).is_empty());
        let cases: Vec<serde_json::Value> = serde_json::from_str(catalog)?;
        json_text_boundaries::run_generated_boundary_cases(&imported.project, &cases, label)?;
    }
    Ok(())
}
