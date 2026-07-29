use super::*;

fn trace_event(node: mapping::NodeId, value: ir::Value) -> cli::TraceEvent {
    let (value_type, preview) = match value {
        ir::Value::Null => ("null", "null".to_owned()),
        ir::Value::JsonNull(_) => ("json-null", "json-null".to_owned()),
        ir::Value::XmlNil(_) => ("xml-nil", "xml-nil".to_owned()),
        ir::Value::Bool(value) => ("bool", value.to_string()),
        ir::Value::Int(value) => ("int", value.to_string()),
        ir::Value::Float(value) => ("float", value.to_string()),
        ir::Value::String(value) => ("string", value),
    };
    cli::TraceEvent::NodeValue {
        node,
        positions: Vec::new(),
        value: cli::TraceValue {
            value_type,
            preview,
            truncated: false,
        },
    }
}

fn trace_scope() -> cli::TraceScope {
    cli::TraceScope {
        target: cli::TraceTarget::Named("Audit".into()),
        target_path: vec!["Orders".into(), "Line".into()],
        structural_path: vec![1, 3],
    }
}

fn temporary_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "ferrule_gui_run_report_{name}_{}",
        std::process::id()
    ))
}

#[test]
fn text_previews_are_bounded_without_splitting_utf8() {
    let path = temporary_path("text");
    let mut content = "a".repeat(MAX_PREVIEW_BYTES - 1);
    content.push('é');
    std::fs::write(&path, content).expect("preview fixture is written");

    let preview = read_preview(&path).expect("preview is read");
    let OutputPreview::Text {
        content,
        total_bytes,
        truncated,
    } = preview
    else {
        panic!("UTF-8 preview should remain text");
    };
    assert!(truncated);
    assert_eq!(total_bytes, MAX_PREVIEW_BYTES as u64 + 1);
    assert_eq!(content.len(), MAX_PREVIEW_BYTES - 1);
    std::fs::remove_file(path).expect("preview fixture is removed");
}

#[test]
fn binary_and_missing_outputs_have_explicit_preview_states() {
    let path = temporary_path("binary");
    std::fs::write(&path, [0xff, 0x00, 0x80]).expect("preview fixture is written");
    assert_eq!(
        read_preview(&path).expect("preview is read"),
        OutputPreview::Binary {
            content: "ff 00 80".into(),
            total_bytes: 3,
            truncated: false,
        }
    );
    std::fs::remove_file(&path).expect("preview fixture is removed");
    assert!(matches!(
        OutputPreview::read(&path),
        OutputPreview::Unavailable { .. }
    ));
}

#[test]
fn report_construction_defers_output_reads_until_selected() {
    let missing = temporary_path("lazy");
    let outcome = cli::RunOutcome {
        records_written: 2,
        input_path: PathBuf::from("input.xml"),
        output_path: missing.clone(),
        primary_outputs: Vec::new(),
        extra_outputs: Vec::new(),
        artifacts: vec![cli::WrittenOutput {
            name: "Primary".into(),
            records_written: 2,
            path: missing,
        }],
    };

    let mut report = RunReport::from_outcome_with_trace(
        outcome,
        Duration::from_millis(12),
        TraceReport::default(),
    );

    assert!(report.outputs[0].preview.is_none());
    assert!(matches!(
        report.outputs[0].preview(),
        OutputPreview::Unavailable { .. }
    ));
}

#[test]
fn payload_reports_keep_ordered_bounded_previews_in_memory() {
    let outcome = cli::PayloadRunOutcome {
        records_written: 2,
        artifacts: vec![
            cli::PayloadArtifact {
                target: "Primary".into(),
                records_written: 1,
                path: PathBuf::from("first.json"),
                bytes: br#"{"value":"first"}"#.to_vec(),
            },
            cli::PayloadArtifact {
                target: "Primary".into(),
                records_written: 1,
                path: PathBuf::from("second.bin"),
                bytes: vec![0xff, 0x00],
            },
        ],
    };

    let mut report = RunReport::from_payload_with_trace(
        outcome,
        PathBuf::from("input.json"),
        Duration::from_millis(3),
        TraceReport::default(),
    );

    assert_eq!(report.kind, RunReportKind::Preview);
    assert_eq!(
        report
            .outputs
            .iter()
            .map(|output| output.name.as_str())
            .collect::<Vec<_>>(),
        ["Primary - first.json", "Primary - second.bin"]
    );
    assert!(report.outputs.iter().all(|output| output.in_memory));
    assert!(matches!(
        report.outputs[0].preview(),
        OutputPreview::Text { content, .. } if content == r#"{"value":"first"}"#
    ));
    assert_eq!(
        report.outputs[1].preview(),
        &OutputPreview::Binary {
            content: "ff 00".into(),
            total_bytes: 2,
            truncated: false,
        }
    );
}

