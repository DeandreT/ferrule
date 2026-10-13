use std::cell::{Cell, RefCell};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::AtomicBool;

use engine::{DebugDecision, EngineError, OwnedCapabilityContext, SourceOwnershipObservers};
use mapping::{Pipeline, PipelineInput, PipelineStage, Project};

use crate::*;

#[derive(Default)]
struct Observers(Cell<usize>);

impl engine::DebugHook for Observers {
    fn before_target_write(&self, _: &engine::PendingTargetWrite) -> DebugDecision {
        self.0.set(self.0.get() + 1);
        DebugDecision::Resume
    }

    fn source_field_probe(&self) -> Option<(usize, String)> {
        self.0.set(self.0.get() + 1);
        None
    }

    fn after_node_value(&self, _: &engine::PendingNodeValue) -> DebugDecision {
        self.0.set(self.0.get() + 1);
        DebugDecision::Resume
    }
}

impl engine::TraceSink for Observers {
    fn record(&self, _: engine::TraceEvent) {
        self.0.set(self.0.get() + 1);
    }
}

fn observer_error(context: OwnedCapabilityContext) -> EngineError {
    EngineError::UnsupportedOwnedContext {
        node: None,
        function: None,
        scope: None,
        context,
    }
}

fn assert_observer_error(error: &anyhow::Error, context: OwnedCapabilityContext) {
    assert_eq!(
        error.downcast_ref::<EngineError>(),
        Some(&observer_error(context))
    );
}

fn assert_stage_error(error: &anyhow::Error, context: OwnedCapabilityContext) {
    match error.downcast_ref::<engine::PipelineError>() {
        Some(engine::PipelineError::StageExecution { stage, source }) => {
            assert_eq!(stage, "later");
            assert_eq!(*source, observer_error(context));
        }
        other => panic!("expected stage-qualified observer refusal, got {other:?}"),
    }
}

fn legacy_project() -> Project {
    let mut project = owned_source_test_project();
    project.graph.nodes.insert(
        9,
        mapping::Node::SourceField {
            path: vec!["Value".into()],
            frame: None,
        },
    );
    project
}

fn selected_project() -> Project {
    let mut project = owned_source_test_project();
    project.extra_targets.push(mapping::NamedTarget {
        name: "owned".into(),
        path: Some("owned.json".into()),
        schema: project.target.clone(),
        options: project.target_options.clone(),
        root: project.root.clone(),
    });
    project.graph.nodes.insert(
        10,
        mapping::Node::SourceField {
            path: vec!["Value".into()],
            frame: None,
        },
    );
    project.root.bindings[0].node = 10;
    project
}

fn later_owned_pipeline() -> Pipeline {
    let mut earlier = legacy_project();
    earlier.target = earlier.source.clone();
    earlier.target_path = Some("early.json".into());
    Pipeline {
        main_mapping_path: None,
        // Declaration order differs from execution order.
        stages: vec![
            PipelineStage {
                id: "later".into(),
                mapping_path: None,
                project: selected_project(),
                source: PipelineInput::StageTarget {
                    stage: "early".into(),
                    target: None,
                },
                extra_sources: vec![],
            },
            PipelineStage {
                id: "early".into(),
                mapping_path: None,
                project: earlier,
                source: PipelineInput::Host {
                    name: "input".into(),
                },
                extra_sources: vec![],
            },
        ],
    }
}

