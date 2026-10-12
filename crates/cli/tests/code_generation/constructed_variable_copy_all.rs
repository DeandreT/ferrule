use super::*;

#[test]
fn constructed_variable_copy_all_compiled_complete_oracles() -> TestResult<()> {
    for (design, nested, overrides, label) in [
        ("explicit-edi.mfd", false, false, "copy_all_explicit"),
        ("json-object.mfd", false, false, "copy_all_json_object"),
        ("json-array.mfd", false, false, "copy_all_json_array"),
        ("edi-array.mfd", false, false, "copy_all_edi_array"),
        ("nested-edi-array.mfd", true, false, "copy_all_parent_frame"),
        ("override-json.mfd", false, true, "copy_all_override"),
        ("alias-json.mfd", false, false, "copy_all_scalar_alias"),
    ] {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../mfd/tests/fixtures/constructed-variable-copy-all")
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
        let mut cases: Vec<serde_json::Value> = serde_json::from_str(catalog)?;
        if overrides {
            for case in &mut cases {
                if case["reject"] != true {
                    case["expected"]["Records"][0]["Text"] = "override".into();
                }
            }
        }
        json_text_boundaries::run_generated_boundary_cases(&imported.project, &cases, label)?;
    }
    Ok(())
}
