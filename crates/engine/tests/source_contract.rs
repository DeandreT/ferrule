use engine::{
    EngineError, ExecutionContext, RuntimeParameters, TargetSelection, TraceEvent, TraceSink,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, ExecutionContract, Graph, Node, Project, Scope, ScopeIteration,
    SourceContractReason as Reason,
};
use std::cell::Cell;
use std::collections::BTreeMap;
use std::path::Path;

fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(name, value)| (name.into(), value))
            .collect::<Vec<_>>()
            .into(),
    )
}

fn project() -> Project {
    let scalar = |name| SchemaNode::scalar(name, ScalarType::String);
    Project {
        execution_contract: Some(ExecutionContract::ClosedPrimarySourceV1),
        source: SchemaNode::group(
            "Input",
            vec![
                scalar("Root"),
                SchemaNode::group(
                    "Row",
                    vec![
                        scalar("Text"),
                        SchemaNode::group("Cell", vec![scalar("Text")]).repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::group("Row", vec![scalar("Result")]).repeating()],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["Text".into()],
                    frame: Some(vec!["Row".into()]),
                },
            )]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Row".into()]),
                bindings: vec![Binding {
                    target_field: "Result".into(),
                    node: 0,
                }],
                ..Default::default()
            }],
            ..Default::default()
        },
    }
}

struct NoEvents(Cell<usize>);
impl TraceSink for NoEvents {
    fn record(&self, _: TraceEvent) {
        self.0.set(self.0.get() + 1);
    }
}

impl engine::DebugHook for NoEvents {
    fn before_target_write(&self, _: &engine::PendingTargetWrite) -> engine::DebugDecision {
        self.0.set(self.0.get() + 1);
        engine::DebugDecision::Resume
    }
    fn after_node_value(&self, _: &engine::PendingNodeValue) -> engine::DebugDecision {
        self.0.set(self.0.get() + 1);
        engine::DebugDecision::Resume
    }
}
impl engine::DynamicSourceLoader for NoEvents {
    fn load(&self, _: &str, _: &str) -> Result<std::sync::Arc<Instance>, String> {
        self.0.set(self.0.get() + 1);
        Err("loader must not run".into())
    }
}
impl document_source::PrimarySourceHost for NoEvents {
    type Error = &'static str;
    fn select(
        &self,
        _: &str,
        _: mapping::PrimaryFileSelection,
    ) -> Result<Vec<String>, Self::Error> {
        self.0.set(self.0.get() + 1);
        Err("primary selection must not run")
    }
    fn load(&self, _: &str) -> Result<document_source::SourceData, Self::Error> {
        self.0.set(self.0.get() + 1);
        Err("primary load must not run")
    }
}

#[test]
fn every_public_output_family_rejects_sparse_inputs_before_trace_events() {
    let project = project();
    let input = group(vec![
        ("Root", text("ROOT")),
        (
            "Row",
            Instance::Repeated(vec![group(vec![("Text", text("OUTER"))])]),
        ),
    ]);
    let trace = NoEvents(Cell::new(0));
    let context = ExecutionContext::new(Path::new("guarded.json"))
        .with_trace_sink(&trace)
        .with_debug_hook(&trace)
        .with_dynamic_source_loader(&trace);
    let outcomes = [
        engine::run(&project, &input).map(|_| ()),
        engine::run_outputs(&project, &input).map(|_| ()),
        engine::run_selected_target(&project, &input, TargetSelection::Primary).map(|_| ()),
        engine::run_with_context(&project, &input, &context).map(|_| ()),
        engine::run_with_sources(&project, &input, vec![]).map(|_| ()),
        engine::run_with_sources_and_context(&project, &input, vec![], &context).map(|_| ()),
        engine::run_outputs_with_sources_and_context(&project, &input, vec![], &context)
            .map(|_| ()),
        engine::run_selected_target_with_sources_and_context(
            &project,
            &input,
            vec![],
            &context,
            TargetSelection::Primary,
        )
        .map(|_| ()),
    ];
    for result in outcomes {
        assert!(
            matches!(result, Err(EngineError::SourceContractViolation(error)) if error.reason == Reason::MissingRepeatedSlot)
        );
    }
    assert_eq!(trace.0.get(), 0);
}