#[test]
fn observer_admission_uses_selected_potential_closures_and_tracing_precedence() {
    let project = selected_project();
    for observers in [
        SourceOwnershipObservers {
            tracing: false,
            debugging: true,
        },
        SourceOwnershipObservers {
            tracing: true,
            debugging: false,
        },
        SourceOwnershipObservers {
            tracing: true,
            debugging: true,
        },
    ] {
        engine::check_source_ownership_capability_with_observers(
            &project,
            Some(TargetSelection::Primary),
            observers,
        )
        .unwrap();
        let expected = if observers.tracing {
            OwnedCapabilityContext::Tracing
        } else {
            OwnedCapabilityContext::Debugging
        };
        for selection in [None, Some(TargetSelection::Named("owned"))] {
            assert_eq!(
                engine::check_source_ownership_capability_with_observers(
                    &project, selection, observers,
                ),
                Err(observer_error(expected)),
            );
        }
    }
    engine::check_source_ownership_capability(&project, None).unwrap();
    let mut potential = project;
    potential.graph.nodes.insert(
        11,
        mapping::Node::Const {
            value: ir::Value::Bool(false),
        },
    );
    potential.graph.nodes.insert(
        12,
        mapping::Node::If {
            condition: 11,
            then: 9,
            else_: 10,
        },
    );
    potential.root.bindings[0].node = 12;
    assert_eq!(
        engine::check_source_ownership_capability_with_observers(
            &potential,
            Some(TargetSelection::Primary),
            SourceOwnershipObservers {
                tracing: false,
                debugging: true
            },
        ),
        Err(observer_error(OwnedCapabilityContext::Debugging)),
    );
}

#[test]
fn static_pipeline_context_refuses_before_earlier_callbacks_after_host_validation() {
    let pipeline = later_owned_pipeline();
    let observers = Observers::default();
    let hosts: std::collections::BTreeMap<String, ir::Instance> = [(
        "input".into(),
        ir::Instance::Group(
            vec![(
                "Value".into(),
                ir::Instance::Scalar(ir::Value::String("valid".into())),
            )]
            .into(),
        ),
    )]
    .into();
    for (execution, context) in [
        (
            engine::ExecutionContext::new(Path::new("pipeline.json")).with_debug_hook(&observers),
            OwnedCapabilityContext::Debugging,
        ),
        (
            engine::ExecutionContext::new(Path::new("pipeline.json")).with_trace_sink(&observers),
            OwnedCapabilityContext::Tracing,
        ),
        (
            engine::ExecutionContext::new(Path::new("pipeline.json"))
                .with_debug_hook(&observers)
                .with_trace_sink(&observers),
            OwnedCapabilityContext::Tracing,
        ),
    ] {
        let error = engine::run_pipeline_with_context(&pipeline, &hosts, &execution).unwrap_err();
        assert_stage_error(&error.into(), context);
        assert!(matches!(
            engine::run_pipeline_with_context(&pipeline, &Default::default(), &execution),
            Err(engine::PipelineError::MissingHostInput { name }) if name == "input"
        ));
        let mut unexpected = hosts.clone();
        unexpected.insert("unexpected".into(), ir::Instance::Scalar(ir::Value::Null));
        assert!(matches!(
            engine::run_pipeline_with_context(&pipeline, &unexpected, &execution),
            Err(engine::PipelineError::UnexpectedHostInput { name }) if name == "unexpected"
        ));
        let mut invalid = pipeline.clone();
        invalid.stages[0].id = "early".into();
        assert!(matches!(
            engine::run_pipeline_with_context(&invalid, &Default::default(), &execution),
            Err(engine::PipelineError::Invalid(_))
        ));
        assert_eq!(observers.0.get(), 0);
    }
    let mut failing = pipeline.clone();
    failing.stages[0].project = closed_owned_source_test_project();
    failing.stages[1]
        .project
        .graph
        .nodes
        .insert(9, mapping::Node::Raise { message: None });
    let execution = engine::ExecutionContext::new(Path::new("pipeline.json"));
    let error = engine::run_pipeline_with_context(&failing, &hosts, &execution).unwrap_err();
    assert!(matches!(
        error,
        engine::PipelineError::StageExecution { stage, source }
            if stage == "early" && source == (EngineError::MappingException { node: 9, message: None })
    ));
    let entered = RefCell::new(Vec::new());
    let error = engine::run_pipeline_with_stage_contexts(&failing, &hosts, |stage| {
        entered.borrow_mut().push(stage.to_owned());
        execution
    })
    .unwrap_err();
    assert!(
        matches!(error, engine::PipelineError::StageExecution { stage, .. } if stage == "early")
    );
    assert_eq!(*entered.borrow(), ["early"]);
    let mut legacy = pipeline;
    legacy.stages[0].project.graph.nodes.insert(
        9,
        mapping::Node::SourceField {
            path: vec!["Value".into()],
            frame: None,
        },
    );
    let execution = engine::ExecutionContext::new(Path::new("pipeline.json"))
        .with_debug_hook(&observers)
        .with_trace_sink(&observers);
    let outcome = engine::run_pipeline_with_context(&legacy, &hosts, &execution).unwrap();
    assert_eq!(
        outcome
            .stages
            .iter()
            .map(|stage| stage.id.as_str())
            .collect::<Vec<_>>(),
        ["early", "later"]
    );
    assert!(observers.0.get() > 0);
}

