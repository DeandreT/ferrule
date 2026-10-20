use super::*;

#[path = "../../../mfd/tests/fixtures/constructed-variable-counts/expected_project.rs"]
mod oracle;

use oracle::Shape;

#[test]
fn constructed_variable_counts_match_generated_rust_and_csharp_hosts() -> TestResult<()> {
    let directory = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../mfd/tests/fixtures/constructed-variable-counts");
    let cases = include_str!("../../../mfd/tests/fixtures/constructed-variable-counts/cases.json");
    let nested_cases =
        include_str!("../../../mfd/tests/fixtures/constructed-variable-counts/nested-cases.json");
    for (designs, shape, catalog, label) in [
        (
            &["root-json.mfd", "chain-json.mfd", "direct-json.mfd"][..],
            Shape {
                nested: false,
                copy_all: true,
            },
            cases,
            "constructed_counts_copy_all",
        ),
        (
            &["root-edi.mfd"][..],
            Shape {
                nested: false,
                copy_all: false,
            },
            cases,
            "constructed_counts_fields",
        ),
        (
            &["nested-json.mfd", "nested-edi.mfd", "direct-edi.mfd"][..],
            Shape {
                nested: true,
                copy_all: false,
            },
            nested_cases,
            "constructed_counts_nested",
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
            include_str!("fixtures/constructed_scalar_rust.rs.txt"),
            include_str!("fixtures/constructed_scalar_csharp.cs.txt"),
        )?;
    }
    Ok(())
}
