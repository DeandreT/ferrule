use super::*;

const ORDERS: &str = include_str!("../../../../cli/tests/fixtures/orders/Orders.xml");
const MAPPING: &[u8] = include_bytes!("../../../../cli/tests/fixtures/orders/project.json");
const EXPECTED: &str =
    include_str!("../../../../cli/tests/fixtures/orders/expected_order_lines.csv");

fn mapping() -> Project {
    mapping::project_file::decode_bytes(MAPPING).unwrap()
}

fn draft(target: PreviewTarget, output: &Path) -> PreviewDraft {
    let mut draft = PreviewDraft::new(target, "Orders.xml".into(), output.to_string_lossy().into());
    draft.input_text = ORDERS.replace("\r\n", "\n");
    draft
}

fn finish(app: &mut FerruleApp) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let context = egui::Context::default();
    while app.pending_preview.is_some() {
        assert!(
            std::time::Instant::now() < deadline,
            "preview worker timed out"
        );
        app.poll_preview(&context);
        std::thread::yield_now();
    }
}

fn keys() -> Vec<String> {
    vec!["Order_ID".into(), "Item_ID".into()]
}

#[test]
fn inspect_and_compare_completed_unsaved_previews_without_reexecuting_or_publishing() {
    let path =
        std::env::temp_dir().join(format!("ferrule-inspect-orders-{}.csv", std::process::id()));
    let mut app = FerruleApp {
        project: mapping(),
        preview_draft: Some(draft(PreviewTarget::Primary, &path)),
        ..Default::default()
    };
    app.execute_preview();
    finish(&mut app);
    assert!(app.run_report.is_some(), "{}", app.status);
    let (target, artifact, table) = app.selected_preview_table().unwrap();
    assert_eq!((target, artifact), ("Primary", path.as_path()));
    assert_eq!(table.rows.len(), 6);
    let ptr = table as *const _;
    let report = &app.run_report.as_ref().unwrap().report;
    let Some(OutputPreview::Text {
        content,
        truncated: false,
        ..
    }) = report.outputs[0].memory_preview()
    else {
        panic!("complete serialized CSV")
    };
    assert_eq!(content, &EXPECTED.replace("\r\n", "\n"));
    assert_eq!(
        app.preview_inspection.capture.as_ref().unwrap().artifacts[0].sha,
        "5326a6785cd7f4fe407a39eb2b13f4db8599505d5a58c0b1d946e50f572302da"
    );
    for row in 0..6 {
        assert!(app.inspect_selected_preview_cell(row, "Desc"));
        assert_eq!(
            (app.preview_inspection.row, app.preview_inspection.column),
            (row, 3)
        );
        assert!(app.pending_preview.is_none() && app.preview_draft.is_none());
        assert_eq!(app.selected_preview_table().unwrap().2 as *const _, ptr);
    }
    assert!(!app.inspect_selected_preview_cell(6, "Desc"));
    assert!(!app.inspect_selected_preview_cell(0, "other"));
    assert_eq!(app.preview_inspection.row, 5);
    app.inspect_preview_field(PreviewTarget::Primary, "Price");
    assert_eq!(
        (app.preview_inspection.row, app.preview_inspection.column),
        (5, 4)
    );
    app.preview_inspection.pin(app.run_report.as_ref().unwrap());
    app.project
        .root
        .bindings
        .iter_mut()
        .find(|b| b.target_field == "Desc")
        .unwrap()
        .node = 3;
    assert!(app.selected_preview_table().is_err());
    assert!(!app.inspect_selected_preview_cell(1, "Desc"));
    app.preview_draft = Some(draft(PreviewTarget::Primary, &path));
    app.execute_preview();
    finish(&mut app);
    let (_, _, b) = app.selected_preview_table().unwrap();
    let a = app.preview_inspection.pinned.as_ref().unwrap();
    let diff = compare::compare(
        &a.table,
        b,
        &MatchBy {
            keys: keys(),
            position: false,
        },
    )
    .unwrap();
    assert_eq!(
        (diff.matched, diff.added, diff.removed, diff.changes.len()),
        (6, 0, 0, 6)
    );
    assert_eq!(a.table.rows[0][3], "PAINT BRUSH");
    assert_eq!(b.rows[0][3], "paint brush");
    let pin = Arc::clone(&a.table);
    let mut bad = draft(PreviewTarget::Primary, &path);
    bad.input_text = "<broken".into();
    app.preview_draft = Some(bad);
    app.execute_preview();
    finish(&mut app);
    assert!(app.selected_preview_table().is_err());
    assert!(!app.inspect_selected_preview_cell(0, "Desc"));
    assert!(Arc::ptr_eq(
        &pin,
        &app.preview_inspection.pinned.as_ref().unwrap().table
    ));
    app.preview_draft = Some(draft(PreviewTarget::Primary, &path));
    app.execute_debug_preview();
    app.preview_command(super::super::preview_ui::PreviewCommand::Cancel);
    finish(&mut app);
    assert_eq!(app.status, "preview cancelled");
    assert!(!app.inspect_selected_preview_cell(0, "Desc"));
    assert!(Arc::ptr_eq(
        &pin,
        &app.preview_inspection.pinned.as_ref().unwrap().table
    ));
    assert!(!path.exists());
}