#[test]
fn payload_observer_refusal_precedes_malformed_field_and_collection_decoding() -> anyhow::Result<()>
{
    let observers = Observers::default();
    for project in [
        owned_source_test_project(),
        owned_collection_host_positive_project(),
    ] {
        let document = PayloadDocument::new(Path::new("input.json"), b"not JSON")?;
        for (options, context) in [
            (
                PayloadRunOptions::new(document).with_debug_hook(&observers),
                OwnedCapabilityContext::Debugging,
            ),
            (
                PayloadRunOptions::new(document).with_trace_sink(&observers),
                OwnedCapabilityContext::Tracing,
            ),
            (
                PayloadRunOptions::new(document)
                    .with_debug_hook(&observers)
                    .with_trace_sink(&observers),
                OwnedCapabilityContext::Tracing,
            ),
        ] {
            let error = run_project_value_payloads(&project, Path::new("project.json"), &options)
                .unwrap_err();
            assert_observer_error(&error, context);
            assert_eq!(observers.0.get(), 0);
        }
    }
    let project = selected_project();
    let document = PayloadDocument::new(Path::new("input.json"), br#"{"Value":"selected"}"#)?;
    let outcome = run_project_value_payloads(
        &project,
        Path::new("project.json"),
        &PayloadRunOptions::new(document)
            .with_target(TargetSelection::Primary)
            .with_debug_hook(&observers)
            .with_trace_sink(&observers),
    )?;
    assert_eq!(outcome.artifacts.len(), 1);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&outcome.artifacts[0].bytes)?,
        serde_json::json!({"Value":"selected"})
    );
    assert!(observers.0.get() > 0);
    Ok(())
}

#[test]
fn file_observer_refusal_precedes_missing_or_malformed_input_and_publication() -> anyhow::Result<()>
{
    let directory = owned_host_test_directory("observer-file")?;
    let input = directory.join("input.json");
    let output = directory.join("output.json");
    let path = directory.join("project.json");
    let observers = Observers::default();
    let published = Cell::new(0);
    let publish = || {
        published.set(published.get() + 1);
        true
    };
    std::fs::write(&output, b"old-output")?;
    for malformed in [false, true] {
        if malformed {
            std::fs::write(&input, b"not JSON")?;
        }
        for project in [
            owned_source_test_project(),
            owned_collection_host_positive_project(),
        ] {
            for (options, context) in [
                (
                    RunOptions::new().with_debug_hook(&observers),
                    OwnedCapabilityContext::Debugging,
                ),
                (
                    RunOptions::new().with_trace_sink(&observers),
                    OwnedCapabilityContext::Tracing,
                ),
            ] {
                let error = run_project_value_with_filesystem_policy(
                    &project,
                    &path,
                    &options.with_before_publish(&publish),
                    None,
                )
                .unwrap_err();
                assert_observer_error(&error, context);
                assert_eq!(observers.0.get(), 0);
                assert_eq!(published.get(), 0);
                assert_eq!(std::fs::read(&output)?.as_slice(), b"old-output".as_slice());
            }
        }
    }
    std::fs::remove_file(input)?;
    std::fs::remove_file(output)?;
    std::fs::remove_dir(directory)?;
    Ok(())
}

