use super::super::{FerruleApp, run_ui::*};
use super::*;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;

const READY: Duration = Duration::from_secs(2);
const RELEASE: Duration = Duration::from_secs(5);

fn trace() -> crate::run_report::TraceReport {
    crate::run_report::TraceReport {
        events: Vec::new(),
        dropped: 17,
        stages: vec!["trace-main-owned-372".into()],
        event_stages: Vec::new(),
    }
}
fn success() -> FileRunCompletion {
    (
        Ok(cli::RunOutcome {
            records_written: 23,
            input_path: "input-372.xml".into(),
            output_path: "output-372.xml".into(),
            primary_outputs: Vec::new(),
            extra_outputs: Vec::new(),
            artifacts: Vec::new(),
        }),
        trace(),
    )
}
fn failure() -> FileRunCompletion {
    (Err(FileRunError::Failed("LATE_ERROR_372".into())), trace())
}
fn cancelled() -> FileRunCompletion {
    (Err(FileRunError::Cancelled), trace())
}
fn assert_trace(value: &crate::run_report::TraceReport) {
    assert!(value.events.is_empty() && value.event_stages.is_empty());
    assert_eq!(value.dropped, 17);
    assert_eq!(value.stages, ["trace-main-owned-372"]);
}
fn name(command: FileRunCommand) -> &'static str {
    match command {
        FileRunCommand::Cancel => "Cancel",
        FileRunCommand::Publish => "Publish",
        FileRunCommand::Pause => "Pause",
        FileRunCommand::Continue => "Continue",
        FileRunCommand::Step => "Step",
    }
}

struct ReleaseGuard(Option<Sender<()>>);
impl ReleaseGuard {
    fn release(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
impl Drop for ReleaseGuard {
    fn drop(&mut self) {
        self.release();
    }
}

struct Fixture {
    release: ReleaseGuard,
    app: Option<FerruleApp>,
    service: Option<SavedFileRunTeardownService>,
    ready: Receiver<()>,
    commands: Option<Receiver<FileRunCommand>>,
    keeper: Option<Sender<FileRunEvent>>,
    joins: Arc<AtomicUsize>,
    cancellation: Arc<AtomicBool>,
    gate_allowed: Arc<AtomicBool>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        // Release first, even when an assertion panics, before app destruction
        // or service Drop might wait for the exact worker.
        self.release.release();
        drop(self.app.take());
        drop(self.keeper.take());
        drop(self.service.take());
    }
}
impl Fixture {
    fn handoff(&mut self) {
        if let Some(app) = &mut self.app {
            app.handoff_saved_file_run();
        }
        drop(self.app.take());
    }
    fn pending(&mut self) -> ShutdownProgress {
        self.service.as_mut().unwrap().drain_for(Duration::ZERO)
    }
    fn terminal(&mut self) -> JoinedFileRun {
        self.release.release();
        let (_, result) = self.service.as_mut().unwrap().finish_after_runner(());
        result.expect("one detached run supplies one exact joined terminal tuple")
    }
    fn command_log(&self) -> Vec<&'static str> {
        self.commands
            .as_ref()
            .map(|r| r.try_iter().map(name).collect())
            .unwrap_or_default()
    }
}

