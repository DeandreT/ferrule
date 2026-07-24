use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};

use engine::RuntimeParameters;
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Project, Scope};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_runtime_parameters_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project() -> Project {
    Project {
        source: SchemaNode::group("Source", vec![]),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("Correlation", ScalarType::String),
                SchemaNode::scalar("Control", ScalarType::Int),
                SchemaNode::scalar("TestMode", ScalarType::Bool),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::RuntimeParameter {
                        name: "correlation_id".into(),
                        ty: ScalarType::String,
                    },
                ),
                (
                    2,
                    Node::RuntimeParameter {
                        name: "control_number".into(),
                        ty: ScalarType::Int,
                    },
                ),
                (
                    3,
                    Node::RuntimeParameter {
                        name: "test_mode".into(),
                        ty: ScalarType::Bool,
                    },
                ),
            ]),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "Correlation".into(),
                    node: 1,
                },
                Binding {
                    target_field: "Control".into(),
                    node: 2,
                },
                Binding {
                    target_field: "TestMode".into(),
                    node: 3,
                },
            ],
            ..Scope::default()
        },
    }
}

fn write_fixture(directory: &Path) -> Result<(PathBuf, PathBuf), Box<dyn std::error::Error>> {
    let project_path = directory.join("mapping.json");
    let source_path = directory.join("source.json");
    std::fs::write(&project_path, serde_json::to_vec_pretty(&project())?)?;
    std::fs::write(&source_path, "{}")?;
    Ok((project_path, source_path))
}

fn assert_output(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let output: serde_json::Value = serde_json::from_slice(&std::fs::read(path)?)?;
    assert_eq!(
        output,
        serde_json::json!({
            "Correlation": "txn=42",
            "Control": 42,
            "TestMode": true,
        })
    );
    Ok(())
}

#[test]
fn run_options_supply_parameters_and_report_every_artifact_in_order()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let (project_path, source_path) = write_fixture(&directory.0)?;
    let output_path = directory.0.join("library-output.json");
    let mut parameters = RuntimeParameters::new();
    parameters.insert("correlation_id", Value::String("txn=42".into()))?;
    parameters.insert("control_number", Value::String("42".into()))?;
    parameters.insert("test_mode", Value::String("true".into()))?;

    let outcome = cli::run_project_with_options(
        &project_path,
        &cli::RunOptions::new()
            .with_input_path(&source_path)
            .with_output_path(&output_path)
            .with_runtime_parameters(&parameters),
    )?;

    assert_output(&output_path)?;
    assert_eq!(
        outcome.artifacts,
        vec![cli::WrittenOutput {
            name: "Target".into(),
            records_written: 1,
            path: output_path,
        }]
    );
    Ok(())
}

#[test]
fn run_command_accepts_repeated_parameters_and_rejects_ambiguous_input()
-> Result<(), Box<dyn std::error::Error>> {
    let directory = TempDir::new()?;
    let (project_path, source_path) = write_fixture(&directory.0)?;
    let output_path = directory.0.join("command-output.json");
    let output = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args([
            "run",
            "--project",
            project_path.to_str().ok_or("project path is not UTF-8")?,
            "--input",
            source_path.to_str().ok_or("source path is not UTF-8")?,
            "--output",
            output_path.to_str().ok_or("output path is not UTF-8")?,
            "--param",
            "correlation_id=txn=42",
            "--param",
            "control_number=42",
            "--param",
            "test_mode=true",
        ])
        .output()?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_output(&output_path)?;

    let duplicate = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args([
            "run",
            "--project",
            project_path.to_str().ok_or("project path is not UTF-8")?,
            "--param",
            "correlation_id=first",
            "--param",
            "correlation_id=second",
        ])
        .output()?;
    assert!(!duplicate.status.success());
    assert!(
        String::from_utf8_lossy(&duplicate.stderr).contains("is duplicated"),
        "{}",
        String::from_utf8_lossy(&duplicate.stderr)
    );

    let malformed = Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .args([
            "run",
            "--project",
            project_path.to_str().ok_or("project path is not UTF-8")?,
            "--param",
            "missing-separator",
        ])
        .output()?;
    assert!(!malformed.status.success());
    assert!(
        String::from_utf8_lossy(&malformed.stderr).contains("must use NAME=VALUE"),
        "{}",
        String::from_utf8_lossy(&malformed.stderr)
    );
    Ok(())
}
