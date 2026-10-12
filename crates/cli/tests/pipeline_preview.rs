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
fn csv_bom_conflicts_reject_metadata_before_pipeline_decode_or_callbacks() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    for source_conflict in [false, true] {
        let mut project = copy_project();
        project.target_path = Some("output.csv".into());
        project.target_options = FormatOptions::default();
        let input_path = if source_conflict {
            project.source_options = FormatOptions {
                csv_utf8_bom: true,
                ..FormatOptions::default()
            };
            Path::new("input.xml")
        } else {
            project.target_options.csv_utf8_bom = true;
            Path::new("input.json")
        };
        let pipeline = single(project);
        assert!(engine::validate_pipeline(&pipeline).is_empty());
        let bytes: &[u8] = if source_conflict {
            b"invalid document"
        } else {
            br#"{"Value":"valid source"}"#
        };
        let hosts = [host("input", input_path, bytes)?];
        let sentinel = dir.0.join(format!("sentinel-{source_conflict}.xml"));
        std::fs::write(&sentinel, b"preserved")?;
        let identities = [PipelinePreviewOutputIdentity {
            stage: "one".into(),
            target: None,
            path: sentinel.clone(),
        }];
        let count = AtomicUsize::new(0);
        let debug = |_: &str, _: &engine::PendingTargetWrite| {
            count.fetch_add(1, Ordering::Relaxed);
            DebugDecision::Resume
        };
        let trace = |_: &str, _: engine::TraceEvent| {
            count.fetch_add(1, Ordering::Relaxed);
        };
        let options = PipelinePreviewOptions::new(&hosts)
            .with_output_identities(&identities)
            .with_stage_debug_hook(&debug)
            .with_stage_trace_sink(&trace);
        let metadata_error =
            validate_pipeline_preview(&pipeline, &dir.0.join("pipeline.json"), &options)
                .unwrap_err();
        assert!(format!("{metadata_error:#}").contains("csv_utf8_bom"));
        let error =
            preview_pipeline_value_payloads(&pipeline, &dir.0.join("pipeline.json"), &options)
                .unwrap_err();
        assert!(format!("{error:#}").contains("csv_utf8_bom"));
        assert_eq!(count.load(Ordering::Relaxed), 0);
        assert_eq!(std::fs::read(&sentinel)?, b"preserved");
    }
    Ok(())
}

#[test]
fn csv_bom_pipeline_preview_returns_exact_marked_bytes_without_writing() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let mut project = copy_project();
    project.source_options = FormatOptions {
        csv_utf8_bom: true,
        ..FormatOptions::default()
    };
    project.target_options = project.source_options.clone();
    project.target_path = Some("output.csv".into());
    project.root.iteration = ScopeIteration::Source(Vec::new());
    let pipeline = single(project);
    let hosts = [host(
        "input",
        Path::new("input.csv"),
        b"\xef\xbb\xbfValue\ncaf\xc3\xa9\n",
    )?];
    let identities = [PipelinePreviewOutputIdentity {
        stage: "one".into(),
        target: None,
        path: dir.0.join("uncreated/output.csv"),
    }];
    let options = PipelinePreviewOptions::new(&hosts).with_output_identities(&identities);
    validate_pipeline_preview(&pipeline, &dir.0.join("pipeline.json"), &options)?;
    let output =
        preview_pipeline_value_payloads(&pipeline, &dir.0.join("pipeline.json"), &options)?;
    assert_eq!(output.artifacts.len(), 1);
    assert_eq!(output.artifacts[0].target, None);
    assert_eq!(output.artifacts[0].records_written, 1);
    assert_eq!(output.artifacts[0].path, identities[0].path);
    assert_eq!(
        output.artifacts[0].bytes,
        b"\xef\xbb\xbfValue\ncaf\xc3\xa9\n"
    );
    assert!(!dir.0.join("uncreated").exists());
    Ok(())
}

