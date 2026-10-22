//! Run, Debug Run and pipeline runs refuse an unsupported route before they
//! save the project, open a Save As dialog, start a worker or read an input.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, SchemaNode};
use mapping::{
    Binding, FunctionId, Graph, Node, OwnedSourceField, OwnedSourceRef, Project, Scope,
    SourceOwner, UserFunction,
};

use super::*;
use crate::app::CanvasDocumentState;
use crate::document::DocumentLocation;

struct TestDir(PathBuf);

impl TestDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-gui-route-preflight-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir(&path)?;
        Ok(Self(path))
    }

    /// Every entry beneath the directory, with file contents.
    fn snapshot(&self) -> std::io::Result<BTreeMap<PathBuf, Option<Vec<u8>>>> {
        fn walk(
            directory: &Path,
            entries: &mut BTreeMap<PathBuf, Option<Vec<u8>>>,
        ) -> std::io::Result<()> {
            for entry in std::fs::read_dir(directory)? {
                let path = entry?.path();
                if path.is_dir() {
                    entries.insert(path.clone(), None);
                    walk(&path, entries)?;
                } else {
                    entries.insert(path.clone(), Some(std::fs::read(&path)?));
                }
            }
            Ok(())
        }
        let mut entries = BTreeMap::new();
        walk(&self.0, &mut entries)?;
        Ok(entries)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A valid design whose owned source field sits in a reusable function, which
/// owned-source admission refuses whatever observers a host requests.
fn closed_owned_source_project() -> Project {
    let record =
        |name: &str| SchemaNode::group(name, vec![SchemaNode::scalar("Value", ScalarType::String)]);
    let function = FunctionId::new(7);
    let owned = Node::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: vec!["Value".into()],
        },
        frame: None,
    });
    Project {
        source: record("Input"),
        target: record("Output"),
        source_path: Some("input.json".into()),
        target_path: Some("output.json".into()),
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: [(
            function,
            UserFunction {
                library: "tests".into(),
                name: "closed_owned_value".into(),
                description: None,
                parameters: Vec::new(),
                output_name: "value".into(),
                output_type: ScalarType::String,
                body: Graph {
                    nodes: [(9, owned)].into(),
                },
                output: 9,
            },
        )]
        .into(),
        graph: Graph {
            nodes: [(
                9,
                Node::UserFunctionCall {
                    function,
                    args: Vec::new(),
                },
            )]
            .into(),
        },
        root: Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 9,
            }],
            ..Scope::default()
        },
    }
}

fn assert_owned_refusal(app: &FerruleApp) {
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("source ownership")),
        "{:?}",
        app.diagnostics.items()
    );
}

#[test]
fn run_refusal_precedes_saving_and_input_acquisition() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let project_path = temp.0.join("mapping.json");
    let mut app = FerruleApp {
        project: closed_owned_source_project(),
        ..FerruleApp::default()
    };
    assert!(cli::validate(&app.project).is_empty());
    app.main_canvas = CanvasDocumentState::main(&app.project);
    app.save_document_to(&project_path)?;
    app.project.graph.nodes.insert(
        20,
        Node::Const {
            value: ir::Value::String("unsaved edit".into()),
        },
    );
    app.observe_editor_history(std::time::Instant::now(), false);
    assert!(app.is_dirty());
    // A directory stands where the input is expected: reading it would fail
    // with an I/O error instead of the capability refusal.
    let input = temp.0.join("input.json");
    std::fs::create_dir(&input)?;
    app.input_path = input.display().to_string();
    app.output_path = temp.0.join("output.json").display().to_string();
    let before = temp.snapshot()?;

    let context = egui::Context::default();
    for debug in [false, true] {
        if debug {
            app.debug_run(&context);
        } else {
            app.run(&context);
        }
        assert_eq!(
            app.status,
            if debug {
                "debug run blocked"
            } else {
                "run blocked"
            }
        );
        assert_owned_refusal(&app);
        assert!(app.pending_file_run.is_none());
        assert!(app.pending_dialog.is_none());
        assert!(app.is_dirty(), "the project was not saved");
        assert_eq!(temp.snapshot()?, before);
    }
    Ok(())
}

#[test]
fn unsaved_run_without_an_input_is_refused_before_save_as() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let mut app = FerruleApp {
        document: DocumentLocation::untitled(temp.0.join("untitled.json")),
        ..FerruleApp::default()
    };
    app.project.source_path = None;
    assert!(cli::validate(&app.project).is_empty());

    app.run(&egui::Context::default());
    assert_eq!(app.status, "run blocked");
    assert!(
        app.diagnostics
            .items()
            .iter()
            .any(|item| item.message.contains("no input path is configured")),
        "{:?}",
        app.diagnostics.items()
    );
    assert!(app.pending_dialog.is_none(), "no Save As dialog opens");
    assert_eq!(app.pending_save_continuation, None);
    assert!(app.pending_file_run.is_none());
    assert!(temp.snapshot()?.is_empty());
    Ok(())
}

#[test]
fn pipeline_run_refusal_precedes_host_input_acquisition() -> anyhow::Result<()> {
    let temp = TestDir::new()?;
    let pipeline_path = temp.0.join("pipeline.json");
    let mut project = closed_owned_source_project();
    project.source_options.json_document = true;
    project.target_options.json_document = true;
    let pipeline = mapping::Pipeline {
        main_mapping_path: None,
        stages: vec![mapping::PipelineStage {
            id: "owned".into(),
            mapping_path: None,
            project,
            source: mapping::PipelineInput::Host {
                name: "orders".into(),
            },
            extra_sources: Vec::new(),
        }],
    };
    std::fs::write(&pipeline_path, serde_json::to_vec_pretty(&pipeline)?)?;
    std::fs::create_dir(temp.0.join("orders.json"))?;
    let mut app = FerruleApp::default();
    app.load_pipeline_for_run(&pipeline_path);
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    assert!(draft.issues.is_empty(), "{:?}", draft.issues);
    draft.inputs[0].path = "orders.json".into();
    draft.outputs[0].path = "result.json".into();
    draft.outputs[0].preview_path = "result.json".into();
    let before = temp.snapshot()?;

    app.start_pipeline_run();
    assert_eq!(app.status, "pipeline run blocked");
    assert_owned_refusal(&app);
    assert!(app.pending_pipeline_run.is_none(), "no worker started");
    app.start_pipeline_preview();
    assert_eq!(app.status, "pipeline preview blocked");
    assert!(app.pending_pipeline_run.is_none(), "no worker started");
    assert_eq!(temp.snapshot()?, before);

    // The preflight declares the observers every GUI pipeline run attaches.
    let (inputs, outputs) = app
        .pipeline_run_draft
        .as_ref()
        .expect("draft remains open")
        .requests()?;
    let cancelled = AtomicBool::new(false);
    let error = pipeline_run(
        &pipeline_path,
        &inputs,
        &outputs,
        &engine::RuntimeParameters::new(),
        &cancelled,
    )
    .unwrap_err();
    assert!(
        error.chain().any(|cause| matches!(
            cause.downcast_ref::<engine::EngineError>(),
            Some(engine::EngineError::UnsupportedSourceOwnership { .. })
        )),
        "{error:#}"
    );
    Ok(())
}
