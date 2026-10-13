use super::*;

#[test]
fn constructed_variable_aliases_compiled_complete_oracles() -> TestResult<()> {
    for (design, nested, label) in [
        ("one.mfd", false, "constructed_scalar_alias"),
        ("edi-alias.mfd", false, "constructed_edi_scalar_alias"),
        ("two.mfd", false, "constructed_scalar_alias_chain"),
        ("nested.mfd", true, "constructed_scalar_alias_frame"),
        ("limit.mfd", false, "constructed_scalar_alias_limit"),
    ] {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mfd/tests/fixtures/constructed-variable-aliases")
            .join(design);
        let imported = mfd::import_with_options(
            &fixture,
            &mfd::ImportOptions::default()
                .with_package_root(fixture.parent().unwrap().parent().unwrap()),
        )?;
        assert!(
            imported.warnings.is_empty(),
            "{design}: {:?}",
            imported.warnings
        );
        assert!(engine::validate(&imported.project).is_empty());
        let catalog = if nested {
            include_str!(
                "../../../mfd/tests/fixtures/constructed-variable-drivers/nested-cases.json"
            )
        } else {
            include_str!("../../../mfd/tests/fixtures/constructed-variable-drivers/cases.json")
        };
        let cases: Vec<serde_json::Value> = serde_json::from_str(catalog)?;
        json_text_boundaries::run_generated_boundary_cases(&imported.project, &cases, label)?;
    }
    Ok(())
}
