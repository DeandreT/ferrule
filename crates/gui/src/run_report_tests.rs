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
fn node_history_keeps_each_evaluation_in_trace_order() {
    let events = vec![
        trace_event(8, ir::Value::String("first".into())),
        cli::TraceEvent::ScopeStarted {
            scope: trace_scope(),
            iteration: cli::TraceIteration::Once,
            positions: Vec::new(),
        },
        trace_event(9, ir::Value::Bool(true)),
        cli::TraceEvent::NodeInputValue {
            consumer: 8,
            input: 9,
            input_index: 0,
            positions: Vec::new(),
            value: cli::TraceValue {
                value_type: "bool",
                preview: "true".into(),
                truncated: false,
            },
        },
        trace_event(8, ir::Value::String("second".into())),
        trace_event(8, ir::Value::String("second".into())),
    ];

    let history = index_node_history(&events);
    assert_eq!(history[&NodeHistoryKey::Graph(8)], [0, 3, 4, 5]);
    assert_eq!(history[&NodeHistoryKey::Graph(9)], [2]);

    let view = RunReportView::new(RunReport {
        kind: RunReportKind::Preview,
        duration: Duration::ZERO,
        records_written: 0,
        input_path: PathBuf::from("input.json"),
        outputs: Vec::new(),
        trace: TraceReport {
            events,
            dropped: 2,
            ..Default::default()
        },
    });
    assert_eq!(view.history_node, Some(NodeHistoryKey::Graph(8)));
    assert_eq!(
        view.history_by_node[&NodeHistoryKey::Graph(8)],
        [0, 3, 4, 5]
    );
    assert_eq!(view.report.trace.dropped, 2);
    assert!(trace_row(3, &view.report.trace.events[3]).contains("node 8 input 1 <- node 9"));
}

#[test]
fn function_history_and_replay_keep_function_and_stage_identity() {
    let outer = mapping::FunctionId::new(1);
    let inner = mapping::FunctionId::new(2);
    let value = cli::TraceValue {
        value_type: "string",
        preview: "result".into(),
        truncated: false,
    };
    let collector = PipelineTraceCollector::new();
    collector.record("first", trace_event(0, ir::Value::String("main".into())));
    collector.record(
        "first",
        cli::TraceEvent::FunctionNodeValue {
            function: outer,
            node: 0,
            positions: Vec::new(),
            value: value.clone(),
        },
    );
    collector.record(
        "first",
        cli::TraceEvent::FunctionNodeInputValue {
            function: outer,
            consumer: 1,
            input: 0,
            input_index: 0,
            positions: Vec::new(),
            value: value.clone(),
        },
    );
    collector.record(
        "first",
        cli::TraceEvent::FunctionNodeValue {
            function: inner,
            node: 0,
            positions: Vec::new(),
            value: value.clone(),
        },
    );
    collector.record(
        "second",
        cli::TraceEvent::FunctionNodeValue {
            function: outer,
            node: 0,
            positions: Vec::new(),
            value,
        },
    );
    let mut view = RunReportView::new(RunReport {
        kind: RunReportKind::Pipeline,
        duration: Duration::ZERO,
        records_written: 0,
        input_path: PathBuf::from("pipeline.json"),
        outputs: Vec::new(),
        trace: collector.finish(),
    });
    assert_eq!(view.history_by_node[&NodeHistoryKey::Graph(0)], [0]);
    assert_eq!(
        view.history_by_node[&NodeHistoryKey::Function(outer, 0)],
        [1]
    );
    assert_eq!(
        view.history_by_node[&NodeHistoryKey::Function(outer, 1)],
        [2]
    );
    assert_eq!(
        view.history_by_node[&NodeHistoryKey::Function(inner, 0)],
        [3]
    );
    assert!(
        trace_row(2, &view.report.trace.events[2]).contains("function 1 node 1 input 1 <- node 0")
    );
    assert_eq!(
        NodeHistoryKey::Function(outer, 0).label(),
        "Function 1 · Node 0"
    );
    view.history_node = Some(NodeHistoryKey::Function(outer, 0));
    view.replay_event = Some(0);
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::NextSelectedNode),
        Some(1)
    );
    assert!(
        replay::replay_event_details_for_view(&view, 1)
            .unwrap()
            .join(" ")
            .contains("function 1 node 0")
    );
    view.select_history_stage(1);
    assert_eq!(view.replay_event, Some(4));
    assert_eq!(view.history_node, Some(NodeHistoryKey::Function(outer, 0)));
    assert_eq!(
        view.history_by_node[&NodeHistoryKey::Function(outer, 0)],
        [4]
    );
    assert!(!view.history_by_node.contains_key(&NodeHistoryKey::Graph(0)));
}