#[test]
fn dynamic_and_extra_outputs_keep_their_declared_order() {
    let outcome = cli::RunOutcome {
        records_written: 3,
        input_path: PathBuf::from("input.xml"),
        output_path: PathBuf::from("dynamic-base"),
        primary_outputs: vec![cli::WrittenOutput {
            name: "Primary 1".into(),
            records_written: 2,
            path: PathBuf::from("first.xml"),
        }],
        extra_outputs: vec![cli::WrittenOutput {
            name: "Audit".into(),
            records_written: 1,
            path: PathBuf::from("audit.json"),
        }],
        artifacts: vec![
            cli::WrittenOutput {
                name: "Primary 1".into(),
                records_written: 2,
                path: PathBuf::from("first.xml"),
            },
            cli::WrittenOutput {
                name: "Audit".into(),
                records_written: 1,
                path: PathBuf::from("audit.json"),
            },
        ],
    };

    let report = RunReport::from_outcome_with_trace(
        outcome,
        Duration::from_millis(4),
        TraceReport::default(),
    );

    assert_eq!(
        report
            .outputs
            .iter()
            .map(|output| output.name.as_str())
            .collect::<Vec<_>>(),
        ["Primary 1", "Audit"]
    );
    assert!(report.outputs.iter().all(|output| output.preview.is_none()));
}

#[test]
fn summary_units_are_compact_and_deterministic() {
    assert_eq!(format_records(1), "1 record");
    assert_eq!(format_records(2), "2 records");
    assert_eq!(format_duration(Duration::from_millis(1250)), "1.25 s");
    assert_eq!(format_bytes(1536), "1.5 KiB");
}

#[test]
fn trace_collection_is_bounded_and_reports_omissions() {
    let collector = TraceCollector::with_limit(2);
    cli::TraceSink::record(&collector, trace_event(1, ir::Value::Int(10)));
    cli::TraceSink::record(&collector, trace_event(2, ir::Value::String("kept".into())));
    cli::TraceSink::record(&collector, trace_event(3, ir::Value::Bool(false)));

    let trace = collector.finish();
    assert_eq!(trace.events.len(), 2);
    assert_eq!(trace.dropped, 1);
    assert!(trace_row(1, &trace.events[1]).contains("node 2"));
}

#[test]
fn scope_trace_rows_expose_searchable_control_context() {
    let started = cli::TraceEvent::ScopeStarted {
        scope: trace_scope(),
        iteration: cli::TraceIteration::Source {
            path: vec!["Order".into(), "Line".into()],
        },
        positions: Vec::new(),
    };
    let filtered = cli::TraceEvent::FilterDecision {
        scope: trace_scope(),
        node: 42,
        phase: cli::TraceFilterPhase::BeforeSort,
        positions: vec![cli::TracePosition {
            collection: vec!["Order".into()],
            index: 3,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        }],
        passed: false,
    };
    let window = cli::TraceEvent::WindowApplied {
        scope: trace_scope(),
        window_index: 2,
        window: cli::TraceWindow::FromTo { first: 2, last: 4 },
        before: 8,
        after: 3,
    };

    assert!(trace_row(0, &started).contains("named:Audit:/Orders/Line #1.3"));
    assert!(trace_row(0, &started).contains("iterate source Order/Line"));
    assert!(trace_row(1, &filtered).contains("filter node 42 before-sort drop"));
    assert!(trace_row(1, &filtered).contains("Order[3]"));
    assert!(trace_row(2, &window).contains("window 2 from 2 to 4  8 -> 3"));
}

#[test]
fn target_field_rows_expose_searchable_binding_context() {
    let written = cli::TraceEvent::TargetFieldWritten {
        scope: trace_scope(),
        field: "delivery-window".into(),
        binding: cli::TraceTargetFieldBinding::DynamicBinding { key: 17, value: 23 },
        positions: Vec::new(),
        kind: cli::TraceOutputKind::Scalar,
        value: Some(cli::TraceValue {
            value_type: "xml-nil",
            preview: "xml-nil".into(),
            truncated: false,
        }),
    };

    let row = trace_row(3, &written);
    assert!(row.contains("field delivery-window"));
    assert!(row.contains("dynamic-binding key-node=17 value-node=23"));
    assert!(row.contains("write scalar value=xml-nil(xml-nil)"));
}

#[test]
fn results_window_renders_and_loads_only_the_selected_preview() {
    let first = temporary_path("window-first");
    let second = temporary_path("window-second");
    std::fs::write(&first, "<result>ok</result>").expect("first output is written");
    std::fs::write(&second, "not selected").expect("second output is written");
    let report = RunReport {
        kind: RunReportKind::Run,
        duration: Duration::from_millis(8),
        records_written: 1,
        input_path: PathBuf::from("input.xml"),
        outputs: vec![
            RunOutput::new("Primary".into(), 1, first.clone()),
            RunOutput::new("Audit".into(), 1, second.clone()),
        ],
        trace: TraceReport {
            events: vec![trace_event(7, ir::Value::String("ok".into()))],
            dropped: 0,
        },
    };
    let mut view = RunReportView::new(report);
    let mut open = true;
    let context = egui::Context::default();
    crate::icons::install(&context);

    let output = context.run_ui(Default::default(), |ui| {
        show(ui.ctx(), &mut open, &mut view);
    });

    assert!(open);
    assert!(!output.shapes.is_empty());
    assert!(view.report.outputs[0].preview.is_some());
    assert!(view.report.outputs[1].preview.is_none());

    view.page = ReportPage::Trace;
    let output = context.run_ui(Default::default(), |ui| {
        show(ui.ctx(), &mut open, &mut view);
    });
    assert!(!output.shapes.is_empty());
    std::fs::remove_file(first).expect("first output is removed");
    std::fs::remove_file(second).expect("second output is removed");
}
