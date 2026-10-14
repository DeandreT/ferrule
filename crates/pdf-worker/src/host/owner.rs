use super::fs_job::{self, Dir};
use super::*;
use std::fs::File;
use std::io::{Read, Write};
use std::marker::PhantomData;
use std::os::fd::{AsRawFd, OwnedFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command, Stdio};
use std::rc::Rc;
use std::time::{Duration, Instant};

struct Capture<T> {
    pipe: Option<T>,
    file: Option<File>,
    prefix: Vec<u8>,
    observed: u64,
    cap: usize,
    eof: bool,
    overflow: bool,
    closed_before_eof: bool,
}
impl<T: Read> Capture<T> {
    fn new(pipe: T, file: File, cap: usize) -> Self {
        Self {
            pipe: Some(pipe),
            file: Some(file),
            prefix: Vec::new(),
            observed: 0,
            cap,
            eof: false,
            overflow: false,
            closed_before_eof: false,
        }
    }
    fn step(&mut self) -> Result<(), Failure> {
        if self.eof {
            if let Some(f) = self.file.take() {
                f.sync_all()
                    .map_err(|e| Failure::io("closed capture fsync", e))?;
            }
            return Ok(());
        }
        let mut block = [0; 4096];
        let n = match self
            .pipe
            .as_mut()
            .expect("live capture has pipe")
            .read(&mut block)
        {
            Ok(n) => n,
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::Interrupted =>
            {
                return Ok(());
            }
            Err(e) => {
                self.pipe.take();
                self.eof = true;
                self.closed_before_eof = true;
                return Err(Failure::io("capture read", e));
            }
        };
        if n == 0 {
            self.eof = true;
            self.pipe.take();
            if let Some(f) = self.file.take() {
                #[cfg(test)]
                super::ownership_fault351::probe("capture-fsync", "capture")
                    .map_err(|e| Failure::io("capture fsync", e))?;
                f.sync_all().map_err(|e| Failure::io("capture fsync", e))?;
            }
            return Ok(());
        }
        self.observed = self
            .observed
            .checked_add(n as u64)
            .ok_or_else(|| Failure::new(FailureKind::Protocol, "capture count overflow"))?;
        let keep = n.min(self.cap - self.prefix.len());
        // Preserve the complete bounded prefix before any expected comparison.
        if let Some(f) = &mut self.file
            && let Err(e) = f.write_all(&block[..keep])
        {
            self.pipe.take();
            self.eof = true;
            self.closed_before_eof = true;
            return Err(Failure::io("capture write", e));
        }
        self.prefix.extend_from_slice(&block[..keep]);
        if keep < n {
            self.overflow = true;
            return Err(Failure::new(FailureKind::Protocol, "capture overflow"));
        }
        Ok(())
    }
}
// Only the two fixed check_budget operations are repeat observations.
// Keep a separate bit per budget class; every other failure remains distinct.
#[derive(Default)]
struct BudgetObservations {
    cancelled: bool,
    timed_out: bool,
}
impl BudgetObservations {
    fn first(&mut self, failure: &Failure) -> bool {
        let seen = match (failure.kind, failure.operation) {
            (FailureKind::Cancelled, "cancellation observed") => &mut self.cancelled,
            (FailureKind::TimedOut, "work cutoff") => &mut self.timed_out,
            _ => return true,
        };
        !std::mem::replace(seen, true)
    }
}

struct Job {
    dir: Dir,
    id: crate::RequestIdentity,
    start: Instant,
    cancel: CancellationFlag,
    child: Option<Child>,
    stdin: Option<ChildStdin>,
    stdout: Option<Capture<ChildStdout>>,
    stderr: Option<Capture<ChildStderr>>,
    pidfd: Option<OwnedFd>,
    stop: Option<Instant>,
    term_attempted: bool,
    kill_attempted: bool,
    wait_lost: bool,
    unknown_children: bool,
    failure: Option<Failure>,
    budget_observations: BudgetObservations,
    failure_epoch: u64,
    secondary_cap_reached: bool,
    history: super::terminal::History,
    secondary: Vec<Failure>,
    receipt: ClosedReceipt,
    terminal: Option<ClosedFailure>,
}
impl Job {
    fn event(&mut self, operation: &'static str, errno: Option<i32>) {
        if self.receipt.events.len() < 64 {
            self.receipt.events.push(Event {
                at_microseconds: micros(self.start.elapsed()),
                operation,
                errno,
            });
        }
    }
    fn fail(&mut self, f: Failure) {
        if !self.budget_observations.first(&f) {
            return;
        }
        self.failure_epoch = self.failure_epoch.saturating_add(1);
        if self.failure.is_none() {
            self.event("stop", f.errno);
            self.failure = Some(f);
            self.stop = Some(Instant::now());
            self.stdin.take();
        } else if self.secondary.len() < 8 {
            self.secondary.push(f);
        } else {
            self.secondary_cap_reached = true;
        }
    }
    fn unresolved(&self) -> bool {
        unresolved(
            self.child.is_some(),
            self.receipt.own_reap,
            self.wait_lost,
            self.unknown_children,
            self.stdout.as_ref().is_some_and(|c| !c.eof),
            self.stderr.as_ref().is_some_and(|c| !c.eof),
        )
    }
    fn snapshot(&self) -> ClosureSnapshot {
        ClosureSnapshot {
            evidence: self.dir.path.clone(),
            child_pid: self.receipt.child_pid,
            primary: self
                .failure
                .clone()
                .unwrap_or_else(|| Failure::new(FailureKind::Ownership, "unresolved child")),
            own_reap: self.receipt.own_reap,
            stop_allowance_exceeded: self.receipt.stop_allowance_exceeded,
        }
    }
    #[cfg(test)]
    fn observe366(&mut self, phase: &'static str) {
        if let Some(failure) = super::terminal::tests::budget(phase) {
            match failure.kind {
                FailureKind::Cancelled => self.cancel.cancel(),
                FailureKind::TimedOut => {
                    self.start = Instant::now()
                        .checked_sub(Duration::from_secs(WORK_SECONDS + 1))
                        .unwrap();
                }
                _ => unreachable!("only fixed modeled budget observations"),
            }
        }
    }
    fn publish_receipt(&mut self) -> Result<(), Failure> {
        self.history.began_epoch = self.failure_epoch;
        self.receipt.elapsed_microseconds = micros(self.start.elapsed());
        self.receipt.failure = self.failure.clone();
        self.receipt.secondary_failures = self.secondary.clone();
        self.receipt.receipt_published_before_cleanup = true;
        let result = (|| {
            self.receipt.files = self
                .dir
                .census()
                .map_err(|e| Failure::classified_io(FailureKind::Protocol, "full job census", e))?;
            let bytes = serde_json::to_vec(&self.receipt)
                .map_err(|e| Failure::io("receipt encode", std::io::Error::other(e)))?;
            if bytes.len() > RECEIPT_BYTES {
                return Err(Failure::new(FailureKind::Receipt, "receipt byte cap"));
            }
            self.dir
                .write_frozen("receipt.part", &bytes)
                .map_err(|e| Failure::io("receipt durable write", e))?;
            self.dir
                .rename("receipt.part", "receipt.json")
                .map_err(|e| Failure::io("receipt publication", e))?;
            self.history.snapshot = Some(super::terminal::snapshot(&self.dir, &bytes)?);
            #[cfg(test)]
            self.observe366("after-snapshot");
            Ok(())
        })();
        if let Err(failure) = &result {
            self.receipt.receipt_published_before_cleanup = false;
            self.history.snapshot_failure = Some(failure.clone());
        }
        result
    }
}
// Leave polling slack inside the unchanged at-most-one-second TERM grace.
// The real signal interval still requires independent physical qualification.
fn kill_due(grace: Duration, kill_attempted: bool) -> bool {
    grace >= Duration::from_millis(900) && !kill_attempted
}

