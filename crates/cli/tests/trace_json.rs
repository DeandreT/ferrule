use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FailureIteration, FailureRule, FailureSelection, Graph, NamedTarget, Node, Project,
    Scope, ScopeIteration, SequenceExpr, SequenceWindow, SortFilterOrder,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Result<Self, std::io::Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_trace_{label}_{}_{}",
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

fn ferrule(dir: &Path, args: &[&str]) -> Result<Output, std::io::Error> {
    Command::new(env!("CARGO_BIN_EXE_ferrule"))
        .current_dir(dir)
        .args(args)
        .output()
}

fn write_project(dir: &Path, project: &Project) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let path = dir.join("project.json");
    std::fs::write(&path, serde_json::to_vec_pretty(project)?)?;
    Ok(path)
}

fn json_lines(path: &Path) -> Result<Vec<serde_json::Value>, Box<dyn std::error::Error>> {
    Ok(std::fs::read_to_string(path)?
        .lines()
        .map(serde_json::from_str)
        .collect::<Result<Vec<_>, _>>()?)
}

fn control_project() -> Project {
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                    .repeating(),
            ],
        ),
        source_path: Some("input.json".into()),
        target_path: Some("output.json".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::Int(4),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![2, 0],
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
                    from: Some(0),
                    to: 1,
                    item: 2,
                }),
                filter: Some(3),
                sort_by: Some(2),
                sort_descending: true,
                sort_filter_order: SortFilterOrder::FilterThenSort,
                group_into_blocks: Some(4),
                windows: vec![SequenceWindow::First { count: 5 }],
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 2,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn selected_target_project() -> Project {
    let schema =
        |name| SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)]);
    let scope = || Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 0,
        }],
        ..Scope::default()
    };
    Project {
        source: schema("Input"),
        target: schema("Primary"),
        source_path: Some("input.json".into()),
        target_path: Some("primary.json".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: vec![NamedTarget {
            name: "audit".into(),
            path: Some("audit.json".into()),
            schema: schema("Audit"),
            options: Default::default(),
            root: scope(),
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [(
                0,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            )]
            .into(),
        },
        root: scope(),
    }
}

fn failing_project() -> Project {
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group("Output", Vec::new()),
        source_path: Some("input.json".into()),
        target_path: Some("output.json".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: vec![FailureRule {
            iteration: FailureIteration::Sequence {
                sequence: SequenceExpr::Generate {
                    from: Some(0),
                    to: 1,
                    item: 2,
                },
            },
            selection: FailureSelection::All,
            message: Some(3),
        }],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::String("forced trace failure".into()),
                    },
                ),
            ]
            .into(),
        },
        root: Scope::default(),
    }
}

#[test]
fn run_writes_versioned_node_and_control_events() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("controls")?;
    let project = write_project(&dir.0, &control_project())?;
    std::fs::write(dir.0.join("input.json"), "{}")?;
    let trace = dir.0.join("run.trace.jsonl");

    let output = ferrule(
        &dir.0,
        &[
            "--diagnostics",
            "json",
            "run",
            "--project",
            project.to_str().ok_or("non-UTF-8 project path")?,
            "--trace-json",
            trace.to_str().ok_or("non-UTF-8 trace path")?,
        ],
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("wrote "));

    let lines = json_lines(&trace)?;
    assert!(!lines.is_empty());
    for (sequence, line) in lines.iter().enumerate() {
        assert_eq!(line["schema_version"], 2);
        assert_eq!(line["sequence"], sequence);
        assert!(line["event"]["kind"].is_string());
    }
    let kinds = lines
        .iter()
        .filter_map(|line| line["event"]["kind"].as_str())
        .collect::<BTreeSet<_>>();
    assert!(kinds.contains("node_value"), "{kinds:?}");
    assert!(kinds.contains("scope_started"), "{kinds:?}");
    assert!(kinds.contains("iteration_candidate"), "{kinds:?}");
    assert!(kinds.contains("filter_decision"), "{kinds:?}");
    assert!(kinds.contains("sort_candidate"), "{kinds:?}");
    assert!(kinds.contains("sort_position"), "{kinds:?}");
    assert!(kinds.contains("group_produced"), "{kinds:?}");
    assert!(kinds.contains("window_applied"), "{kinds:?}");
    assert!(kinds.contains("target_field_written"), "{kinds:?}");
    assert!(kinds.contains("target_produced"), "{kinds:?}");
    assert!(kinds.contains("scope_finished"), "{kinds:?}");

    let first_trace = std::fs::read(&trace)?;
    let second_trace = dir.0.join("second.trace.jsonl");
    std::fs::write(&second_trace, "previous trace\n")?;
    let second_output = ferrule(
        &dir.0,
        &[
            "run",
            "--project",
            project.to_str().ok_or("non-UTF-8 project path")?,
            "--trace-json",
            second_trace.to_str().ok_or("non-UTF-8 trace path")?,
        ],
    )?;
    assert!(
        second_output.status.success(),
        "{}",
        String::from_utf8_lossy(&second_output.stderr)
    );
    assert_eq!(std::fs::read(second_trace)?, first_trace);
    Ok(())
}