#[test]
fn pipeline_history_and_replay_disambiguate_reused_node_ids_by_stage() {
    let collector = PipelineTraceCollector::new();
    collector.record("prepare", trace_event(7, ir::Value::String("A".into())));
    collector.record("finish", trace_event(7, ir::Value::String("B".into())));
    collector.record("finish", trace_event(7, ir::Value::String("C".into())));
    let trace = collector.finish();
    assert_eq!(trace.stages, ["prepare", "finish"]);
    assert_eq!(trace.event_stages, [0, 1, 1]);
    let report = RunReport::from_pipeline_outcome(
        cli::PipelineRunOutcome {
            stages_executed: vec!["prepare".into(), "finish".into()],
            artifacts: Vec::new(),
        },
        PathBuf::from("pipeline.json"),
        Duration::ZERO,
        trace,
    );
    let mut view = RunReportView::new(report);
    assert_eq!(view.history_stage, Some(0));
    assert_eq!(view.history_node, Some(NodeHistoryKey::Graph(7)));
    assert_eq!(view.history_by_node[&NodeHistoryKey::Graph(7)], [0]);
    assert!(
        view.trace_row(0, &view.report.trace.events[0])
            .contains("stage prepare")
    );
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::NextSelectedNode),
        None
    );

    view.select_history_stage(1);
    assert_eq!(view.replay_event, Some(1));
    assert_eq!(view.history_by_node[&NodeHistoryKey::Graph(7)], [1, 2]);
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::NextSelectedNode),
        Some(2)
    );
    assert_eq!(
        replay::replay_event_details_for_view(&view, 1).unwrap()[0],
        "Stage: finish"
    );
    view.select_history_stage(0);
    assert_eq!(view.replay_event, Some(0));
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::NextSelectedNode),
        None
    );
}

#[test]
fn pipeline_trace_collector_bounds_events_across_all_stages() {
    let collector = PipelineTraceCollector::with_limit(2);
    collector.record("prepare", trace_event(0, ir::Value::Int(1)));
    collector.record("finish", trace_event(0, ir::Value::Int(2)));
    collector.record("finish", trace_event(0, ir::Value::Int(3)));
    let trace = collector.finish();
    assert_eq!(trace.events.len(), 2);
    assert_eq!(trace.dropped, 1);
    assert_eq!(trace.stages, ["prepare", "finish"]);
    assert_eq!(trace.event_stages, [0, 1]);
}

#[test]
fn pipeline_source_row_history_follows_the_selected_stage() {
    let collector = PipelineTraceCollector::new();
    let candidate = cli::TraceEvent::IterationCandidate {
        scope: trace_scope(),
        ordinal: 1,
        positions: Vec::new(),
        source_row: Some(cli::TraceSourceRow {
            kind: cli::TraceOutputKind::Group,
            value: None,
            fields: Vec::new(),
            omitted_fields: 0,
        }),
    };
    collector.record("prepare", candidate.clone());
    collector.record("finish", candidate);
    let view = RunReport::from_pipeline_outcome(
        cli::PipelineRunOutcome {
            stages_executed: vec!["prepare".into(), "finish".into()],
            artifacts: Vec::new(),
        },
        PathBuf::from("pipeline.json"),
        Duration::ZERO,
        collector.finish(),
    );
    let mut view = RunReportView::new(view);
    assert_eq!(source_rows::filtered_source_row_indices(&view), [0]);
    view.select_history_stage(1);
    assert_eq!(source_rows::filtered_source_row_indices(&view), [1]);
}