#[test]
fn explain_scope_keeps_empty_groups_and_matches_reordered_compound_keys() {
    let project = mapping();
    let input = ORDERS.replace(
        "</Orders>",
        "<Order><Order_ID>empty</Order_ID><Items/></Order></Orders>",
    );
    let scope = RowScope::capture(&input, &project, &project.root).unwrap();
    assert_eq!(scope.path, "/Orders/Order/Items/Item");
    assert_eq!(
        scope
            .groups
            .iter()
            .map(|g| g.items.len())
            .collect::<Vec<_>>(),
        [3, 2, 1, 0]
    );
    assert_eq!(scope.label(1, None, &keys()), "A-1002");
    assert_eq!(scope.label(3, None, &keys()), "empty");
    assert_eq!(scope.items[0].line, 11);
    assert!(input[scope.items[0].excerpt.clone()].contains("<Desc>paint brush</Desc>"));
    let mut table = PreviewTable::parse(
        EXPECTED.as_bytes(),
        &project.target_options,
        &project.target,
    )
    .unwrap();
    let matched = scope.match_rows(&table, &keys()).unwrap();
    assert_eq!(
        &matched[scope.groups[1].items.clone()],
        &[RowMatch::Row(3), RowMatch::Row(4)]
    );
    table.rows.reverse();
    assert_eq!(
        &scope.match_rows(&table, &keys()).unwrap()[3..5],
        &[RowMatch::Row(2), RowMatch::Row(1)]
    );
    table.rows.push(table.rows[2].clone());
    assert_eq!(
        scope.match_rows(&table, &keys()).unwrap()[3],
        RowMatch::Ambiguous
    );
    table.rows[1][2].clear();
    assert_eq!(
        scope.match_rows(&table, &keys()).unwrap()[4],
        RowMatch::NoOutput
    );
    let duplicate =
        RowScope::capture(&input.replace("BR-100", "GL-220"), &project, &project.root).unwrap();
    assert_eq!(
        duplicate.match_rows(&table, &keys()).unwrap()[0],
        RowMatch::Ambiguous
    );
    let missing = RowScope::capture(
        &input.replace("<Item_ID>BR-100</Item_ID>", ""),
        &project,
        &project.root,
    )
    .unwrap();
    assert_eq!(
        missing.match_rows(&table, &keys()).unwrap()[0],
        RowMatch::MissingKey
    );
}

