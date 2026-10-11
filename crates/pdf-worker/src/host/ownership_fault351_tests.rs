//! Root-selected qualification. The first ten controls model already-closed
//! process fields explicitly; they exercise real manager/file failure branches,
//! but never credit the fabricated fields as kernel ownership or reap evidence.
use super::super::ownership_fault351 as fault;
use super::*;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn root(key: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var_os(key).expect("fresh root-only admission required"));
    assert!(path.is_absolute() && !path.exists());
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn original(path: &Path, value: impl std::fmt::Debug) {
    fault::retained(path, format!("{value:#?}\n").as_bytes()).unwrap();
}
fn sub(root: &Path, key: &str) -> PathBuf {
    let path = root.join(key);
    std::fs::create_dir(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn manager(output: &Path, name: &str) -> PdfWorkerManager {
    let source = PathBuf::from(
        std::env::var_os("FERRULE_PDF_WORKER_351_FIXTURE").expect("qualified fixture ELF required"),
    );
    let executable = output.join(name);
    std::fs::copy(source, &executable).unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o500)).unwrap();
    let program = WorkerProgram::admit_test_fixture(&executable).unwrap();
    let scratch = sub(output, "scratch");
    PdfWorkerManager::new(
        program,
        ScratchPolicy::admit(&scratch, 0).unwrap(),
        ExclusiveReaperAdmission::for_current_host(),
    )
    .unwrap()
}
fn layout() -> mapping::PdfLayout {
    mapping::PdfLayout::new(
        "Document",
        mapping::PdfPageSelection::First,
        vec![mapping::PdfCommand::Capture(mapping::PdfCapture {
            name: "Value".into(),
            region: mapping::PdfRegion::full(),
            algorithm: Default::default(),
        })],
    )
    .unwrap()
}
fn retained_files(output: &Path, dir: &Dir) {
    let rows = dir.census().unwrap();
    original(&output.join("FILES_BEFORE.original.txt"), &rows);
    for row in rows {
        let bytes = dir
            .read(&row.name, usize::try_from(row.byte_len).unwrap())
            .unwrap();
        fault::retained(
            &output.join(format!("before-{}.original", row.name)),
            &bytes,
        )
        .unwrap();
    }
}
fn closed_model(manager: &PdfWorkerManager) -> Job {
    let (dir, id) = manager.scratch.create_job().unwrap();
    for (name, bytes) in [
        ("input.pdf", b"synthetic-not-parsed".as_slice()),
        ("request.json", b"{}".as_slice()),
        ("control.bin", b"".as_slice()),
        ("stderr.bin", b"".as_slice()),
    ] {
        dir.write_frozen(name, bytes).unwrap();
    }
    let instance = ir::Instance::Group(
        vec![(
            "Value".into(),
            ir::Instance::Scalar(ir::Value::String("Hello".into())),
        )]
        .into(),
    );
    let mut bytes = Vec::new();
    let descriptor = crate::encode_instance(&instance, &mut bytes).unwrap();
    dir.write_frozen("instance.json", &bytes).unwrap();
    bytes.clear();
    let result = crate::ResultEnvelope::new(
        id,
        crate::Outcome::Ok(
            crate::InstanceDescriptor::new(descriptor.byte_len, descriptor.sha256).unwrap(),
        ),
    )
    .unwrap();
    crate::encode_result(&result, &mut bytes).unwrap();
    dir.write_frozen("result.json", &bytes).unwrap();
    // Explicit injected closed-state input, NOT an observed status/PID/reap.
    let receipt = ClosedReceipt {
        request_id: id.request_id().to_hex(),
        profile: id.profile().as_str(),
        program: manager.program.record.clone(),
        source_before: None,
        source_after: None,
        observed_unexpected_children: Vec::new(),
        child_pid: None,
        child_birth: None,
        child_ppid: None,
        pidfd_acquired: false,
        ready: true,
        start_sent: true,
        effective_limits: None,
        work_seconds: WORK_SECONDS,
        stop_seconds: STOP_SECONDS,
        raw_wait_status: Some(0),
        reap_usage: None,
        own_reap: true,
        stderr_observed_bytes: 0,
        stdout_observed_bytes: 0,
        stderr_overflow: false,
        stdout_closed_before_eof: false,
        stderr_closed_before_eof: false,
        polls: 0,
        first_poll_microseconds: None,
        last_poll_microseconds: None,
        events: Vec::new(),
        files: Vec::new(),
        cleanup_removed: Vec::new(),
        failure: None,
        secondary_failures: Vec::new(),
        stop_allowance_exceeded: false,
        receipt_published_before_cleanup: false,
        elapsed_microseconds: 0,
        successful_originals_retained: false,
    };
    Job {
        dir,
        id,
        start: Instant::now(),
        cancel: CancellationFlag::new(),
        child: None,
        stdin: None,
        stdout: None,
        stderr: None,
        pidfd: None,
        stop: None,
        term_attempted: false,
        kill_attempted: false,
        wait_lost: false,
        unknown_children: false,
        failure: None,
        budget_observations: BudgetObservations::default(),
        failure_epoch: 0,
        secondary_cap_reached: false,
        history: super::super::terminal::History::default(),
        secondary: Vec::new(),
        receipt,
        terminal: None,
    }
}
struct BrokenRead;
impl Read for BrokenRead {
    fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
        Err(std::io::Error::from_raw_os_error(libc::EIO))
    }
}
fn capture<T>(pipe: T, file: Option<File>) -> Capture<T> {
    Capture {
        pipe: Some(pipe),
        file,
        prefix: Vec::new(),
        observed: 0,
        cap: 64,
        eof: false,
        overflow: false,
        closed_before_eof: false,
    }
}
#[test]
#[ignore = "root-only serial real filesystem plus injected closed-state branch qualification; no child launch"]
fn manager_fault351_closed_file_boundaries() {
    let out = root("FERRULE_PDF_WORKER_351_MODEL_EVIDENCE");
    let read_out = sub(&out, "M01-capture-read");
    let mut read = capture(BrokenRead, None);
    let result = read.step();
    original(
        &read_out.join("ACTUAL.original.txt"),
        (
            &result,
            read.eof,
            read.closed_before_eof,
            read.observed,
            &read.prefix,
        ),
    );
    let error = result.unwrap_err();
    assert_eq!(
        (error.kind, error.operation, error.errno),
        (FailureKind::Io, "capture read", Some(libc::EIO))
    );
    assert!(read.eof && read.closed_before_eof && read.pipe.is_none());
    let write_out = sub(&out, "M02-capture-write");
    let path = write_out.join("owned-capture.bin");
    fault::retained(&path, b"").unwrap();
    let mut write = capture(std::io::Cursor::new(b"X"), Some(File::open(&path).unwrap()));
    let result = write.step();
    original(
        &write_out.join("ACTUAL.original.txt"),
        (
            &result,
            write.eof,
            write.closed_before_eof,
            write.observed,
            &write.prefix,
        ),
    );
    let error = result.unwrap_err();
    assert_eq!(
        (error.kind, error.operation, error.errno),
        (FailureKind::Io, "capture write", Some(libc::EBADF))
    );
    assert!(write.closed_before_eof && write.prefix.is_empty() && write.observed == 1);
    let sync_out = sub(&out, "M03-capture-fsync");
    let _guard = fault::arm(Some(("capture-fsync", "capture")), &sync_out);
    let path = sync_out.join("owned-capture.bin");
    fault::retained(&path, b"").unwrap();
    let mut sync = capture(std::io::Cursor::new(b""), Some(File::open(&path).unwrap()));
    let result = sync.step();
    original(
        &sync_out.join("ACTUAL.original.txt"),
        (&result, sync.eof, sync.closed_before_eof, fault::facts()),
    );
    let error = result.unwrap_err();
    assert_eq!(
        (error.kind, error.operation, error.errno),
        (FailureKind::Io, "capture fsync", Some(libc::EIO))
    );
    assert!(fault::facts().0 && sync.eof && !sync.closed_before_eof);
    drop(_guard);
    for (key, point, name, operation, published) in [
        (
            "M04-receipt-write",
            "frozen-write",
            "receipt.part",
            "receipt durable write",
            false,
        ),
        (
            "M05-receipt-fsync",
            "frozen-fsync",
            "receipt.part",
            "receipt durable write",
            false,
        ),
        (
            "M06-receipt-directory-fsync",
            "frozen-directory-fsync",
            "receipt.part",
            "receipt durable write",
            false,
        ),
        (
            "M07-receipt-rename",
            "rename",
            "receipt.part",
            "receipt publication",
            false,
        ),
        (
            "M08-cleanup-intent-fsync",
            "frozen-fsync",
            "cleanup-0.json",
            "durable cleanup intent",
            true,
        ),
        (
            "M09-artifact-unlink",
            "artifact-unlink",
            "*",
            "exact completed artifact unlink",
            true,
        ),
        (
            "M10-directory-unlink",
            "directory-unlink",
            "job",
            "empty owned directory unlink",
            true,
        ),
    ] {
        let case = sub(&out, key);
        let mut manager = manager(&case, "fixture351-success");
        let job = closed_model(&manager);
        retained_files(&case, &job.dir);
        original(
            &case.join("INJECTED_CLOSED_STATE.original.txt"),
            &job.receipt,
        );
        manager.job = Some(job);
        let _guard = fault::arm(Some((point, name)), &case);
        let actual = manager.poll();
        original(&case.join("ACTUAL.original.txt"), &actual);
        original(&case.join("BOUNDARY_JOURNAL.original.txt"), fault::facts());
        let files = manager.job.as_ref().unwrap().dir.census();
        original(&case.join("FILES_AFTER.original.txt"), &files);
        let JobProgress::FailedClosed { failure, retained } = actual else {
            panic!("{key}: normal success forbidden");
        };
        assert_eq!(
            (
                failure.failure.kind,
                failure.failure.operation,
                failure.failure.errno
            ),
            (FailureKind::Io, operation, Some(libc::EIO))
        );
        assert_eq!(failure.receipt.receipt_published_before_cleanup, published);
        assert!(
            fault::facts().0
                && retained.exists()
                && manager.job.as_ref().unwrap().terminal.is_some()
        );
        assert!(failure.receipt.child_pid.is_none() && !failure.receipt.pidfd_acquired);
        if key == "M09-artifact-unlink" {
            assert!(failure.receipt.cleanup_removed.is_empty());
        }
        if key == "M10-directory-unlink" {
            assert!(files.unwrap().is_empty() && !failure.receipt.cleanup_removed.is_empty());
        }
    }
}
fn start_real(manager: &mut PdfWorkerManager, case: &Path, cancel: &CancellationFlag) {
    let input = case.join("input.data");
    fault::retained(&input, b"synthetic protocol fixture; no PDF parser").unwrap();
    let begin = manager.begin(&input, &layout(), cancel);
    original(&case.join("BEGIN.original.txt"), &begin);
    match begin {
        Ok(_) => {}
        Err(BeginFailure::NoChild(_)) => panic!("real fixture refused before child"),
        Err(BeginFailure::OwnedStopping { .. }) => {
            let closed = closed_real(manager, case);
            original(&case.join("BEGIN_FAILURE_CLOSED.original.txt"), &closed);
            panic!("real fixture begin failed; no qualification");
        }
    }
    // No deadline substitutes for closure: root V14 must rescue uncertain actors
    // as a failed qualification, never credit a timeout or manager Drop as reap.
    loop {
        let progress = manager.poll();
        if matches!(
            progress,
            JobProgress::Pending {
                state: JobState::Running,
                ..
            }
        ) {
            original(&case.join("RUNNING.original.txt"), &progress);
            return;
        }
        if matches!(
            progress,
            JobProgress::Succeeded { .. } | JobProgress::FailedClosed { .. }
        ) {
            original(&case.join("UNEXPECTED_CLOSED.original.txt"), &progress);
            panic!("expected owned Running synthetic child");
        }
        std::thread::yield_now();
    }
}
fn closed_real(manager: &mut PdfWorkerManager, case: &Path) -> JobProgress {
    let mut incomplete_saved = false;
    loop {
        let progress = manager.poll();
        if !incomplete_saved && matches!(progress, JobProgress::ClosureIncomplete { .. }) {
            original(
                &case.join("FIRST_CLOSURE_INCOMPLETE.original.txt"),
                &progress,
            );
            incomplete_saved = true;
        }
        if matches!(
            progress,
            JobProgress::Succeeded { .. } | JobProgress::FailedClosed { .. }
        ) {
            original(&case.join("CLOSED.original.txt"), &progress);
            return progress;
        }
        std::thread::yield_now();
    }
}
#[test]
#[ignore = "root-only owned child with deferred wait observations and an injected stop clock; V14 rescue is failure"]
fn manager_fault351_later_real_own_reap_after_modeled_expiry() {
    let out = root("FERRULE_PDF_WORKER_351_LATE_REAP_EVIDENCE");
    let mut manager = manager(&out, "fixture351-ignore-term");
    let cancel = CancellationFlag::new();
    start_real(&mut manager, &out, &cancel);
    let _guard = fault::arm(None, &out);
    fault::set_deferred_wait(true);
    cancel.cancel();
    let first = manager.poll();
    original(&out.join("FIRST_STOP.original.txt"), &first);
    let job = manager.job.as_mut().expect("owned job retained");
    job.stop = Some(
        Instant::now()
            .checked_sub(Duration::from_millis(6001))
            .unwrap(),
    );
    original(
        &out.join("INJECTED_STOP_CLOCK.original.txt"),
        "6001ms-old stop clock; not an elapsed OS allowance observation",
    );
    let incomplete = manager.poll();
    original(&out.join("INCOMPLETE.original.txt"), &incomplete);
    original(&out.join("DEFERRED_WAIT.original.txt"), fault::facts());
    let deferred_calls = fault::facts().1;
    fault::set_deferred_wait(false);
    let actual = closed_real(&mut manager, &out);
    original(&out.join("JOURNAL.original.txt"), fault::facts());
    let JobProgress::FailedClosed { failure, .. } = actual else {
        panic!("expiry/cancellation never success");
    };
    assert!(
        matches!(incomplete, JobProgress::ClosureIncomplete { snapshot } if snapshot.stop_allowance_exceeded && !snapshot.own_reap)
    );
    assert_eq!(failure.failure.kind, FailureKind::Cancelled);
    assert!(
        failure.receipt.stop_allowance_exceeded
            && failure.receipt.pidfd_acquired
            && failure.receipt.own_reap
    );
    assert!(
        failure.receipt.child_birth.is_some()
            && failure.receipt.reap_usage.is_some()
            && deferred_calls >= 2
    );
    let status = failure.receipt.raw_wait_status.unwrap();
    assert!(libc::WIFSIGNALED(status) && libc::WTERMSIG(status) == libc::SIGKILL);
    assert_eq!(
        failure
            .receipt
            .events
            .iter()
            .filter(|e| e.operation == "own wait4 reap")
            .count(),
        1
    );
    assert!(
        failure.receipt.secondary_failures.is_empty()
            && failure.receipt.observed_unexpected_children.is_empty()
    );
}
#[test]
#[ignore = "root-only physical ignored TERM then held-PIDfd KILL; exact <=1s bound, no injection; V14 rescue is failure"]
fn manager_fault351_physical_term_to_kill_at_most_one_second() {
    let out = root("FERRULE_PDF_WORKER_351_TIMING_EVIDENCE");
    let mut manager = manager(&out, "fixture351-ignore-term");
    let cancel = CancellationFlag::new();
    start_real(&mut manager, &out, &cancel);
    let clock = Instant::now();
    let mut term = None;
    let mut kill = None;
    let mut intervals = Vec::new();
    original(
        &out.join("PRE_CANCEL_RECEIPT.original.txt"),
        &manager.job.as_ref().unwrap().receipt,
    );
    let mut seen = manager.job.as_ref().unwrap().receipt.events.len();
    let mut incomplete_saved = false;
    cancel.cancel();
    let actual = loop {
        let before = clock.elapsed().as_nanos();
        let progress = manager.poll();
        let after = clock.elapsed().as_nanos();
        if !incomplete_saved && matches!(progress, JobProgress::ClosureIncomplete { .. }) {
            original(
                &out.join("FIRST_CLOSURE_INCOMPLETE.original.txt"),
                &progress,
            );
            incomplete_saved = true;
        }
        let events = match &progress {
            JobProgress::FailedClosed { failure, .. } => &failure.receipt.events,
            JobProgress::Succeeded { receipt, .. } => &receipt.events,
            _ => {
                &manager
                    .job
                    .as_ref()
                    .expect("nonterminal owner retained")
                    .receipt
                    .events
            }
        };
        for event in &events[seen..] {
            if event.operation == "held pidfd TERM" {
                term = Some((before, after, event.errno));
            }
            if event.operation == "held pidfd KILL" {
                kill = Some((before, after, event.errno));
            }
            intervals.push((
                before,
                after,
                event.operation,
                event.at_microseconds,
                event.errno,
            ));
        }
        seen = events.len();
        if matches!(
            progress,
            JobProgress::Succeeded { .. } | JobProgress::FailedClosed { .. }
        ) {
            break progress;
        }
        std::thread::yield_now();
    };
    original(&out.join("ACTUAL.original.txt"), &actual);
    original(&out.join("EVENT_POLL_INTERVALS.original.txt"), &intervals);
    original(
        &out.join("TERM_KILL_INTERVALS.original.txt"),
        (&term, &kill),
    );
    // Every syscall is within its enclosing poll. This upper bound avoids
    // treating rounded receipt timestamps or samples as exact syscall clocks.
    let upper = kill
        .as_ref()
        .zip(term.as_ref())
        .and_then(|(k, t)| k.1.checked_sub(t.0));
    original(&out.join("CONSERVATIVE_UPPER_NS.original.txt"), upper);
    let JobProgress::FailedClosed { failure, .. } = actual else {
        panic!("cancellation never normal success");
    };
    assert_eq!(failure.failure.kind, FailureKind::Cancelled);
    assert!(
        failure.receipt.pidfd_acquired
            && failure.receipt.child_birth.is_some()
            && failure.receipt.own_reap
    );
    assert!(term.is_some_and(|t| t.2.is_none()) && kill.is_some_and(|k| k.2.is_none()));
    assert!(
        upper.is_some_and(|n| n <= 1_000_000_000),
        "exact <=1s requirement NOT established"
    );
    let status = failure.receipt.raw_wait_status.unwrap();
    assert!(libc::WIFSIGNALED(status) && libc::WTERMSIG(status) == libc::SIGKILL);
    assert!(
        failure.receipt.secondary_failures.is_empty()
            && failure.receipt.observed_unexpected_children.is_empty()
    );
    assert_eq!(
        failure
            .receipt
            .events
            .iter()
            .filter(|e| e.operation == "own wait4 reap")
            .count(),
        1
    );
}