fn micros(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}
/// Single-thread, single-job ownership. Dropping this manager does not prove
/// reap or cleanup. Embedders must continue polling ClosureIncomplete.
pub struct PdfWorkerManager {
    program: WorkerProgram,
    scratch: ScratchPolicy,
    job: Option<Job>,
    retained_successes: Vec<Dir>,
    retain_success: bool,
    last_terminal_evidence: Option<TerminalEvidence>,
    launch_thread: std::thread::ThreadId,
    _not_send: PhantomData<Rc<()>>,
}
impl PdfWorkerManager {
    pub fn new(
        program: WorkerProgram,
        scratch: ScratchPolicy,
        _: ExclusiveReaperAdmission,
    ) -> Result<Self, Failure> {
        super::linux::ordinary_sigchld()
            .map_err(|e| Failure::io("ordinary exclusive SIGCHLD", e))?;
        Ok(Self {
            program,
            scratch,
            job: None,
            retained_successes: Vec::new(),
            retain_success: false,
            last_terminal_evidence: None,
            launch_thread: std::thread::current().id(),
            _not_send: PhantomData,
        })
    }
    pub fn retain_successful_originals(&mut self, retain: bool) -> Result<(), Failure> {
        if self.job.is_some() {
            return Err(Failure::new(
                FailureKind::Ownership,
                "retention choice during job",
            ));
        }
        self.retain_success = retain;
        Ok(())
    }
    pub fn evidence_paths(&self) -> Vec<std::path::PathBuf> {
        self.retained_successes
            .iter()
            .map(|d| d.path.clone())
            .chain(self.job.as_ref().map(|j| j.dir.path.clone()))
            .collect()
    }
    /// Exact parent-relative failure leaves; `evidence_paths` keeps job semantics.
    /// Clone this descriptor before a new job replaces the last cleanup metadata.
    pub fn terminal_evidence(&self) -> Option<&TerminalEvidence> {
        self.job
            .as_ref()
            .and_then(|job| job.history.evidence.as_ref())
            .or(self.last_terminal_evidence.as_ref())
    }
    pub fn request_cancel(&mut self) {
        if let Some(j) = &self.job {
            j.cancel.cancel();
        }
    }
    pub fn begin(
        &mut self,
        source: &std::path::Path,
        layout: &mapping::PdfLayout,
        cancel: &CancellationFlag,
    ) -> Result<crate::RequestIdentity, BeginFailure> {
        if let Some(kind) = layout.repair_dependency() {
            let e = format_pdf::PdfError::RepairDependency(kind);
            return Err(BeginFailure::NoChild(Failure {
                kind: FailureKind::RepairDependency,
                operation: "repair-dependent layout",
                diagnostics: crate::error::capture(&e),
                errno: None,
            }));
        }
        let start = Instant::now();
        if self.launch_thread != std::thread::current().id()
            || self.job.is_some()
            || self.retained_successes.len() >= 32
        {
            return Err(BeginFailure::NoChild(Failure::new(
                FailureKind::Ownership,
                "busy/wrong-thread/retention cap",
            )));
        }
        fs_job::check_budget(start, cancel).map_err(BeginFailure::NoChild)?;
        self.program.check().map_err(|e| {
            BeginFailure::NoChild(Failure::classified_io(
                FailureKind::UnsupportedHost,
                "packaged worker changed",
                e,
            ))
        })?;
        super::linux::ordinary_sigchld()
            .map_err(|e| BeginFailure::NoChild(Failure::io("fresh SIGCHLD admission", e)))?;
        let (dir, id) = self
            .scratch
            .create_job()
            .map_err(|e| BeginFailure::NoChild(Failure::io("fresh job admission", e)))?;
        let receipt = ClosedReceipt {
            request_id: id.request_id().to_hex(),
            profile: id.profile().as_str(),
            program: self.program.record.clone(),
            source_before: None,
            source_after: None,
            observed_unexpected_children: Vec::new(),
            child_pid: None,
            child_birth: None,
            child_ppid: None,
            pidfd_acquired: false,
            ready: false,
            start_sent: false,
            effective_limits: None,
            work_seconds: WORK_SECONDS,
            stop_seconds: STOP_SECONDS,
            raw_wait_status: None,
            reap_usage: None,
            own_reap: false,
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
            successful_originals_retained: self.retain_success,
        };
        self.last_terminal_evidence = None;
        let mut job = Job {
            dir,
            id,
            start,
            cancel: cancel.clone(),
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
            history: super::terminal::History::default(),
            secondary: Vec::new(),
            receipt,
            terminal: None,
        };
        let prepare = (|| -> Result<(), Failure> {
            let (before, after) = fs_job::stage(
                &job.dir,
                id,
                source,
                layout,
                start,
                cancel,
                (
                    &mut job.receipt.source_before,
                    &mut job.receipt.source_after,
                ),
            )?;
            job.receipt.source_before = Some(before);
            job.receipt.source_after = Some(after);
            let stdout_file = job
                .dir
                .create("control.bin")
                .map_err(|e| Failure::io("control original", e))?;
            let stderr_file = job
                .dir
                .create("stderr.bin")
                .map_err(|e| Failure::io("stderr original", e))?;
            fs_job::check_budget(start, cancel)?;
            let child = Command::new(&self.program.path)
                .arg(id.profile().as_str())
                .arg(std::process::id().to_string())
                .arg(id.request_id().to_hex())
                .current_dir(&job.dir.path)
                .env_clear()
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0)
                .spawn()
                .map_err(|e| Failure::io("spawn exact worker", e))?;
            job.receipt.child_pid = Some(child.id());
            job.child = Some(child);
            job.event("spawn", None);
            let child = job.child.as_mut().expect("spawned child retained");
            job.stdin = child.stdin.take();
            job.stdout = child
                .stdout
                .take()
                .map(|pipe| Capture::new(pipe, stdout_file, 4097));
            job.stderr = child
                .stderr
                .take()
                .map(|pipe| Capture::new(pipe, stderr_file, 65536));
            let pid = child.id();
            job.pidfd = Some(super::linux::pidfd(pid).map_err(|e| {
                Failure::classified_io(FailureKind::UnsupportedHost, "held pidfd", e)
            })?);
            job.receipt.pidfd_acquired = true;
            let (birth, parent) =
                super::linux::birth(pid).map_err(|e| Failure::io("child birth/parent", e))?;
            if parent != std::process::id() {
                return Err(Failure::new(FailureKind::Ownership, "child parent differs"));
            }
            job.receipt.child_birth = Some(birth);
            job.receipt.child_ppid = Some(parent);
            for fd in [
                job.stdin.as_ref().map(AsRawFd::as_raw_fd),
                job.stdout
                    .as_ref()
                    .and_then(|c| c.pipe.as_ref())
                    .map(AsRawFd::as_raw_fd),
                job.stderr
                    .as_ref()
                    .and_then(|c| c.pipe.as_ref())
                    .map(AsRawFd::as_raw_fd),
            ] {
                super::linux::nonblocking(
                    fd.ok_or_else(|| Failure::new(FailureKind::Ownership, "missing pipe"))?,
                )
                .map_err(|e| Failure::io("nonblocking capture", e))?;
            }
            Ok(())
        })();
        if let Err(f) = prepare {
            // A pipe whose nonblocking setup failed must never enter a blocking
            // poll read. Close only that owned capture and retain this limitation.
            if let Some(c) = &mut job.stdout
                && let Some(pipe) = &c.pipe
                && super::linux::nonblocking(pipe.as_raw_fd()).is_err()
            {
                c.pipe.take();
                c.eof = true;
                c.closed_before_eof = true;
            }
            if let Some(c) = &mut job.stderr
                && let Some(pipe) = &c.pipe
                && super::linux::nonblocking(pipe.as_raw_fd()).is_err()
            {
                c.pipe.take();
                c.eof = true;
                c.closed_before_eof = true;
            }
            job.fail(f.clone());
            let evidence = job.dir.path.clone();
            self.job = Some(job);
            return Err(BeginFailure::OwnedStopping {
                failure: Box::new(f),
                evidence,
            });
        }
        self.job = Some(job);
        Ok(id)
    }
    pub fn poll(&mut self) -> JobProgress {
        let Some(mut job) = self.job.take() else {
            return JobProgress::ClosureIncomplete {
                snapshot: ClosureSnapshot {
                    evidence: std::path::PathBuf::new(),
                    child_pid: None,
                    primary: Failure::new(FailureKind::Ownership, "no active job"),
                    own_reap: false,
                    stop_allowance_exceeded: false,
                },
            };
        };
        if let Some(terminal) = &job.terminal {
            let progress = JobProgress::FailedClosed {
                failure: Box::new(ClosedFailure {
                    failure: terminal.failure.clone(),
                    child_outcome: terminal.child_outcome.clone(),
                    receipt: terminal.receipt.clone(),
                }),
                retained: job.history.retained_path(&job.dir.path),
            };
            self.job = Some(job);
            return progress;
        }
        job.receipt.polls = job.receipt.polls.saturating_add(1);
        let now = micros(job.start.elapsed());
        job.receipt.first_poll_microseconds.get_or_insert(now);
        job.receipt.last_poll_microseconds = Some(now);
        if job.failure.is_none()
            && let Err(f) = fs_job::check_budget(job.start, &job.cancel)
        {
            job.fail(f);
        }
        if let Some(c) = &mut job.stdout
            && let Err(f) = c.step()
        {
            job.fail(f);
        }
        if let Some(c) = &mut job.stderr
            && let Err(f) = c.step()
        {
            job.fail(f);
        }
        // The production worker creates no children. Any observed violation is
        // retained and permanently prevents closure promotion; unknown children
        // are never signaled or waited for by this manager.
        if !job.receipt.own_reap
            && !job.unknown_children
            && let Some(pid) = job.receipt.child_pid
        {
            match super::linux::children(pid) {
                Ok(children) if !children.is_empty() => {
                    job.receipt.observed_unexpected_children = children;
                    job.unknown_children = true;
                    job.fail(Failure::new(
                        FailureKind::Ownership,
                        "unexpected worker descendants",
                    ));
                }
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => {
                    job.unknown_children = true;
                    job.fail(Failure::io("worker descendant visibility unresolved", e));
                }
            }
        }
        // Exactly one nonblocking reap attempt per poll. Even during stop there
        // is no std Child wait, /proc-absence inference, or external waiter.
        if !job.receipt.own_reap
            && !job.wait_lost
            && let Some(pid) = job.receipt.child_pid
        {
            #[cfg(test)]
            let wait = if super::ownership_fault351::defer_wait() {
                Ok(super::linux::Wait::Pending)
            } else {
                super::linux::own_wait(pid)
            };
            #[cfg(not(test))]
            let wait = super::linux::own_wait(pid);
            match wait {
                Ok(super::linux::Wait::Pending) => {}
                Ok(super::linux::Wait::Reaped { status, usage }) => {
                    job.receipt.own_reap = true;
                    job.receipt.raw_wait_status = Some(status);
                    job.receipt.reap_usage = Some(usage);
                    job.stdin.take();
                    job.pidfd.take();
                    job.event("own wait4 reap", None);
                }
                Err(e) => {
                    job.wait_lost = true;
                    job.fail(Failure::io("own wait4 unresolved", e));
                }
            }
        }
        if job.failure.is_none() && !job.receipt.start_sent {
            let readiness = job.stdout.as_ref().map(|c| (c.eof, c.prefix.clone()));
            // Empty pre-Ready EOF must await our own exit status before it can
            // be associated with a declared ResourceSetup refusal. No Start is
            // sent while pending; earlier failures and the work cutoff still win.
            if let Some((eof, bytes)) = readiness
                && eof
                && (!bytes.is_empty() || job.receipt.own_reap)
            {
                let ready = (|| -> Result<(), Failure> {
                    if bytes.is_empty()
                        && !job.receipt.ready
                        && job.receipt.own_reap
                        && let Ok(raw) = job.dir.read("result.json", crate::MAX_RESULT_BYTES)
                        && resource_setup_before_ready(&raw, &job.id, job.receipt.raw_wait_status)
                    {
                        return Err(Failure::new(
                            FailureKind::ResourceSetup,
                            "complete pre-Ready resource setup refusal",
                        ));
                    }
                    let body = bytes
                        .strip_suffix(b"\n")
                        .ok_or_else(|| Failure::new(FailureKind::Protocol, "ready EOF/frame"))?;
                    if body.contains(&b'\n') {
                        return Err(Failure::new(FailureKind::Protocol, "multiple ready frames"));
                    }
                    let ready = crate::decode_ready(body, &job.id)
                        .map_err(|e| Failure::codec("ready", e))?;
                    if job.receipt.own_reap {
                        return Err(Failure::new(FailureKind::WorkerExit, "exit before Start"));
                    }
                    self.program
                        .check_child(
                            job.receipt.child_pid.unwrap(),
                            job.receipt.child_birth.unwrap(),
                        )
                        .map_err(|e| {
                            Failure::classified_io(
                                FailureKind::Protocol,
                                "actual child exe/birth",
                                e,
                            )
                        })?;
                    job.dir
                        .check_child_cwd(job.receipt.child_pid.unwrap())
                        .map_err(|e| {
                            Failure::classified_io(FailureKind::Protocol, "actual child cwd", e)
                        })?;
                    fs_job::check_budget(job.start, &job.cancel)?;
                    job.receipt.ready = true;
                    job.receipt.effective_limits = Some(ready.effective_limits().values());
                    job.event("ready/exe admitted", None);
                    let mut start = Vec::new();
                    crate::encode_start(&crate::StartFrame::new(job.id.request_id()), &mut start)
                        .map_err(|e| Failure::codec("start", e))?;
                    start.push(b'\n');
                    job.stdin
                        .as_mut()
                        .ok_or_else(|| Failure::new(FailureKind::Ownership, "Start pipe closed"))?
                        .write_all(&start)
                        .map_err(|e| Failure::io("Start write", e))?;
                    job.stdin.take();
                    job.receipt.start_sent = true;
                    job.event("Start sent and closed", None);
                    Ok(())
                })();
                if let Err(f) = ready {
                    job.fail(f);
                }
            }
        }
        if job.failure.is_some() && !job.receipt.own_reap {
            let grace = job.stop.map(|s| s.elapsed()).unwrap_or_default();
            if !job.term_attempted {
                job.term_attempted = true;
                if let Some(fd) = &job.pidfd {
                    let r = super::linux::signal(fd, libc::SIGTERM);
                    job.event(
                        "held pidfd TERM",
                        r.as_ref().err().and_then(std::io::Error::raw_os_error),
                    );
                    if let Err(e) = r {
                        job.secondary.push(Failure::io("pidfd TERM", e));
                    }
                }
            }
            if kill_due(grace, job.kill_attempted) {
                job.kill_attempted = true;
                if let Some(fd) = &job.pidfd {
                    let r = super::linux::signal(fd, libc::SIGKILL);
                    job.event(
                        "held pidfd KILL",
                        r.as_ref().err().and_then(std::io::Error::raw_os_error),
                    );
                    if let Err(e) = r {
                        job.secondary.push(Failure::io("pidfd KILL", e));
                    }
                }
            }
        }
        if let Some(stop) = job.stop
            && stop.elapsed() >= Duration::from_secs(STOP_SECONDS)
        {
            job.receipt.stop_allowance_exceeded = true;
        }
        if let Some(c) = &job.stdout {
            job.receipt.stdout_observed_bytes = c.observed;
            job.receipt.stdout_closed_before_eof = c.closed_before_eof;
        }
        if let Some(c) = &job.stderr {
            job.receipt.stderr_observed_bytes = c.observed;
            job.receipt.stderr_overflow = c.overflow;
            job.receipt.stderr_closed_before_eof = c.closed_before_eof;
        }
        if job.unresolved() {
            let progress =
                if job.receipt.stop_allowance_exceeded || job.wait_lost || job.unknown_children {
                    JobProgress::ClosureIncomplete {
                        snapshot: job.snapshot(),
                    }
                } else {
                    JobProgress::Pending {
                        state: if job.failure.is_some() {
                            JobState::Stopping
                        } else if job.receipt.start_sent {
                            JobState::Running
                        } else {
                            JobState::Starting
                        },
                        snapshot: PendingSnapshot {
                            child_pid: job.receipt.child_pid,
                            own_reap: job.receipt.own_reap,
                            elapsed_microseconds: micros(job.start.elapsed()),
                        },
                    }
                };
            self.job = Some(job);
            return progress;
        }
        // All owned process/pipe responsibility closed. Drop the already-reaped
        // Child handle only now; its Drop is not credited as a reap operation.
        job.child.take();
        job.stdin.take();
        job.pidfd.take();
        let mut instance = None;
        let mut outcome = None;
        if let Ok(raw) = job.dir.read("result.json", crate::MAX_RESULT_BYTES)
            && let Ok(result) = crate::decode_result(&raw, &job.id)
        {
            outcome = Some(result.outcome().clone());
        }
        if job.failure.is_none() {
            let verify = (|| -> Result<ir::Instance, Failure> {
                if !job.receipt.start_sent {
                    return Err(Failure::new(FailureKind::Protocol, "no Start"));
                }
                let status = job
                    .receipt
                    .raw_wait_status
                    .ok_or_else(|| Failure::new(FailureKind::Ownership, "no own status"))?;
                if libc::WIFSIGNALED(status) {
                    return Err(Failure::new(
                        FailureKind::WorkerExit,
                        "worker terminated by signal",
                    ));
                }
                let raw = job
                    .dir
                    .read("result.json", crate::MAX_RESULT_BYTES)
                    .map_err(|e| {
                        Failure::classified_io(
                            if libc::WIFEXITED(status) && libc::WEXITSTATUS(status) != 0 {
                                FailureKind::WorkerExit
                            } else {
                                FailureKind::Protocol
                            },
                            "complete result original",
                            e,
                        )
                    })?;
                let result =
                    crate::decode_result(&raw, &job.id).map_err(|e| Failure::codec("result", e))?;
                outcome = Some(result.outcome().clone());
                if !libc::WIFEXITED(status)
                    || libc::WEXITSTATUS(status) != result.required_exit_code()
                {
                    return Err(Failure::new(
                        FailureKind::Protocol,
                        "exit/result association",
                    ));
                }
                match result.outcome() {
                    crate::Outcome::Ok(desc) => {
                        let raw = job
                            .dir
                            .read("instance.json", crate::MAX_INSTANCE_BYTES)
                            .map_err(|e| {
                                Failure::classified_io(
                                    FailureKind::Protocol,
                                    "complete instance original",
                                    e,
                                )
                            })?;
                        crate::decode_verified_instance(&raw, desc)
                            .map_err(|e| Failure::codec("verified typed instance", e))
                    }
                    _ => Err(Failure::new(
                        FailureKind::Pdf,
                        "complete declared worker error",
                    )),
                }
            })();
            match verify {
                Ok(i) => instance = Some(i),
                Err(f) => job.fail(f),
            }
        }
        if let Err(f) = fs_job::check_budget(job.start, &job.cancel) {
            job.fail(f);
            instance = None;
        }
        if job.receipt.stop_allowance_exceeded && job.failure.is_none() {
            job.fail(Failure::new(
                FailureKind::Ownership,
                "stop allowance exceeded",
            ));
            instance = None;
        }
        if let Err(f) = job.publish_receipt() {
            job.fail(f);
            instance = None;
        }
        if instance.is_some() && !self.retain_success {
            // Durable intents precede every exact unlink. Unknown nodes cause
            // refusal; no recursive remove or original-input deletion exists.
            if let Err(f) = self.cleanup_job(&mut job) {
                job.fail(f);
                instance = None;
            }
        }
        // Success boundary includes publication/cleanup. A late finish remains
        // failure even if some originals were already deleted; removed facts stay.
        if let Err(f) = fs_job::check_budget(job.start, &job.cancel) {
            job.fail(f);
            instance = None;
        }
        #[cfg(test)]
        if let Some(failure) = super::terminal::tests::budget("late-final") {
            job.fail(failure);
            instance = None;
        }
        if job.failure_epoch != job.history.began_epoch {
            // Only a newly observed post-snapshot/attempt failure activates disk
            // history. Existing repeated budget observations never do so.
            self.persist_terminal_history(&mut job);
            if let Some(evidence) = &mut job.history.evidence {
                evidence.secondary_cap_reached = job.secondary_cap_reached;
            }
            instance = None;
        }
        job.receipt.elapsed_microseconds = micros(job.start.elapsed());
        job.receipt.failure = job.failure.clone();
        job.receipt.secondary_failures = job.secondary.clone();
        if let Some(instance) = instance {
            let receipt = Box::new(job.receipt.clone());
            if self.retain_success {
                self.retained_successes.push(job.dir);
            }
            return JobProgress::Succeeded { instance, receipt };
        }
        let failure = job
            .failure
            .clone()
            .unwrap_or_else(|| Failure::new(FailureKind::Protocol, "missing instance"));
        job.terminal = Some(ClosedFailure {
            failure: failure.clone(),
            child_outcome: outcome.clone(),
            receipt: job.receipt.clone(),
        });
        let progress = JobProgress::FailedClosed {
            failure: Box::new(ClosedFailure {
                failure,
                child_outcome: outcome,
                receipt: job.receipt.clone(),
            }),
            retained: job.history.retained_path(&job.dir.path),
        };
        self.job = Some(job);
        progress
    }
    fn persist_terminal_history(&self, job: &mut Job) {
        use super::terminal::{self, Delta};
        job.history.activate(
            &self.scratch,
            &job.receipt.request_id,
            job.secondary_cap_reached,
        );
        let capacity = self
            .scratch
            .terminal_capacity(terminal::TOTAL_BYTES)
            .map_err(|e| Failure::io("terminal history capacity", e));
        if let Err(failure) = capacity {
            job.history.evidence.as_mut().unwrap().persistence_failure = Some(failure.clone());
            job.fail(failure);
            if let Err(failure) = fs_job::check_budget(job.start, &job.cancel) {
                job.fail(failure);
            }
            return;
        }
        if let Some(snapshot) = &job.history.snapshot {
            let (leaf, result) = terminal::publish(
                &self.scratch,
                &job.receipt.request_id,
                "pre-cleanup-receipt",
                &snapshot.bytes,
            );
            job.history.evidence.as_mut().unwrap().pre_cleanup = Some(leaf);
            if let Err(failure) = result {
                job.history.evidence.as_mut().unwrap().persistence_failure = Some(failure.clone());
                job.fail(failure);
                if let Err(failure) = fs_job::check_budget(job.start, &job.cancel) {
                    job.fail(failure);
                }
                return;
            }
        }
        loop {
            job.receipt.elapsed_microseconds = micros(job.start.elapsed());
            job.receipt.failure = job.failure.clone();
            job.receipt.secondary_failures = job.secondary.clone();
            let prior_epoch = job.failure_epoch;
            let evidence = job.history.evidence.as_ref().unwrap();
            let sequence = evidence.versions.len();
            if sequence >= 3 {
                break;
            } // Two fixed budget classes give at most3.
            let delta = Delta {
                request_id: &job.receipt.request_id,
                profile: job.receipt.profile,
                sequence,
                prior_record_sha256: evidence.latest_sha256.as_deref(),
                pre_cleanup_original: job.history.snapshot.as_ref().map(|s| &s.original),
                job_directory: job.history.directory(),
                cancelled_observed: job.budget_observations.cancelled,
                timed_out_observed: job.budget_observations.timed_out,
                secondary_cap_reached: job.secondary_cap_reached,
                recorded_job_files_are_historical: true,
                closed_receipt: &job.receipt,
            };
            let encoded = terminal::encode(&delta);
            let (leaf, result) = match encoded {
                Ok(bytes) => terminal::publish(
                    &self.scratch,
                    &job.receipt.request_id,
                    ["terminal-0", "terminal-1", "terminal-2"][sequence],
                    &bytes,
                ),
                Err(failure) => {
                    job.history.evidence.as_mut().unwrap().persistence_failure =
                        Some(failure.clone());
                    job.fail(failure);
                    break;
                }
            };
            let directory = job.history.directory();
            let evidence = job.history.evidence.as_mut().unwrap();
            evidence.job_directory = directory;
            evidence.secondary_cap_reached = job.secondary_cap_reached;
            if let Ok(record) = &result {
                evidence.latest_sha256 = Some(record.sha256.clone());
            }
            evidence.versions.push(leaf);
            if let Err(failure) = result {
                job.history.evidence.as_mut().unwrap().persistence_failure = Some(failure.clone());
                job.fail(failure);
                if let Err(f) = fs_job::check_budget(job.start, &job.cancel) {
                    job.fail(f);
                }
                break; // Unknown persistence is never automatically retried.
            }
            if let Err(f) = fs_job::check_budget(job.start, &job.cancel) {
                job.fail(f);
            }
            #[cfg(test)]
            if let Some(f) = super::terminal::tests::budget("after-terminal") {
                job.fail(f);
            }
            if prior_epoch == job.failure_epoch {
                break;
            }
        }
        if let Some(evidence) = &mut job.history.evidence {
            evidence.secondary_cap_reached = job.secondary_cap_reached;
        }
    }
    fn cleanup_job(&self, job: &mut Job) -> Result<(), Failure> {
        let rows = job
            .dir
            .census()
            .map_err(|e| Failure::io("cleanup full census", e))?;
        for (i, row) in rows.iter().enumerate() {
            let name = format!("cleanup-{i}.json");
            let bytes = serde_json::to_vec(row)
                .map_err(|e| Failure::io("cleanup intent encode", std::io::Error::other(e)))?;
            if bytes.len() > 4096 {
                return Err(Failure::new(FailureKind::Cleanup, "cleanup intent cap"));
            }
            job.dir
                .write_frozen(&name, &bytes)
                .map_err(|e| Failure::io("durable cleanup intent", e))?;
            job.dir
                .remove_exact(row)
                .map_err(|e| Failure::io("exact completed artifact unlink", e))?;
            job.receipt.cleanup_removed.push(row.name.clone());
        }
        let intents = job
            .dir
            .census()
            .map_err(|e| Failure::io("cleanup afterfacts", e))?;
        for row in &intents {
            if !row.name.starts_with("cleanup-") {
                return Err(Failure::new(
                    FailureKind::Cleanup,
                    "unexpected cleanup residue",
                ));
            }
            job.dir
                .remove_exact(row)
                .map_err(|e| Failure::io("completed intent unlink", e))?;
            job.receipt.cleanup_removed.push(row.name.clone());
        }
        job.history.directory = Some(JobDirectoryState::Unknown);
        let result = self
            .scratch
            .remove_job(&job.dir)
            .map_err(|e| Failure::io("empty owned directory unlink", e));
        if result.is_ok() {
            job.history.directory = Some(JobDirectoryState::Removed);
            #[cfg(test)]
            job.observe366("after-job-remove");
        }
        result
    }
    fn cleanup_terminal_failure(&self, job: &mut Job) -> Result<CleanupReceipt, Failure> {
        use super::terminal::{self, CleanupIntent};
        let state = job.history.directory();
        if state == JobDirectoryState::Unknown {
            return Err(Failure::new(
                FailureKind::Cleanup,
                "unknown job directory cleanup refused",
            ));
        }
        let evidence = job.history.evidence.as_ref().unwrap();
        let parent_rows = terminal::verify_known(&self.scratch, evidence)?;
        let job_rows = if state == JobDirectoryState::Present {
            job.dir
                .census()
                .map_err(|e| Failure::io("terminal cleanup job census", e))?
        } else {
            Vec::new()
        }; // Removed jobs are never reopened or recreated.
        let intent = CleanupIntent {
            request_id: &job.receipt.request_id,
            operation: "exact closed terminal evidence cleanup",
            job_directory: state,
            job_records: &job_rows,
            parent_records: &parent_rows,
        };
        let bytes = terminal::encode(&intent)?;
        let (leaf, result) = terminal::publish(
            &self.scratch,
            &job.receipt.request_id,
            "terminal-cleanup-intent",
            &bytes,
        );
        job.history.evidence.as_mut().unwrap().cleanup_intent = Some(leaf);
        if let Err(failure) = result {
            job.history.evidence.as_mut().unwrap().persistence_failure = Some(failure.clone());
            return Err(failure); // No deletion on unknown intent durability.
        }
        if state == JobDirectoryState::Present {
            self.cleanup_job(job)?;
        }
        for row in &parent_rows {
            self.scratch
                .parent
                .remove_exact(row)
                .map_err(|e| Failure::io("exact terminal evidence unlink", e))?;
            job.history
                .evidence
                .as_mut()
                .unwrap()
                .cleanup_removed
                .push(row.name.clone());
        }
        let evidence = job.history.evidence.as_mut().unwrap();
        evidence.job_directory = JobDirectoryState::Removed;
        // This remaining immutable intent is authorization, not rewritten
        // deletion afterfacts. Its lifetime belongs to the explicit caller.
        let mut removed = job.receipt.cleanup_removed.clone();
        removed.extend(evidence.cleanup_removed.iter().cloned());
        Ok(CleanupReceipt {
            removed,
            directory_removed: true,
        })
    }
    pub fn cleanup_closed_failure(&mut self) -> Result<CleanupReceipt, CleanupFailure> {
        let Some(mut job) = self.job.take() else {
            return Ok(CleanupReceipt {
                removed: Vec::new(),
                directory_removed: false,
            });
        };
        if job.unresolved() || job.terminal.is_none() {
            self.job = Some(job);
            return Err(Failure::new(
                FailureKind::Ownership,
                "failed cleanup before closed receipt",
            ));
        }
        if job.history.evidence.is_some() {
            let result = self.cleanup_terminal_failure(&mut job);
            return match result {
                Ok(receipt) => {
                    self.last_terminal_evidence = job.history.evidence.take();
                    Ok(receipt)
                }
                Err(failure) => {
                    let state = job.history.directory();
                    if let Some(evidence) = &mut job.history.evidence {
                        evidence.job_directory = state;
                        evidence.cleanup_failure = Some(failure.clone());
                    }
                    self.job = Some(job);
                    Err(failure)
                }
            };
        }
        match self.cleanup_job(&mut job) {
            Ok(()) => Ok(CleanupReceipt {
                removed: job.receipt.cleanup_removed,
                directory_removed: true,
            }),
            Err(f) => {
                self.job = Some(job);
                Err(f)
            }
        }
    }
}

