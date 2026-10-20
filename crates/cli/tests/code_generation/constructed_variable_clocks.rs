use super::*;

#[path = "../../../mfd/tests/fixtures/constructed-variable-clocks/expected_project.rs"]
mod oracle;

use oracle::Shape;

#[test]
fn constructed_variable_clocks_match_generated_rust_and_csharp_hosts() -> TestResult<()> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/constructed-variable-clocks");
    for (designs, shape, catalog, label) in [
        (
            ["root-json.mfd", "root-edi.mfd"],
            Shape::Conditional { nested: false },
            include_str!("../../../mfd/tests/fixtures/constructed-variable-clocks/cases.json"),
            "constructed_clocks_root",
        ),
        (
            ["nested-json.mfd", "nested-edi.mfd"],
            Shape::Conditional { nested: true },
            include_str!(
                "../../../mfd/tests/fixtures/constructed-variable-clocks/nested-cases.json"
            ),
            "constructed_clocks_nested",
        ),
        (
            ["direct-json.mfd", "direct-edi.mfd"],
            Shape::Direct,
            include_str!(
                "../../../mfd/tests/fixtures/constructed-variable-clocks/direct-cases.json"
            ),
            "constructed_clocks_direct",
        ),
    ] {
        // JSON and EDI variables lower to the same complete Project, so one
        // generated library per shape serves every authored design.
        let expected = oracle::expected(shape);
        for design in designs {
            let path = directory.join(design);
            let imported = mfd::import_with_options(
                &path,
                &mfd::ImportOptions::default().with_package_root(&directory),
            )?;
            assert!(
                imported.warnings.is_empty(),
                "{design}: {:?}",
                imported.warnings
            );
            assert_eq!(
                mapping::project_file::encode_pretty(&imported.project)?,
                mapping::project_file::encode_pretty(&expected)?,
                "{design}"
            );
        }
        assert!(engine::validate(&expected).is_empty());
        let cases: Vec<serde_json::Value> = serde_json::from_str(catalog)?;
        json_text_boundaries::run_generated_boundary_cases_with_harnesses(
            &expected,
            &cases,
            label,
            include_str!("fixtures/constructed_clocks_rust.rs.txt"),
            include_str!("fixtures/constructed_clocks_csharp.cs.txt"),
        )?;
    }
    Ok(())
}
