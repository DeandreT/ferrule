use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use engine::{DebugDecision, EngineError, PendingTargetWrite};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, Node, Pipeline, PipelineInput, PipelineStage, Project, Scope};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-pipeline-debug-{}-{}",
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

fn project(value: &str) -> Project {
    let schema = SchemaNode::group(
        "Record",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    let mut project = Project {
        source: schema.clone(),
        target: schema,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph::default(),
        root: Scope::default(),
    };
    project.source_options.json_document = true;
    project.target_options.json_document = true;
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String(value.into()),
        },
    );
    project.root.bindings.push(Binding {
        target_field: "Value".into(),
        node: 0,
    });
    project
}

fn prepare(
    directory: &TempDir,
) -> anyhow::Result<(
    PathBuf,
    Vec<cli::PipelineHostFile>,
    Vec<cli::PipelineOutputFile>,
)> {
    let pipeline = Pipeline {
        main_mapping_path: None,
        stages: vec![
            PipelineStage {
                id: "prepare".into(),
                mapping_path: None,
                project: project("A"),
                source: PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: Vec::new(),
            },
            PipelineStage {
                id: "finish".into(),
                mapping_path: None,
                project: project("B"),
                source: PipelineInput::StageTarget {
                    stage: "prepare".into(),
                    target: None,
                },
                extra_sources: Vec::new(),
            },
        ],
    };
    let pipeline_path = directory.path("pipeline.json");
    std::fs::write(&pipeline_path, serde_json::to_vec(&pipeline)?)?;
    let input_path = directory.path("input.json");
    std::fs::write(&input_path, r#"{"Value":"source"}"#)?;
    let outputs = ["prepare", "finish"]
        .into_iter()
        .map(|stage| cli::PipelineOutputFile {
            stage: stage.into(),
            target: None,
            path: directory.path(&format!("{stage}.json")),
        })
        .collect::<Vec<_>>();
    for output in &outputs {
        std::fs::write(&output.path, "old output")?;
    }
    Ok((
        pipeline_path,
        vec![cli::PipelineHostFile {
            name: "input".into(),
            path: input_path,
        }],
        outputs,
    ))
}

fn assert_old_outputs(outputs: &[cli::PipelineOutputFile]) -> std::io::Result<()> {
    for output in outputs {
        assert_eq!(std::fs::read_to_string(&output.path)?, "old output");
    }
    Ok(())
}

#[test]
fn stage_debug_hook_cancels_later_stage_without_publishing() -> anyhow::Result<()> {
    let directory = TempDir::new()?;
    let (pipeline, inputs, outputs) = prepare(&directory)?;
    let seen = RefCell::new(Vec::<String>::new());
    let hook = |stage: &str, write: &PendingTargetWrite| {
        assert_eq!(write.field, "Value");
        seen.borrow_mut().push(stage.into());
        if stage == "finish" {
            DebugDecision::Cancel
        } else {
            DebugDecision::Resume
        }
    };
    let options = cli::PipelineRunOptions::default().with_stage_debug_hook(&hook);
    let error =
        cli::run_pipeline_file_with_options(&pipeline, &inputs, &outputs, &options).unwrap_err();
    assert!(error.chain().any(|cause| matches!(
        cause.downcast_ref::<EngineError>(),
        Some(EngineError::DebugCancelled)
    )));
    assert_eq!(*seen.borrow(), ["prepare", "finish"]);
    assert_old_outputs(&outputs)?;
    Ok(())
}

#[test]
fn pipeline_gate_cancels_after_all_stages_before_any_staging() -> anyhow::Result<()> {
    let directory = TempDir::new()?;
    let (pipeline, inputs, outputs) = prepare(&directory)?;
    let calls = Cell::new(0usize);
    let gate = || {
        calls.set(calls.get() + 1);
        false
    };
    let options = cli::PipelineRunOptions::default().with_before_publish(&gate);
    let error =
        cli::run_pipeline_file_with_options(&pipeline, &inputs, &outputs, &options).unwrap_err();
    assert!(matches!(
        error.downcast_ref::<EngineError>(),
        Some(EngineError::DebugCancelled)
    ));
    assert_eq!(calls.get(), 1);
    assert_old_outputs(&outputs)?;
    Ok(())
}

#[test]
fn approved_pipeline_gate_publishes_all_selected_stages() -> anyhow::Result<()> {
    let directory = TempDir::new()?;
    let (pipeline, inputs, outputs) = prepare(&directory)?;
    let calls = Cell::new(0usize);
    let gate = || {
        calls.set(calls.get() + 1);
        true
    };
    let options = cli::PipelineRunOptions::default().with_before_publish(&gate);
    let outcome = cli::run_pipeline_file_with_options(&pipeline, &inputs, &outputs, &options)?;
    assert_eq!(calls.get(), 1);
    assert_eq!(outcome.stages_executed, ["prepare", "finish"]);
    assert_eq!(outcome.artifacts.len(), 2);
    assert!(std::fs::read_to_string(&outputs[0].path)?.contains("\"A\""));
    assert!(std::fs::read_to_string(&outputs[1].path)?.contains("\"B\""));
    Ok(())
}
