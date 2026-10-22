//! File-backed previews with the owned PDF worker opted in: one owned
//! acquisition per input, outputs kept in memory, and cancellation, owner
//! loss and app close that wait for the child to be reaped and the preview
//! thread to be joined.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use super::pdf_extraction::tests::{
    HELLO_PDF, SENTINEL, WorkerRoot, fixture_pipeline, fixture_project, released,
};
use super::pdf_extraction::{ExtractionPhase, ExtractionStatus, ReaderControls};
use super::pdf_run_tests::HOLD;
use super::preview_service::PreviewInputSource;
use super::*;
use crate::test_ui::ContextExt;

const WAIT: Duration = Duration::from_secs(90);

fn frame(
    app: &mut FerruleApp,
    context: &egui::Context,
    mut body: impl FnMut(&mut FerruleApp, &egui::Context),
) -> Vec<egui::ViewportCommand> {
    let output = context.run_test_ui(egui::RawInput::default(), |ui| body(app, ui.ctx()));
    output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .map(|viewport| viewport.commands.clone())
        .unwrap_or_default()
}

fn poll_previews(app: &mut FerruleApp, context: &egui::Context) -> Vec<egui::ViewportCommand> {
    frame(app, context, |app, context| {
        app.poll_preview(context);
        app.poll_preview_service(context);
    })
}

fn closes(commands: &[egui::ViewportCommand]) -> usize {
    commands
        .iter()
        .filter(|command| matches!(command, egui::ViewportCommand::Close))
        .count()
}

/// A saved PDF project with a file-input preview draft for its primary target.
fn file_preview_app(root: &WorkerRoot, controls: ReaderControls) -> FerruleApp {
    let project = fixture_project("simple.json");
    let project_path = root.write(
        "project.json",
        &serde_json::to_vec_pretty(&project).expect("project JSON"),
    );
    root.write("input.pdf", HELLO_PDF);
    root.write("out.json", SENTINEL);
    let mut app = FerruleApp::default();
    app.load_project_from(&project_path);
    assert!(app.document.saved_path().is_some(), "{}", app.status);
    app.owned_pdf_worker = true;
    app.pdf_worker_package = root.package();
    app.pdf_reader_controls = controls;
    app.preview_input_source = PreviewInputSource::ConfiguredFiles;
    app.begin_preview();
    let draft = app.preview_draft.as_ref().expect("preview draft");
    assert_eq!(draft.input_identity, "input.pdf");
    assert_eq!(draft.output_identity, "out.json");
    app
}

fn preview_status(app: &FerruleApp) -> Arc<ExtractionStatus> {
    Arc::clone(
        app.pending_preview
            .as_ref()
            .and_then(|pending| pending.extraction.as_ref())
            .expect("owned preview extraction"),
    )
}

fn wait_for_child(status: &ExtractionStatus) -> u32 {
    let deadline = Instant::now() + WAIT;
    loop {
        let snapshot = status.snapshot();
        if snapshot.phase == ExtractionPhase::Starting
            && let Some(pid) = snapshot.child_pid
        {
            return pid;
        }
        assert!(
            Instant::now() < deadline,
            "worker child starts: {snapshot:?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn finish_previews(app: &mut FerruleApp, context: &egui::Context) -> Vec<egui::ViewportCommand> {
    let deadline = Instant::now() + WAIT;
    let mut commands = Vec::new();
    while app.pending_preview.is_some() || app.preview_service.pending() > 0 {
        assert!(Instant::now() < deadline, "preview and service finish");
        commands.extend(poll_previews(app, context));
        std::thread::sleep(Duration::from_millis(5));
    }
    // One more frame completes a deferred close.
    commands.extend(poll_previews(app, context));
    commands
}

fn preview_json(output: &mut crate::run_report::RunOutput) -> serde_json::Value {
    let crate::run_report::OutputPreview::Text { content, .. } = output.preview() else {
        panic!("preview artifact is not text");
    };
    serde_json::from_str(content).expect("JSON artifact")
}

#[test]
fn file_preview_extracts_once_through_the_owned_worker_and_writes_nothing() {
    let root = WorkerRoot::new("preview-file");
    let mut app = file_preview_app(&root, ReaderControls::default());
    let context = egui::Context::default();
    app.execute_preview();
    let status = preview_status(&app);
    assert!(
        app.pending_preview
            .as_ref()
            .is_some_and(preview_ui::PendingPreview::is_service_owned)
    );
    finish_previews(&mut app, &context);
    assert!(app.diagnostics.is_empty(), "{}", app.status);
    let report = &mut app.run_report.as_mut().expect("preview report").report;
    assert_eq!(report.outputs.len(), 1);
    assert_eq!(
        preview_json(&mut report.outputs[0]),
        serde_json::json!({"Value": "Hello"})
    );
    assert_eq!(std::fs::read(root.work("out.json")).unwrap(), SENTINEL);
    let snapshot = status.snapshot();
    assert_eq!((snapshot.jobs_started, snapshot.jobs_closed), (1, 1));
    assert_eq!(snapshot.last_own_reap, Some(true));
    assert!(released(snapshot.child_pid.expect("child pid")));
    assert!(root.scratch_entries().is_empty());
}

#[test]
fn editor_text_preview_keeps_the_payload_route_with_the_worker_opted_in() {
    let root = WorkerRoot::new("preview-text");
    let project = fixture_project("plain.json");
    let project_path = root.write(
        "project.json",
        &serde_json::to_vec_pretty(&project).expect("project JSON"),
    );
    let mut app = FerruleApp::default();
    app.load_project_from(&project_path);
    app.owned_pdf_worker = true;
    // No worker exists in this package; a payload preview never asks for one.
    app.pdf_worker_package = root.empty_package();
    app.begin_preview();
    let input = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cli/tests/filesystem_pdf_reader/inputs/plain.json"
    ))
    .expect("plain input");
    app.preview_draft.as_mut().expect("draft").input_text = input;
    let context = egui::Context::default();
    app.execute_preview();
    let pending = app.pending_preview.as_ref().expect("preview pending");
    assert!(pending.extraction.is_none() && !pending.is_service_owned());
    finish_previews(&mut app, &context);
    assert!(app.diagnostics.is_empty(), "{}", app.status);
    let report = &mut app.run_report.as_mut().expect("preview report").report;
    let expected: serde_json::Value = serde_json::from_slice(
        &std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../cli/tests/filesystem_pdf_reader/expected/plain.json"
        ))
        .unwrap(),
    )
    .unwrap();
    assert_eq!(preview_json(&mut report.outputs[0]), expected);
}