#[test]
fn unexpected_extras_and_parameters_are_rejected_by_supplied_count_not_declared_names() {
    let project = project();
    let input = group(vec![("Row", Instance::Repeated(vec![]))]);
    let extras = || {
        vec![
            ("same".into(), text("PRIVATE-A")),
            ("same".into(), text("PRIVATE-B")),
        ]
    };
    let trace = NoEvents(Cell::new(0));
    let mut parameters = RuntimeParameters::new();
    parameters
        .insert("undeclared", Value::String("PRIVATE-PARAMETER".into()))
        .unwrap();
    let context = ExecutionContext::new(Path::new("guarded.json")).with_trace_sink(&trace);
    for result in [
        engine::run_with_sources(&project, &input, extras()).map(|_| ()),
        engine::run_with_sources_and_context(&project, &input, extras(), &context).map(|_| ()),
        engine::run_outputs_with_sources_and_context(&project, &input, extras(), &context)
            .map(|_| ()),
        engine::run_selected_target_with_sources_and_context(
            &project,
            &input,
            extras(),
            &context,
            TargetSelection::Primary,
        )
        .map(|_| ()),
    ] {
        let Err(EngineError::SourceContractViolation(error)) = result else {
            panic!("must refuse supplied bindings");
        };
        assert_eq!(error.reason, Reason::UnexpectedBinding);
        assert!(!error.to_string().contains("PRIVATE"));
    }
    let context = context.with_parameters(&parameters);
    for result in [
        engine::run_with_context(&project, &input, &context).map(|_| ()),
        engine::run_with_sources_and_context(&project, &input, vec![], &context).map(|_| ()),
        engine::run_outputs_with_sources_and_context(&project, &input, vec![], &context)
            .map(|_| ()),
        engine::run_selected_target_with_sources_and_context(
            &project,
            &input,
            vec![],
            &context,
            TargetSelection::Primary,
        )
        .map(|_| ()),
    ] {
        assert!(
            matches!(result, Err(EngineError::SourceContractViolation(error)) if error.reason == Reason::UnexpectedParameter)
        );
    }
    assert_eq!(trace.0.get(), 0);
    let empty = RuntimeParameters::new();
    let context = ExecutionContext::new(Path::new("guarded.json")).with_parameters(&empty);
    assert_eq!(
        engine::run_with_context(&project, &input, &context).unwrap(),
        group(vec![("Row", Instance::Repeated(vec![]))])
    );
}

#[test]
fn legacy_models_keep_existing_innermost_shadowing_but_guarded_models_refuse_it() {
    let mut project = project();
    let ir::SchemaKind::Group { children, .. } = &mut project.source.kind else {
        unreachable!()
    };
    children[0].name = "Text".into();
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Text".into()],
            frame: None,
        },
    );
    let input = group(vec![
        ("Text", text("ROOT")),
        (
            "Row",
            Instance::Repeated(vec![group(vec![
                ("Text", text("ROW")),
                ("Cell", Instance::Repeated(vec![])),
            ])]),
        ),
    ]);
    assert!(
        matches!(engine::run(&project, &input), Err(EngineError::SourceContractViolation(error)) if error.reason == Reason::UnprovedOwner)
    );
    project.execution_contract = None;
    assert_eq!(
        engine::run(&project, &input).unwrap(),
        group(vec![(
            "Row",
            Instance::Repeated(vec![group(vec![("Result", text("ROW"))])])
        )])
    );
}

#[test]
fn injected_undeclared_shadow_fields_fail_even_when_the_saved_root_broadcast_was_proved() {
    let mut project = project();
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Root".into()],
            frame: None,
        },
    );
    let row = |injected| {
        let mut fields = vec![
            ("Text", text("OUTER")),
            ("Cell", Instance::Repeated(vec![])),
        ];
        if injected {
            fields.push(("Root", text("HIDDEN")));
        }
        group(vec![
            ("Root", text("ROOT")),
            ("Row", Instance::Repeated(vec![group(fields)])),
        ])
    };
    assert_eq!(
        engine::run(&project, &row(false)).unwrap(),
        group(vec![(
            "Row",
            Instance::Repeated(vec![group(vec![("Result", text("ROOT"))])])
        )])
    );
    assert!(
        matches!(engine::run(&project, &row(true)), Err(EngineError::SourceContractViolation(error)) if error.reason == Reason::UnexpectedField)
    );
}

