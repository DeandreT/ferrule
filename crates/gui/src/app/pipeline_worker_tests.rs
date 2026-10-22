//! Exact-handle ownership of the pipeline worker: a reported result is not
//! completion until the thread exited, and dropping the owner drains it.

use std::time::{Duration, Instant};

use super::*;

fn held_pipeline_run(
    phase: PipelineRunPhase,
) -> (
    PendingPipelineRun,
    Sender<PipelineRunEvent>,
    Sender<()>,
    Arc<AtomicBool>,
) {
    let (worker, release, exited) = OwnedWorker::held_after_report_for_test((
        Err(PipelineRunError::Failed("held pipeline failure".into())),
        crate::run_report::TraceReport::default(),
    ));
    let (events, receiver) = mpsc::channel();
    let (commands, _command_receiver) = mpsc::channel();
    let pending = PendingPipelineRun {
        worker,
        receiver,
        commands,
        cancelled: Arc::new(AtomicBool::new(false)),
        started: Instant::now(),
        path: PathBuf::from("pipeline.json"),
        debug: false,
        preview: false,
        phase,
    };
    (pending, events, release, exited)
}

#[test]
fn close_during_pipeline_publication_waits_for_worker() {
    let (pending, _events, release, exited) = held_pipeline_run(PipelineRunPhase::Publishing);
    let mut app = FerruleApp {
        pending_pipeline_run: Some(pending),
        ..FerruleApp::default()
    };
    let context = egui::Context::default();

    app.guard_app_close_requested(&context, true);
    assert!(app.close_after_pipeline_run);
    let pending = app
        .pending_pipeline_run
        .as_ref()
        .expect("worker remains active");
    assert!(matches!(pending.phase, PipelineRunPhase::Publishing));
    assert!(!pending.cancelled.load(Ordering::Acquire));

    release.send(()).expect("held worker waits for release");
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.pending_pipeline_run.is_some() {
        assert!(Instant::now() < deadline, "pipeline worker exits");
        app.poll_pipeline_run(&context);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(exited.load(Ordering::Acquire));
    assert!(!app.close_after_pipeline_run);
}

#[test]
fn pipeline_completion_waits_until_the_worker_thread_exited() {
    let (pending, events, release, exited) = held_pipeline_run(PipelineRunPhase::Running);
    let mut app = FerruleApp {
        pending_pipeline_run: Some(pending),
        ..FerruleApp::default()
    };
    let context = egui::Context::default();
    // A live worker still relays its events after reporting a result.
    events
        .send(PipelineRunEvent::ReadyToPublish)
        .expect("event channel is open");
    let held_until = Instant::now() + Duration::from_millis(100);
    while Instant::now() < held_until {
        app.poll_pipeline_run(&context);
        assert!(
            app.pending_pipeline_run.is_some(),
            "a live worker is not complete"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(matches!(
        app.pending_pipeline_run
            .as_ref()
            .map(|pending| &pending.phase),
        Some(PipelineRunPhase::Publishing)
    ));
    assert!(!exited.load(Ordering::Acquire));

    release.send(()).expect("held worker waits for release");
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.pending_pipeline_run.is_some() {
        assert!(Instant::now() < deadline, "pipeline worker exits");
        app.poll_pipeline_run(&context);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        exited.load(Ordering::Acquire),
        "completion followed thread exit"
    );
    assert_eq!(app.status, "pipeline run failed");
}

#[test]
fn dropping_a_pending_pipeline_run_cancels_and_waits_for_its_worker() {
    for (phase, cancels) in [
        (PipelineRunPhase::Running, true),
        (PipelineRunPhase::Publishing, false),
    ] {
        let (pending, _events, release, exited) = held_pipeline_run(phase);
        let flag = pending
            .cancellation_for_test()
            .upgrade()
            .expect("pending run owns its flag");
        let releaser = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(50));
            let _ = release.send(());
        });
        drop(pending);
        assert_eq!(flag.load(Ordering::Acquire), cancels);
        assert!(
            exited.load(Ordering::Acquire),
            "drop returned only after the worker exited"
        );
        releaser.join().expect("releaser finishes");
    }
}
