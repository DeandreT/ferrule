use super::*;

const READY_WAIT: Duration = Duration::from_secs(2);
const RELEASE_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug)]
struct Snapshot {
    pending: bool,
    owned_handle: bool,
    worker_finished: Option<bool>,
    completed: Option<String>,
    phase: Option<FileRunPhase>,
    cancelled: Option<bool>,
    deferred_close: bool,
    status: String,
    report: bool,
    show_report: bool,
}

fn snapshot(app: &FerruleApp) -> Snapshot {
    let pending = app.pending_file_run.as_ref();
    Snapshot {
        pending: pending.is_some(),
        owned_handle: pending.is_some_and(|pending| pending.worker.is_some()),
        worker_finished: pending
            .and_then(|pending| pending.worker.as_ref().map(JoinHandle::is_finished)),
        completed: pending.and_then(|pending| {
            pending
                .completed
                .as_ref()
                .map(|result| format!("{result:#?}"))
        }),
        phase: pending.map(|pending| pending.phase.clone()),
        cancelled: pending.map(|pending| pending.cancelled.load(Ordering::Acquire)),
        deferred_close: app.close_after_file_run,
        status: app.status.clone(),
        report: app.run_report.is_some(),
        show_report: app.show_run_report,
    }
}

fn held_result(
    result: Result<cli::RunOutcome, FileRunError>,
    trace: crate::run_report::TraceReport,
    phase: FileRunPhase,
    panic_after_release: bool,
) -> (FerruleApp, Receiver<()>, Sender<()>) {
    let (events, receiver) = mpsc::channel();
    let (commands, _command_receiver) = mpsc::channel();
    let (ready, readiness) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let sent = events
            .send(FileRunEvent::Finished(Box::new((result, trace))))
            .map_err(|failure| match failure.0 {
                FileRunEvent::Finished(completion) => {
                    let (result, trace) = *completion;
                    format!("{result:#?} {trace:#?}")
                }
                _ => unreachable!("fixture sends only a complete result"),
            });
        eprintln!("held file worker complete send, retaining any returned result: {sent:?}");
        let _ = ready.send(());
        let released = released.recv_timeout(RELEASE_WAIT);
        eprintln!("held file worker release: {released:?}");
        if panic_after_release {
            panic!("fixed saved-file thread panic control");
        }
    });
    (
        FerruleApp {
            pending_file_run: Some(PendingFileRun::from_test_core(FileRunCore {
                receiver,
                worker: Some(worker),
                completed: None,
                commands,
                cancelled: Arc::new(AtomicBool::new(false)),
                started: Instant::now(),
                debug: false,
                phase,
                shutdown: ShutdownIntent::default(),
            })),
            ..FerruleApp::default()
        },
        readiness,
        release,
    )
}