// Caller requires no prior failure, no Ready/Start, empty stdout EOF and own
// reap. This predicate adds complete bounded result identity and exact exit2.
fn resource_setup_before_ready(
    raw: &[u8],
    id: &crate::RequestIdentity,
    status: Option<i32>,
) -> bool {
    let Some(status) = status else {
        return false;
    };
    libc::WIFEXITED(status)
        && libc::WEXITSTATUS(status) == 2
        && crate::decode_result(raw, id).is_ok_and(|result| {
            result.required_exit_code() == 2
                && matches!(
                    result.outcome(),
                    crate::Outcome::HostError {
                        error: crate::HostError::ResourceSetup,
                        ..
                    }
                )
        })
}

fn unresolved(
    child: bool,
    own_reap: bool,
    wait_lost: bool,
    unknown: bool,
    stdout_open: bool,
    stderr_open: bool,
) -> bool {
    child && !own_reap || wait_lost || unknown || stdout_open || stderr_open
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn b06_pre_ready_resource_setup_requires_complete_result_and_own_exit2() {
        let id = crate::RequestIdentity::new(
            crate::RequestId::parse_hex("00000000000000000000000000000000").unwrap(),
            crate::Profile::LinuxX86_64V1,
        );
        let other_id = crate::RequestIdentity::new(
            crate::RequestId::parse_hex("11111111111111111111111111111111").unwrap(),
            crate::Profile::LinuxX86_64V1,
        );
        let outcome = crate::Outcome::HostError {
            error: crate::HostError::ResourceSetup,
            diagnostics: crate::Diagnostics::empty(),
        };
        let envelope = crate::ResultEnvelope::new(id, outcome).unwrap();
        let mut raw = Vec::new();
        crate::encode_result(&envelope, &mut raw).unwrap();
        // Frozen association vectors; encoded input is controlled fixture data,
        // not an oracle generated from the host classifier. No OS is invoked.
        for (key, status, expected) in [
            ("own-exit2", Some(512), true),
            ("no-own-status", None, false),
            ("exit0", Some(0), false),
            ("exit1", Some(256), false),
            ("signaled", Some(15), false),
        ] {
            let actual = resource_setup_before_ready(&raw, &id, status);
            eprintln!("B06 association original {key} {actual}");
            assert_eq!(actual, expected);
        }
        assert!(!resource_setup_before_ready(&raw, &other_id, Some(512)));
        let wrong_profile = String::from_utf8(raw.clone())
            .unwrap()
            .replace("linux-x86_64-v1", "unknown-profile");
        assert!(!resource_setup_before_ready(
            wrong_profile.as_bytes(),
            &id,
            Some(512)
        ));
        assert!(!resource_setup_before_ready(b"{invalid", &id, Some(512)));
        assert!(!resource_setup_before_ready(
            &raw[..raw.len() - 1],
            &id,
            Some(512)
        ));
        let mut trailing = raw.clone();
        trailing.extend_from_slice(b"{} ");
        assert!(!resource_setup_before_ready(&trailing, &id, Some(512)));
        let mut over = vec![b' '; crate::MAX_RESULT_BYTES + 1];
        over[..raw.len()].copy_from_slice(&raw);
        assert!(!resource_setup_before_ready(&over, &id, Some(512)));
        let other = crate::ResultEnvelope::new(
            id,
            crate::Outcome::HostError {
                error: crate::HostError::SnapshotMismatch,
                diagnostics: crate::Diagnostics::empty(),
            },
        )
        .unwrap();
        let mut other_raw = Vec::new();
        crate::encode_result(&other, &mut other_raw).unwrap();
        assert!(!resource_setup_before_ready(&other_raw, &id, Some(512)));
    }
    #[test]
    fn d15_d17_d18_nonterminal_ownership_fields() {
        // Frozen state predicates only, no process/pipe/kernel qualification.
        let rows = [
            ("still-held", true, false, false, false, false, false, true),
            ("lost-wait", true, false, true, false, false, false, true),
            (
                "unknown-child",
                false,
                true,
                false,
                true,
                false,
                false,
                true,
            ),
            (
                "post-reap-pipe",
                true,
                true,
                false,
                false,
                true,
                false,
                true,
            ),
            (
                "own-reap-drained",
                true,
                true,
                false,
                false,
                false,
                false,
                false,
            ),
            (
                "no-child-stage-error",
                false,
                false,
                false,
                false,
                false,
                false,
                false,
            ),
        ];
        for (key, child, reap, lost, unknown, out, err, expected) in rows {
            let actual = unresolved(child, reap, lost, unknown, out, err);
            eprintln!("ownership predicate original {key} {actual}");
            assert_eq!(actual, expected);
        }
        // A five-second stop allowance changes the reported category, never
        // the ownership predicate. Later own-reap/drain is separately required.
        assert!(unresolved(true, false, false, false, false, false));
        assert!(!unresolved(true, true, false, false, false, false));
    }
    struct BrokenRead;
    impl Read for BrokenRead {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::from_raw_os_error(libc::EIO))
        }
    }
    #[test]
    fn capture_exact_prefix_and_read_failure_do_not_invent_eof() {
        // Capture has no filesystem handle in this narrow injected test.
        let mut exact = Capture {
            pipe: Some(std::io::Cursor::new(vec![b'X'; 65536])),
            file: None,
            prefix: Vec::new(),
            observed: 0,
            cap: 65536,
            eof: false,
            overflow: false,
            closed_before_eof: false,
        };
        while !exact.eof {
            exact.step().unwrap();
        }
        eprintln!(
            "capture exact original len={} observed={} closed_before_eof={}",
            exact.prefix.len(),
            exact.observed,
            exact.closed_before_eof
        );
        assert_eq!(exact.prefix, vec![b'X'; 65536]);
        assert_eq!(exact.observed, 65536);
        assert!(!exact.overflow && !exact.closed_before_eof);
        let mut over = Capture {
            pipe: Some(std::io::Cursor::new(vec![b'X'; 65537])),
            file: None,
            prefix: Vec::new(),
            observed: 0,
            cap: 65536,
            eof: false,
            overflow: false,
            closed_before_eof: false,
        };
        let mut failure = None;
        while !over.eof {
            if let Err(e) = over.step() {
                failure = Some(e);
            }
        }
        eprintln!(
            "capture over original {failure:#?} observed={}",
            over.observed
        );
        assert_eq!(over.prefix, vec![b'X'; 65536]);
        assert_eq!(over.observed, 65537);
        assert_eq!(failure.unwrap().kind, FailureKind::Protocol);
        let mut broken = Capture {
            pipe: Some(BrokenRead),
            file: None,
            prefix: Vec::new(),
            observed: 0,
            cap: 65536,
            eof: false,
            overflow: false,
            closed_before_eof: false,
        };
        let failure = broken.step();
        eprintln!("capture failure original {failure:#?}");
        assert!(failure.is_err());
        assert!(broken.closed_before_eof && broken.eof);
        assert!(broken.pipe.is_none());
    }
}

#[cfg(test)]
#[path = "escalation358_tests.rs"]
mod escalation358_tests;

#[cfg(all(test, feature = "host-tests"))]
#[path = "ownership_fault351_tests.rs"]
mod ownership_fault351_tests;

#[cfg(test)]
#[path = "budget363_tests.rs"]
mod budget363_tests;

#[cfg(all(test, feature = "host-tests"))]
#[path = "terminal_owner366_tests.rs"]
mod terminal_owner366_tests;