#[test]
fn stdio_trace_refusal_leaves_input_unread_and_retains_output_policy() -> anyhow::Result<()> {
    struct Input(usize);
    impl Read for Input {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            self.0 += 1;
            panic!("observer refusal must precede stdin reads")
        }
    }
    let directory = owned_host_test_directory("observer-stdio")?;
    let path = directory.join("project.json");
    let observers = Observers::default();
    let mut input = Input(0);
    for project in [
        owned_source_test_project(),
        owned_collection_host_positive_project(),
    ] {
        std::fs::write(&path, serde_json::to_vec(&project)?)?;
        let options = StandardIoRunOptions::new()
            .with_input_path(Path::new("-"))
            .with_output_path(Path::new("-"))
            .with_trace_sink(&observers);
        let error = run_project_with_standard_streams(&path, &options, &mut input).unwrap_err();
        assert_observer_error(&error, OwnedCapabilityContext::Tracing);
        let error = run_project_with_standard_streams(
            &path,
            &options.with_output_path(Path::new("output.json")),
            &mut input,
        )
        .unwrap_err();
        assert!(error.to_string().contains("requires `--output -`"));
        assert_eq!(input.0, 0);
        assert_eq!(observers.0.get(), 0);
    }
    std::fs::remove_file(path)?;
    std::fs::remove_dir(directory)?;
    Ok(())
}

#[test]
fn file_pipeline_refuses_later_unpublished_owned_target_before_earlier_callbacks()
-> anyhow::Result<()> {
    let directory = owned_host_test_directory("observer-pipeline")?;
    let path = directory.join("pipeline.json");
    let input = directory.join("input.json");
    let output = directory.join("early.json");
    std::fs::write(&path, serde_json::to_vec(&later_owned_pipeline())?)?;
    std::fs::write(&output, b"old-output")?;
    let calls = Cell::new(0);
    let debug = |_: &str, _: &engine::PendingTargetWrite| {
        calls.set(calls.get() + 1);
        DebugDecision::Resume
    };
    let trace = |_: &str, _: engine::TraceEvent| {
        calls.set(calls.get() + 1);
    };
    let probe = |_: &str| {
        calls.set(calls.get() + 1);
        None
    };
    let publish = || {
        calls.set(calls.get() + 1);
        true
    };
    let hosts = [PipelineHostFile {
        name: "input".into(),
        path: input.clone(),
    }];
    let outputs = [PipelineOutputFile {
        stage: "early".into(),
        target: None,
        path: output.clone(),
    }];
    for bytes in [b"not JSON".as_slice(), br#"{"Value":"valid"}"#.as_slice()] {
        std::fs::write(&input, bytes)?;
        for (options, context) in [
            (
                PipelineRunOptions::default().with_stage_debug_hook(&debug),
                OwnedCapabilityContext::Debugging,
            ),
            (
                PipelineRunOptions::default().with_stage_trace_sink(&trace),
                OwnedCapabilityContext::Tracing,
            ),
            (
                PipelineRunOptions::default().with_stage_source_field_probe(&probe),
                OwnedCapabilityContext::Debugging,
            ),
        ] {
            let error = run_pipeline_file_with_options(
                &path,
                &hosts,
                &outputs,
                &options.with_before_publish(&publish),
            )
            .unwrap_err();
            assert_stage_error(&error, context);
            assert_eq!(calls.get(), 0);
            assert_eq!(std::fs::read(&output)?.as_slice(), b"old-output".as_slice());
        }
    }
    let mut legacy = later_owned_pipeline();
    legacy.stages[0].project.graph.nodes.insert(
        9,
        mapping::Node::SourceField {
            path: vec!["Value".into()],
            frame: None,
        },
    );
    std::fs::write(&path, serde_json::to_vec(&legacy)?)?;
    let probe_stages = RefCell::new(Vec::new());
    let legacy_probe = |stage: &str| {
        probe_stages.borrow_mut().push(stage.to_owned());
        Some((0, "Value".into()))
    };
    let outcome = run_pipeline_file_with_options(
        &path,
        &hosts,
        &outputs,
        &PipelineRunOptions::default().with_stage_source_field_probe(&legacy_probe),
    )?;
    assert_eq!(outcome.stages_executed, ["early", "later"]);
    assert!(probe_stages.borrow().iter().any(|stage| stage == "early"));
    assert!(probe_stages.borrow().iter().any(|stage| stage == "later"));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&std::fs::read(&output)?)?,
        serde_json::json!({"Value":"valid"})
    );
    std::fs::remove_file(path)?;
    std::fs::remove_file(input)?;
    std::fs::remove_file(output)?;
    std::fs::remove_dir(directory)?;
    Ok(())
}

