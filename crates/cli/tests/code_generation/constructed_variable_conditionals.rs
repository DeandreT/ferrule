use super::*;

#[test]
fn constructed_variable_conditionals_compiled_complete_oracles() -> TestResult<()> {
    for (design, nested, label) in [
        ("root-json.mfd", false, "conditional_json"),
        ("root-edi-explicit.mfd", false, "conditional_explicit_edi"),
        ("nested-edi.mfd", true, "conditional_parent_frame"),
        ("alias-edi.mfd", false, "conditional_scalar_alias"),
        ("chain-json.mfd", false, "conditional_nested_branches"),
        (
            "xml-edi-explicit.mfd",
            false,
            "conditional_xml_attributes_typed_json_boundary",
        ),
    ] {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mfd/tests/fixtures/constructed-variable-conditionals")
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
                "../../../mfd/tests/fixtures/constructed-variable-conditionals/nested-cases.json"
            )
        } else if design == "chain-json.mfd" {
            include_str!(
                "../../../mfd/tests/fixtures/constructed-variable-conditionals/chain-cases.json"
            )
        } else {
            include_str!("../../../mfd/tests/fixtures/constructed-variable-conditionals/cases.json")
        };
        let cases: Vec<serde_json::Value> = serde_json::from_str(catalog)?;
        json_text_boundaries::run_generated_boundary_cases(&imported.project, &cases, label)?;
    }
    Ok(())
}
