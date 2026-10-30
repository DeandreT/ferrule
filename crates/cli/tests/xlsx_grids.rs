//! Composite XLSX sources holding a grid per worksheet through the CLI.

use std::path::{Path, PathBuf};

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_xlsx_grids_{name}_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn grid_pair_project() -> mapping::Project {
    let design =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/xlsx-grid-pair.mfd");
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    imported.project
}

fn workbook() -> Vec<u8> {
    let mut workbook = rust_xlsxwriter::Workbook::new();
    for (name, cells) in [
        (
            "Quarterly",
            &[(0, 0, "2026"), (0, 1, "Q1"), (1, 0, "101"), (1, 1, "10.5")][..],
        ),
        (
            "Annual",
            &[
                (0, 1, "USD"),
                (2, 2, "FY2026"),
                (4, 0, "301"),
                (4, 2, "1.5"),
            ][..],
        ),
    ] {
        let worksheet = workbook.add_worksheet();
        worksheet.set_name(name).unwrap();
        for (row, column, value) in cells {
            worksheet.write_string(*row, *column, *value).unwrap();
        }
    }
    workbook.save_to_buffer().unwrap()
}

#[test]
fn composite_grid_source_runs_through_the_file_route() {
    let project = grid_pair_project();
    let scratch = Scratch::new("file");
    let project_path = scratch.0.join("project.json");
    let input_path = scratch.0.join("grids.xlsx");
    let output_path = scratch.0.join("report.xml");
    std::fs::write(&project_path, serde_json::to_vec(&project).unwrap()).unwrap();
    let bytes = workbook();
    std::fs::write(&input_path, &bytes).unwrap();

    cli::run_project(&project_path, &input_path, &output_path).unwrap();

    // One header record per grid: each reads its own header row, fixed cell,
    // and data rows.
    let report = std::fs::read_to_string(&output_path).unwrap();
    for expected in [
        r#"Month="Q1""#,
        r#"Column="2""#,
        r#"Year="2026""#,
        r#"Region="101""#,
        r#"Amount="10.5""#,
        r#"Label="FY2026""#,
        r#"Column="3""#,
        r#"Unit="USD""#,
        r#"Region="301""#,
        r#"Amount="1.5""#,
    ] {
        assert_eq!(report.matches(expected).count(), 1, "{expected}: {report}");
    }
}

#[test]
fn composite_grid_target_is_refused_explicitly() {
    let mut project = grid_pair_project();
    project.target_options = project.source_options.clone();
    project.target = project.source.clone();
    let scratch = Scratch::new("target");
    let project_path = scratch.0.join("project.json");
    let input_path = scratch.0.join("grids.xlsx");
    let output_path = scratch.0.join("out.xlsx");
    std::fs::write(&project_path, serde_json::to_vec(&project).unwrap()).unwrap();
    std::fs::write(&input_path, workbook()).unwrap();

    let error = cli::run_project(&project_path, &input_path, &output_path).unwrap_err();
    assert!(
        format!("{error:#}").contains("grid XLSX output is not supported"),
        "{error:#}"
    );
    assert!(!output_path.exists());
}

#[test]
fn generated_code_refuses_composite_grid_sources() {
    let project = grid_pair_project();
    for target in [
        cli::GenerateTarget::CSharp,
        cli::GenerateTarget::Rust {
            runtime_path: PathBuf::from("runtime"),
        },
    ] {
        let error = cli::check_generate_project(&project, &target, cli::GenerateProfile::Ordinary)
            .unwrap_err();
        assert!(format!("{error:#}").contains("XLSX grid"), "{error:#}");
    }
}