fn complete_artifacts(
    app: &mut FerruleApp,
    draft: PreviewDraft,
    artifacts: Vec<(PathBuf, Vec<u8>, usize)>,
) {
    let outcome = cli::PayloadRunOutcome {
        records_written: artifacts.iter().map(|a| a.2).sum(),
        artifacts: artifacts
            .into_iter()
            .map(|(path, bytes, records_written)| cli::PayloadArtifact {
                path,
                bytes,
                records_written,
                target: draft.target.label().into(),
            })
            .collect(),
    };
    let trace = TraceReport::default();
    let capture = Capture::new(
        &app.project,
        &draft,
        PreviewInputSource::EditorText,
        &outcome,
        &trace,
    );
    let view = RunReportView::new(crate::run_report::RunReport::from_payload_with_trace(
        outcome,
        PathBuf::from("Orders.xml"),
        Default::default(),
        trace,
    ));
    app.preview_inspection
        .complete(capture, &view, Some(draft), Vec::new());
    app.run_report = Some(view);
}

#[test]
fn selected_named_artifact_never_falls_back_to_primary_or_another_artifact() {
    let mut project = mapping();
    project.extra_targets.push(mapping::NamedTarget {
        name: "Detail".into(),
        path: Some("detail.csv".into()),
        schema: project.target.clone(),
        options: project.target_options.clone(),
        root: project.root.clone(),
    });
    let mut app = FerruleApp {
        project,
        ..Default::default()
    };
    let draft = draft(
        PreviewTarget::Named("Detail".into()),
        Path::new("detail.csv"),
    );
    complete_artifacts(
        &mut app,
        draft,
        vec![
            ("first.csv".into(), b"n,n\nx,y\n".to_vec(), 1),
            ("second.csv".into(), b"n\nz\n".to_vec(), 1),
            ("empty.csv".into(), b"n\n".to_vec(), 0),
            ("large.csv".into(), vec![b'x'; MAX_PREVIEW_BYTES + 1], 1),
            ("document.json".into(), b"{}".to_vec(), 1),
            ("invalid.csv".into(), b"n,m\nx\n".to_vec(), 1),
        ],
    );
    assert!(!app.inspect_selected_preview_cell(0, "n"));
    let select = |app: &mut FerruleApp, index| {
        let report = app.run_report.as_mut().unwrap();
        report.select_output(index);
        app.preview_inspection.select(report);
    };
    select(&mut app, 1);
    assert_eq!(app.selected_preview_table().unwrap().0, "Detail");
    assert_eq!(
        app.selected_preview_table().unwrap().1,
        Path::new("second.csv")
    );
    assert!(app.inspect_selected_preview_cell(0, "n"));
    select(&mut app, 2);
    assert!(app.selected_preview_table().unwrap().2.rows.is_empty());
    assert!(!app.inspect_selected_preview_cell(0, "n"));
    for index in 3..6 {
        select(&mut app, index);
        assert!(app.selected_preview_table().is_err());
        assert!(!app.inspect_selected_preview_cell(0, "n"));
    }
}

#[test]
fn a_capture_needs_the_same_cell_write_and_source_read_to_describe_runtime_provenance() {
    let mut app = FerruleApp {
        project: mapping(),
        preview_draft: Some(draft(PreviewTarget::Primary, Path::new("orders.csv"))),
        ..Default::default()
    };
    app.execute_preview();
    finish(&mut app);
    let (capture, report, table) = app.fresh_preview().unwrap();
    let provenance = capture.trace.as_ref().unwrap();
    let trace = &report.report.trace;
    assert_eq!(
        provenance.cell(trace, table, 0, 3).as_deref(),
        Some("Captured /Orders/Order/Items/Item/Desc: paint brush → upper → PAINT BRUSH")
    );
    assert_eq!(
        provenance.cell(trace, table, 5, 2).as_deref(),
        Some("Captured /Orders/Order/Items/Item/Item_ID: WG-300 → WG-300")
    );
    // Matching data values or static edges alone can never supply provenance.
    assert!(
        provenance.cell(trace, table, 0, 1).is_none(),
        "Customer name is outside the captured Item candidate"
    );
    let altered = TraceReport {
        events: trace.events.iter().filter(|event| {
            !matches!(event,engine::TraceEvent::TargetFieldWritten {field,..} if field=="Desc")
        }).cloned().collect(),
        ..Default::default()
    };
    let weakened = provenance::Provenance::capture(
        &app.project,
        &app.project.root,
        &PreviewTarget::Primary,
        &altered,
    )
    .unwrap();
    assert!(weakened.cell(&altered, table, 0, 3).is_none());
    assert!(
        provenance::Provenance::capture(
            &app.project,
            &app.project.root,
            &PreviewTarget::Primary,
            &TraceReport {
                dropped: 1,
                ..Default::default()
            }
        )
        .is_none()
    );
}