#[test]
fn invalid_guarded_models_refuse_before_failures_or_loader_observers() {
    let mut project = project();
    project.extra_sources.push(mapping::NamedSource {
        name: "unexpected".into(),
        path: "DO-NOT-LOAD.xml".into(),
        schema: SchemaNode::group("Extra", vec![]),
        options: Default::default(),
        dynamic_path: Some(mapping::DynamicSourcePath::legacy(9, vec!["Row".into()])),
    });
    project.graph.nodes.insert(
        9,
        Node::Const {
            value: Value::String("DO-NOT-LOAD.xml".into()),
        },
    );
    project.failure_rules.push(mapping::FailureRule {
        iteration: mapping::FailureIteration::Source {
            collection: vec!["Row".into()],
        },
        selection: mapping::FailureSelection::All,
        message: Some(9),
    });
    let events = NoEvents(Cell::new(0));
    let context = ExecutionContext::new(Path::new("guarded.json"))
        .with_trace_sink(&events)
        .with_debug_hook(&events)
        .with_dynamic_source_loader(&events);
    assert!(
        matches!(engine::run_with_context(&project, &group(vec![]), &context), Err(EngineError::SourceContractViolation(error)) if error.reason == Reason::UnsupportedModel)
    );
    assert_eq!(events.0.get(), 0);
}

#[test]
fn pipeline_entry_points_cannot_bypass_the_stage_input_guard() {
    let pipeline = mapping::Pipeline {
        main_mapping_path: None,
        stages: vec![mapping::PipelineStage {
            id: "guarded".into(),
            mapping_path: None,
            project: project(),
            source: mapping::PipelineInput::Host {
                name: "input".into(),
            },
            extra_sources: vec![],
        }],
    };
    let hosts = BTreeMap::from([(
        "input".into(),
        group(vec![(
            "Row",
            Instance::Repeated(vec![group(vec![("Text", text("OUTER"))])]),
        )]),
    )]);
    let events = NoEvents(Cell::new(0));
    let context = ExecutionContext::new(Path::new("guarded.json"))
        .with_trace_sink(&events)
        .with_debug_hook(&events);
    for result in [
        engine::run_pipeline(&pipeline, &hosts).map(|_| ()),
        engine::run_pipeline_with_context(&pipeline, &hosts, &context).map(|_| ()),
        engine::run_pipeline_with_stage_contexts(&pipeline, &hosts, |_| context).map(|_| ()),
    ] {
        assert!(
            matches!(result, Err(engine::PipelineError::StageExecution { source: EngineError::SourceContractViolation(error), .. }) if error.reason == Reason::MissingRepeatedSlot)
        );
    }
    assert!(
        matches!(engine::run_pipeline_retaining(&pipeline, hosts.clone(), &[], |_| context),
        Err(engine::PipelineError::StageExecution { source: EngineError::SourceContractViolation(error), .. }) if error.reason == Reason::MissingRepeatedSlot)
    );
    assert_eq!(events.0.get(), 0);
}

#[test]
fn document_and_filename_entries_reject_parameters_before_primary_acquisition() {
    let mut project = project();
    project.source_path = Some("input.xml".into());
    project.target_path = Some("output.xml".into());
    project.source_options.xml_document = true;
    project.target_options.xml_document = true;
    project.graph.nodes.insert(
        9,
        Node::Const {
            value: Value::String("DO-NOT-LOAD.xml".into()),
        },
    );
    let document = mapping::DocumentProject::new(project, mapping::PrimarySourcePlan::new(9));
    let events = NoEvents(Cell::new(0));
    let mut parameters = RuntimeParameters::new();
    parameters
        .insert("unexpected", Value::String("PRIVATE".into()))
        .unwrap();
    let context = ExecutionContext::new(Path::new("guarded.json"))
        .with_parameters(&parameters)
        .with_trace_sink(&events)
        .with_debug_hook(&events);
    let parsed = engine::run_primary_document(
        &document,
        &context,
        &events,
        1,
        |_, _| -> Result<Instance, &'static str> { panic!("parse must not run") },
    );
    assert!(
        matches!(parsed, Err(engine::PrimaryDocumentRunError::Mapping(EngineError::SourceContractViolation(error))) if error.reason == Reason::UnexpectedParameter)
    );
    assert!(
        matches!(engine::evaluate_primary_filename(&document, &context), Err(engine::PrimaryFilenameEvaluationError::Parameter(EngineError::SourceContractViolation(error))) if error.reason == Reason::UnexpectedParameter)
    );
    assert!(
        matches!(engine::evaluate_output_filename(&document, &context), Err(engine::PrimaryFilenameEvaluationError::Parameter(EngineError::SourceContractViolation(error))) if error.reason == Reason::UnexpectedParameter)
    );
    assert_eq!(events.0.get(), 0);
}

