use super::*;

#[path = "../../../mfd/tests/fixtures/constructed-variable-dates/expected_project.rs"]
mod oracle;

use oracle::Shape;

#[test]
fn constructed_variable_dates_match_generated_rust_and_csharp_hosts() -> TestResult<()> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/constructed-variable-dates");
    let picture = |picture| Shape::Source {
        nested: false,
        picture,
    };
    for (designs, shape, catalog, label) in [
        (
            &[
                "root-json.mfd",
                "root-edi.mfd",
                "alias-json.mfd",
                "alias-edi.mfd",
                "connection-edi.mfd",
            ][..],
            picture("[Y]-[M]-[D]"),
            include_str!("../../../mfd/tests/fixtures/constructed-variable-dates/cases.json"),
            "constructed_dates_root",
        ),
        (
            &["nested-json.mfd", "nested-edi.mfd"][..],
            Shape::Source {
                nested: true,
                picture: "[Y]-[M]-[D]",
            },
            include_str!(
                "../../../mfd/tests/fixtures/constructed-variable-dates/nested-cases.json"
            ),
            "constructed_dates_nested",
        ),
        (
            &["parser-json.mfd", "parser-edi.mfd"][..],
            Shape::Parser,
            include_str!(
                "../../../mfd/tests/fixtures/constructed-variable-dates/parser-cases.json"
            ),
            "constructed_dates_parser",
        ),
        (
            &["malformed-picture.mfd"][..],
            picture("[]-x"),
            include_str!(
                "../../../mfd/tests/fixtures/constructed-variable-dates/picture-cases.json"
            ),
            "constructed_dates_malformed_picture",
        ),
        (
            &["blank-picture.mfd"][..],
            picture(""),
            include_str!(
                "../../../mfd/tests/fixtures/constructed-variable-dates/picture-cases.json"
            ),
            "constructed_dates_blank_picture",
        ),
    ] {
        // Every authored design for a shape lowers to the same complete
        // Project, so one generated library per shape serves all of them.
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
            include_str!("fixtures/constructed_dates_rust.rs.txt"),
            include_str!("fixtures/constructed_dates_csharp.cs.txt"),
        )?;
    }
    Ok(())
}