#[test]
fn field_navigation_checks_live_mapping_input_attempt_and_selected_artifact_before_focus() {
    let target = PreviewTarget::Primary;
    let path = Path::new("orders.csv");
    let mut app = FerruleApp {
        project: mapping(),
        preview_draft: Some(draft(target.clone(), path)),
        ..Default::default()
    };
    app.execute_preview();
    finish(&mut app);
    assert!(app.inspect_selected_preview_cell(5, "Price"));
    app.preview_inspection.pin(app.run_report.as_ref().unwrap());
    let pin = Arc::clone(&app.preview_inspection.pinned.as_ref().unwrap().table);
    let unchanged = |app: &FerruleApp| {
        assert_eq!(
            (app.preview_inspection.row, app.preview_inspection.column),
            (5, 4)
        );
        assert!(!app.preview_inspection.row_focus && app.pending_preview.is_none());
        assert!(Arc::ptr_eq(
            &pin,
            &app.preview_inspection.pinned.as_ref().unwrap().table
        ));
    };
    // Equal target/header. Change the live graph without polling editor history.
    let desc = app
        .project
        .root
        .bindings
        .iter()
        .position(|b| b.target_field == "Desc")
        .unwrap();
    app.project.root.bindings[desc].node = 3;
    app.inspect_preview_field(target.clone(), "Desc");
    unchanged(&app);
    assert!(app.status.contains("Stale preview"));
    app.project.root.bindings[desc].node = 4;
    let mut newer = draft(target.clone(), path);
    newer.input_text.push('\n');
    app.preview_draft = Some(newer);
    app.inspect_preview_field(target.clone(), "Desc");
    unchanged(&app);
    app.preview_draft = None;
    app.preview_inspection.attempt = Some("Preview cancelled.".into());
    app.inspect_preview_field(target.clone(), "Desc");
    unchanged(&app);
    assert!(app.status.contains("cancelled"));

    complete_artifacts(
        &mut app,
        draft(target.clone(), path),
        vec![
            ("orders.csv".into(), b"id,Desc\n1,old\n".to_vec(), 1),
            ("other.csv".into(), b"other\nnew\n".to_vec(), 1),
        ],
    );
    let report = app.run_report.as_mut().unwrap();
    report.select_output(1); // Before the inspector decodes this selection.
    app.inspect_preview_field(target, "Desc");
    assert!(app.status.contains("Output selection has changed"));
    assert_eq!(app.preview_inspection.column, 0);
    assert!(!app.preview_inspection.row_focus && app.pending_preview.is_none());

    let duplicate = mapping::NamedTarget {
        name: "detail".into(),
        schema: app.project.target.clone(),
        root: app.project.root.clone(),
        path: Some("detail.csv".into()),
        options: Default::default(),
    };
    app.project.extra_targets = vec![duplicate.clone(), duplicate];
    app.inspect_preview_field(PreviewTarget::Named("detail".into()), "Desc");
    assert!(app.status.contains("Ambiguous target"));
    assert_eq!(app.mapping_workspace.active, MappingDocument::Main);
    assert!(app.preview_draft.is_none() && app.pending_preview.is_none());
}