#[test]
fn preview_refuses_all_observer_requests_before_decoding_or_earlier_stage_callbacks()
-> anyhow::Result<()> {
    let pipeline = later_owned_pipeline();
    let calls = Cell::new(0);
    let debug = |_: &str, _: &engine::PendingTargetWrite| {
        calls.set(calls.get() + 1);
        DebugDecision::Resume
    };
    let node = |_: &str, _: &engine::PendingNodeValue| {
        calls.set(calls.get() + 1);
        DebugDecision::Resume
    };
    let function_node = |_: &str, _: &engine::PendingFunctionNodeValue| {
        calls.set(calls.get() + 1);
        DebugDecision::Resume
    };
    let input = |_: &str, _: &engine::PendingNodeInput| {
        calls.set(calls.get() + 1);
        DebugDecision::Resume
    };
    let function_input = |_: &str, _: &engine::PendingFunctionNodeInput| {
        calls.set(calls.get() + 1);
        DebugDecision::Resume
    };
    let failure = |_: &str, _: &engine::PendingNodeFailure| {
        calls.set(calls.get() + 1);
        DebugDecision::Resume
    };
    let function_failure = |_: &str, _: &engine::PendingFunctionNodeFailure| {
        calls.set(calls.get() + 1);
        DebugDecision::Resume
    };
    let probe = |_: &str| {
        calls.set(calls.get() + 1);
        None
    };
    let trace = |_: &str, _: engine::TraceEvent| {
        calls.set(calls.get() + 1);
    };
    let cancellation = AtomicBool::new(false);
    for bytes in [b"not JSON".as_slice(), br#"{"Value":"valid"}"#.as_slice()] {
        let document = PayloadDocument::new(Path::new("input.json"), bytes)?;
        let hosts = [PipelineHostPayload::new("input", document)?];
        for (options, context) in [
            (
                PipelinePreviewOptions::new(&hosts).with_stage_debug_hook(&debug),
                OwnedCapabilityContext::Debugging,
            ),
            (
                PipelinePreviewOptions::new(&hosts).with_stage_node_debug_hook(&node),
                OwnedCapabilityContext::Debugging,
            ),
            (
                PipelinePreviewOptions::new(&hosts)
                    .with_stage_function_node_debug_hook(&function_node),
                OwnedCapabilityContext::Debugging,
            ),
            (
                PipelinePreviewOptions::new(&hosts).with_stage_input_debug_hook(&input),
                OwnedCapabilityContext::Debugging,
            ),
            (
                PipelinePreviewOptions::new(&hosts)
                    .with_stage_function_input_debug_hook(&function_input),
                OwnedCapabilityContext::Debugging,
            ),
            (
                PipelinePreviewOptions::new(&hosts).with_stage_node_failure_debug_hook(&failure),
                OwnedCapabilityContext::Debugging,
            ),
            (
                PipelinePreviewOptions::new(&hosts)
                    .with_stage_function_failure_debug_hook(&function_failure),
                OwnedCapabilityContext::Debugging,
            ),
            (
                PipelinePreviewOptions::new(&hosts).with_stage_source_field_probe(&probe),
                OwnedCapabilityContext::Debugging,
            ),
            (
                PipelinePreviewOptions::new(&hosts).with_cancellation(&cancellation),
                OwnedCapabilityContext::Debugging,
            ),
            (
                PipelinePreviewOptions::new(&hosts).with_stage_trace_sink(&trace),
                OwnedCapabilityContext::Tracing,
            ),
            (
                PipelinePreviewOptions::new(&hosts)
                    .with_stage_debug_hook(&debug)
                    .with_stage_trace_sink(&trace),
                OwnedCapabilityContext::Tracing,
            ),
        ] {
            let validation =
                validate_pipeline_preview(&pipeline, Path::new("pipeline.json"), &options)
                    .unwrap_err();
            assert_stage_error(&validation, context);
            let execution =
                preview_pipeline_value_payloads(&pipeline, Path::new("pipeline.json"), &options)
                    .unwrap_err();
            assert_stage_error(&execution, context);
            assert_eq!(calls.get(), 0);
        }
        let cancelled = AtomicBool::new(true);
        let options = PipelinePreviewOptions::new(&hosts)
            .with_cancellation(&cancelled)
            .with_stage_trace_sink(&trace);
        let error =
            preview_pipeline_value_payloads(&pipeline, Path::new("pipeline.json"), &options)
                .unwrap_err();
        assert_eq!(
            error.downcast_ref::<EngineError>(),
            Some(&EngineError::DebugCancelled)
        );
        assert_eq!(calls.get(), 0);
        let plain = PipelinePreviewOptions::new(&hosts);
        validate_pipeline_preview(&pipeline, Path::new("pipeline.json"), &plain)?;
        let outcome =
            preview_pipeline_value_payloads(&pipeline, Path::new("pipeline.json"), &plain);
        if bytes == b"not JSON" {
            let error = outcome.unwrap_err();
            assert!(error.downcast_ref::<engine::PipelineError>().is_none());
            assert_eq!(error.to_string(), "reading pipeline host input `input`");
        } else {
            let outcome = outcome?;
            assert_eq!(outcome.stages_executed, ["early", "later"]);
            assert_eq!(outcome.artifacts.len(), 3);
            for artifact in outcome.artifacts {
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&artifact.bytes)?,
                    serde_json::json!({"Value":"valid"})
                );
            }
        }
    }
    Ok(())
}