struct PrimaryDataProbe {
    calls: Cell<usize>,
    resolved_path: bool,
}
impl document_source::PrimarySourceHost for PrimaryDataProbe {
    type Error = &'static str;
    fn select(
        &self,
        filename: &str,
        _: mapping::PrimaryFileSelection,
    ) -> Result<Vec<String>, Self::Error> {
        self.calls.set(self.calls.get() + 1);
        Ok(vec![filename.into()])
    }
    fn load(&self, _: &str) -> Result<document_source::SourceData, Self::Error> {
        self.calls.set(self.calls.get() + 1);
        Ok(document_source::SourceData {
            bytes: b"<Input/>".to_vec(),
            resolved_path: self.resolved_path.then(|| "synthetic/input.xml".into()),
        })
    }
}

#[test]
fn guarded_primary_document_checks_parameters_before_text_and_resolved_path_hosts() {
    let mut project = project();
    project.source_path = Some("input.xml".into());
    project.target_path = Some("output.xml".into());
    project.source_options.xml_document = true;
    project.target_options.xml_document = true;
    project.graph.nodes.insert(
        9,
        Node::Const {
            value: Value::String("input.xml".into()),
        },
    );
    let document = mapping::DocumentProject::new(project, mapping::PrimarySourcePlan::new(9));
    assert!(engine::validate_primary_document(&document).is_empty());
    let mut parameters = RuntimeParameters::new();
    parameters
        .insert("unexpected", Value::String("PRIVATE".into()))
        .unwrap();
    let rejected = ExecutionContext::new(Path::new("guarded.json")).with_parameters(&parameters);
    let empty = ExecutionContext::new(Path::new("guarded.json"));
    for resolved_path in [false, true] {
        let host = PrimaryDataProbe {
            calls: Cell::new(0),
            resolved_path,
        };
        let parses = Cell::new(0);
        let source = || {
            group(vec![(
                "Row",
                Instance::Repeated(vec![group(vec![
                    ("Text", text("OUTER")),
                    ("Cell", Instance::Repeated(vec![])),
                ])]),
            )])
        };
        let result = engine::run_primary_document(
            &document,
            &rejected,
            &host,
            1,
            |_, _| -> Result<Instance, &'static str> {
                parses.set(parses.get() + 1);
                Ok(source())
            },
        );
        assert!(
            matches!(result, Err(engine::PrimaryDocumentRunError::Mapping(EngineError::SourceContractViolation(error))) if error.reason == Reason::UnexpectedParameter)
        );
        assert_eq!((host.calls.get(), parses.get()), (0, 0));
        let mut invalid = document.project().clone();
        invalid.graph.nodes.insert(
            10,
            Node::RuntimeValue {
                value: mapping::RuntimeValue::MappingFilePath,
            },
        );
        let invalid = mapping::DocumentProject::new(invalid, mapping::PrimarySourcePlan::new(9));
        let result = engine::run_primary_document(
            &invalid,
            &empty,
            &host,
            1,
            |_, _| -> Result<Instance, &'static str> {
                parses.set(parses.get() + 1);
                Ok(source())
            },
        );
        assert!(
            matches!(result, Err(engine::PrimaryDocumentRunError::Mapping(EngineError::SourceContractViolation(error))) if error.reason == Reason::UnsupportedModel)
        );
        assert_eq!((host.calls.get(), parses.get()), (0, 0));
        let result = engine::run_primary_document(
            &document,
            &empty,
            &host,
            1,
            |_, _| -> Result<Instance, &'static str> {
                parses.set(parses.get() + 1);
                Ok(source())
            },
        )
        .unwrap();
        assert_eq!(
            result,
            group(vec![(
                "Row",
                Instance::Repeated(vec![group(vec![("Result", text("OUTER"))])])
            )])
        );
        assert_eq!((host.calls.get(), parses.get()), (2, 1));
    }
}