struct Options {
    phase: FileRunPhase,
    held: Option<FileRunCompletion>,
    terminal: Option<FileRunCompletion>,
    panic: bool,
    disconnect: bool,
    keep_sender: bool,
    actual_publication_gate: bool,
}
impl Default for Options {
    fn default() -> Self {
        Self {
            phase: FileRunPhase::Running,
            held: None,
            terminal: Some(cancelled()),
            panic: false,
            disconnect: false,
            keep_sender: false,
            actual_publication_gate: false,
        }
    }
}
fn fixture(mut options: Options) -> Fixture {
    let service = SavedFileRunTeardownService::new();
    let port = service.port();
    let reservation = port.reserve().unwrap();
    let (events, receiver) = mpsc::channel();
    let keeper = options.keep_sender.then(|| events.clone());
    let (commands, command_receiver) = mpsc::channel();
    let (ready, readiness) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let joins = Arc::new(AtomicUsize::new(0));
    let cancellation = Arc::new(AtomicBool::new(false));
    let worker_cancelled = cancellation.clone();
    let gate_allowed = Arc::new(AtomicBool::new(false));
    let worker_allowed = gate_allowed.clone();
    let mut core = FileRunCore {
        receiver,
        worker: None,
        completed: None,
        commands,
        cancelled: cancellation.clone(),
        started: Instant::now(),
        debug: false,
        phase: options.phase,
        shutdown: ShutdownIntent {
            join_observer: Some(joins.clone()),
            ..Default::default()
        },
    };
    if matches!(core.phase, FileRunPhase::Stopping) {
        core.cancelled.store(true, Ordering::Release);
    }
    let (gate_commands, observable_commands) = if options.actual_publication_gate {
        (Some(command_receiver), None)
    } else {
        (None, Some(command_receiver))
    };
    core.worker = Some(std::thread::spawn(move || {
        if let Some(completion) = options.held.take()
            && let Err(error) = events.send(FileRunEvent::Finished(Box::new(completion)))
            && let FileRunEvent::Finished(completion) = error.0
        {
            let (result, trace) = *completion;
            eprintln!("372 undelivered held original: {result:#?} {trace:#?}");
        }
        let mut events = Some(events);
        if let Some(commands) = gate_commands {
            let allowed = file_run_publication_gate_for_test(
                events.as_ref().unwrap().clone(),
                commands,
                worker_cancelled,
            );
            worker_allowed.store(allowed, Ordering::Release);
            if !allowed {
                options.terminal = Some(cancelled());
            }
        }
        if options.disconnect {
            drop(events.take());
        }
        let _ = ready.send(());
        let released = released.recv_timeout(RELEASE);
        eprintln!("372 release original before terminal action: {released:?}");
        if let Some(completion) = options.terminal.take()
            && let Some(sender) = events
            && let Err(error) = sender.send(FileRunEvent::Finished(Box::new(completion)))
            && let FileRunEvent::Finished(completion) = error.0
        {
            let (result, trace) = *completion;
            eprintln!("372 undelivered terminal original: {result:#?} {trace:#?}");
        }
        if options.panic {
            panic!("FIXED_PANIC_372");
        }
    }));
    Fixture {
        release: ReleaseGuard(Some(release)),
        app: Some(FerruleApp {
            pending_file_run: Some(PendingFileRun::new(core, reservation)),
            file_run_teardown: port,
            ..FerruleApp::default()
        }),
        service: Some(service),
        ready: readiness,
        commands: observable_commands,
        keeper,
        joins,
        cancellation,
        gate_allowed,
    }
}
fn ready(fixture: &Fixture) {
    let ready = fixture.ready.recv_timeout(READY);
    eprintln!("372 readiness original: {ready:?}");
    assert!(ready.is_ok());
}
fn joined_once(fixture: &Fixture) {
    assert_eq!(fixture.joins.load(Ordering::Acquire), 1);
}

