use super::*;

#[test]
fn constructed_variable_drivers_compiled_complete_oracles() -> TestResult<()> {
    for (design, catalog, label) in [
        (
            "xml.mfd",
            include_str!("../../../mfd/tests/fixtures/constructed-variable-drivers/cases.json"),
            "constructed_xml_attribute_driver",
        ),
        (
            "edi.mfd",
            include_str!("../../../mfd/tests/fixtures/constructed-variable-drivers/cases.json"),
            "constructed_edi_driver",
        ),
        (
            "json.mfd",
            include_str!("../../../mfd/tests/fixtures/constructed-variable-drivers/cases.json"),
            "constructed_json_driver",
        ),
        (
            "nested-edi.mfd",
            include_str!(
                "../../../mfd/tests/fixtures/constructed-variable-drivers/nested-cases.json"
            ),
            "constructed_nested_edi_driver",
        ),
    ] {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mfd/tests/fixtures/constructed-variable-drivers")
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
