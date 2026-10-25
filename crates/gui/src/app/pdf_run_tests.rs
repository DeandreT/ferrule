//! Saved-file and pipeline runs with the owned PDF worker opted in. Real
//! packaged children extract small self-authored PDFs; cancellation and app
//! close stop and reap them before the run reports or the window closes.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::pdf_extraction::tests::{
    HELLO_JSON, HELLO_PDF, SENTINEL, WorkerRoot, fixture_pipeline, released,
};
use super::pdf_extraction::{ExtractionPhase, ExtractionStatus, ReaderControls};
use super::*;
use crate::test_ui::ContextExt;

/// Generous for a heavily loaded host; the worker's own deadline is 30 s.
const WAIT: Duration = Duration::from_secs(90);

pub(super) const HOLD: ReaderControls = ReaderControls {
    hold_until_cancelled: true,
    release_gate: None,
    cancel_when_starting: false,
    cancel_when_running: false,
    cancel_when_checked: false,
};

fn owned_app(root: &WorkerRoot, controls: ReaderControls) -> FerruleApp {
    let project = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../cli/tests/filesystem_pdf_reader/projects/simple.json"
    ))
    .expect("frozen project fixture");
    let project_path = root.write("project.json", &project);
    root.write("input.pdf", HELLO_PDF);
    root.write("out.json", SENTINEL);
    let mut app = FerruleApp::default();
    app.load_project_from(&project_path);
    assert!(app.document.saved_path().is_some(), "{}", app.status);
    app.owned_pdf_worker = true;
    app.pdf_worker_package = root.package();
    app.pdf_reader_controls = controls;
    app
}

/// Runs one frame and returns the viewport commands it sent.
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

fn file_run_status(app: &FerruleApp) -> Arc<ExtractionStatus> {
    Arc::clone(
        app.pending_file_run
            .as_ref()
            .and_then(|pending| pending.extraction.as_ref())
            .expect("owned extraction status"),
    )
}

