//! A file run releases unselected intermediates, while preview still renders
//! every stage target; both must agree on the targets a file run publishes.

use std::collections::BTreeMap;
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use cli::{PayloadDocument, PipelineHostPayload, PipelinePreviewOptions};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FormatOptions, Graph, NamedSource, NamedTarget, Node, Pipeline, PipelineInput,
    PipelineNamedInput, PipelineStage, Project, Scope, ScopeConstruction,
};

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_pipeline_release_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn json() -> FormatOptions {
    FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    }
}

fn record() -> SchemaNode {
    SchemaNode::group(
        "Record",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
}

fn bind(node: u32) -> Scope {
    Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node,
        }],
        ..Scope::default()
    }
}

fn copy_project(id: &str) -> Project {
    Project {
        source: record(),
        target: record(),
        source_path: None,
        target_path: Some(format!("{id}.json")),
        source_options: json(),
        target_options: json(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph::default(),
        root: Scope {
            construction: ScopeConstruction::CopyCurrentSource,
            ..Scope::default()
        },
    }
}

/// Concatenates the primary `Value` with one named source's `Value`.
fn concat_project(id: &str, named: &str) -> Project {
    let mut project = copy_project(id);
    project.extra_sources.push(NamedSource {
        name: named.into(),
        path: String::new(),
        schema: record(),
        options: json(),
        dynamic_path: None,
    });
    project.graph.nodes = BTreeMap::from([
        (
            0,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: None,
            },
        ),
        (
            1,
            Node::SourceField {
                path: vec![named.into(), "Value".into()],
                frame: None,
            },
        ),
        (
            2,
            Node::Call {
                function: "concat".into(),
                args: vec![0, 1],
            },
        ),
    ]);
    project.root = bind(2);
    project
}

fn output(stage: &str, target: Option<&str>) -> PipelineInput {
    PipelineInput::StageTarget {
        stage: stage.into(),
        target: target.map(str::to_owned),
    }
}

fn stage(
    id: &str,
    project: Project,
    source: PipelineInput,
    named: Option<(&str, PipelineInput)>,
) -> PipelineStage {
    PipelineStage {
        id: id.into(),
        mapping_path: None,
        project,
        source,
        extra_sources: named
            .into_iter()
            .map(|(name, from)| PipelineNamedInput {
                name: name.into(),
                from,
            })
            .collect(),
    }
}

/// `prepare` fans out to `left` and `right`; `right` reads it twice; `final`
/// joins both branches; `left` also reads the selected `prepare.audit`.
fn fan_out_pipeline() -> Pipeline {
    let mut prepare = copy_project("prepare");
    prepare.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("audit".into()),
        },
    );
    prepare.extra_targets.push(NamedTarget {
        name: "audit".into(),
        path: Some("audit.json".into()),
        schema: record(),
        options: json(),
        root: bind(0),
    });
    Pipeline {
        main_mapping_path: None,
        stages: vec![
            stage(
                "final",
                concat_project("final", "other"),
                output("left", None),
                Some(("other", output("right", None))),
            ),
            stage(
                "prepare",
                prepare,
                PipelineInput::Host {
                    name: "orders".into(),
                },
                None,
            ),
            stage(
                "left",
                concat_project("left", "aside"),
                output("prepare", None),
                Some(("aside", output("prepare", Some("audit")))),
            ),
            stage(
                "right",
                concat_project("right", "again"),
                output("prepare", None),
                Some(("again", output("prepare", None))),
            ),
        ],
    }
}

fn file_output(stage: &str, target: Option<&str>, path: &Path) -> cli::PipelineOutputFile {
    cli::PipelineOutputFile {
        stage: stage.into(),
        target: target.map(str::to_owned),
        path: path.to_path_buf(),
    }
}

#[test]
fn file_run_publishes_the_bytes_preview_renders_while_preview_keeps_every_intermediate()
-> Result<(), Box<dyn Error>> {
    let dir = TempDir::new()?;
    let pipeline = fan_out_pipeline();
    let pipeline_path = dir.0.join("pipeline.json");
    std::fs::write(&pipeline_path, serde_json::to_vec_pretty(&pipeline)?)?;
    let input = br#"{"Value":"o"}"#;
    let input_path = dir.0.join("orders.json");
    std::fs::write(&input_path, input)?;
    let final_path = dir.0.join("final.json");
    let audit_path = dir.0.join("audit.json");

    let outcome = cli::run_pipeline_file(
        &pipeline_path,
        &[cli::PipelineHostFile {
            name: "orders".into(),
            path: input_path.clone(),
        }],
        &[
            file_output("final", None, &final_path),
            file_output("prepare", Some("audit"), &audit_path),
        ],
    )?;
    assert_eq!(
        outcome.stages_executed,
        ["prepare", "left", "right", "final"]
    );
    assert_eq!(
        outcome
            .artifacts
            .iter()
            .map(|artifact| (artifact.stage.as_str(), artifact.target.as_deref()))
            .collect::<Vec<_>>(),
        [("final", None), ("prepare", Some("audit"))]
    );

    let hosts = [PipelineHostPayload::new(
        "orders",
        PayloadDocument::new(&input_path, input)?,
    )?];
    let preview = cli::preview_pipeline_value_payloads(
        &pipeline,
        &pipeline_path,
        &PipelinePreviewOptions::new(&hosts),
    )?;
    assert_eq!(preview.stages_executed, outcome.stages_executed);
    // Preview intentionally retains consumed and unselected intermediates.
    let rendered = preview
        .artifacts
        .iter()
        .map(|artifact| {
            let value: serde_json::Value = serde_json::from_slice(&artifact.bytes)?;
            Ok((
                (artifact.stage.as_str(), artifact.target.as_deref()),
                (value["Value"].clone(), artifact.bytes.as_slice()),
            ))
        })
        .collect::<Result<BTreeMap<_, _>, serde_json::Error>>()?;
    let values = rendered
        .iter()
        .map(|(key, (value, _))| (*key, value.as_str().unwrap_or_default()))
        .collect::<Vec<_>>();
    assert_eq!(
        values,
        [
            (("final", None), "oauditoo"),
            (("left", None), "oaudit"),
            (("prepare", None), "o"),
            (("prepare", Some("audit")), "audit"),
            (("right", None), "oo"),
        ]
    );
    assert_eq!(std::fs::read(&final_path)?, rendered[&("final", None)].1);
    assert_eq!(
        std::fs::read(&audit_path)?,
        rendered[&("prepare", Some("audit"))].1
    );
    Ok(())
}