#[derive(Default)]
struct PrimaryCsvProbe {
    selections: Cell<usize>,
    loads: Cell<usize>,
}

impl document_source::PrimarySourceHost for PrimaryCsvProbe {
    type Error = &'static str;

    fn select(
        &self,
        filename: &str,
        _: mapping::PrimaryFileSelection,
    ) -> Result<Vec<String>, Self::Error> {
        self.selections.set(self.selections.get() + 1);
        Ok(vec![filename.into()])
    }

    fn load(&self, _: &str) -> Result<document_source::SourceData, Self::Error> {
        self.loads.set(self.loads.get() + 1);
        Ok(document_source::SourceData {
            bytes: b"Text\nvalue\n".to_vec(),
            resolved_path: None,
        })
    }
}

#[test]
fn guarded_csv_wrappers_refuse_rows_and_invalid_models_before_selection_or_load() {
    use document_source::PrimarySourceHost;
    use format_csv::{CsvAcquisitionError, CsvByteDecodingPolicy};

    let mut project = project();
    project.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("Text", ScalarType::String)],
    );
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Result", ScalarType::String)],
    );
    project.source_path = Some("input.csv".into());
    project.target_path = Some("output.csv".into());
    project.graph.nodes.insert(
        0,
        Node::SourceField {
            path: vec!["Text".into()],
            frame: None,
        },
    );
    project.graph.nodes.insert(
        9,
        Node::Const {
            value: Value::String("input.csv".into()),
        },
    );
    project.root = Scope {
        bindings: vec![Binding {
            target_field: "Result".into(),
            node: 0,
        }],
        ..Default::default()
    };
    assert!(project.validate_execution_contract().unwrap().is_some());
    let mut parameters = RuntimeParameters::new();
    parameters
        .insert("unexpected", Value::String("PRIVATE".into()))
        .unwrap();
    let empty = ExecutionContext::new(Path::new("guarded.json"));
    let supplied = empty.with_parameters(&parameters);
    for invalid_model in [false, true] {
        let mut project = project.clone();
        let without_parameters = if invalid_model {
            project.graph.nodes.insert(
                0,
                Node::SourceField {
                    path: vec!["Missing".into()],
                    frame: None,
                },
            );
            Reason::UnprovedOwner
        } else {
            Reason::UnsupportedModel
        };
        let document = mapping::DocumentProject::new(project, mapping::PrimarySourcePlan::new(9));
        let route = document.primary_route().unwrap();
        assert_eq!(route.format, mapping::PrimaryDocumentFormat::Csv);
        assert_eq!(route.shape, mapping::PrimaryDocumentShape::Rows);
        for (context, expected) in [
            (&empty, without_parameters),
            (&supplied, Reason::UnexpectedParameter),
        ] {
            let host = PrimaryCsvProbe::default();
            let result = engine::acquire_primary_csv(
                &document,
                context,
                |filename| {
                    Ok(host
                        .select(filename, document.primary_source().selection)?
                        .into_iter()
                        .map(Ok))
                },
                |request| host.load(request),
                CsvByteDecodingPolicy::StrictUtf8,
                1,
            );
            let Err(CsvAcquisitionError::Filename(
                engine::PrimaryFilenameEvaluationError::Parameter(
                    EngineError::SourceContractViolation(error),
                ),
            )) = result
            else {
                panic!("guarded CSV acquisition must refuse before returning an iterator");
            };
            assert_eq!(error.reason, expected);
            assert_eq!((host.selections.get(), host.loads.get()), (0, 0));

            let result = engine::run_primary_csv_document(
                &document,
                context,
                &host,
                CsvByteDecodingPolicy::StrictUtf8,
                1,
            );
            let Err(engine::PrimaryDocumentRunError::Mapping(
                EngineError::SourceContractViolation(error),
            )) = result
            else {
                panic!("guarded CSV document run must refuse before acquisition");
            };
            assert_eq!(error.reason, expected);
            assert_eq!((host.selections.get(), host.loads.get()), (0, 0));
        }
    }
}