fn wait_finished(app: &FerruleApp) -> bool {
    let deadline = Instant::now() + READY_WAIT;
    loop {
        if app
            .pending_file_run
            .as_ref()
            .is_some_and(|pending| pending.worker.as_ref().is_some_and(JoinHandle::is_finished))
        {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

pub(super) fn close_during_publication() {
    let (mut app, readiness, release) = held_result(
        Err(FileRunError::Cancelled),
        crate::run_report::TraceReport::default(),
        FileRunPhase::Publishing,
        false,
    );
    let ready = readiness.recv_timeout(READY_WAIT);
    let context = egui::Context::default();
    app.guard_app_close_requested(&context, true);
    app.poll_file_run(&context);
    let held = snapshot(&app);
    let released = release.send(());
    let finished = wait_finished(&app);
    app.poll_file_run(&context);
    let joined = snapshot(&app);
    eprintln!(
        "J01 complete observations before comparisons: {ready:?} {released:?} {finished:?} {held:#?} {joined:#?}"
    );
    assert!(ready.is_ok() && released.is_ok() && finished);
    assert!(held.pending && held.owned_handle && held.deferred_close);
    assert_eq!(held.worker_finished, Some(false));
    assert!(held.completed.is_some());
    assert!(matches!(held.phase, Some(FileRunPhase::Publishing)));
    assert_eq!(held.cancelled, Some(false));
    assert!(!joined.pending && !joined.owned_handle && !joined.deferred_close);
    assert_eq!(joined.status, "run cancelled");
}

#[test]
fn disconnected_live_worker_remains_owned_until_joined() {
    let (events, receiver) = mpsc::channel();
    let (commands, _command_receiver) = mpsc::channel();
    let (ready, readiness) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        drop(events);
        let _ = ready.send(());
        let released = released.recv_timeout(RELEASE_WAIT);
        eprintln!("disconnected file worker release: {released:?}");
    });
    let mut app = FerruleApp {
        pending_file_run: Some(PendingFileRun::from_test_core(FileRunCore {
            receiver,
            worker: Some(worker),
            completed: None,
            commands,
            cancelled: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
            debug: false,
            phase: FileRunPhase::Running,
            shutdown: ShutdownIntent::default(),
        })),
        ..FerruleApp::default()
    };
    let ready = readiness.recv_timeout(READY_WAIT);
    let context = egui::Context::default();
    app.poll_file_run(&context);
    let disconnected = snapshot(&app);
    app.guard_app_close_requested(&context, true);
    let close = snapshot(&app);
    let released = release.send(());
    let finished = wait_finished(&app);
    app.poll_file_run(&context);
    let joined = snapshot(&app);
    eprintln!(
        "J02 complete observations before comparisons: {ready:?} {released:?} {finished:?} {disconnected:#?} {close:#?} {joined:#?}"
    );
    assert!(ready.is_ok() && released.is_ok() && finished);
    assert!(disconnected.pending && disconnected.owned_handle);
    assert_eq!(disconnected.worker_finished, Some(false));
    assert_eq!(disconnected.cancelled, Some(true));
    assert!(
        disconnected
            .completed
            .as_ref()
            .is_some_and(|result| result.contains("Unreported"))
    );
    assert!(close.pending && close.deferred_close);
    assert!(!joined.pending && !joined.deferred_close);
    assert_eq!(joined.status, "run failed");
}

#[test]
fn join_panic_stays_visible_after_requested_cancellation() {
    let (mut app, readiness, release) = held_result(
        Err(FileRunError::Cancelled),
        crate::run_report::TraceReport::default(),
        FileRunPhase::Running,
        true,
    );
    let ready = readiness.recv_timeout(READY_WAIT);
    let context = egui::Context::default();
    app.file_run_command(FileRunCommand::Cancel);
    app.poll_file_run(&context);
    let held = snapshot(&app);
    let released = release.send(());
    let finished = wait_finished(&app);
    app.poll_file_run(&context);
    let joined = snapshot(&app);
    eprintln!(
        "J03 complete observations before comparisons: {ready:?} {released:?} {finished:?} {held:#?} {joined:#?}"
    );
    assert!(ready.is_ok() && released.is_ok() && finished);
    assert!(held.pending && held.owned_handle);
    assert!(matches!(held.phase, Some(FileRunPhase::Stopping)));
    assert_eq!(held.cancelled, Some(true));
    assert!(!joined.pending);
    assert_eq!(joined.status, "run failed");
}

#[test]
fn finished_thread_without_event_has_typed_unreported_result() {
    let (_events, receiver) = mpsc::channel();
    let (commands, _command_receiver) = mpsc::channel();
    let mut app = FerruleApp {
        pending_file_run: Some(PendingFileRun::from_test_core(FileRunCore {
            receiver,
            worker: Some(std::thread::spawn(|| {})),
            completed: None,
            commands,
            cancelled: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
            debug: false,
            phase: FileRunPhase::Running,
            shutdown: ShutdownIntent::default(),
        })),
        ..FerruleApp::default()
    };
    let finished = wait_finished(&app);
    let result = app
        .pending_file_run
        .as_mut()
        .and_then(PendingFileRun::take_completion_if_finished);
    let second = app
        .pending_file_run
        .as_mut()
        .and_then(PendingFileRun::take_completion_if_finished);
    let handle_retained = app
        .pending_file_run
        .as_ref()
        .is_some_and(|pending| pending.worker.is_some());
    if let Some(pending) = app.pending_file_run.as_mut() {
        pending.worker = Some(std::thread::spawn(|| {}));
    }
    let next_finished = wait_finished(&app);
    let empty_connected = snapshot(&app);
    app.poll_file_run(&egui::Context::default());
    let next_joined = snapshot(&app);
    eprintln!(
        "J04 complete observations before comparisons: {finished:?} {result:#?} {second:#?} {handle_retained:?} {next_finished:?} {empty_connected:#?} {next_joined:#?}"
    );
    assert!(finished);
    assert!(matches!(
        result,
        Some((
            Err(FileRunError::Worker(FileRunWorkerFailure::Unreported)),
            _
        ))
    ));
    assert!(second.is_none() && !handle_retained);
    assert!(next_finished && empty_connected.pending && empty_connected.owned_handle);
    assert!(empty_connected.completed.is_none());
    assert!(!next_joined.pending);
    assert_eq!(next_joined.status, "run failed");
}

#[test]
fn joined_result_and_complete_trace_are_preserved() {
    let trace = crate::run_report::TraceReport {
        events: Vec::new(),
        dropped: 7,
        stages: vec!["frozen-stage".into()],
        event_stages: Vec::new(),
    };
    let (mut app, readiness, release) = held_result(
        Err(FileRunError::Failed("frozen mapping failure".into())),
        trace,
        FileRunPhase::Running,
        false,
    );
    let ready = readiness.recv_timeout(READY_WAIT);
    app.poll_file_run(&egui::Context::default());
    let held = snapshot(&app);
    let released = release.send(());
    let finished = wait_finished(&app);
    let result = app
        .pending_file_run
        .as_mut()
        .and_then(PendingFileRun::take_completion_if_finished);
    let second = app
        .pending_file_run
        .as_mut()
        .and_then(PendingFileRun::take_completion_if_finished);
    eprintln!(
        "J05 complete observations before comparisons: {ready:?} {released:?} {finished:?} {held:#?} {result:#?} {second:#?}"
    );
    assert!(ready.is_ok() && released.is_ok() && finished);
    assert!(held.pending && held.owned_handle && held.completed.is_some());
    let Some((Err(FileRunError::Failed(message)), trace)) = result else {
        panic!("the complete frozen mapping failure and trace are required");
    };
    assert_eq!(message, "frozen mapping failure");
    assert!(trace.events.is_empty());
    assert_eq!(trace.dropped, 7);
    assert_eq!(trace.stages, ["frozen-stage"]);
    assert!(trace.event_stages.is_empty());
    assert!(second.is_none());
}

#[test]
fn requested_cancel_suppresses_a_late_success_after_join() {
    let outcome = cli::RunOutcome {
        records_written: 3,
        input_path: PathBuf::from("frozen-input.xml"),
        output_path: PathBuf::from("frozen-output.xml"),
        primary_outputs: Vec::new(),
        extra_outputs: Vec::new(),
        artifacts: Vec::new(),
    };
    let (mut app, readiness, release) = held_result(
        Ok(outcome),
        crate::run_report::TraceReport::default(),
        FileRunPhase::Running,
        false,
    );
    let ready = readiness.recv_timeout(READY_WAIT);
    let context = egui::Context::default();
    app.file_run_command(FileRunCommand::Cancel);
    app.poll_file_run(&context);
    let held = snapshot(&app);
    let released = release.send(());
    let finished = wait_finished(&app);
    app.poll_file_run(&context);
    let joined = snapshot(&app);
    eprintln!(
        "J06 complete observations before comparisons: {ready:?} {released:?} {finished:?} {held:#?} {joined:#?}"
    );
    assert!(ready.is_ok() && released.is_ok() && finished);
    assert!(held.pending && held.owned_handle && held.completed.is_some());
    assert!(!joined.pending && !joined.report && !joined.show_report);
    assert_eq!(joined.status, "run cancelled");
}

#[test]
fn join_panic_has_typed_priority_over_a_reported_mapping_failure() {
    let (mut app, readiness, release) = held_result(
        Err(FileRunError::Failed("frozen mapping failure".into())),
        crate::run_report::TraceReport::default(),
        FileRunPhase::Stopping,
        true,
    );
    let ready = readiness.recv_timeout(READY_WAIT);
    app.poll_file_run(&egui::Context::default());
    let held = snapshot(&app);
    let released = release.send(());
    let finished = wait_finished(&app);
    let result = app
        .pending_file_run
        .as_mut()
        .and_then(PendingFileRun::take_completion_if_finished);
    let second = app
        .pending_file_run
        .as_mut()
        .and_then(PendingFileRun::take_completion_if_finished);
    eprintln!(
        "J07 complete observations before comparisons: {ready:?} {released:?} {finished:?} {held:#?} {result:#?} {second:#?}"
    );
    assert!(ready.is_ok() && released.is_ok() && finished);
    assert!(matches!(
        result,
        Some((Err(FileRunError::Worker(FileRunWorkerFailure::Panicked)), _))
    ));
    assert!(second.is_none());
}

#[test]
fn queued_publication_event_does_not_discard_the_following_result() {
    let (events, receiver) = mpsc::channel();
    let (commands, _command_receiver) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let ready = events.send(FileRunEvent::ReadyToPublish).is_ok();
        let finished = events
            .send(FileRunEvent::Finished(Box::new((
                Err(FileRunError::Cancelled),
                crate::run_report::TraceReport::default(),
            ))))
            .map_err(|failure| match failure.0 {
                FileRunEvent::Finished(completion) => {
                    let (result, trace) = *completion;
                    format!("{result:#?} {trace:#?}")
                }
                _ => unreachable!("fixture sends only a complete result"),
            });
        eprintln!("queued file worker sends: {ready:?} {finished:?}");
    });
    let mut app = FerruleApp {
        pending_file_run: Some(PendingFileRun::from_test_core(FileRunCore {
            receiver,
            worker: Some(worker),
            completed: None,
            commands,
            cancelled: Arc::new(AtomicBool::new(false)),
            started: Instant::now(),
            debug: false,
            phase: FileRunPhase::Running,
            shutdown: ShutdownIntent::default(),
        })),
        ..FerruleApp::default()
    };
    let finished = wait_finished(&app);
    let context = egui::Context::default();
    app.poll_file_run(&context);
    let publication = snapshot(&app);
    app.poll_file_run(&context);
    let joined = snapshot(&app);
    eprintln!(
        "J08 complete observations before comparisons: {finished:?} {publication:#?} {joined:#?}"
    );
    assert!(finished);
    assert!(publication.pending && publication.owned_handle);
    assert_eq!(publication.worker_finished, Some(true));
    assert!(publication.completed.is_none());
    assert!(matches!(publication.phase, Some(FileRunPhase::Publishing)));
    assert!(!joined.pending);
    assert_eq!(joined.status, "run cancelled");
}