#[test]
fn t01_normal_attached_completion_has_no_second_shutdown_join() {
    let mut f = fixture(Options {
        held: Some(failure()),
        terminal: None,
        ..Default::default()
    });
    ready(&f);
    f.app
        .as_mut()
        .unwrap()
        .poll_file_run(&egui::Context::default());
    f.release.release();
    let end = Instant::now() + READY;
    while !f
        .app
        .as_ref()
        .unwrap()
        .pending_file_run
        .as_ref()
        .unwrap()
        .worker
        .as_ref()
        .unwrap()
        .is_finished()
    {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
    let result = f
        .app
        .as_mut()
        .unwrap()
        .pending_file_run
        .as_mut()
        .unwrap()
        .take_completion_if_finished();
    f.handoff();
    let progress = f.pending();
    eprintln!(
        "T01 before comparison {result:#?} {progress:?} joins={}",
        f.joins.load(Ordering::Acquire)
    );
    assert!(matches!(result, Some((Err(FileRunError::Failed(_)), _))));
    assert_eq!(progress, ShutdownProgress::Idle);
    joined_once(&f);
}
#[test]
fn t02_app_destruction_transfers_exact_live_core() {
    let mut f = fixture(Options::default());
    ready(&f);
    drop(f.app.take());
    let pending = f.pending();
    let commands = f.command_log();
    let before = f.joins.load(Ordering::Acquire);
    let terminal = f.terminal();
    eprintln!("T02 before comparison {pending:?} {commands:?} {before} {terminal:#?}");
    assert_eq!(pending, ShutdownProgress::PendingOwned);
    assert_eq!(before, 0);
    assert_eq!(commands, ["Cancel"]);
    assert_eq!(terminal.shutdown_cancel_attempts, 1);
    joined_once(&f);
}

struct Supervisor {
    handle: Option<JoinHandle<()>>,
    release: Sender<()>,
}
impl Drop for Supervisor {
    fn drop(&mut self) {
        let _ = self.release.send(());
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}
fn runner_result_control(error: bool) {
    let mut f = fixture(Options {
        terminal: Some(failure()),
        ..Default::default()
    });
    ready(&f);
    f.handoff();
    let pending = f.pending();
    let mut owner = f.service.take().unwrap();
    let (entered, entering) = mpsc::channel();
    let (done, completed) = mpsc::channel();
    let result: Result<(), &'static str> = if error {
        Err("RUNNER_ERROR_372")
    } else {
        Ok(())
    };
    let release = f.release.0.as_ref().unwrap().clone();
    let mut supervisor = Supervisor {
        release,
        handle: Some(std::thread::spawn(move || {
            let _ = entered.send(());
            let output = owner.finish_after_runner(result);
            let _ = done.send(output);
        })),
    };
    let entry = entering.recv_timeout(READY);
    let held = completed.try_recv();
    f.release.release();
    let output = completed.recv_timeout(RELEASE);
    let joined = supervisor.handle.take().unwrap().join();
    eprintln!("T03/T04 before comparison {pending:?} {entry:?} {held:#?} {output:#?} {joined:?}");
    assert_eq!(pending, ShutdownProgress::PendingOwned);
    assert!(entry.is_ok());
    assert!(matches!(held, Err(TryRecvError::Empty)));
    assert!(joined.is_ok());
    let (actual, terminal) = output.unwrap();
    assert_eq!(actual, result);
    let terminal = terminal.unwrap();
    assert!(matches!(terminal.result, Err(FileRunError::Failed(_))));
    assert_trace(&terminal.trace);
    joined_once(&f);
}
#[test]
fn t03_runner_ok_waits_for_its_live_saved_file() {
    runner_result_control(false);
}
#[test]
fn t04_runner_error_waits_without_replacing_original_error() {
    runner_result_control(true);
}
#[test]
fn t05_shutdown_cancel_is_idempotent() {
    let mut f = fixture(Options::default());
    ready(&f);
    f.handoff();
    let p1 = f.pending();
    let p2 = f.pending();
    let commands = f.command_log();
    let terminal = f.terminal();
    eprintln!("T05 before comparison {p1:?} {p2:?} {commands:?} {terminal:#?}");
    assert_eq!(
        (p1, p2),
        (
            ShutdownProgress::PendingOwned,
            ShutdownProgress::PendingOwned
        )
    );
    assert_eq!(commands, ["Cancel"]);
    assert!(f.cancellation.load(Ordering::Acquire));
    assert_eq!(terminal.shutdown_cancel_attempts, 1);
    joined_once(&f);
}
#[test]
fn t06_stopping_has_no_additional_shutdown_cancel() {
    let mut f = fixture(Options {
        phase: FileRunPhase::Stopping,
        ..Default::default()
    });
    ready(&f);
    f.handoff();
    let pending = f.pending();
    let commands = f.command_log();
    let terminal = f.terminal();
    eprintln!("T06 before comparison {pending:?} {commands:?} {terminal:#?}");
    assert_eq!(pending, ShutdownProgress::PendingOwned);
    assert!(commands.is_empty());
    assert_eq!(terminal.shutdown_cancel_attempts, 0);
    joined_once(&f);
}
#[test]
fn t07_actual_permission_preserves_publishing_late_success() {
    let mut f = fixture(Options {
        actual_publication_gate: true,
        terminal: Some(success()),
        ..Default::default()
    });
    let context = egui::Context::default();
    let end = Instant::now() + READY;
    while !matches!(
        f.app
            .as_ref()
            .unwrap()
            .pending_file_run
            .as_ref()
            .unwrap()
            .phase,
        FileRunPhase::Publishing
    ) {
        f.app.as_mut().unwrap().poll_file_run(&context);
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
    ready(&f);
    f.handoff();
    let pending = f.pending();
    let terminal = f.terminal();
    eprintln!(
        "T07 before comparison {pending:?} {terminal:#?} gate={}",
        f.gate_allowed.load(Ordering::Acquire)
    );
    assert_eq!(pending, ShutdownProgress::PendingOwned);
    assert!(f.gate_allowed.load(Ordering::Acquire));
    assert!(!f.cancellation.load(Ordering::Acquire));
    assert_eq!(terminal.shutdown_cancel_attempts, 0);
    let outcome = terminal.result.unwrap();
    assert_eq!(outcome.records_written, 23);
    assert_eq!(
        outcome.input_path,
        std::path::PathBuf::from("input-372.xml")
    );
    assert_eq!(
        outcome.output_path,
        std::path::PathBuf::from("output-372.xml")
    );
    assert_trace(&terminal.trace);
    joined_once(&f);
}
#[test]
fn t08_queued_ready_is_not_shutdown_publication_permission() {
    let mut f = fixture(Options {
        actual_publication_gate: true,
        ..Default::default()
    });
    // Observe and retain the actual queued Ready event, without going through
    // ordinary app polling or granting Publish. The phase remains Running.
    let queued = f
        .app
        .as_ref()
        .unwrap()
        .pending_file_run
        .as_ref()
        .unwrap()
        .receiver
        .recv_timeout(READY);
    let was_ready = matches!(queued, Ok(FileRunEvent::ReadyToPublish));
    f.handoff();
    ready(&f);
    let pending = f.pending();
    let terminal = f.terminal();
    eprintln!(
        "T08 before comparison queued_ready={was_ready} {pending:?} {terminal:#?} gate={}",
        f.gate_allowed.load(Ordering::Acquire)
    );
    assert!(was_ready);
    assert_eq!(pending, ShutdownProgress::PendingOwned);
    assert!(!f.gate_allowed.load(Ordering::Acquire));
    assert!(matches!(terminal.result, Err(FileRunError::Cancelled)));
    assert_eq!(terminal.shutdown_cancel_attempts, 1);
    joined_once(&f);
}
#[test]
fn t09_reported_tuple_is_held_until_exact_worker_exit() {
    let mut f = fixture(Options {
        held: Some((Err(FileRunError::Failed("RESULT_HELD_372".into())), trace())),
        terminal: None,
        ..Default::default()
    });
    ready(&f);
    f.app
        .as_mut()
        .unwrap()
        .poll_file_run(&egui::Context::default());
    let held = format!(
        "{:?}",
        f.app
            .as_ref()
            .unwrap()
            .pending_file_run
            .as_ref()
            .unwrap()
            .completed
    );
    f.handoff();
    let pending = f.pending();
    let joins = f.joins.load(Ordering::Acquire);
    let terminal = f.terminal();
    eprintln!("T09 before comparison {held} {pending:?} {joins} {terminal:#?}");
    assert!(held.contains("RESULT_HELD_372") && held.contains("trace-main-owned-372"));
    assert_eq!(pending, ShutdownProgress::PendingOwned);
    assert_eq!(joins, 0);
    assert!(
        matches!(terminal.result,Err(FileRunError::Failed(ref text)) if text=="RESULT_HELD_372")
    );
    assert_trace(&terminal.trace);
    joined_once(&f);
}
#[test]
fn t10_panic_without_report_has_priority_over_close_intent() {
    let mut f = fixture(Options {
        panic: true,
        terminal: None,
        ..Default::default()
    });
    ready(&f);
    f.handoff();
    let terminal = f.terminal();
    eprintln!("T10 before comparison {terminal:#?}");
    assert!(matches!(
        terminal.result,
        Err(FileRunError::Worker(FileRunWorkerFailure::Panicked))
    ));
    joined_once(&f);
}
#[test]
fn t11_held_trace_survives_a_later_worker_panic() {
    let mut f = fixture(Options {
        held: Some(failure()),
        terminal: None,
        panic: true,
        ..Default::default()
    });
    ready(&f);
    f.handoff();
    let pending = f.pending();
    let terminal = f.terminal();
    eprintln!("T11 before comparison {pending:?} {terminal:#?}");
    assert_eq!(pending, ShutdownProgress::PendingOwned);
    assert!(matches!(
        terminal.result,
        Err(FileRunError::Worker(FileRunWorkerFailure::Panicked))
    ));
    assert_trace(&terminal.trace);
    joined_once(&f);
}
#[test]
fn t12_finished_connected_empty_has_typed_unreported_terminal() {
    let mut f = fixture(Options {
        terminal: None,
        keep_sender: true,
        ..Default::default()
    });
    ready(&f);
    f.handoff();
    let pending = f.pending();
    let terminal = f.terminal();
    eprintln!("T12 before comparison {pending:?} {terminal:#?}");
    assert_eq!(pending, ShutdownProgress::PendingOwned);
    assert!(matches!(
        terminal.result,
        Err(FileRunError::Worker(FileRunWorkerFailure::Unreported))
    ));
    joined_once(&f);
}
#[test]
fn t13_disconnect_does_not_complete_a_live_thread() {
    let mut f = fixture(Options {
        terminal: None,
        disconnect: true,
        ..Default::default()
    });
    ready(&f);
    f.handoff();
    let pending = f.pending();
    let before = f.joins.load(Ordering::Acquire);
    let terminal = f.terminal();
    eprintln!("T13 before comparison {pending:?} {before} {terminal:#?}");
    assert_eq!(pending, ShutdownProgress::PendingOwned);
    assert_eq!(before, 0);
    assert!(matches!(
        terminal.result,
        Err(FileRunError::Worker(FileRunWorkerFailure::Unreported))
    ));
    joined_once(&f);
}
#[test]
fn t14_service_outlives_all_app_ports() {
    let mut f = fixture(Options::default());
    ready(&f);
    drop(f.app.take());
    let pending = f.pending();
    let terminal = f.terminal();
    eprintln!("T14 before comparison {pending:?} {terminal:#?}");
    assert_eq!(pending, ShutdownProgress::PendingOwned);
    assert!(matches!(terminal.result, Err(FileRunError::Cancelled)));
    joined_once(&f);
}
#[test]
fn t15_last_owner_fallback_waits_and_joins_without_detach() {
    let mut f = fixture(Options::default());
    ready(&f);
    let pending = f.app.as_mut().unwrap().pending_file_run.take().unwrap();
    drop(f.app.take());
    // Remove the idle attached service without running its blocking Drop: its
    // port is replaced with an empty owner, leaving the pending reservation as
    // the last reference to the original exact slot.
    let mut service = f.service.take().unwrap();
    service.port = SavedFileRunTeardownPort::default();
    drop(service);
    let (entered, entry) = mpsc::channel();
    let (returned, result) = mpsc::channel();
    let release = f.release.0.as_ref().unwrap().clone();
    let mut supervisor = Supervisor {
        release,
        handle: Some(std::thread::spawn(move || {
            let _ = entered.send(());
            drop(pending);
            let _ = returned.send(());
        })),
    };
    let entered = entry.recv_timeout(READY);
    let blocked = result.try_recv();
    f.release.release();
    let returned = result.recv_timeout(RELEASE);
    let joined = supervisor.handle.take().unwrap().join();
    eprintln!(
        "T15 before comparison {entered:?} {blocked:?} {returned:?} {joined:?} joins={}",
        f.joins.load(Ordering::Acquire)
    );
    assert!(entered.is_ok() && returned.is_ok() && joined.is_ok());
    assert!(matches!(blocked, Err(TryRecvError::Empty)));
    joined_once(&f);
}
#[test]
fn t16_one_slot_refuses_busy_and_unconsumed_terminal() {
    let mut f = fixture(Options::default());
    ready(&f);
    let busy = f.service.as_ref().unwrap().port().reserve().is_none();
    f.handoff();
    f.release.release();
    let owner = f.service.as_mut().unwrap();
    let terminal_progress = owner.drain_for(RELEASE);
    let unconsumed = owner.port().reserve().is_none();
    let terminal = owner.take_terminal();
    // A closing owner does not reopen admissions; a new non-closing standalone
    // slot admits again after an attached normally joined facade is consumed.
    let closing = owner.port().reserve().is_none();
    let mut normal = fixture(Options {
        held: Some(failure()),
        terminal: None,
        ..Default::default()
    });
    ready(&normal);
    normal
        .app
        .as_mut()
        .unwrap()
        .poll_file_run(&egui::Context::default());
    normal.release.release();
    let end = Instant::now() + READY;
    while !normal
        .app
        .as_ref()
        .unwrap()
        .pending_file_run
        .as_ref()
        .unwrap()
        .worker
        .as_ref()
        .unwrap()
        .is_finished()
    {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(1));
    }
    let consumed = normal
        .app
        .as_mut()
        .unwrap()
        .pending_file_run
        .as_mut()
        .unwrap()
        .take_completion_if_finished();
    normal.app.as_mut().unwrap().handoff_saved_file_run();
    let next = normal.service.as_ref().unwrap().port().reserve();
    let reusable = next.is_some();
    drop(next);
    eprintln!(
        "T16 before comparison {busy} {terminal_progress:?} {unconsumed} {terminal:#?} {closing} normal={consumed:#?} {reusable}"
    );
    assert!(busy && unconsumed && closing && reusable);
    assert_eq!(terminal_progress, ShutdownProgress::Joined);
    assert!(terminal.is_some() && consumed.is_some());
    joined_once(&f);
    joined_once(&normal);
}
#[test]
fn t17_normal_to_shutdown_transfer_has_one_join_consumer() {
    let mut f = fixture(Options {
        held: Some(failure()),
        terminal: None,
        ..Default::default()
    });
    ready(&f);
    f.app
        .as_mut()
        .unwrap()
        .poll_file_run(&egui::Context::default());
    let normal = f
        .app
        .as_mut()
        .unwrap()
        .pending_file_run
        .as_mut()
        .unwrap()
        .take_completion_if_finished();
    f.handoff();
    let pending = f.pending();
    let terminal = f.terminal();
    eprintln!("T17 before comparison {normal:#?} {pending:?} {terminal:#?}");
    assert!(normal.is_none());
    assert_eq!(pending, ShutdownProgress::PendingOwned);
    assert_trace(&terminal.trace);
    joined_once(&f);
}
#[test]
fn t18_late_failure_and_trace_are_not_rewritten_as_cancelled() {
    let mut f = fixture(Options {
        terminal: Some(failure()),
        ..Default::default()
    });
    ready(&f);
    f.handoff();
    let terminal = f.terminal();
    eprintln!("T18 before comparison {terminal:#?}");
    assert!(
        matches!(terminal.result,Err(FileRunError::Failed(ref text)) if text=="LATE_ERROR_372")
    );
    assert_trace(&terminal.trace);
    joined_once(&f);
}