#[test]
fn source_row_history_indexes_candidates_and_shows_bounded_fields() {
    let candidate = cli::TraceEvent::IterationCandidate {
        scope: trace_scope(),
        ordinal: 3,
        positions: vec![cli::TracePosition {
            collection: vec!["Order".into(), "Line".into()],
            index: 4,
            grouped: false,
            join: None,
            join_position: None,
            document_path: Some("part.xml".into()),
        }],
        source_row: Some(cli::TraceSourceRow {
            kind: cli::TraceOutputKind::Group,
            value: None,
            fields: vec![
                cli::TraceSourceField {
                    name: "Descript".into(),
                    name_truncated: true,
                    kind: cli::TraceOutputKind::Scalar,
                    value: Some(cli::TraceValue {
                        value_type: "string",
                        preview: "é".into(),
                        truncated: true,
                    }),
                },
                cli::TraceSourceField {
                    name: "Children".into(),
                    name_truncated: false,
                    kind: cli::TraceOutputKind::Repeated,
                    value: None,
                },
            ],
            omitted_fields: 2,
        }),
    };
    let events = vec![
        trace_event(8, ir::Value::Int(1)),
        cli::TraceEvent::IterationCandidate {
            scope: trace_scope(),
            ordinal: 1,
            positions: Vec::new(),
            source_row: None,
        },
        candidate,
    ];
    assert_eq!(index_source_rows(&events), [2]);
    let summary = source_row_summary(2, &events[2]).expect("source row summary");
    assert!(summary.contains("event      3"));
    assert!(summary.contains("candidate 3"));
    assert!(summary.contains("Order/Line[4] @part.xml"));
    assert!(summary.contains("Descript...=string(é...)"));
    assert!(summary.contains("Children=repeated"));
    assert!(summary.contains("+2 more"));
    let details = source_row_details(2, &events[2]).expect("source row details");
    assert_eq!(details.len(), 4);
    assert_eq!(details[3], "  +2 more fields omitted");
    assert!(trace_row(2, &events[2]).contains("source row group"));

    let report = RunReport {
        kind: RunReportKind::Preview,
        duration: Duration::ZERO,
        records_written: 0,
        input_path: PathBuf::from("input.xml"),
        outputs: Vec::new(),
        trace: TraceReport {
            events,
            dropped: 1,
            ..Default::default()
        },
    };
    let mut view = RunReportView::new(report);
    assert_eq!(view.source_rows, [2]);
    assert_eq!(view.selected_source_row, Some(2));
    view.page = ReportPage::History;
    view.history_mode = HistoryMode::SourceRows;
    let context = egui::Context::default();
    crate::icons::install(&context);
    let mut open = true;
    let output = context.run_ui(Default::default(), |ui| {
        show(ui.ctx(), &mut open, &mut view);
    });
    assert!(open);
    assert!(!output.shapes.is_empty());
}

#[test]
fn replay_moves_only_within_the_retained_event_prefix() {
    let collector = TraceCollector::with_limit(4);
    for node in [8, 9, 8, 8, 9] {
        cli::TraceSink::record(&collector, trace_event(node, ir::Value::Int(node as i64)));
    }
    let view = RunReportView::new(RunReport {
        kind: RunReportKind::Preview,
        duration: Duration::ZERO,
        records_written: 0,
        input_path: PathBuf::from("input.json"),
        outputs: Vec::new(),
        trace: collector.finish(),
    });
    assert_eq!(view.report.trace.events.len(), 4);
    assert_eq!(view.report.trace.dropped, 1);
    assert_eq!(view.replay_event, Some(0));
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::First),
        None
    );
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::Previous),
        None
    );
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::Next),
        Some(1)
    );
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::NextSelectedNode),
        Some(2)
    );

    let mut view = view;
    view.replay_event = Some(2);
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::First),
        Some(0)
    );
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::Previous),
        Some(1)
    );
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::Next),
        Some(3)
    );
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::NextSelectedNode),
        Some(3)
    );
    view.replay_event = Some(3);
    assert_eq!(replay::replay_target(&view, replay::ReplayStep::Next), None);
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::NextSelectedNode),
        None
    );
    view.history_node = Some(NodeHistoryKey::Graph(9));
    assert_eq!(
        replay::replay_target(&view, replay::ReplayStep::NextSelectedNode),
        None
    );
}