#[test]
fn selected_target_trace_identifies_only_that_target() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("selected")?;
    let project = write_project(&dir.0, &selected_target_project())?;
    std::fs::write(dir.0.join("input.json"), r#"{"Value":"selected"}"#)?;
    let trace = dir.0.join("selected.trace.jsonl");

    let output = ferrule(
        &dir.0,
        &[
            "--diagnostics",
            "json",
            "run",
            "--project",
            project.to_str().ok_or("non-UTF-8 project path")?,
            "--target",
            "audit",
            "--trace-json",
            trace.to_str().ok_or("non-UTF-8 trace path")?,
        ],
    )?;
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!dir.0.join("primary.json").exists());

    let scope_targets = json_lines(&trace)?
        .into_iter()
        .filter_map(|line| line["event"]["scope"]["target"].as_object().cloned())
        .collect::<Vec<_>>();
    assert!(!scope_targets.is_empty());
    assert!(scope_targets.iter().all(|target| {
        target.get("kind").and_then(serde_json::Value::as_str) == Some("named")
            && target.get("name").and_then(serde_json::Value::as_str) == Some("audit")
    }));
    Ok(())
}

#[test]
fn failed_execution_preserves_existing_trace() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("failure")?;
    let project = write_project(&dir.0, &failing_project())?;
    std::fs::write(dir.0.join("input.json"), "{}")?;
    let trace = dir.0.join("run.trace.jsonl");
    std::fs::write(&trace, "existing trace\n")?;

    let output = ferrule(
        &dir.0,
        &[
            "--diagnostics",
            "json",
            "run",
            "--project",
            project.to_str().ok_or("non-UTF-8 project path")?,
            "--trace-json",
            trace.to_str().ok_or("non-UTF-8 trace path")?,
        ],
    )?;
    assert!(!output.status.success());
    let diagnostics = String::from_utf8_lossy(&output.stderr)
        .lines()
        .map(serde_json::from_str::<serde_json::Value>)
        .collect::<Result<Vec<_>, _>>()?;
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0]["schema_version"], 1);
    assert_eq!(diagnostics[0]["command"], "run");
    assert!(
        diagnostics[0]["message"]
            .as_str()
            .is_some_and(|message| message.contains("forced trace failure")),
        "{diagnostics:?}"
    );
    assert_eq!(std::fs::read_to_string(trace)?, "existing trace\n");
    assert!(!dir.0.join("output.json").exists());
    assert!(
        std::fs::read_dir(&dir.0)?
            .filter_map(Result::ok)
            .all(|entry| !entry.file_name().to_string_lossy().contains("trace-stage"))
    );
    Ok(())
}

#[test]
fn invalid_trace_destinations_fail_before_execution() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("invalid")?;
    let project = write_project(&dir.0, &control_project())?;
    std::fs::write(dir.0.join("input.json"), "{}")?;
    let trace_directory = dir.0.join("trace");
    std::fs::create_dir(&trace_directory)?;

    let directory_output = ferrule(
        &dir.0,
        &[
            "run",
            "--project",
            project.to_str().ok_or("non-UTF-8 project path")?,
            "--trace-json",
            trace_directory.to_str().ok_or("non-UTF-8 trace path")?,
        ],
    )?;
    assert!(!directory_output.status.success());
    assert!(
        String::from_utf8_lossy(&directory_output.stderr).contains("is a directory"),
        "{}",
        String::from_utf8_lossy(&directory_output.stderr)
    );
    assert!(!dir.0.join("output.json").exists());

    let stdout_output = ferrule(
        &dir.0,
        &[
            "run",
            "--project",
            project.to_str().ok_or("non-UTF-8 project path")?,
            "--trace-json",
            "-",
        ],
    )?;
    assert!(!stdout_output.status.success());
    assert!(
        String::from_utf8_lossy(&stdout_output.stderr).contains("reserves stdout"),
        "{}",
        String::from_utf8_lossy(&stdout_output.stderr)
    );
    assert!(!dir.0.join("output.json").exists());
    Ok(())
}

#[test]
fn trace_cannot_replace_a_mapping_output() -> Result<(), Box<dyn std::error::Error>> {
    let dir = TempDir::new("output_collision")?;
    let project = write_project(&dir.0, &control_project())?;
    std::fs::write(dir.0.join("input.json"), "{}")?;

    let output = ferrule(
        &dir.0,
        &[
            "run",
            "--project",
            project.to_str().ok_or("non-UTF-8 project path")?,
            "--trace-json",
            "output.json",
        ],
    )?;
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("is reserved by the host"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !dir.0.join("output.json").exists(),
        "collision must fail before publishing the mapping output"
    );
    Ok(())
}
