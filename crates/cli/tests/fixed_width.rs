use std::path::{Path, PathBuf};

use mapping::{FixedFieldWidth, FixedWidthLayout, FormatOptions};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn test_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ferrule_cli_fixed_width_{name}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn fixture_project() -> mapping::Project {
    let text = std::fs::read_to_string(fixture_dir().join("project.json")).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn fixed_width(widths: &[u32]) -> FixedWidthLayout {
    let widths = widths
        .iter()
        .copied()
        .map(|width| FixedFieldWidth::new(width).unwrap())
        .collect();
    FixedWidthLayout::new(widths, ' ', true, true).unwrap()
}

fn write_project(dir: &Path, project: &mapping::Project) -> PathBuf {
    let path = dir.join("project.json");
    std::fs::write(&path, serde_json::to_string_pretty(project).unwrap()).unwrap();
    path
}

fn configured_project(source_path: &str, target_path: &str) -> mapping::Project {
    let mut project = fixture_project();
    project.source_path = Some(source_path.to_owned());
    project.target_path = Some(target_path.to_owned());
    project.source_options.fixed_width = Some(fixed_width(&[6, 6, 2]));
    project.target_options.fixed_width = Some(fixed_width(&[12, 2]));
    project
}

#[test]
fn fixed_width_uses_project_relative_dat_paths() {
    let dir = test_dir("relative_dat");
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::create_dir_all(dir.join("results")).unwrap();
    std::fs::write(
        dir.join("data/input.dat"),
        "Jane  Doe   29\nJohn  Smith 41\n",
    )
    .unwrap();
    let project = write_project(
        &dir,
        &configured_project("data/input.dat", "results/output.dat"),
    );

    let outcome = cli::run_project_with_paths(&project, None, None).unwrap();

    assert_eq!(outcome.records_written, 2);
    assert_eq!(outcome.input_path, dir.join("data/input.dat"));
    assert_eq!(outcome.output_path, dir.join("results/output.dat"));
    assert_eq!(
        std::fs::read_to_string(outcome.output_path).unwrap(),
        "Jane Doe    30\nJohn Smith  42\n"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn explicit_extensionless_paths_override_fixed_width_defaults() {
    let dir = test_dir("explicit_override");
    std::fs::write(
        dir.join("explicit-input"),
        "Jane  Doe   29\nJohn  Smith 41\n",
    )
    .unwrap();
    let project = write_project(
        &dir,
        &configured_project("missing-input.dat", "missing-output.dat"),
    );

    let outcome = cli::run_project_with_paths(
        &project,
        Some(&dir.join("explicit-input")),
        Some(&dir.join("explicit-output")),
    )
    .unwrap();

    assert_eq!(outcome.records_written, 2);
    assert_eq!(
        std::fs::read_to_string(outcome.output_path).unwrap(),
        "Jane Doe    30\nJohn Smith  42\n"
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn fixed_width_rejects_csv_options_on_input_and_output() {
    let dir = test_dir("conflicts");
    std::fs::write(dir.join("input.dat"), "Jane  Doe   29\n").unwrap();

    let mut input_conflict = configured_project("input.dat", "output.dat");
    input_conflict.source_options.delimiter = Some('|');
    let project = write_project(&dir, &input_conflict);
    let error = cli::run_project_with_paths(&project, None, None).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("fixed_width"), "{message}");
    assert!(message.contains("delimiter"), "{message}");
    assert!(message.contains("input"), "{message}");

    let mut output_conflict = configured_project("input.dat", "output.dat");
    output_conflict.target_options.has_header_row = Some(false);
    let project = write_project(&dir, &output_conflict);
    let error = cli::run_project_with_paths(&project, None, None).unwrap_err();
    let message = format!("{error:#}");
    assert!(message.contains("fixed_width"), "{message}");
    assert!(message.contains("has_header_row"), "{message}");
    assert!(message.contains("output"), "{message}");

    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn txt_paths_use_csv_when_fixed_width_is_not_configured() {
    let dir = test_dir("txt_csv");
    std::fs::copy(fixture_dir().join("input.csv"), dir.join("input.txt")).unwrap();
    let mut project = fixture_project();
    project.source_path = Some("input.txt".to_owned());
    project.target_path = Some("output.txt".to_owned());
    project.source_options = FormatOptions::default();
    project.target_options = FormatOptions::default();
    let project = write_project(&dir, &project);

    let outcome = cli::run_project_with_paths(&project, None, None).unwrap();

    assert_eq!(outcome.records_written, 2);
    assert_eq!(
        std::fs::read_to_string(outcome.output_path).unwrap(),
        std::fs::read_to_string(fixture_dir().join("expected_output.csv")).unwrap()
    );
    std::fs::remove_dir_all(dir).unwrap();
}
