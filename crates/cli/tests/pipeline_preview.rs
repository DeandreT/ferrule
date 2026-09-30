use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use cli::{
    PayloadDocument, PipelineHostPayload, PipelinePreviewOptions, PipelinePreviewOutputIdentity,
    preview_pipeline_value_payloads, validate_pipeline_preview,
};
use engine::{DebugDecision, EngineError, ExecutionContext, PipelineError, RuntimeParameters};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    Binding, DynamicSourcePath, EdiBoundaryKind, FormatOptions, Graph, NamedSource, NamedTarget,
    Node, Pipeline, PipelineInput, PipelineNamedInput, PipelineStage, Project, RuntimeValue, Scope,
    ScopeConstruction, ScopeIteration,
};

struct TempDir(PathBuf);
impl TempDir {
    fn new() -> std::io::Result<Self> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_pipeline_preview_{}_{}",
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

fn json_options() -> FormatOptions {
    FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    }
}
fn value_schema() -> SchemaNode {
    SchemaNode::group(
        "Record",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
}
fn copy_project() -> Project {
    Project {
        source: value_schema(),
        target: value_schema(),
        source_path: None,
        target_path: Some("result.json".into()),
        source_options: json_options(),
        target_options: json_options(),
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
fn single(project: Project) -> Pipeline {
    Pipeline {
        main_mapping_path: None,
        stages: vec![PipelineStage {
            id: "one".into(),
            mapping_path: None,
            project,
            source: PipelineInput::Host {
                name: "input".into(),
            },
            extra_sources: Vec::new(),
        }],
    }
}
fn host<'a>(
    name: &'a str,
    path: &'a Path,
    bytes: &'a [u8],
) -> anyhow::Result<PipelineHostPayload<'a>> {
    PipelineHostPayload::new(name, PayloadDocument::new(path, bytes)?)
}
fn bindings(value: u32, choice: u32) -> Vec<Binding> {
    [
        ("Value", value),
        ("Choice", choice),
        ("Clock", 3),
        ("Active", 4),
        ("Main", 5),
    ]
    .into_iter()
    .map(|(field, node)| Binding {
        target_field: field.into(),
        node,
    })
    .collect()
}
fn preview_project(preview: &str) -> Project {
    let mut project = copy_project();
    project.target = SchemaNode::group(
        "Record",
        ["Value", "Choice", "Clock", "Active", "Main"]
            .into_iter()
            .map(|name| SchemaNode::scalar(name, ScalarType::String))
            .collect(),
    );
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
            Node::Const {
                value: Value::String("fallback".into()),
            },
        ),
        (
            2,
            Node::RuntimeParameterDefault {
                name: "choice".into(),
                ty: ScalarType::String,
                default: 1,
                preview: Some(preview.into()),
            },
        ),
        (
            3,
            Node::RuntimeValue {
                value: RuntimeValue::CurrentDateTime,
            },
        ),
        (
            4,
            Node::RuntimeValue {
                value: RuntimeValue::MappingFilePath,
            },
        ),
        (
            5,
            Node::RuntimeValue {
                value: RuntimeValue::MainMappingFilePath,
            },
        ),
    ]);
    project.root = Scope {
        bindings: bindings(0, 2),
        ..Scope::default()
    };
    project
}
fn preview_pipeline() -> Pipeline {
    let mut producer = preview_project("producer");
    producer.target_path = Some("maps/first.xml".into());
    producer.target_options = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };
    producer.extra_sources.push(NamedSource {
        name: "lookup".into(),
        path: "missing.xml".into(),
        schema: value_schema(),
        options: FormatOptions {
            xml_document: true,
            ..FormatOptions::default()
        },
        dynamic_path: None,
    });
    producer.graph.nodes.insert(
        6,
        Node::SourceField {
            path: vec!["lookup".into(), "Value".into()],
            frame: None,
        },
    );
    producer.graph.nodes.insert(
        7,
        Node::RuntimeParameter {
            name: "choice".into(),
            ty: ScalarType::String,
            preview: Some("named".into()),
        },
    );
    producer.extra_targets.push(NamedTarget {
        name: "selected".into(),
        path: Some("maps/named.xml".into()),
        schema: producer.target.clone(),
        options: producer.target_options.clone(),
        root: Scope {
            bindings: bindings(6, 7),
            ..Scope::default()
        },
    });
    let mut consumer = preview_project("consumer");
    consumer.source = producer.target.clone();
    consumer.target_path = Some("final.json".into());
    Pipeline {
        main_mapping_path: Some("main/design.mfd".into()),
        // The consumer comes first in storage, but depends on a named XML target.
        // Its JSON source options must not cause the edge to be reparsed.
        stages: vec![
            PipelineStage {
                id: "consume".into(),
                mapping_path: Some("maps/consumer.mfd".into()),
                project: consumer,
                source: PipelineInput::StageTarget {
                    stage: "prepare".into(),
                    target: Some("selected".into()),
                },
                extra_sources: Vec::new(),
            },
            PipelineStage {
                id: "prepare".into(),
                mapping_path: Some("maps/producer.mfd".into()),
                project: producer,
                source: PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: vec![PipelineNamedInput {
                    name: "lookup".into(),
                    from: PipelineInput::Host {
                        name: "catalog".into(),
                    },
                }],
            },
        ],
    }
}

