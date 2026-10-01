use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use cli::{PayloadDocument, PayloadRunOptions, PipelineHostPayload, PipelinePreviewOptions};
use engine::{DebugDecision, ExecutionPurpose};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, CsvTextRepairCause, CsvTextRepairDependency, FormatOptions, Graph, NamedSource,
    NamedTarget, Node, Pipeline, PipelineInput, PipelineNamedInput, PipelineStage, Project, Scope,
    ScopeIteration, TabularBoundaryKind,
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_cli_csv_repair_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
fn dependency() -> CsvTextRepairDependency {
    CsvTextRepairDependency::new(CsvTextRepairCause::Encoding)
}
fn schema() -> SchemaNode {
    SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
}
fn options() -> FormatOptions {
    FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Csv),
        has_header_row: Some(false),
        ..Default::default()
    }
}
fn project() -> Project {
    Project {
        source: schema(),
        target: schema(),
        source_path: Some("input.csv".into()),
        target_path: Some("output.csv".into()),
        source_options: options(),
        target_options: options(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Value".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            iteration: ScopeIteration::Source(Vec::new()),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 0,
            }],
            ..Default::default()
        },
    }
}
fn assert_dependency(error: &anyhow::Error) {
    assert!(error.chain().any(|cause| matches!(cause.downcast_ref::<format_csv::CsvFormatError>(), Some(format_csv::CsvFormatError::RepairDependency(d)) if *d == dependency())), "{error:#}");
}
fn mark(project: &mut Project, boundary: usize) {
    match boundary {
        0 => project.source_options.csv_text_repair_dependency = Some(dependency()),
        1 => project.target_options.csv_text_repair_dependency = Some(dependency()),
        2 => project.extra_sources.push(NamedSource {
            name: "lookup".into(),
            path: "missing.json".into(),
            schema: schema(),
            options: FormatOptions {
                csv_text_repair_dependency: Some(dependency()),
                ..options()
            },
            dynamic_path: None,
        }),
        3 => project.extra_targets.push(NamedTarget {
            name: "audit".into(),
            path: Some("audit.json".into()),
            schema: schema(),
            options: FormatOptions {
                csv_text_repair_dependency: Some(dependency()),
                ..options()
            },
            root: project.root.clone(),
        }),
        _ => unreachable!(),
    }
}

#[test]
fn saved_repairs_block_file_and_payload_paths_in_run_and_preview() {
    let temp = TempDir::new();
    let input = temp.0.join("input.csv");
    std::fs::write(&input, b"data\n").unwrap();
    let renamed = temp.0.join("renamed.json");
    std::fs::write(&renamed, b"not JSON").unwrap();
    for boundary in 0..4 {
        let mut project = project();
        mark(&mut project, boundary);
        let saved = temp.0.join(format!("project-{boundary}.json"));
        std::fs::write(
            &saved,
            mapping::project_file::encode_pretty(&project).unwrap(),
        )
        .unwrap();
        let project =
            mapping::project_file::decode_str(&std::fs::read_to_string(&saved).unwrap()).unwrap();
        assert!(engine::validate(&project).is_empty());
        let typed = Instance::Repeated(vec![Instance::Group(vec![(
            "Value".into(),
            Instance::Scalar(Value::String("data".into())),
        )])]);
        assert!(
            engine::run(&project, &typed).is_ok(),
            "typed input does not perform CSV byte I/O"
        );
        let output = temp.0.join("sentinel.csv");
        std::fs::write(&output, b"sentinel").unwrap();
        let audit = temp.0.join("audit.json");
        std::fs::write(&audit, b"audit sentinel").unwrap();
        let actual_input = if boundary == 0 { &renamed } else { &input };
        assert_dependency(
            &cli::run_project_with_paths(&saved, Some(actual_input), Some(&output)).unwrap_err(),
        );
        assert_eq!(std::fs::read(&output).unwrap(), b"sentinel");
        assert_eq!(std::fs::read(&audit).unwrap(), b"audit sentinel");
        for purpose in [ExecutionPurpose::Run, ExecutionPurpose::Preview] {
            let primary = PayloadDocument::new(
                if boundary == 0 {
                    Path::new("renamed.json")
                } else {
                    Path::new("input.csv")
                },
                b"data\n",
            )
            .unwrap();
            let named_document =
                PayloadDocument::new(Path::new("renamed.xml"), b"not XML").unwrap();
            let named = [cli::NamedPayloadInput::new("lookup", named_document).unwrap()];
            let options = PayloadRunOptions::new(primary)
                .with_output_path(&output)
                .with_execution_purpose(purpose);
            let options = if boundary == 2 {
                options.with_extra_sources(&named)
            } else {
                options
            };
            assert_dependency(
                &cli::run_project_value_payloads(&project, &saved, &options).unwrap_err(),
            );
            assert_eq!(std::fs::read(&output).unwrap(), b"sentinel");
            assert_eq!(std::fs::read(&audit).unwrap(), b"audit sentinel");
        }
    }
}

fn pipeline(project: Project) -> Pipeline {
    let named = if project.extra_sources.is_empty() {
        Vec::new()
    } else {
        vec![PipelineNamedInput {
            name: "lookup".into(),
            from: PipelineInput::Host {
                name: "lookup-input".into(),
            },
        }]
    };
    Pipeline {
        main_mapping_path: None,
        stages: vec![PipelineStage {
            id: "one".into(),
            mapping_path: None,
            project,
            source: PipelineInput::Host {
                name: "input".into(),
            },
            extra_sources: named,
        }],
    }
}

