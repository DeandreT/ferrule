//! Exact-handle ownership of the preview worker: a reported result is not
//! completion until the thread exited, and dropping the owner drains it.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use super::*;

fn held_preview(
    result: Result<cli::PayloadRunOutcome, PreviewRunError>,
) -> (PendingPreview, Sender<()>, Arc<AtomicBool>) {
    let (worker, release, exited) = OwnedWorker::held_after_report_for_test((
        result,
        crate::run_report::TraceReport::default(),
    ));
    let (_events, receiver) = mpsc::channel();
    let (commands, _command_receiver) = mpsc::channel();
    let pending = PendingPreview {
        worker,
        receiver,
        commands,
        cancelled: Arc::new(AtomicBool::new(false)),
        started: Instant::now(),
        input_path: PathBuf::from("input.json"),
        target_label: "Primary".into(),
        debug: false,
        phase: PreviewPhase::Running,
        extraction: None,
        service: None,
    };
    (pending, release, exited)
}

#[test]
fn preview_completion_waits_until_the_worker_thread_exited() {
    let (pending, release, exited) =
        held_preview(Err(PreviewRunError::Failed("held preview failure".into())));
    let mut app = FerruleApp {
        pending_preview: Some(pending),
        ..FerruleApp::default()
    };
    let context = egui::Context::default();
    // The result was reported at once, but the thread is still running.
    let held_until = Instant::now() + Duration::from_millis(100);
    while Instant::now() < held_until {
        app.poll_preview(&context);
        assert!(
            app.pending_preview.is_some(),
            "a live worker is not complete"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(!exited.load(Ordering::Acquire));

    release.send(()).expect("held worker waits for release");
    let deadline = Instant::now() + Duration::from_secs(10);
    while app.pending_preview.is_some() {
        assert!(Instant::now() < deadline, "preview worker exits");
        app.poll_preview(&context);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(
        exited.load(Ordering::Acquire),
        "completion followed thread exit"
    );
    assert_eq!(app.status, "preview failed");
}

#[test]
fn dropping_a_pending_preview_cancels_and_waits_for_its_worker() {
    let (pending, release, exited) = held_preview(Err(PreviewRunError::Failed("unused".into())));
    let cancelled = pending.cancellation_for_test();
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        let _ = release.send(());
    });
    let flag = cancelled.upgrade().expect("pending preview owns its flag");
    drop(pending);
    assert!(flag.load(Ordering::Acquire), "drop requested cancellation");
    assert!(
        exited.load(Ordering::Acquire),
        "drop returned only after the worker exited"
    );
    releaser.join().expect("releaser finishes");
}
