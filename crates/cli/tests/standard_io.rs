use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::Context;

fn fixture_directory() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn test_directory(name: &str) -> anyhow::Result<PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "ferrule_cli_standard_io_{name}_{}",
        std::process::id()
    ));
    if path.exists() {
        std::fs::remove_dir_all(&path)?;
    }
    std::fs::create_dir_all(&path)?;
    Ok(path)
}

fn project() -> anyhow::Result<mapping::Project> {
    let text = std::fs::read_to_string(fixture_directory().join("project.json"))?;
    Ok(serde_json::from_str(&text)?)
}

fn write_project(directory: &Path, project: &mapping::Project) -> anyhow::Result<PathBuf> {
    let path = directory.join("mapping.json");
    std::fs::write(&path, serde_json::to_vec(project)?)?;
    Ok(path)
}

fn run_with_stdin(args: &[&str], input: &[u8]) -> anyhow::Result<std::process::Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    child
        .stdin
        .take()
        .context("child stdin was unavailable")?
        .write_all(input)?;
    Ok(child.wait_with_output()?)
}

#[test]
fn positional_dash_paths_stream_raw_mapping_bytes() -> anyhow::Result<()> {
    let directory = test_directory("positional")?;
    let mut project = project()?;
    project.source_path = Some("input.csv".into());
    project.target_path = Some("output.csv".into());
    let mapping = write_project(&directory, &project)?;
    let input = std::fs::read(fixture_directory().join("input.csv"))?;

    let output = run_with_stdin(
        &["run", mapping.to_str().context("mapping utf-8")?, "-", "-"],
        &input,
    )?;

    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        output.stdout,
        std::fs::read(fixture_directory().join("expected_output.csv"))?
    );
    assert!(output.stderr.is_empty());
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn stdout_rejects_multiple_target_artifacts_without_partial_bytes() -> anyhow::Result<()> {
    let directory = test_directory("multiple")?;
    let mut project = project()?;
    project.source_path = Some("input.csv".into());
    project.target_path = Some("output.csv".into());
    project.extra_targets.push(mapping::NamedTarget {
        name: "second".into(),
        path: Some("second.csv".into()),
        schema: project.target.clone(),
        options: mapping::FormatOptions::default(),
        root: project.root.clone(),
    });
    let mapping = write_project(&directory, &project)?;
    let input = std::fs::read(fixture_directory().join("input.csv"))?;

    let output = run_with_stdin(
        &[
            "run",
            "--project",
            mapping.to_str().context("mapping utf-8")?,
            "--input",
            "-",
            "--output",
            "-",
        ],
        &input,
    )?;

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("exactly one output artifact"));
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn stream_mode_keeps_trace_json_stdout_rejected() -> anyhow::Result<()> {
    let directory = test_directory("trace")?;
    let mut project = project()?;
    project.source_path = Some("input.csv".into());
    project.target_path = Some("output.csv".into());
    let mapping = write_project(&directory, &project)?;

    let output = run_with_stdin(
        &[
            "run",
            mapping.to_str().context("mapping utf-8")?,
            "-",
            "-",
            "--trace-json",
            "-",
        ],
        b"first_name,last_name,age\nJane,Doe,29\n",
    )?;

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--trace-json -"));
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn stream_mode_rejects_persistent_sqlite_and_update_existing_xlsx() -> anyhow::Result<()> {
    let directory = test_directory("persistent")?;
    let mut sqlite_project = project()?;
    sqlite_project.source_path = Some("input.sqlite".into());
    sqlite_project.target_path = Some("output.csv".into());
    let sqlite_mapping = write_project(&directory, &sqlite_project)?;

    let sqlite = run_with_stdin(
        &[
            "run",
            sqlite_mapping.to_str().context("mapping utf-8")?,
            "-",
            "-",
        ],
        b"not a database",
    )?;
    assert!(!sqlite.status.success());
    assert!(sqlite.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&sqlite.stderr)
            .contains("SQLite input requires a persistent database")
    );

    let mut xlsx_project = project()?;
    xlsx_project.source_path = Some("input.csv".into());
    xlsx_project.target_path = Some("output.xlsx".into());
    xlsx_project.target_options.xlsx_update_existing = true;
    let xlsx_mapping = directory.join("xlsx-mapping.json");
    std::fs::write(&xlsx_mapping, serde_json::to_vec(&xlsx_project)?)?;
    let xlsx = run_with_stdin(
        &[
            "run",
            xlsx_mapping.to_str().context("mapping utf-8")?,
            "-",
            "-",
        ],
        b"first_name,last_name,age\nJane,Doe,29\n",
    )?;
    assert!(!xlsx.status.success());
    assert!(xlsx.stdout.is_empty());
    assert!(
        String::from_utf8_lossy(&xlsx.stderr)
            .contains("update-existing XLSX output requires a persistent workbook")
    );

    std::fs::remove_dir_all(directory)?;
    Ok(())
}