#[test]
fn stage_trace_capacity_skips_source_snapshots_but_keeps_event_order() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let pipeline = single(dynamic_project());
    let bytes = br#"{"Rows":[{"File":"first.json","Value":"first"},{"File":"second.json","Value":"second"}]}"#;
    let hosts = [host("input", Path::new("input.json"), bytes)?];
    let identities = [PipelinePreviewOutputIdentity {
        stage: "one".into(),
        target: None,
        path: dir.0.join("outputs"),
    }];
    let full_events = RefCell::new(Vec::new());
    let full_trace = |stage: &str, event| {
        full_events.borrow_mut().push((stage.to_owned(), event));
    };
    let full = preview_pipeline_value_payloads(
        &pipeline,
        &dir.0.join("pipeline.json"),
        &PipelinePreviewOptions::new(&hosts)
            .with_output_identities(&identities)
            .with_stage_trace_sink(&full_trace),
    )?;

    let capacity = || false;
    let bounded_events = RefCell::new(Vec::new());
    let bounded_trace = |stage: &str, event| {
        bounded_events.borrow_mut().push((stage.to_owned(), event));
    };
    let bounded = preview_pipeline_value_payloads(
        &pipeline,
        &dir.0.join("pipeline.json"),
        &PipelinePreviewOptions::new(&hosts)
            .with_output_identities(&identities)
            .with_stage_trace_sink(&bounded_trace)
            .with_stage_trace_source_row_capacity(&capacity),
    )?;
    assert_eq!(full, bounded);
    let full_events = full_events.into_inner();
    let bounded_events = bounded_events.into_inner();
    assert_eq!(full_events.len(), bounded_events.len());
    assert!(full_events.iter().any(|(_, event)| matches!(
        event,
        engine::TraceEvent::IterationCandidate {
            source_row: Some(_),
            ..
        }
    )));
    for ((full_stage, full_event), (bounded_stage, bounded_event)) in
        full_events.iter().zip(&bounded_events)
    {
        assert_eq!(full_stage, bounded_stage);
        assert_eq!(
            std::mem::discriminant(full_event),
            std::mem::discriminant(bounded_event)
        );
        if let engine::TraceEvent::IterationCandidate { source_row, .. } = bounded_event {
            assert!(source_row.is_none());
        }
    }
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
fn typed_primary_and_named_edges_ignore_unused_source_file_hints() -> anyhow::Result<()> {
    let dir = TempDir::new()?;
    let pipeline_path = dir.0.join("missing/pipeline.json");
    let hosts = [host(
        "input",
        Path::new("input.json"),
        br#"{"Value":"upstream"}"#,
    )?];
    for (primary_hint, named_hint) in [
        ("unused.sqlite", "unused.unknown"),
        ("unused.unknown", "unused.sqlite"),
        (
            "https://invalid.example/unused",
            "http://invalid.example/unused",
        ),
    ] {
        let mut pipeline = single(copy_project());
        let mut consumer = copy_project();
        consumer.source_path = Some(primary_hint.into());
        consumer.source_options = FormatOptions::default();
        consumer.extra_sources.push(NamedSource {
            name: "lookup".into(),
            path: named_hint.into(),
            schema: value_schema(),
            options: FormatOptions::default(),
            dynamic_path: None,
        });
        consumer.graph.nodes = BTreeMap::from([
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
                    path: vec!["lookup".into(), "Value".into()],
                    frame: None,
                },
            ),
            (
                2,
                Node::Const {
                    value: Value::String("-".into()),
                },
            ),
            (
                3,
                Node::Call {
                    function: "concat".into(),
                    args: vec![0, 2, 1],
                },
            ),
        ]);
        consumer.root = Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 3,
            }],
            ..Scope::default()
        };
        pipeline.stages.push(PipelineStage {
            id: "consumer".into(),
            mapping_path: Some("unopened/mapping.mfd".into()),
            project: consumer,
            source: PipelineInput::StageTarget {
                stage: "one".into(),
                target: None,
            },
            extra_sources: vec![PipelineNamedInput {
                name: "lookup".into(),
                from: PipelineInput::StageTarget {
                    stage: "one".into(),
                    target: None,
                },
            }],
        });
        validate_pipeline_preview(
            &pipeline,
            &pipeline_path,
            &PipelinePreviewOptions::new(&hosts),
        )?;
        let outcome = preview_pipeline_value_payloads(
            &pipeline,
            &pipeline_path,
            &PipelinePreviewOptions::new(&hosts),
        )?;
        assert_eq!(outcome.stages_executed, ["one", "consumer"]);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&outcome.artifacts[1].bytes)?,
            serde_json::json!({"Value":"upstream-upstream"})
        );
    }
    assert!(!dir.0.join("missing").exists());
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

mod source_ownership_admission {
    use super::*;

    struct Case {
        name: &'static str,
        pipeline: Pipeline,
        stage: &'static str,
        published_stage: &'static str,
        expected: engine::EngineError,
        display: &'static str,
    }