#[test]
fn all_stage_targets_use_typed_edges_and_one_context_without_touching_paths() -> anyhow::Result<()>
{
    let dir = TempDir::new()?;
    let pipeline_path = dir.0.join("nonexistent/pipeline.json");
    let sentinel = dir.0.join("existing.xml");
    std::fs::write(&sentinel, "preserved")?;
    let hosts = [
        host(
            "input",
            Path::new("absent/input.json"),
            br#"{"Value":"primary"}"#,
        )?,
        host(
            "catalog",
            Path::new("absent/catalog.xml"),
            b"<Record><Value>lookup</Value></Record>",
        )?,
    ];
    let identities = [
        PipelinePreviewOutputIdentity {
            stage: "prepare".into(),
            target: Some("selected".into()),
            path: sentinel.clone(),
        },
        // Equal paths in different stages remain distinct logical artifacts.
        PipelinePreviewOutputIdentity {
            stage: "consume".into(),
            target: None,
            path: sentinel.clone(),
        },
    ];
    let writes = RefCell::new(Vec::new());
    let trace_stages = RefCell::new(Vec::new());
    let debug = |stage: &str, _: &engine::PendingTargetWrite| {
        writes.borrow_mut().push(stage.to_owned());
        DebugDecision::Resume
    };
    let trace = |stage: &str, _: engine::TraceEvent| {
        trace_stages.borrow_mut().push(stage.to_owned());
    };
    let pipeline = preview_pipeline();
    let outcome = preview_pipeline_value_payloads(
        &pipeline,
        &pipeline_path,
        &PipelinePreviewOptions::new(&hosts)
            .with_output_identities(&identities)
            .with_stage_debug_hook(&debug)
            .with_stage_trace_sink(&trace),
    )?;
    assert_eq!(outcome.stages_executed, ["prepare", "consume"]);
    assert_eq!(
        outcome
            .artifacts
            .iter()
            .map(|a| (a.stage.as_str(), a.target.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("prepare", None),
            ("prepare", Some("selected")),
            ("consume", None)
        ]
    );
    assert_eq!(
        outcome.artifacts[0].path,
        dir.0.join("nonexistent/maps/first.xml")
    );
    assert_eq!(outcome.artifacts[1].path, sentinel);
    assert_eq!(outcome.artifacts[2].path, sentinel);
    assert!(
        outcome
            .artifacts
            .iter()
            .all(|artifact| artifact.records_written == 1)
    );
    let first = format_xml::from_str(
        std::str::from_utf8(&outcome.artifacts[0].bytes)?,
        &pipeline.stages[1].project.target,
    )?;
    let named = format_xml::from_str(
        std::str::from_utf8(&outcome.artifacts[1].bytes)?,
        &pipeline.stages[1].project.target,
    )?;
    let first: serde_json::Value = serde_json::from_str(&format_json::to_string(
        &pipeline.stages[1].project.target,
        &first,
    )?)?;
    let named: serde_json::Value = serde_json::from_str(&format_json::to_string(
        &pipeline.stages[1].project.target,
        &named,
    )?)?;
    let last: serde_json::Value = serde_json::from_slice(&outcome.artifacts[2].bytes)?;
    assert_eq!(first["Value"], "primary");
    assert_eq!(named["Value"], "lookup");
    assert_eq!(last["Value"], "lookup");
    assert_eq!(first["Choice"], "producer");
    assert_eq!(named["Choice"], "named");
    assert_eq!(last["Choice"], "consumer");
    assert_eq!(first["Clock"], last["Clock"]);
    assert_eq!(named["Clock"], last["Clock"]);
    assert_eq!(first["Main"], last["Main"]);
    assert_eq!(
        last["Main"],
        pipeline_path
            .parent()
            .unwrap()
            .join("main/design.mfd")
            .to_str()
            .unwrap()
    );
    assert_eq!(
        first["Active"],
        pipeline_path
            .parent()
            .unwrap()
            .join("maps/producer.mfd")
            .to_str()
            .unwrap()
    );
    assert_eq!(
        last["Active"],
        pipeline_path
            .parent()
            .unwrap()
            .join("maps/consumer.mfd")
            .to_str()
            .unwrap()
    );
    assert_eq!(
        writes
            .borrow()
            .iter()
            .filter(|stage| *stage == "prepare")
            .count(),
        10
    );
    assert_eq!(
        writes
            .borrow()
            .iter()
            .filter(|stage| *stage == "consume")
            .count(),
        5
    );
    assert!(trace_stages.borrow().iter().any(|stage| stage == "prepare"));
    assert!(trace_stages.borrow().iter().any(|stage| stage == "consume"));
    assert_eq!(std::fs::read_to_string(&sentinel)?, "preserved");
    assert!(!dir.0.join("nonexistent").exists());
    Ok(())
}

#[test]
fn shared_host_values_including_null_override_each_nodes_preview() -> anyhow::Result<()> {
    let pipeline = preview_pipeline();
    let hosts = [
        host("input", Path::new("in.json"), br#"{"Value":"primary"}"#)?,
        host(
            "catalog",
            Path::new("catalog.xml"),
            b"<Record><Value>lookup</Value></Record>",
        )?,
    ];
    for value in [Value::String("supplied".into()), Value::Null] {
        let mut parameters = RuntimeParameters::new();
        parameters.insert("choice", value.clone())?;
        let output = preview_pipeline_value_payloads(
            &pipeline,
            Path::new("pipeline.json"),
            &PipelinePreviewOptions::new(&hosts).with_runtime_parameters(&parameters),
        )?;
        for artifact in &output.artifacts {
            let json: serde_json::Value = if artifact.stage == "prepare" {
                let instance = format_xml::from_str(
                    std::str::from_utf8(&artifact.bytes)?,
                    &pipeline.stages[1].project.target,
                )?;
                serde_json::from_str(&format_json::to_string(
                    &pipeline.stages[1].project.target,
                    &instance,
                )?)?
            } else {
                serde_json::from_slice(&artifact.bytes)?
            };
            if value == Value::Null {
                assert!(json.get("Choice").is_none());
            } else {
                assert_eq!(json["Choice"], "supplied");
            }
        }
    }
    // Normal interpretation still chooses connected defaults rather than previews.
    let project = preview_project("preview");
    let source = format_json::from_str(r#"{"Value":"run"}"#, &project.source)?;
    let context = ExecutionContext::new(Path::new("normal.mfd"))
        .with_current_datetime("2026-09-30T12:00:00Z");
    let output = engine::run_with_context(&project, &source, &context)?;
    let json: serde_json::Value =
        serde_json::from_str(&format_json::to_string(&project.target, &output)?)?;
    assert_eq!(json["Choice"], "fallback");
    Ok(())
}

#[test]
fn late_stage_failure_returns_no_outcome_and_preserves_files() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let sentinel = dir.0.join("result.json");
    std::fs::write(&sentinel, "preserved")?;
    let mut pipeline = preview_pipeline();
    pipeline.stages[0].project.graph.nodes.insert(
        2,
        Node::RuntimeParameter {
            name: "missing".into(),
            ty: ScalarType::String,
            preview: None,
        },
    );
    let hosts = [
        host("input", Path::new("in.json"), br#"{"Value":"primary"}"#)?,
        host(
            "catalog",
            Path::new("catalog.xml"),
            b"<Record><Value>lookup</Value></Record>",
        )?,
    ];
    let identities = [PipelinePreviewOutputIdentity {
        stage: "consume".into(),
        target: None,
        path: sentinel.clone(),
    }];
    let calls = RefCell::new(Vec::new());
    let debug = |stage: &str, _: &engine::PendingTargetWrite| {
        calls.borrow_mut().push(stage.to_owned());
        DebugDecision::Resume
    };
    let error = preview_pipeline_value_payloads(
        &pipeline,
        &dir.0.join("pipeline.json"),
        &PipelinePreviewOptions::new(&hosts)
            .with_output_identities(&identities)
            .with_stage_debug_hook(&debug),
    )
    .unwrap_err();
    assert!(
        matches!(error.downcast_ref::<PipelineError>(), Some(PipelineError::StageExecution { stage, source: EngineError::MissingRuntimeParameter { name, .. } }) if stage == "consume" && name == "missing")
    );
    assert!(calls.borrow().iter().any(|stage| stage == "prepare"));
    assert_eq!(std::fs::read_to_string(sentinel)?, "preserved");
    assert!(!dir.0.join("maps").exists());
    Ok(())
}

#[test]
fn metadata_validation_does_not_parse_inputs_or_call_observers() -> anyhow::Result<()> {
    let pipeline = single(copy_project());
    let hosts = [host("input", Path::new("input.json"), b"invalid JSON")?];
    let calls = AtomicUsize::new(0);
    let debug = |_: &str, _: &engine::PendingTargetWrite| {
        calls.fetch_add(1, Ordering::Relaxed);
        DebugDecision::Resume
    };
    let options = PipelinePreviewOptions::new(&hosts).with_stage_debug_hook(&debug);
    validate_pipeline_preview(&pipeline, Path::new("absent/pipeline.json"), &options)?;
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert!(
        preview_pipeline_value_payloads(&pipeline, Path::new("absent/pipeline.json"), &options)
            .is_err()
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    Ok(())
}

#[test]
fn host_set_and_output_identities_are_validated_before_decoding() -> anyhow::Result<()> {
    let pipeline = single(copy_project());
    let valid = host("input", Path::new("input.json"), b"invalid JSON")?;
    for (hosts, message) in [
        (Vec::new(), "missing"),
        (vec![valid, valid], "more than once"),
        (
            vec![host("other", Path::new("input.json"), b"invalid JSON")?],
            "not used",
        ),
    ] {
        let error = preview_pipeline_value_payloads(
            &pipeline,
            Path::new("pipeline.json"),
            &PipelinePreviewOptions::new(&hosts),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains(message), "{error:#}");
    }
    let hosts = [valid];
    for (identities, message) in [
        (
            vec![PipelinePreviewOutputIdentity {
                stage: "missing".into(),
                target: None,
                path: "a.json".into(),
            }],
            "unknown stage",
        ),
        (
            vec![PipelinePreviewOutputIdentity {
                stage: "one".into(),
                target: Some("missing".into()),
                path: "a.json".into(),
            }],
            "unknown target",
        ),
        (
            vec![
                PipelinePreviewOutputIdentity {
                    stage: "one".into(),
                    target: None,
                    path: "a.json".into()
                };
                2
            ],
            "more than once",
        ),
    ] {
        let error = preview_pipeline_value_payloads(
            &pipeline,
            Path::new("pipeline.json"),
            &PipelinePreviewOptions::new(&hosts).with_output_identities(&identities),
        )
        .unwrap_err();
        assert!(format!("{error:#}").contains(message), "{error:#}");
    }
    let mut missing = pipeline.clone();
    missing.stages[0].project.target_path = None;
    let error = validate_pipeline_preview(
        &missing,
        Path::new("pipeline.json"),
        &PipelinePreviewOptions::new(&hosts),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("logical output identity"));
    Ok(())
}

#[test]
fn a_shared_host_requires_compatible_schema_and_identical_format_options() -> anyhow::Result<()> {
    let project = copy_project();
    let mut pipeline = single(project.clone());
    let mut second = pipeline.stages[0].clone();
    second.id = "two".into();
    second.project.source_options = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };
    pipeline.stages.push(second);
    let hosts = [host("input", Path::new("input.json"), b"invalid JSON")?];
    let error = validate_pipeline_preview(
        &pipeline,
        Path::new("pipeline.json"),
        &PipelinePreviewOptions::new(&hosts),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("inconsistent schema or format"));
    pipeline.stages[1].project.source_options = json_options();
    pipeline.stages[1].project.source =
        SchemaNode::group("Record", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
    pipeline.stages[1].project.root = Scope::default();
    let error = validate_pipeline_preview(
        &pipeline,
        Path::new("pipeline.json"),
        &PipelinePreviewOptions::new(&hosts),
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("incompatible schemas"),
        "{error:#}"
    );
    Ok(())
}

#[test]
fn all_known_unsupported_formats_reject_before_any_stage_callback() -> anyhow::Result<()> {
    let mut variants = Vec::new();
    let mut sqlite = copy_project();
    sqlite.target_options = FormatOptions::default();
    sqlite.target_path = Some("sentinel.sqlite".into());
    variants.push(sqlite);
    let mut update = copy_project();
    update.target_options = FormatOptions {
        xlsx_update_existing: true,
        ..FormatOptions::default()
    };
    update.target_path = Some("sentinel.xlsx".into());
    variants.push(update);
    let mut transposed = copy_project();
    transposed.target_options = FormatOptions {
        xlsx_rows: vec![1],
        ..FormatOptions::default()
    };
    transposed.target_path = Some("sentinel.xlsx".into());
    variants.push(transposed);
    let mut json5_lines = copy_project();
    json5_lines.target_options = FormatOptions {
        json_lines: true,
        ..FormatOptions::default()
    };
    json5_lines.target_path = Some("sentinel.json5".into());
    variants.push(json5_lines);
    let mut idoc = copy_project();
    idoc.target_options = FormatOptions {
        edi_kind: Some(EdiBoundaryKind::Idoc),
        ..FormatOptions::default()
    };
    idoc.target_path = Some("sentinel.idoc".into());
    variants.push(idoc);
    let mut file_set = copy_project();
    file_set.source_options = FormatOptions {
        xml_document: true,
        local_xml_file_set: true,
        ..FormatOptions::default()
    };
    variants.push(file_set);
    let mut sqlite_input = copy_project();
    sqlite_input.source_options = FormatOptions::default();
    variants.push(sqlite_input);
    let calls = AtomicUsize::new(0);
    let debug = |_: &str, _: &engine::PendingTargetWrite| {
        calls.fetch_add(1, Ordering::Relaxed);
        DebugDecision::Resume
    };
    let trace = |_: &str, _: engine::TraceEvent| {
        calls.fetch_add(1, Ordering::Relaxed);
    };
    for (index, project) in variants.into_iter().enumerate() {
        let pipeline = single(project);
        let path = if index == 6 {
            Path::new("input.sqlite")
        } else {
            Path::new("input.json")
        };
        let hosts = [host("input", path, b"invalid JSON")?];
        let options = PipelinePreviewOptions::new(&hosts)
            .with_stage_debug_hook(&debug)
            .with_stage_trace_sink(&trace);
        let error =
            preview_pipeline_value_payloads(&pipeline, Path::new("absent/pipeline.json"), &options)
                .unwrap_err();
        assert!(
            !format!("{error:#}").contains("reading pipeline host"),
            "{error:#}"
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
    }
    Ok(())
}

fn dynamic_project() -> Project {
    let mut project = copy_project();
    project.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("File", ScalarType::String),
                    SchemaNode::scalar("Value", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    project.graph.nodes = BTreeMap::from([
        (
            0,
            Node::SourceField {
                path: vec!["File".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
        (
            1,
            Node::SourceField {
                path: vec!["Value".into()],
                frame: Some(vec!["Rows".into()]),
            },
        ),
    ]);
    project.root = Scope {
        iteration: ScopeIteration::DynamicDocuments {
            source: vec!["Rows".into()],
            output_path: 0,
        },
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 1,
        }],
        ..Scope::default()
    };
    project.target_path = None;
    project
}

#[test]
fn dynamic_outputs_render_each_member_and_validate_options_before_execution() -> anyhow::Result<()>
{
    let dir = TempDir::new()?;
    let pipeline = single(dynamic_project());
    let bytes = br#"{"Rows":[{"File":"first.json","Value":"first"},{"File":"nested/second.json","Value":"second"}]}"#;
    let hosts = [host("input", Path::new("input.json"), bytes)?];
    let identities = [PipelinePreviewOutputIdentity {
        stage: "one".into(),
        target: None,
        path: dir.0.join("uncreated"),
    }];
    let output = preview_pipeline_value_payloads(
        &pipeline,
        &dir.0.join("pipeline.json"),
        &PipelinePreviewOptions::new(&hosts).with_output_identities(&identities),
    )?;
    assert_eq!(output.artifacts.len(), 2);
    assert_eq!(output.artifacts[0].path, dir.0.join("uncreated/first.json"));
    assert_eq!(
        output.artifacts[1].path,
        dir.0.join("uncreated/nested/second.json")
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&output.artifacts[1].bytes)?,
        serde_json::json!({"Value":"second"})
    );
    assert!(!dir.0.join("uncreated").exists());
    let mut unsupported = pipeline;
    unsupported.stages[0].project.target_options = FormatOptions {
        xlsx_rows: vec![1],
        ..FormatOptions::default()
    };
    let count = AtomicUsize::new(0);
    let debug = |_: &str, _: &engine::PendingTargetWrite| {
        count.fetch_add(1, Ordering::Relaxed);
        DebugDecision::Resume
    };
    let error = preview_pipeline_value_payloads(
        &unsupported,
        &dir.0.join("pipeline.json"),
        &PipelinePreviewOptions::new(&hosts)
            .with_output_identities(&identities)
            .with_stage_debug_hook(&debug),
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("input-only XLSX"),
        "{error:#}"
    );
    assert_eq!(count.load(Ordering::Relaxed), 0);
    Ok(())
}

#[test]
fn dynamic_named_sources_cannot_trigger_local_loading() -> anyhow::Result<()> {
    let mut project = dynamic_project();
    project.extra_sources.push(NamedSource {
        name: "dynamic".into(),
        path: "missing.json".into(),
        schema: value_schema(),
        options: json_options(),
        dynamic_path: Some(DynamicSourcePath {
            node: 0,
            iteration: vec!["Rows".into()],
        }),
    });
    let pipeline = single(project);
    let hosts = [host("input", Path::new("input.json"), b"invalid JSON")?];
    let error = preview_pipeline_value_payloads(
        &pipeline,
        Path::new("pipeline.json"),
        &PipelinePreviewOptions::new(&hosts),
    )
    .unwrap_err();
    assert!(
        format!("{error:#}").contains("dynamic named source"),
        "{error:#}"
    );
    Ok(())
}

#[test]
fn cancellation_before_execution_and_after_a_node_callback_discards_outputs() -> anyhow::Result<()>
{
    let pipeline = single(preview_project("preview"));
    let hosts = [host(
        "input",
        Path::new("input.json"),
        br#"{"Value":"input"}"#,
    )?];
    let cancelled = AtomicBool::new(true);
    let error = preview_pipeline_value_payloads(
        &pipeline,
        Path::new("pipeline.json"),
        &PipelinePreviewOptions::new(&hosts).with_cancellation(&cancelled),
    )
    .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<EngineError>(),
        Some(EngineError::DebugCancelled)
    ));
    cancelled.store(false, Ordering::Relaxed);
    let calls = AtomicUsize::new(0);
    let node = |_: &str, _: &engine::PendingNodeValue| {
        calls.fetch_add(1, Ordering::Relaxed);
        cancelled.store(true, Ordering::Relaxed);
        DebugDecision::Resume
    };
    let error = preview_pipeline_value_payloads(
        &pipeline,
        Path::new("pipeline.json"),
        &PipelinePreviewOptions::new(&hosts)
            .with_cancellation(&cancelled)
            .with_stage_node_debug_hook(&node),
    )
    .unwrap_err();
    assert!(matches!(
        error.downcast_ref::<PipelineError>(),
        Some(PipelineError::StageExecution {
            source: EngineError::DebugCancelled,
            ..
        })
    ));
    assert_eq!(calls.load(Ordering::Relaxed), 1);
    Ok(())
}

#[test]
fn input_total_and_artifact_count_limits_precede_parsing_and_rendering() -> anyhow::Result<()> {
    // Borrow one buffer for five distinct host identities to test cumulative
    // accounting without allocating five full documents.
    let bytes = vec![b' '; cli::MAX_PAYLOAD_DOCUMENT_BYTES];
    let mut pipeline = single(copy_project());
    for index in 1..5 {
        let mut stage = pipeline.stages[0].clone();
        stage.id = format!("stage{index}");
        stage.source = PipelineInput::Host {
            name: format!("input{index}"),
        };
        pipeline.stages.push(stage);
    }
    let names = ["input", "input1", "input2", "input3", "input4"];
    let hosts = names
        .iter()
        .map(|name| host(name, Path::new("input.json"), &bytes))
        .collect::<anyhow::Result<Vec<_>>>()?;
    let error = validate_pipeline_preview(
        &pipeline,
        Path::new("pipeline.json"),
        &PipelinePreviewOptions::new(&hosts),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("256 MiB total"));
    let mut project = copy_project();
    project.extra_targets = (0..cli::MAX_PAYLOAD_ARTIFACTS)
        .map(|index| NamedTarget {
            name: format!("target{index}"),
            path: Some("out.json".into()),
            schema: value_schema(),
            options: json_options(),
            root: Scope::default(),
        })
        .collect();
    let pipeline = single(project);
    let hosts = [host("input", Path::new("input.json"), b"invalid JSON")?];
    let error = validate_pipeline_preview(
        &pipeline,
        Path::new("pipeline.json"),
        &PipelinePreviewOptions::new(&hosts),
    )
    .unwrap_err();
    assert!(format!("{error:#}").contains("stage targets"));
    Ok(())
}