#[test]
fn refused_admission_fails_the_file_preview_without_a_fallback() {
    let root = WorkerRoot::new("preview-refused");
    let mut app = file_preview_app(&root, ReaderControls::default());
    app.pdf_worker_package = root.empty_package();
    let context = egui::Context::default();
    app.execute_preview();
    finish_previews(&mut app, &context);
    assert_eq!(app.status, "preview failed");
    assert!(app.run_report.is_none());
    assert!(app.diagnostics.items().iter().any(|item| {
        item.message
            .contains("the packaged PDF worker was not admitted")
    }));
}

#[test]
fn cancelling_a_file_preview_stops_and_reaps_the_held_child() {
    let root = WorkerRoot::new("preview-cancel");
    let mut app = file_preview_app(&root, HOLD);
    let context = egui::Context::default();
    app.execute_preview();
    let status = preview_status(&app);
    let pid = wait_for_child(&status);
    assert!(!released(pid));
    app.preview_command(preview_ui::PreviewCommand::Cancel);
    finish_previews(&mut app, &context);
    assert_eq!(app.status, "preview cancelled");
    assert!(app.run_report.is_none());
    assert_eq!(status.snapshot().last_own_reap, Some(true));
    assert!(released(pid));
}

static OWNER_LOSS_GATE: AtomicBool = AtomicBool::new(false);

#[test]
fn losing_the_preview_owner_hands_the_live_worker_to_the_service() {
    let root = WorkerRoot::new("preview-owner-loss");
    let mut app = file_preview_app(
        &root,
        ReaderControls {
            release_gate: Some(&OWNER_LOSS_GATE),
            ..HOLD
        },
    );
    let context = egui::Context::default();
    app.execute_preview();
    let status = preview_status(&app);
    let pid = wait_for_child(&status);

    // Dropping the UI owner returns at once; the service keeps the thread.
    let started = Instant::now();
    drop(app.pending_preview.take());
    assert!(started.elapsed() < Duration::from_secs(5));
    assert_eq!(app.preview_service.pending(), 1);
    for _ in 0..20 {
        poll_previews(&mut app, &context);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.preview_service.pending(), 1, "no closure is inferred");
    assert!(!released(pid), "the child is still owned and unreaped");

    OWNER_LOSS_GATE.store(true, Ordering::Release);
    finish_previews(&mut app, &context);
    assert_eq!(app.preview_service.pending(), 0);
    assert_eq!(status.snapshot().last_own_reap, Some(true));
    assert!(released(pid));
    assert!(app.run_report.is_none());
}

static CLOSE_GATE: AtomicBool = AtomicBool::new(false);