    fn owned_field() -> Node {
        Node::OwnedSourceField(mapping::OwnedSourceField {
            source: mapping::OwnedSourceRef {
                owner: mapping::SourceOwner::Primary,
                path: vec!["Value".into()],
            },
            frame: None,
        })
    }

    fn bound(node: u32) -> Scope {
        Scope {
            bindings: vec![Binding {
                target_field: "Value".into(),
                node,
            }],
            ..Scope::default()
        }
    }

    fn one(project: Project) -> Pipeline {
        Pipeline {
            main_mapping_path: None,
            stages: vec![PipelineStage {
                id: "one".into(),
                mapping_path: None,
                project,
                source: PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: vec![],
            }],
        }
    }

    fn unused_owned_project(lazy: bool) -> Project {
        let mut project = copy_project();
        project.graph.nodes.insert(47, owned_field());
        if lazy {
            project.graph.nodes.insert(
                62,
                Node::Const {
                    value: Value::Bool(true),
                },
            );
            project.graph.nodes.insert(
                63,
                Node::Const {
                    value: Value::String("original".into()),
                },
            );
            project
                .graph
                .nodes
                .insert(65, Node::Raise { message: None });
            project.graph.nodes.insert(
                64,
                Node::If {
                    condition: 62,
                    then: 63,
                    else_: 65,
                },
            );
            project.root = bound(64);
        }
        project
    }

    fn isolated(mut project: Project, node: u32, output_type: ScalarType) -> Project {
        let function = mapping::FunctionId::new(7);
        let owned = project.graph.nodes[&node].clone();
        project.user_functions.insert(
            function,
            mapping::UserFunction {
                library: "tests".into(),
                name: "closed_owned_value".into(),
                description: None,
                parameters: vec![],
                output_name: "value".into(),
                output_type,
                body: Graph {
                    nodes: [(node, owned)].into(),
                },
                output: node,
            },
        );
        project.graph.nodes.insert(
            node,
            Node::UserFunctionCall {
                function,
                args: vec![],
            },
        );
        project
    }