#[test]
fn report_links_replay_exact_retained_indices_after_filtering() {
    let candidate = |ordinal| cli::TraceEvent::IterationCandidate {
        scope: trace_scope(),
        ordinal,
        positions: Vec::new(),
        source_row: Some(cli::TraceSourceRow {
            kind: cli::TraceOutputKind::Group,
            value: None,
            fields: Vec::new(),
            omitted_fields: 0,
        }),
    };
    let collector = TraceCollector::with_limit(4);
    for event in [
        trace_event(7, ir::Value::Int(1)),
        candidate(1),
        trace_event(7, ir::Value::Int(2)),
        candidate(2),
        trace_event(8, ir::Value::Int(3)),
    ] {
        cli::TraceSink::record(&collector, event);
    }
    let mut view = RunReportView::new(RunReport {
        kind: RunReportKind::Preview,
        duration: Duration::ZERO,
        records_written: 0,
        input_path: PathBuf::from("input.xml"),
        outputs: Vec::new(),
        trace: collector.finish(),
    });
    assert_eq!(view.report.trace.dropped, 1);
    assert_eq!(view.history_by_node[&NodeHistoryKey::Graph(7)], [0, 2]);
    assert_eq!(view.source_rows, [1, 3]);

    view.trace_filter = "candidate 2".into();
    assert_eq!(view.filtered_trace_indices(), [3]);
    assert_eq!(source_rows::filtered_source_row_indices(&view), [3]);
    assert!(view.replay_from(3));
    assert_eq!(view.page, ReportPage::Replay);
    assert_eq!(view.replay_event, Some(3));

    view.page = ReportPage::History;
    assert!(!view.replay_from(4)); // The fifth event was omitted.
    assert!(!view.replay_from(usize::MAX));
    assert_eq!(view.page, ReportPage::History);
    assert_eq!(view.replay_event, Some(3));

    view.trace_filter = "node 7".into();
    assert_eq!(view.filtered_trace_indices(), [0, 2]);
    assert!(view.replay_from(view.history_by_node[&NodeHistoryKey::Graph(7)][1]));
    assert_eq!(view.replay_event, Some(2));
    assert_eq!(view.page, ReportPage::Replay);

    view.report.trace.events.truncate(2);
    view.trace_filter.clear();
    assert_eq!(source_rows::filtered_source_row_indices(&view), [1]);
    assert!(!view.replay_from(3));
    assert_eq!(replay::replay_target(&view, replay::ReplayStep::Next), None);

    let mut empty = RunReportView::new(RunReport {
        kind: RunReportKind::Pipeline,
        duration: Duration::ZERO,
        records_written: 0,
        input_path: PathBuf::from("pipeline.json"),
        outputs: Vec::new(),
        trace: TraceReport::default(),
    });
    assert!(!empty.replay_from(0));
    assert_eq!(empty.page, ReportPage::Output);
    assert_eq!(empty.replay_event, None);
}

#[test]
fn replay_action_click_opens_the_chosen_retained_event() {
    let context = egui::Context::default();
    let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
    let input = |events| egui::RawInput {
        screen_rect: Some(screen_rect),
        events,
        ..Default::default()
    };
    let mut view = RunReportView::new(RunReport {
        kind: RunReportKind::Preview,
        duration: Duration::ZERO,
        records_written: 0,
        input_path: PathBuf::from("input.json"),
        outputs: Vec::new(),
        trace: TraceReport {
            events: vec![
                trace_event(1, ir::Value::Int(1)),
                trace_event(2, ir::Value::Int(2)),
                trace_event(3, ir::Value::Int(3)),
            ],
            dropped: 0,
            ..Default::default()
        },
    });
    let mut button_rect = egui::Rect::NOTHING;
    let output = context.run_ui(input(Vec::new()), |ui| {
        button_rect = replay_action(ui, 2, "Replay").rect;
        ui.add(egui::Label::new("selectable trace text").selectable(true));
    });
    assert!(button_rect.is_positive());
    assert!(!output.shapes.is_empty());
    let pos = button_rect.center();
    let pointer = |pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::default(),
    };
    let _ = context.run_ui(
        input(vec![egui::Event::PointerMoved(pos), pointer(true)]),
        |ui| {
            if replay_action(ui, 2, "Replay").clicked() {
                view.replay_from(2);
            }
            ui.add(egui::Label::new("selectable trace text").selectable(true));
        },
    );
    assert_eq!(view.page, ReportPage::Output);
    let _ = context.run_ui(input(vec![pointer(false)]), |ui| {
        if replay_action(ui, 2, "Replay").clicked() {
            view.replay_from(2);
        }
        ui.add(egui::Label::new("selectable trace text").selectable(true));
    });
    assert_eq!(view.page, ReportPage::Replay);
    assert_eq!(view.replay_event, Some(2));
}

#[test]
fn replay_details_keep_positions_and_source_fields_on_the_same_event() {
    let position = cli::TracePosition {
        collection: vec!["Order".into(), "Line".into()],
        index: 4,
        grouped: false,
        join: None,
        join_position: None,
        document_path: Some("part.xml".into()),
    };
    let candidate = cli::TraceEvent::IterationCandidate {
        scope: trace_scope(),
        ordinal: 4,
        positions: vec![position.clone()],
        source_row: Some(cli::TraceSourceRow {
            kind: cli::TraceOutputKind::Group,
            value: None,
            fields: vec![cli::TraceSourceField {
                name: "Sku".into(),
                name_truncated: false,
                kind: cli::TraceOutputKind::Scalar,
                value: Some(cli::TraceValue {
                    value_type: "string",
                    preview: "ABC".into(),
                    truncated: false,
                }),
            }],
            omitted_fields: 1,
        }),
    };
    let node = cli::TraceEvent::NodeValue {
        node: 8,
        positions: vec![position],
        value: cli::TraceValue {
            value_type: "string",
            preview: "derived".into(),
            truncated: false,
        },
    };

    let candidate_details = replay::replay_event_details(0, &candidate).join("\n");
    assert!(candidate_details.contains("candidate 4"));
    assert!(candidate_details.contains("Positions: Order/Line[4] @part.xml"));
    assert!(candidate_details.contains("Source row fields for this candidate event:"));
    assert!(candidate_details.contains("Sku=string(ABC)"));
    assert!(candidate_details.contains("+1 more fields omitted"));
    let node_details = replay::replay_event_details(1, &node).join("\n");
    assert!(node_details.contains("node 8"));
    assert!(node_details.contains("Positions: Order/Line[4] @part.xml"));
    assert!(node_details.contains("string(derived)"));
    assert!(!node_details.contains("Sku="));
}