#[test]
fn closing_waits_for_the_owned_preview_then_keeps_the_dirty_confirmation() {
    let root = WorkerRoot::new("preview-close");
    let mut app = file_preview_app(
        &root,
        ReaderControls {
            release_gate: Some(&CLOSE_GATE),
            ..HOLD
        },
    );
    let context = egui::Context::default();
    app.execute_preview();
    let status = preview_status(&app);
    let pid = wait_for_child(&status);
    app.project.target_path = Some("edited.json".into());
    assert!(app.is_dirty());

    let commands = frame(&mut app, &context, |app, context| {
        app.guard_app_close_requested(context, true)
    });
    assert!(commands.contains(&egui::ViewportCommand::CancelClose));
    assert_eq!(closes(&commands), 0);
    assert!(app.close_after_preview);
    for _ in 0..20 {
        assert_eq!(closes(&poll_previews(&mut app, &context)), 0);
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        app.pending_preview.is_some(),
        "unresolved closure stays pending"
    );
    assert!(!released(pid));

    CLOSE_GATE.store(true, Ordering::Release);
    let commands = finish_previews(&mut app, &context);
    assert_eq!(
        closes(&commands),
        1,
        "the window closes once, after joining"
    );
    assert!(!app.close_after_preview);
    assert_eq!(app.status, "preview cancelled");
    assert_eq!(status.snapshot().last_own_reap, Some(true));
    assert!(released(pid));

    // The repeated close request reaches the unchanged dirty-document guard.
    let commands = frame(&mut app, &context, |app, context| {
        app.guard_app_close_requested(context, true)
    });
    assert!(commands.contains(&egui::ViewportCommand::CancelClose));
    assert!(matches!(
        app.pending_destructive_action,
        Some(DestructiveAction::Close)
    ));
}

static RUNNER_GATE: AtomicBool = AtomicBool::new(false);

#[test]
fn the_runner_owner_joins_a_preview_left_by_a_torn_down_app() {
    let root = WorkerRoot::new("preview-runner");
    let service = PreviewService::new();
    let mut app = file_preview_app(
        &root,
        ReaderControls {
            release_gate: Some(&RUNNER_GATE),
            ..HOLD
        },
    );
    app.preview_service = service.port();
    app.execute_preview();
    let status = preview_status(&app);
    let pid = wait_for_child(&status);

    let started = Instant::now();
    drop(app);
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "teardown is not blocked"
    );
    assert!(!released(pid));
    let release = std::thread::spawn(|| {
        std::thread::sleep(Duration::from_millis(200));
        RUNNER_GATE.store(true, Ordering::Release);
    });
    assert_eq!(service.finish_after_runner(), 1);
    release.join().expect("gate thread");
    assert_eq!(status.snapshot().last_own_reap, Some(true));
    assert!(released(pid));
}

fn pipeline_preview_app(root: &WorkerRoot) -> FerruleApp {
    let pipeline = fixture_pipeline("same-host.json");
    let pipeline_path = root.write(
        "pipeline.json",
        &serde_json::to_vec_pretty(&pipeline).expect("pipeline JSON"),
    );
    root.write("input.pdf", HELLO_PDF);
    root.write("out.json", SENTINEL);
    let mut app = FerruleApp::default();
    app.load_pipeline_for_run(&pipeline_path);
    let draft = app.pipeline_run_draft.as_mut().expect("pipeline opens");
    draft.inputs[0].path = root.work("input.pdf").display().to_string();
    for (index, output) in draft.outputs.iter_mut().enumerate() {
        output.preview_path = format!("stage-{index}.json");
    }
    app.owned_pdf_worker = true;
    app.pdf_worker_package = root.package();
    app
}

#[test]
fn pipeline_file_preview_acquires_a_shared_host_once() {
    let root = WorkerRoot::new("preview-pipeline");
    let mut app = pipeline_preview_app(&root);
    app.start_pipeline_preview();
    let pending = app.pending_pipeline_run.as_ref().expect("preview pending");
    let status = Arc::clone(pending.extraction.as_ref().expect("owned extraction"));
    assert!(pending.service.is_some());
    let context = egui::Context::default();
    let deadline = Instant::now() + WAIT;
    while app.pending_pipeline_run.is_some() {
        assert!(Instant::now() < deadline, "pipeline preview finishes");
        frame(&mut app, &context, |app, context| {
            app.poll_pipeline_run(context)
        });
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(app.diagnostics.is_empty(), "{}", app.status);
    let report = &mut app.run_report.as_mut().expect("preview report").report;
    assert_eq!(report.outputs.len(), 2);
    for output in &mut report.outputs {
        assert_eq!(preview_json(output), serde_json::json!({"Value": "Hello"}));
    }
    let snapshot = status.snapshot();
    assert_eq!((snapshot.jobs_started, snapshot.jobs_closed), (1, 1));
    assert_eq!(std::fs::read(root.work("out.json")).unwrap(), SENTINEL);
    for index in 0..2 {
        assert!(!root.work(&format!("stage-{index}.json")).exists());
    }
}