    fn cases() -> Vec<Case> {
        let mut direct = isolated(unused_owned_project(false), 47, ScalarType::String);
        direct.root = bound(47);
        let expected = || engine::EngineError::UnsupportedSourceOwnership {
            node: 47,
            function: Some(mapping::FunctionId::new(7)),
            kind: "owned_source_field",
        };
        let field_display = "source ownership for owned_source_field node 47 (function Some(FunctionId(7))) is not supported";
        let mut position = copy_project();
        position.target =
            SchemaNode::group("Record", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
        position.graph.nodes.insert(
            48,
            Node::OwnedPosition(mapping::OwnedPosition {
                owner: mapping::SourceOwner::Primary,
                collection: Some(vec![]),
            }),
        );
        position.root = bound(48);
        position = isolated(position, 48, ScalarType::Int);

        let mut hidden = isolated(unused_owned_project(false), 47, ScalarType::String);
        hidden.extra_targets.push(NamedTarget {
            name: "hidden".into(),
            path: Some("hidden.json".into()),
            schema: hidden.target.clone(),
            options: hidden.target_options.clone(),
            root: bound(47),
        });
        let mut disconnected = one(copy_project());
        disconnected.stages.push(PipelineStage {
            id: "disconnected".into(),
            mapping_path: None,
            project: direct.clone(),
            source: PipelineInput::Host {
                name: "input".into(),
            },
            extra_sources: vec![],
        });

        let function = mapping::FunctionId::new(7);
        let mut called = copy_project();
        called.user_functions.insert(
            function,
            mapping::UserFunction {
                library: "tests".into(),
                name: "owned_value".into(),
                description: None,
                parameters: vec![],
                output_name: "value".into(),
                output_type: ScalarType::String,
                body: Graph {
                    nodes: BTreeMap::from([(47, owned_field())]),
                },
                output: 47,
            },
        );
        called.graph.nodes.insert(
            61,
            Node::UserFunctionCall {
                function,
                args: vec![],
            },
        );
        called.root = bound(61);

        let mut potential = isolated(unused_owned_project(false), 47, ScalarType::String);
        potential.graph.nodes.insert(
            62,
            Node::Const {
                value: Value::Bool(false),
            },
        );
        potential.graph.nodes.insert(
            63,
            Node::Const {
                value: Value::String("original".into()),
            },
        );
        potential.graph.nodes.insert(
            64,
            Node::If {
                condition: 62,
                then: 47,
                else_: 63,
            },
        );
        potential.root = bound(64);
        let mut forward = one(direct.clone());
        forward.stages[0].id = "consume".into();
        forward.stages[0].source = PipelineInput::StageTarget {
            stage: "prepare".into(),
            target: None,
        };
        let mut producer = one(direct.clone()).stages.remove(0);
        producer.id = "prepare".into();
        forward.stages.push(producer);
        vec![
            Case {
                name: "direct-field",
                pipeline: one(direct),
                stage: "one",
                published_stage: "one",
                expected: expected(),
                display: field_display,
            },
            Case {
                name: "direct-position",
                pipeline: one(position),
                stage: "one",
                published_stage: "one",
                expected: engine::EngineError::UnsupportedSourceOwnership {
                    node: 48,
                    function: Some(mapping::FunctionId::new(7)),
                    kind: "owned_position",
                },
                display: "source ownership for owned_position node 48 (function Some(FunctionId(7))) is not supported",
            },
            Case {
                name: "unselected-named-target",
                pipeline: one(hidden),
                stage: "one",
                published_stage: "one",
                expected: expected(),
                display: field_display,
            },
            Case {
                name: "disconnected-stage",
                pipeline: disconnected,
                stage: "disconnected",
                published_stage: "one",
                expected: expected(),
                display: field_display,
            },
            Case {
                name: "called-function",
                pipeline: one(called),
                stage: "one",
                published_stage: "one",
                expected: engine::EngineError::UnsupportedSourceOwnership {
                    node: 47,
                    function: Some(function),
                    kind: "owned_source_field",
                },
                display: "source ownership for owned_source_field node 47 (function Some(FunctionId(7))) is not supported",
            },
            Case {
                name: "potential-if",
                pipeline: one(potential),
                stage: "one",
                published_stage: "one",
                expected: expected(),
                display: field_display,
            },
            Case {
                name: "forward-stage-order",
                pipeline: forward,
                stage: "prepare",
                published_stage: "consume",
                expected: expected(),
                display: field_display,
            },
        ]
    }

    fn assert_refusal(error: &anyhow::Error, case: &Case) {
        let chain = error.chain().map(ToString::to_string).collect::<Vec<_>>();
        eprintln!(
            "{} full_chain={chain:?} typed={:?}",
            case.name,
            error.downcast_ref::<engine::PipelineError>()
        );
        match error.downcast_ref::<engine::PipelineError>() {
            Some(engine::PipelineError::StageExecution { stage, source }) => {
                assert_eq!(stage.as_str(), case.stage);
                assert_eq!(source, &case.expected);
            }
            other => panic!("expected stage ownership failure, got {other:?}"),
        }
        assert_eq!(
            chain,
            [
                format!("stage `{}` failed: {}", case.stage, case.display),
                case.display.to_owned()
            ]
        );
    }

    #[test]
    fn all_stages_and_targets_refuse_before_preview_decode() -> anyhow::Result<()> {
        for case in cases() {
            let dir = TempDir::new()?;
            let pipeline_path = dir.0.join("pipeline.json");
            let sentinel = dir.0.join("result.json");
            std::fs::write(&sentinel, b"old-output")?;
            let hosts = [host("input", Path::new("absent/input.json"), b"not-json")?];
            let debug_calls = RefCell::new(Vec::<String>::new());
            let trace_calls = RefCell::new(Vec::<(String, String)>::new());
            let debug = |stage: &str, _: &engine::PendingTargetWrite| {
                debug_calls.borrow_mut().push(stage.to_owned());
                DebugDecision::Resume
            };
            let trace = |stage: &str, event: engine::TraceEvent| {
                trace_calls
                    .borrow_mut()
                    .push((stage.to_owned(), format!("{event:?}")));
            };
            let options = PipelinePreviewOptions::new(&hosts)
                .with_stage_debug_hook(&debug)
                .with_stage_trace_sink(&trace);
            eprintln!(
                "{} pipeline={:?} publication_stage={} complete_input={:?}",
                case.name,
                case.pipeline,
                case.published_stage,
                hosts[0].document().bytes()
            );
            let metadata = validate_pipeline_preview(&case.pipeline, &pipeline_path, &options);
            eprintln!("{} metadata={metadata:?}", case.name);
            assert_refusal(
                &metadata.expect_err("metadata admission must reject unsupported mapping"),
                &case,
            );
            let result = preview_pipeline_value_payloads(&case.pipeline, &pipeline_path, &options);
            let output_after = std::fs::read(&sentinel)?;
            eprintln!(
                "{} result={result:?} output_after={output_after:?} debug={:?} trace={:?}",
                case.name,
                debug_calls.borrow(),
                trace_calls.borrow()
            );
            assert_refusal(
                &result.expect_err("ownership refusal must precede invalid JSON decoding"),
                &case,
            );
            assert_eq!(output_after.as_slice(), b"old-output".as_slice());
            assert!(debug_calls.borrow().is_empty());
            assert!(trace_calls.borrow().is_empty());
        }
        Ok(())
    }

    #[test]
    fn preview_metadata_and_cancellation_errors_keep_precedence() -> anyhow::Result<()> {
        let case = cases().remove(0);
        let input = host("input", Path::new("input.json"), b"not-json")?;
        for (name, hosts, cancelled, expected) in [
            (
                "missing-binding",
                vec![],
                false,
                "host input `input` is missing",
            ),
            (
                "duplicate-binding",
                vec![input, input],
                false,
                "host input `input` was supplied more than once",
            ),
            (
                "cancelled",
                vec![input],
                true,
                "debug run cancelled before target field insertion",
            ),
        ] {
            let cancellation = AtomicBool::new(cancelled);
            let result = preview_pipeline_value_payloads(
                &case.pipeline,
                Path::new("pipeline.json"),
                &PipelinePreviewOptions::new(&hosts).with_cancellation(&cancellation),
            );
            eprintln!(
                "{name} complete_input={:?} result={result:?}",
                hosts
                    .iter()
                    .map(|host| (host.name(), host.document().path(), host.document().bytes()))
                    .collect::<Vec<_>>()
            );
            let error = result.expect_err("metadata/cancellation refusal must precede ownership");
            let chain = error.chain().map(ToString::to_string).collect::<Vec<_>>();
            eprintln!(
                "{name} full_chain={chain:?} typed={:?}",
                error.downcast_ref::<engine::EngineError>()
            );
            assert_eq!(chain, [expected.to_owned()]);
            if cancelled {
                assert_eq!(
                    error.downcast_ref::<engine::EngineError>(),
                    Some(&engine::EngineError::DebugCancelled)
                );
            } else {
                assert!(error.downcast_ref::<engine::EngineError>().is_none());
            }
        }
        Ok(())
    }

    #[test]
    fn unused_owned_storage_preserves_preview_and_legacy_lazy_execution() -> anyhow::Result<()> {
        for lazy in [false, true] {
            let dir = TempDir::new()?;
            let pipeline = one(unused_owned_project(lazy));
            let pipeline_path = dir.0.join("pipeline.json");
            let sentinel = dir.0.join("result.json");
            std::fs::write(&sentinel, b"old-output")?;
            let hosts = [host(
                "input",
                Path::new("absent/input.json"),
                br#"{"Value":"original"}"#,
            )?];
            let result = preview_pipeline_value_payloads(
                &pipeline,
                &pipeline_path,
                &PipelinePreviewOptions::new(&hosts),
            );
            let output_after = std::fs::read(&sentinel)?;
            eprintln!("unused lazy={lazy} result={result:?} output_after={output_after:?}");
            let outcome = result?;
            let tree: serde_json::Value = serde_json::from_slice(&outcome.artifacts[0].bytes)?;
            let metadata = outcome
                .artifacts
                .iter()
                .map(|artifact| {
                    (
                        artifact.stage.clone(),
                        artifact.target.clone(),
                        artifact.path.clone(),
                        artifact.records_written,
                    )
                })
                .collect::<Vec<_>>();
            assert_eq!(
                (outcome.stages_executed, metadata, tree),
                (
                    vec!["one".to_owned()],
                    vec![("one".to_owned(), None::<String>, sentinel, 1usize)],
                    serde_json::json!({"Value":"original"}),
                )
            );
            assert_eq!(output_after.as_slice(), b"old-output".as_slice());
        }
        Ok(())
    }

    #[test]
    fn supported_basic_field_preview_accepts_metadata_before_payload_parse_and_cancel()
    -> anyhow::Result<()> {
        let dir = TempDir::new()?;
        let mut project = unused_owned_project(false);
        project.root = bound(47);
        let pipeline = one(project);
        let pipeline_path = dir.0.join("pipeline.json");
        let sentinel = dir.0.join("result.json");
        std::fs::write(&sentinel, b"old-output")?;
        let hosts = [host(
            "input",
            Path::new("absent/input.json"),
            br#"{"Value":"supported"}"#,
        )?];
        let options = PipelinePreviewOptions::new(&hosts);
        validate_pipeline_preview(&pipeline, &pipeline_path, &options)?;
        let outcome = preview_pipeline_value_payloads(&pipeline, &pipeline_path, &options)?;
        assert_eq!(outcome.stages_executed, vec!["one".to_owned()]);
        assert_eq!(outcome.artifacts.len(), 1);
        let artifact = &outcome.artifacts[0];
        assert_eq!(artifact.stage.as_str(), "one");
        assert!(artifact.target.is_none());
        assert_eq!(artifact.path, sentinel);
        assert_eq!(artifact.records_written, 1);
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&artifact.bytes)?,
            serde_json::json!({"Value":"supported"})
        );
        assert_eq!(
            std::fs::read(&sentinel)?.as_slice(),
            b"old-output".as_slice()
        );
        let debug_calls = RefCell::new(0usize);
        let trace_calls = RefCell::new(0usize);
        let debug = |_: &str, _: &engine::PendingTargetWrite| {
            *debug_calls.borrow_mut() += 1;
            DebugDecision::Resume
        };
        let trace = |_: &str, _: engine::TraceEvent| {
            *trace_calls.borrow_mut() += 1;
        };
        let cancellation = AtomicBool::new(false);
        for (label, requested, expected_context) in [
            (
                "debug",
                PipelinePreviewOptions::new(&hosts).with_stage_debug_hook(&debug),
                engine::OwnedCapabilityContext::Debugging,
            ),
            (
                "cancellation",
                PipelinePreviewOptions::new(&hosts).with_cancellation(&cancellation),
                engine::OwnedCapabilityContext::Debugging,
            ),
            (
                "trace",
                PipelinePreviewOptions::new(&hosts).with_stage_trace_sink(&trace),
                engine::OwnedCapabilityContext::Tracing,
            ),
        ] {
            validate_pipeline_preview(&pipeline, &pipeline_path, &requested)?;
            let error = preview_pipeline_value_payloads(&pipeline, &pipeline_path, &requested)
                .expect_err("requested owned observer contexts must remain unsupported");
            eprintln!("{label} requested_context_result={error:?}");
            match error.downcast_ref::<engine::PipelineError>() {
                Some(engine::PipelineError::StageExecution { stage, source }) => {
                    assert_eq!(stage.as_str(), "one");
                    assert_eq!(
                        source,
                        &engine::EngineError::UnsupportedOwnedContext {
                            context: expected_context,
                            node: None,
                            function: None,
                            scope: None,
                        }
                    );
                }
                other => panic!("expected typed owned observer context refusal, got {other:?}"),
            }
            assert_eq!(*debug_calls.borrow(), 0);
            assert_eq!(*trace_calls.borrow(), 0);
            assert_eq!(
                std::fs::read(&sentinel)?.as_slice(),
                b"old-output".as_slice()
            );
        }
        let malformed = [host("input", Path::new("absent/input.json"), b"not-json")?];
        let options = PipelinePreviewOptions::new(&malformed);
        validate_pipeline_preview(&pipeline, &pipeline_path, &options)?;
        let error = preview_pipeline_value_payloads(&pipeline, &pipeline_path, &options)
            .expect_err(
                "metadata accepts Basic capability; its malformed payload must reach the parser",
            );
        assert!(format!("{error:#}").to_ascii_lowercase().contains("json"));
        assert!(error.downcast_ref::<engine::EngineError>().is_none());
        assert!(!matches!(
            error.downcast_ref::<engine::PipelineError>(),
            Some(engine::PipelineError::StageExecution {
                source: engine::EngineError::UnsupportedSourceOwnership { .. }
                    | engine::EngineError::UnsupportedOwnedContext { .. },
                ..
            })
        ));
        assert_eq!(
            std::fs::read(&sentinel)?.as_slice(),
            b"old-output".as_slice()
        );
        let cancelled = AtomicBool::new(true);
        let error = preview_pipeline_value_payloads(
            &pipeline,
            &pipeline_path,
            &options.with_cancellation(&cancelled),
        )
        .expect_err("pre-decode cancellation must still precede malformed Basic input");
        assert_eq!(
            error.downcast_ref::<engine::EngineError>(),
            Some(&engine::EngineError::DebugCancelled)
        );
        assert_eq!(
            std::fs::read(&sentinel)?.as_slice(),
            b"old-output".as_slice()
        );
        Ok(())
    }
}