/// Waits until the held child has been spawned and is waiting for Start.
fn wait_for_held_child(status: &ExtractionStatus) -> u32 {
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

/// Polls the file run until it reports, returning every viewport command.
fn finish_file_run(app: &mut FerruleApp, context: &egui::Context) -> Vec<egui::ViewportCommand> {
    let deadline = Instant::now() + WAIT;
    let mut commands = Vec::new();
    while app.pending_file_run.is_some() {
        assert!(Instant::now() < deadline, "file run reports");
        commands.extend(frame(app, context, |app, context| {
            app.poll_file_run(context)
        }));
        std::thread::sleep(Duration::from_millis(5));
    }
    commands
}

fn finish_pipeline_run(
    app: &mut FerruleApp,
    context: &egui::Context,
) -> Vec<egui::ViewportCommand> {
    let deadline = Instant::now() + WAIT;
    let mut commands = Vec::new();
    while app.pending_pipeline_run.is_some() {
        assert!(Instant::now() < deadline, "pipeline run reports");
        commands.extend(frame(app, context, |app, context| {
            app.poll_pipeline_run(context)
        }));
        std::thread::sleep(Duration::from_millis(5));
    }
    commands
}

fn closes(commands: &[egui::ViewportCommand]) -> usize {
    commands
        .iter()
        .filter(|command| matches!(command, egui::ViewportCommand::Close))
        .count()
}

#[test]
fn owned_saved_run_publishes_the_exact_mapping_output() {
    let root = WorkerRoot::new("gui-run");
    let mut app = owned_app(&root, ReaderControls::default());
    let context = egui::Context::default();
    app.run_saved(false);
    let status = file_run_status(&app);
    finish_file_run(&mut app, &context);
    assert!(app.status.starts_with("wrote 1 record"), "{}", app.status);
    assert_eq!(std::fs::read(root.work("out.json")).unwrap(), HELLO_JSON);
    let snapshot = status.snapshot();
    assert_eq!((snapshot.jobs_started, snapshot.jobs_closed), (1, 1));
    assert_eq!(snapshot.last_own_reap, Some(true));
    assert!(released(snapshot.child_pid.expect("child pid")));
    assert!(root.scratch_entries().is_empty());
}

#[test]
fn cancelling_a_saved_run_stops_and_reaps_the_held_child_without_publishing() {
    let root = WorkerRoot::new("gui-cancel");
    let mut app = owned_app(&root, HOLD);
    let context = egui::Context::default();
    app.run_saved(false);
    let status = file_run_status(&app);
    let pid = wait_for_held_child(&status);
    frame(&mut app, &context, |app, context| {
        app.poll_file_run(context)
    });
    let pending = app.pending_file_run.as_ref().expect("run is pending");
    assert!(!pending.worker.as_ref().is_some_and(JoinHandle::is_finished));
    assert!(!released(pid), "the held child is alive and owned");

    app.file_run_command(run_ui::FileRunCommand::Cancel);
    finish_file_run(&mut app, &context);
    assert_eq!(app.status, "run cancelled");
    assert!(app.run_report.is_none());
    assert_eq!(std::fs::read(root.work("out.json")).unwrap(), SENTINEL);
    let snapshot = status.snapshot();
    assert_eq!(snapshot.phase, ExtractionPhase::Idle);
    assert_eq!(snapshot.last_own_reap, Some(true));
    assert!(released(pid));
}

#[test]
fn closing_the_app_defers_until_the_owned_child_is_reaped() {
    let root = WorkerRoot::new("gui-close");
    let mut app = owned_app(&root, HOLD);
    let context = egui::Context::default();
    app.run_saved(false);
    let status = file_run_status(&app);
    let pid = wait_for_held_child(&status);

    let commands = frame(&mut app, &context, |app, context| {
        app.guard_app_close_requested(context, true)
    });
    assert!(commands.contains(&egui::ViewportCommand::CancelClose));
    assert_eq!(closes(&commands), 0);
    assert!(app.close_after_file_run);
    let pending = app.pending_file_run.as_ref().expect("close is deferred");
    assert!(pending.cancelled.load(Ordering::Acquire));
    assert!(matches!(pending.phase, run_ui::FileRunPhase::Stopping));

    let commands = finish_file_run(&mut app, &context);
    assert_eq!(
        closes(&commands),
        1,
        "the window closes once, after the run"
    );
    assert!(matches!(
        commands.last(),
        Some(egui::ViewportCommand::Close)
    ));
    assert_eq!(app.status, "run cancelled");
    assert!(!app.close_after_file_run);
    assert_eq!(std::fs::read(root.work("out.json")).unwrap(), SENTINEL);
    assert_eq!(status.snapshot().last_own_reap, Some(true));
    assert!(released(pid));
}

#[test]
fn debug_run_pauses_and_resumes_after_owned_extraction() {
    let root = WorkerRoot::new("gui-debug");
    let mut app = owned_app(&root, ReaderControls::default());
    let context = egui::Context::default();
    app.run_saved(true);
    let status = file_run_status(&app);
    let deadline = Instant::now() + WAIT;
    while !app
        .pending_file_run
        .as_ref()
        .is_some_and(|pending| matches!(pending.phase, run_ui::FileRunPhase::Paused(_)))
    {
        assert!(Instant::now() < deadline, "debug run pauses");
        frame(&mut app, &context, |app, context| {
            app.poll_file_run(context)
        });
        std::thread::sleep(Duration::from_millis(5));
    }
    // Extraction closed before the interpreter reached its first write.
    assert_eq!(status.snapshot().jobs_closed, 1);
    assert_eq!(std::fs::read(root.work("out.json")).unwrap(), SENTINEL);
    app.file_run_command(run_ui::FileRunCommand::Continue);
    finish_file_run(&mut app, &context);
    assert!(app.status.starts_with("wrote 1 record"), "{}", app.status);
    assert_eq!(std::fs::read(root.work("out.json")).unwrap(), HELLO_JSON);
}

fn owned_pipeline_app(root: &WorkerRoot, controls: ReaderControls) -> FerruleApp {
    let pipeline = fixture_pipeline("single.json");
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
    draft.outputs[0].path = root.work("out.json").display().to_string();
    app.owned_pdf_worker = true;
    app.pdf_worker_package = root.package();
    app.pdf_reader_controls = controls;
    app
}

fn pipeline_status(app: &FerruleApp) -> Arc<ExtractionStatus> {
    Arc::clone(
        app.pending_pipeline_run
            .as_ref()
            .and_then(|pending| pending.extraction.as_ref())
            .expect("owned extraction status"),
    )
}

#[test]
fn owned_pipeline_run_publishes_the_exact_mapping_output() {
    let root = WorkerRoot::new("gui-pipeline");
    let mut app = owned_pipeline_app(&root, ReaderControls::default());
    let context = egui::Context::default();
    app.start_pipeline_run();
    let status = pipeline_status(&app);
    finish_pipeline_run(&mut app, &context);
    assert!(app.diagnostics.is_empty(), "{}", app.status);
    assert_eq!(std::fs::read(root.work("out.json")).unwrap(), HELLO_JSON);
    let snapshot = status.snapshot();
    assert_eq!((snapshot.jobs_started, snapshot.jobs_closed), (1, 1));
    assert_eq!(snapshot.last_own_reap, Some(true));
}

#[test]
fn closing_during_an_owned_pipeline_run_stops_the_child_before_closing() {
    let root = WorkerRoot::new("gui-pipeline-close");
    let mut app = owned_pipeline_app(&root, HOLD);
    let context = egui::Context::default();
    app.start_pipeline_run();
    let status = pipeline_status(&app);
    let pid = wait_for_held_child(&status);
    assert!(!released(pid));

    let commands = frame(&mut app, &context, |app, context| {
        app.guard_app_close_requested(context, true)
    });
    assert!(commands.contains(&egui::ViewportCommand::CancelClose));
    assert!(app.close_after_pipeline_run);
    assert!(app.pending_pipeline_run.is_some(), "close is deferred");

    let commands = finish_pipeline_run(&mut app, &context);
    assert_eq!(closes(&commands), 1);
    assert_eq!(app.status, "pipeline cancelled");
    assert_eq!(std::fs::read(root.work("out.json")).unwrap(), SENTINEL);
    assert_eq!(status.snapshot().last_own_reap, Some(true));
    assert!(released(pid));
}
