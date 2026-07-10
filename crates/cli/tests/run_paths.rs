use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn test_dir(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ferrule_cli_run_paths_{name}_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(&path).unwrap();
    path
}

fn project_with_paths(source_path: Option<&str>, target_path: Option<&str>) -> mapping::Project {
    let text = std::fs::read_to_string(fixture_dir().join("project.json")).unwrap();
    let mut project: mapping::Project = serde_json::from_str(&text).unwrap();
    project.source_path = source_path.map(str::to_owned);
    project.target_path = target_path.map(str::to_owned);
    project
}

fn write_project(dir: &Path, project: &mapping::Project) -> PathBuf {
    let path = dir.join("project.json");
    std::fs::write(&path, serde_json::to_string_pretty(project).unwrap()).unwrap();
    path
}

fn ferrule(current_dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .current_dir(current_dir)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn run_uses_project_relative_input_and_output_defaults() {
    let dir = test_dir("defaults");
    std::fs::copy(fixture_dir().join("input.csv"), dir.join("input.csv")).unwrap();
    std::fs::create_dir(dir.join("results")).unwrap();
    let project = write_project(
        &dir,
        &project_with_paths(Some("input.csv"), Some("results/output.csv")),
    );

    let output = ferrule(
        Path::new("/"),
        &["run", "--project", project.to_str().unwrap()],
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("results/output.csv")).unwrap(),
        std::fs::read_to_string(fixture_dir().join("expected_output.csv")).unwrap()
    );
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(dir.join("results/output.csv").to_str().unwrap())
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn explicit_paths_override_project_defaults() {
    let dir = test_dir("overrides");
    std::fs::copy(
        fixture_dir().join("input.csv"),
        dir.join("explicit-input.csv"),
    )
    .unwrap();
    let project = write_project(
        &dir,
        &project_with_paths(Some("missing.csv"), Some("missing/output.csv")),
    );

    let output = ferrule(
        &dir,
        &[
            "run",
            "--project",
            project.to_str().unwrap(),
            "--input",
            "explicit-input.csv",
            "--output",
            "explicit-output.csv",
        ],
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("explicit-output.csv")).unwrap(),
        std::fs::read_to_string(fixture_dir().join("expected_output.csv")).unwrap()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_input_default_reports_how_to_configure_it() {
    let dir = test_dir("missing_input");
    let project = write_project(&dir, &project_with_paths(None, Some("output.csv")));

    let output = ferrule(
        &dir,
        &[
            "--diagnostics",
            "json",
            "run",
            "--project",
            project.to_str().unwrap(),
        ],
    );

    assert!(!output.status.success());
    let diagnostic: serde_json::Value =
        serde_json::from_slice(output.stderr.strip_suffix(b"\n").unwrap()).unwrap();
    assert_eq!(diagnostic["command"], "run");
    let message = diagnostic["message"].as_str().unwrap();
    assert!(message.contains("--input <PATH>"), "{message}");
    assert!(message.contains("source_path"), "{message}");
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn missing_output_default_reports_how_to_configure_it() {
    let dir = test_dir("missing_output");
    std::fs::copy(fixture_dir().join("input.csv"), dir.join("input.csv")).unwrap();
    let project = write_project(&dir, &project_with_paths(Some("input.csv"), None));

    let output = ferrule(&dir, &["run", "--project", project.to_str().unwrap()]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--output <PATH>"), "{stderr}");
    assert!(stderr.contains("target_path"), "{stderr}");
    std::fs::remove_dir_all(dir).unwrap();
}
