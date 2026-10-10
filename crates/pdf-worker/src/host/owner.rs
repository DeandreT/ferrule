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
        if self.failure.is_none() {
            self.event("stop", f.errno);
            self.failure = Some(f);
            self.stop = Some(Instant::now());
            self.stdin.take();
        } else if self.secondary.len() < 8 {
            self.secondary.push(f);
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
    fn publish_receipt(&mut self) -> Result<(), Failure> {
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
                .map_err(|e| Failure::io("receipt publication", e))
        })();
        if result.is_err() {
            self.receipt.receipt_published_before_cleanup = false;
        }
        result
    }
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
                failure: f,
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
                failure: ClosedFailure {
                    failure: terminal.failure.clone(),
                    child_outcome: terminal.child_outcome.clone(),
                    receipt: terminal.receipt.clone(),
                },
                retained: job.dir.path.clone(),
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
            match super::linux::own_wait(pid) {
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
            if grace >= Duration::from_secs(1) && !job.kill_attempted {
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
        job.receipt.elapsed_microseconds = micros(job.start.elapsed());
        job.receipt.failure = job.failure.clone();
        job.receipt.secondary_failures = job.secondary.clone();
        if let Some(instance) = instance {
            let receipt = job.receipt.clone();
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
            failure: ClosedFailure {
                failure,
                child_outcome: outcome,
                receipt: job.receipt.clone(),
            },
            retained: job.dir.path.clone(),
        };
        self.job = Some(job);
        progress
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
        self.scratch
            .remove_job(&job.dir)
            .map_err(|e| Failure::io("empty owned directory unlink", e))
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
