use std::cell::Cell;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use engine::{DebugDecision, DebugHook, EngineError, PendingTargetWrite};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, NamedTarget, Node, Project, Scope};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-file-debug-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn project() -> Project {
    let target = SchemaNode::group(
        "root",
        vec![
            SchemaNode::scalar("first", ScalarType::String),
            SchemaNode::scalar("second", ScalarType::String),
        ],
    );
    Project {
        source: SchemaNode::group("root", Vec::new()),
        target,
        source_path: None,
        target_path: Some("primary.xml".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: vec![NamedTarget {
            name: "audit".into(),
            path: Some("audit.xml".into()),
            schema: SchemaNode::group(
                "audit",
                vec![SchemaNode::scalar("first", ScalarType::String)],
            ),
            options: Default::default(),
            root: Scope {
                bindings: vec![Binding {
                    target_field: "first".into(),
                    node: 0,
                }],
                ..Scope::default()
            },
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::String("A".into()),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("B".into()),
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "first".into(),
                    node: 0,
                },
                Binding {
                    target_field: "second".into(),
                    node: 1,
                },
            ],
            ..Scope::default()
        },
    }
}

fn prepare(directory: &TempDir) -> anyhow::Result<(PathBuf, PathBuf)> {
    let input = directory.path("input.xml");
    let mapping = directory.path("project.json");
    std::fs::write(&input, "<root/>")?;
    std::fs::write(&mapping, serde_json::to_vec(&project())?)?;
    std::fs::write(directory.path("primary.xml"), "old primary")?;
    std::fs::write(directory.path("audit.xml"), "old audit")?;
    Ok((mapping, input))
}

struct CancelAtWrite;

impl DebugHook for CancelAtWrite {
    fn before_target_write(&self, _: &PendingTargetWrite) -> DebugDecision {
        DebugDecision::Cancel
    }
}

fn assert_old_outputs(directory: &TempDir) -> std::io::Result<()> {
    assert_eq!(
        std::fs::read_to_string(directory.path("primary.xml"))?,
        "old primary"
    );
    assert_eq!(
        std::fs::read_to_string(directory.path("audit.xml"))?,
        "old audit"
    );
    assert!(
        std::fs::read_dir(&directory.0)?
            .filter_map(Result::ok)
            .all(|entry| !entry
                .file_name()
                .to_string_lossy()
                .starts_with(".ferrule-stage-"))
    );
    Ok(())
}

#[test]
fn debug_hook_cancel_keeps_primary_and_named_output_files() -> anyhow::Result<()> {
    let directory = TempDir::new()?;
    let (mapping, input) = prepare(&directory)?;
    let hook = CancelAtWrite;
    let options = cli::RunOptions::new()
        .with_input_path(&input)
        .with_debug_hook(&hook);
    let error = cli::run_project_with_options(&mapping, &options).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<EngineError>(),
        Some(EngineError::DebugCancelled)
    ));
    assert_old_outputs(&directory)?;
    Ok(())
}

#[test]
fn before_publish_gate_cancels_after_evaluation_before_staging() -> anyhow::Result<()> {
    let directory = TempDir::new()?;
    let (mapping, input) = prepare(&directory)?;
    let calls = Cell::new(0usize);
    let deny = || {
        calls.set(calls.get() + 1);
        false
    };
    let options = cli::RunOptions::new()
        .with_input_path(&input)
        .with_before_publish(&deny);
    let error = cli::run_project_with_options(&mapping, &options).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<EngineError>(),
        Some(EngineError::DebugCancelled)
    ));
    assert_eq!(calls.get(), 1);
    assert_old_outputs(&directory)?;
    Ok(())
}

#[test]
fn approved_gate_publishes_primary_and_named_targets() -> anyhow::Result<()> {
    let directory = TempDir::new()?;
    let (mapping, input) = prepare(&directory)?;
    let calls = Cell::new(0usize);
    let approve = || {
        calls.set(calls.get() + 1);
        true
    };
    let options = cli::RunOptions::new()
        .with_input_path(&input)
        .with_before_publish(&approve);
    let outcome = cli::run_project_with_options(&mapping, &options)?;
    assert_eq!(calls.get(), 1);
    assert_eq!(outcome.artifacts.len(), 2);
    assert!(std::fs::read_to_string(directory.path("primary.xml"))?.contains("<first>A</first>"));
    assert!(std::fs::read_to_string(directory.path("audit.xml"))?.contains("<first>A</first>"));
    Ok(())
}