#[test]
fn pipeline_preflight_blocks_actual_csv_boundaries_before_any_callbacks_or_files() {
    let temp = TempDir::new();
    let path = temp.0.join("pipeline.json");
    for boundary in 0..4 {
        let mut project = project();
        mark(&mut project, boundary);
        if boundary == 0 {
            project.source_path = Some("renamed.xml".into());
        }
        if boundary == 1 {
            project.target_path = Some("renamed.json".into());
        }
        let pipeline = pipeline(project);
        let primary = PipelineHostPayload::new(
            "input",
            PayloadDocument::new(
                if boundary == 0 {
                    Path::new("renamed.xml")
                } else {
                    Path::new("input.csv")
                },
                b"data\n",
            )
            .unwrap(),
        )
        .unwrap();
        let lookup = PipelineHostPayload::new(
            "lookup-input",
            PayloadDocument::new(Path::new("renamed.json"), b"not JSON").unwrap(),
        )
        .unwrap();
        let hosts = if boundary == 2 {
            vec![primary, lookup]
        } else {
            vec![primary]
        };
        let callbacks = Cell::new(0_usize);
        let debug = |_: &str, _: &engine::PendingTargetWrite| {
            callbacks.set(callbacks.get() + 1);
            DebugDecision::Resume
        };
        let trace = |_: &str, _: engine::TraceEvent| {
            callbacks.set(callbacks.get() + 1);
        };
        let options = PipelinePreviewOptions::new(&hosts)
            .with_stage_debug_hook(&debug)
            .with_stage_trace_sink(&trace);
        assert_dependency(&cli::validate_pipeline_preview(&pipeline, &path, &options).unwrap_err());
        assert_dependency(
            &cli::preview_pipeline_value_payloads(&pipeline, &path, &options).unwrap_err(),
        );
        assert_eq!(callbacks.get(), 0);
        assert!(!temp.0.join("output.csv").exists());
        assert!(!temp.0.join("audit.json").exists());
        // Filesystem pipeline execution shares the same configured decoder.
        let saved = mapping::pipeline_file::encode_pretty(&pipeline).unwrap();
        std::fs::write(&path, saved).unwrap();
        let primary_path = temp.0.join(if boundary == 0 {
            "host-renamed.json"
        } else {
            "host.csv"
        });
        std::fs::write(&primary_path, b"data\n").unwrap();
        let mut file_inputs = vec![cli::PipelineHostFile {
            name: "input".into(),
            path: primary_path,
        }];
        if boundary == 2 {
            file_inputs.push(cli::PipelineHostFile {
                name: "lookup-input".into(),
                path: temp.0.join("missing.json"),
            });
        }
        let output = temp.0.join(if boundary == 1 {
            "sentinel-renamed.json"
        } else {
            "sentinel.csv"
        });
        std::fs::write(&output, b"sentinel").unwrap();
        let mut file_outputs = vec![cli::PipelineOutputFile {
            stage: "one".into(),
            target: None,
            path: output.clone(),
        }];
        if boundary == 3 {
            file_outputs.push(cli::PipelineOutputFile {
                stage: "one".into(),
                target: Some("audit".into()),
                path: temp.0.join("audit.csv"),
            });
        }
        assert_dependency(&cli::run_pipeline_file(&path, &file_inputs, &file_outputs).unwrap_err());
        assert_eq!(std::fs::read(&output).unwrap(), b"sentinel");
        assert!(!temp.0.join("audit.csv").exists());
    }
}

#[test]
fn ordinary_csv_payload_and_pipeline_preview_still_render_identical_bytes() {
    let project = project();
    let input = PayloadDocument::new(Path::new("input.csv"), b"data\n").unwrap();
    let single = cli::run_project_value_payloads(
        &project,
        Path::new("/logical/project.json"),
        &PayloadRunOptions::new(input),
    )
    .unwrap();
    let hosts = [PipelineHostPayload::new("input", input).unwrap()];
    let result = cli::preview_pipeline_value_payloads(
        &pipeline(project),
        Path::new("/logical/pipeline.json"),
        &PipelinePreviewOptions::new(&hosts),
    )
    .unwrap();
    assert_eq!(single.artifacts[0].bytes, b"data\n");
    assert_eq!(result.artifacts[0].bytes, single.artifacts[0].bytes);
}

#[test]
fn dynamic_csv_target_repair_rejects_during_metadata_preflight() {
    let temp = TempDir::new();
    let mut project = project();
    project.target_path = None;
    project.target_options.csv_text_repair_dependency = Some(dependency());
    project.graph.nodes.insert(
        1,
        Node::Const {
            value: Value::String("result.csv".into()),
        },
    );
    assert!(project.root.set_output_path(Some(1)));
    let pipeline = pipeline(project);
    let findings = engine::validate_pipeline(&pipeline);
    assert!(findings.is_empty(), "{findings:?}");
    let hosts = [PipelineHostPayload::new(
        "input",
        PayloadDocument::new(Path::new("input.csv"), b"data\n").unwrap(),
    )
    .unwrap()];
    let count = Cell::new(0_usize);
    let debug = |_: &str, _: &engine::PendingTargetWrite| {
        count.set(count.get() + 1);
        DebugDecision::Resume
    };
    let identities = [cli::PipelinePreviewOutputIdentity {
        stage: "one".into(),
        target: None,
        path: temp.0.join("uncreated"),
    }];
    let options = PipelinePreviewOptions::new(&hosts)
        .with_output_identities(&identities)
        .with_stage_debug_hook(&debug);
    assert_dependency(
        &cli::validate_pipeline_preview(&pipeline, Path::new("/logical/pipeline.json"), &options)
            .unwrap_err(),
    );
    assert_dependency(
        &cli::preview_pipeline_value_payloads(
            &pipeline,
            Path::new("/logical/pipeline.json"),
            &options,
        )
        .unwrap_err(),
    );
    assert_eq!(count.get(), 0);
    assert!(!temp.0.join("uncreated").exists());
}