#[test]
fn replay_renders_empty_and_recorded_runs() {
    let mut view = RunReportView::new(RunReport {
        kind: RunReportKind::Pipeline,
        duration: Duration::ZERO,
        records_written: 0,
        input_path: PathBuf::from("pipeline.json"),
        outputs: Vec::new(),
        trace: TraceReport::default(),
    });
    assert_eq!(view.replay_event, None);
    assert_eq!(replay::replay_target(&view, replay::ReplayStep::Next), None);
    view.page = ReportPage::Replay;
    let context = egui::Context::default();
    crate::icons::install(&context);
    let mut open = true;
    let output = context.run_ui(Default::default(), |ui| {
        show(ui.ctx(), &mut open, &mut view);
    });
    assert!(open);
    assert!(!output.shapes.is_empty());

    view.report
        .trace
        .events
        .push(trace_event(8, ir::Value::Int(42)));
    view = RunReportView::new(view.report);
    view.page = ReportPage::Replay;
    let output = context.run_ui(Default::default(), |ui| {
        show(ui.ctx(), &mut open, &mut view);
    });
    assert_eq!(view.replay_event, Some(0));
    assert!(!output.shapes.is_empty());
}

#[test]
fn history_rows_show_nested_join_context_and_bounded_values() {
    let join = mapping::JoinId::new(4);
    let positions = [
        cli::TracePosition {
            collection: vec!["Order".into()],
            index: 2,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        },
        cli::TracePosition {
            collection: vec!["Order".into(), "Line".into()],
            index: 3,
            grouped: true,
            join: Some(join),
            join_position: Some((join, 5)),
            document_path: Some("nested/part.xml".into()),
        },
    ];
    let input = cli::TraceEvent::NodeInputValue {
        consumer: 8,
        input: 9,
        input_index: 1,
        positions: positions.to_vec(),
        value: cli::TraceValue {
            value_type: "string",
            preview: "é".into(),
            truncated: true,
        },
    };
    let row = history_row(2, 6, &input).expect("input history row");

    assert!(row.contains("     2  event      7"));
    assert!(row.contains("input 2 <- node 9"));
    assert!(row.contains("Order[2] > Order/Line[3] group join=4 tuple=4[5] @nested/part.xml"));
    assert!(row.contains("string(é...) [truncated]"));
    assert!(
        history_row(1, 0, &trace_event(8, ir::Value::Null))
            .expect("output history row")
            .contains("output  <root>  null(null)")
    );
}

#[test]
fn history_renders_an_empty_pipeline_trace() {
    let report = RunReport {
        kind: RunReportKind::Pipeline,
        duration: Duration::ZERO,
        records_written: 0,
        input_path: PathBuf::from("pipeline.json"),
        outputs: Vec::new(),
        trace: TraceReport::default(),
    };
    let mut view = RunReportView::new(report);
    assert!(view.history_by_node.is_empty());
    assert_eq!(view.history_node, None);
    view.page = ReportPage::History;
    let context = egui::Context::default();
    crate::icons::install(&context);
    let mut open = true;
    let output = context.run_ui(Default::default(), |ui| {
        show(ui.ctx(), &mut open, &mut view);
    });
    assert!(open);
    assert!(!output.shapes.is_empty());
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
            ..Default::default()
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

    view.page = ReportPage::History;
    assert_eq!(view.history_node, Some(NodeHistoryKey::Graph(7)));
    let output = context.run_ui(Default::default(), |ui| {
        show(ui.ctx(), &mut open, &mut view);
    });
    assert!(!output.shapes.is_empty());
    std::fs::remove_file(first).expect("first output is removed");
    std::fs::remove_file(second).expect("second output is removed");
}