#[test]
fn rest_observer_refusal_precedes_request_files_and_retains_policy_and_cancel_order() {
    let project = owned_source_test_project();
    let observers = Observers::default();
    let cancel = || false;
    let mut options =
        RestJsonMappingOptions::new(Path::new("response.json"), Path::new("output.json"));
    options.debug_hook = Some(&observers);
    options.trace_sink = Some(&observers);
    options.should_continue = Some(&cancel);
    let run = |options: &RestJsonMappingOptions<'_>, policy| {
        run_project_value_rest_json_request_file_payloads(
            &project,
            Path::new("project.json"),
            Path::new("absent-request.json"),
            None,
            policy,
            options,
        )
    };
    assert!(matches!(
        run(&options, RestExecutionPolicy::Deny),
        Err(RestJsonError::Disabled)
    ));
    let allow = RestExecutionPolicy::AllowSingleRequest {
        allow_insecure_http: false,
    };
    let RestJsonError::Mapping(error) = run(&options, allow).unwrap_err() else {
        panic!("mapping refusal expected");
    };
    assert_observer_error(&error, OwnedCapabilityContext::Tracing);
    options.response_path = Path::new("response.xml");
    assert!(matches!(
        run(&options, allow),
        Err(RestJsonError::RequestPolicy(_))
    ));
    options.response_path = Path::new("response.json");
    options.trace_sink = None;
    let RestJsonError::Mapping(error) = run(&options, allow).unwrap_err() else {
        panic!("mapping refusal expected");
    };
    assert_observer_error(&error, OwnedCapabilityContext::Debugging);
    options.debug_hook = None;
    // The in-memory route checks cancellation after mapping preflight and before transport.
    assert!(matches!(
        run_project_value_rest_json_payloads(
            &project,
            Path::new("project.json"),
            &RestJsonRequest::get("invalid-url"),
            allow,
            &options,
        ),
        Err(RestJsonError::Cancelled)
    ));
    assert_eq!(observers.0.get(), 0);
}
