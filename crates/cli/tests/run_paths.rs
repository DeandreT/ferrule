use std::cell::RefCell;
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
fn in_memory_project_runs_before_its_project_file_exists() {
    let dir = test_dir("unsaved_project");
    std::fs::copy(fixture_dir().join("input.csv"), dir.join("input.csv")).unwrap();
    let project = project_with_paths(Some("input.csv"), Some("output.csv"));
    let virtual_project_path = dir.join("not-saved-yet.json");

    let outcome =
        cli::run_project_value_with_paths(&project, &virtual_project_path, None, None).unwrap();

    assert!(!virtual_project_path.exists());
    assert_eq!(outcome.records_written, 2);
    assert_eq!(outcome.output_path, dir.join("output.csv"));
    assert_eq!(
        std::fs::read_to_string(dir.join("output.csv")).unwrap(),
        std::fs::read_to_string(fixture_dir().join("expected_output.csv")).unwrap()
    );
    std::fs::remove_dir_all(dir).unwrap();
}

#[derive(Default)]
struct TraceCollector(RefCell<Vec<cli::TraceEvent>>);

impl cli::TraceSink for TraceCollector {
    fn record(&self, event: cli::TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

#[test]
fn in_memory_run_forwards_deterministic_trace_events() {
    let dir = test_dir("trace");
    std::fs::copy(fixture_dir().join("input.csv"), dir.join("input.csv")).unwrap();
    let project = project_with_paths(Some("input.csv"), Some("output.csv"));
    let trace = TraceCollector::default();

    let outcome = cli::run_project_value_with_paths_and_trace(
        &project,
        &dir.join("mapping.json"),
        None,
        None,
        &trace,
    )
    .unwrap();

    assert_eq!(outcome.records_written, 2);
    assert!(!trace.0.into_inner().is_empty());
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
fn json_lines_extensions_run_as_repeated_json_documents() {
    let dir = test_dir("json_lines");
    std::fs::write(
        dir.join("input.jsonl"),
        "{\"first_name\":\"Jane\",\"last_name\":\"Doe\",\"age\":29}\n\
         {\"first_name\":\"John\",\"last_name\":\"Smith\",\"age\":41}\n",
    )
    .unwrap();
    let project = write_project(
        &dir,
        &project_with_paths(Some("input.jsonl"), Some("output.ndjson")),
    );

    let output = ferrule(&dir, &["run", "--project", project.to_str().unwrap()]);

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let lines = std::fs::read_to_string(dir.join("output.ndjson")).unwrap();
    let values = lines
        .lines()
        .map(|line| serde_json::from_str::<serde_json::Value>(line).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(values.len(), 2);
    assert_eq!(values[0]["full_name"], "Jane Doe");
    assert_eq!(values[1]["age_next_year"], 42);
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

#[test]
fn run_supplies_the_active_project_path_to_runtime_nodes() {
    let dir = test_dir("runtime_project_path");
    std::fs::copy(fixture_dir().join("input.csv"), dir.join("input.csv")).unwrap();
    let mut project = project_with_paths(Some("input.csv"), Some("output.csv"));
    project.graph.nodes.insert(
        3,
        mapping::Node::RuntimeValue {
            value: mapping::RuntimeValue::MappingFilePath,
        },
    );
    let project_path = write_project(&dir, &project);

    let output = ferrule(
        Path::new("/"),
        &["run", "--project", project_path.to_str().unwrap()],
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let expected_path = std::fs::canonicalize(&project_path).unwrap();
    let csv = std::fs::read_to_string(dir.join("output.csv")).unwrap();
    assert_eq!(csv.matches(expected_path.to_str().unwrap()).count(), 2);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn run_supplies_one_valid_current_datetime_to_every_row() {
    let dir = test_dir("runtime_current_datetime");
    std::fs::copy(fixture_dir().join("input.csv"), dir.join("input.csv")).unwrap();
    let mut project = project_with_paths(Some("input.csv"), Some("output.csv"));
    project.graph.nodes.insert(
        3,
        mapping::Node::RuntimeValue {
            value: mapping::RuntimeValue::CurrentDateTime,
        },
    );
    let project_path = write_project(&dir, &project);

    let output = ferrule(
        Path::new("/"),
        &["run", "--project", project_path.to_str().unwrap()],
    );

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let csv = std::fs::read_to_string(dir.join("output.csv")).unwrap();
    let timestamps = csv
        .lines()
        .skip(1)
        .filter_map(|line| line.split(',').next())
        .collect::<Vec<_>>();
    assert_eq!(timestamps.len(), 2);
    assert_eq!(timestamps[0], timestamps[1]);
    assert!(timestamps[0].parse::<jiff::Timestamp>().is_ok());
    std::fs::remove_dir_all(dir).unwrap();
}
