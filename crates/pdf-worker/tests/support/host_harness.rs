//! Shared helpers for plain owned-host integration tests: fresh private roots,
//! packaged fixture programs, and closure checks against real children.
#![allow(dead_code)]
use mapping::{PdfCapture, PdfCommand, PdfLayout, PdfPageSelection, PdfRegion};
use pdf_worker::host::*;
use std::collections::BTreeMap;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub const HELLO_PDF: &[u8] = include_bytes!("../fixtures/hello-page.pdf");
pub const EMPTY_PDF: &[u8] = include_bytes!("../fixtures/empty-page.pdf");
/// Address space, CPU soft/hard, file size and core limits of the fixed profile.
pub const LIMITS: [u64; 5] = [536870912, 30, 31, 67108864, 0];
const CLOSURE_BOUND: Duration = Duration::from_secs(60);

static SERIAL: Mutex<()> = Mutex::new(());
static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

/// Descriptor and thread-child censuses are process-wide, so owned cases in
/// one test binary run one at a time.
pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// A fresh mode-0700 directory under the temporary directory. It is removed
/// after a passing test and kept for inspection after a failure.
pub struct Root(PathBuf);
impl Root {
    pub fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "ferrule-pdf-host-{}-{}-{name}",
            std::process::id(),
            NEXT_ROOT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self(path)
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
    pub fn dir(&self, name: &str) -> PathBuf {
        private_dir(&self.0.join(name))
    }
    pub fn file(&self, name: impl AsRef<Path>, bytes: &[u8]) -> PathBuf {
        let path = self.0.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

pub fn private_dir(path: &Path) -> PathBuf {
    std::fs::create_dir(path).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path.to_path_buf()
}

pub fn layout() -> PdfLayout {
    PdfLayout::new(
        "Document",
        PdfPageSelection::First,
        vec![PdfCommand::Capture(PdfCapture {
            name: "Value".into(),
            region: PdfRegion::full(),
            algorithm: Default::default(),
        })],
    )
    .unwrap()
}

pub fn value_instance(value: ir::Value) -> ir::Instance {
    ir::Instance::Group(vec![("Value".into(), ir::Instance::Scalar(value))].into())
}

pub fn hello_instance() -> ir::Instance {
    value_instance(ir::Value::String("Hello".into()))
}

/// Copies the feature-gated fixture under `name`, which selects its mode.
pub fn install_program(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::copy(env!("CARGO_BIN_EXE_pdf-worker-fixture"), &path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o500)).unwrap();
    path
}

pub fn program(dir: &Path, name: &str) -> WorkerProgram {
    WorkerProgram::admit_test_fixture(&install_program(dir, name)).unwrap()
}

pub struct Host {
    pub manager: PdfWorkerManager,
    pub scratch: PathBuf,
}

/// One manager per case with retained successful originals, so a test can
/// inspect the closed job directory of either outcome.
pub fn host(root: &Root, program_name: &str) -> Host {
    let package = root.dir("package");
    let scratch = root.dir("scratch");
    let mut manager = PdfWorkerManager::new(
        program(&package, program_name),
        ScratchPolicy::admit(&scratch, 0).unwrap(),
        ExclusiveReaperAdmission::for_current_host(),
    )
    .unwrap();
    manager.retain_successful_originals(true).unwrap();
    Host { manager, scratch }
}

pub fn thread_children() -> Vec<u32> {
    std::fs::read_to_string("/proc/thread-self/children")
        .unwrap()
        .split_whitespace()
        .map(|pid| pid.parse().unwrap())
        .collect()
}

pub fn pidfd_open(pid: u32) -> std::io::Result<OwnedFd> {
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, pid as libc::pid_t, 0u32) };
    if raw < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(unsafe { OwnedFd::from_raw_fd(raw as i32) })
    }
}

/// The direct child launched by this test thread, held by an independent
/// PIDfd. Only the manager polls can reap it; until then it remains visible.
pub struct Launch {
    pub pid: Option<u32>,
    pidfd: Option<OwnedFd>,
}
impl Launch {
    pub fn observe() -> Self {
        let children = thread_children();
        assert!(children.len() <= 1, "one owned child at most: {children:?}");
        let pid = children.first().copied();
        Self {
            pid,
            pidfd: pid.map(|pid| pidfd_open(pid).unwrap()),
        }
    }
}

pub fn begin(
    host: &mut Host,
    input: &Path,
    cancel: &CancellationFlag,
) -> (Result<pdf_worker::RequestIdentity, BeginFailure>, Launch) {
    let begun = host.manager.begin(input, &layout(), cancel);
    (begun, Launch::observe())
}

pub fn finish(manager: &mut PdfWorkerManager) -> JobProgress {
    let started = Instant::now();
    loop {
        match manager.poll() {
            JobProgress::Pending { .. } | JobProgress::ClosureIncomplete { .. } => {
                assert!(
                    started.elapsed() < CLOSURE_BOUND,
                    "owned job did not close within the test bound"
                );
                std::thread::sleep(Duration::from_millis(5));
            }
            terminal => return terminal,
        }
    }
}

pub fn closed_failure(progress: JobProgress) -> (Box<ClosedFailure>, PathBuf) {
    match progress {
        JobProgress::FailedClosed { failure, retained } => (failure, retained),
        other => panic!("expected a closed failure, got {other:#?}"),
    }
}

pub fn succeeded(progress: JobProgress) -> (ir::Instance, Box<ClosedReceipt>) {
    match progress {
        JobProgress::Succeeded { instance, receipt } => (instance, receipt),
        other => panic!("expected success, got {other:#?}"),
    }
}

/// The manager alone reaped exactly the launched child; no other child of
/// this thread or descendant observation remains.
pub fn assert_closed(launch: Launch, receipt: &ClosedReceipt) {
    assert_eq!(receipt.child_pid, launch.pid);
    if let Some(pidfd) = &launch.pidfd {
        let mut info = unsafe { std::mem::zeroed::<libc::siginfo_t>() };
        let rc = unsafe {
            libc::waitid(
                libc::P_PIDFD,
                pidfd.as_raw_fd() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG,
            )
        };
        let error = std::io::Error::last_os_error();
        assert_eq!(rc, -1, "launched child must already be reaped");
        assert_eq!(error.raw_os_error(), Some(libc::ECHILD));
        assert!(receipt.own_reap && receipt.pidfd_acquired);
        assert!(receipt.raw_wait_status.is_some() && receipt.reap_usage.is_some());
        assert!(receipt.child_birth.is_some());
        assert_eq!(receipt.child_ppid, Some(std::process::id()));
    } else {
        assert!(!receipt.own_reap && !receipt.pidfd_acquired);
        assert!(receipt.raw_wait_status.is_none());
    }
    assert!(receipt.observed_unexpected_children.is_empty());
    assert!(!receipt.stop_allowance_exceeded);
    assert!(!receipt.stdout_closed_before_eof && !receipt.stderr_closed_before_eof);
    assert!(thread_children().is_empty());
}

/// Every open descriptor of this process and its target. Take one census
/// before constructing a manager and compare after dropping it.
pub fn descriptors() -> BTreeMap<i32, PathBuf> {
    let listing = std::fs::read_dir("/proc/self/fd").unwrap();
    let mut open = BTreeMap::new();
    for entry in listing {
        let entry = entry.unwrap();
        let fd = entry.file_name().to_str().unwrap().parse::<i32>().unwrap();
        if let Ok(target) = std::fs::read_link(entry.path()) {
            open.insert(fd, target);
        }
    }
    // The listing's own directory descriptor is closed again on return.
    let own = PathBuf::from(format!("/proc/{}/fd", std::process::id()));
    open.retain(|_, target| *target != own);
    open
}

/// Names directly inside a directory, sorted.
pub fn names(dir: &Path) -> Vec<String> {
    let mut names = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    names
}

pub fn status_signal(status: i32) -> Option<i32> {
    libc::WIFSIGNALED(status).then(|| libc::WTERMSIG(status))
}

pub fn status_exit(status: i32) -> Option<i32> {
    libc::WIFEXITED(status).then(|| libc::WEXITSTATUS(status))
}

/// Raw wait status classes asserted for a closed child.
#[derive(Clone, Copy, Debug)]
pub enum Status {
    Exit(i32),
    Signal(i32),
    /// The owner stopped the child; it either took the TERM or saw its
    /// closed Start pipe first and exited with this code.
    StoppedOrExit(i32),
}
impl Status {
    pub fn check(self, raw: Option<i32>) {
        let raw = raw.expect("own wait status");
        match self {
            Self::Exit(code) => assert_eq!(status_exit(raw), Some(code), "status {raw}"),
            Self::Signal(signal) => assert_eq!(status_signal(raw), Some(signal), "status {raw}"),
            Self::StoppedOrExit(code) => assert!(
                status_signal(raw) == Some(libc::SIGTERM) || status_exit(raw) == Some(code),
                "status {raw}"
            ),
        }
    }
}

/// The child's own complete result, as decoded by the owner.
#[derive(Clone, Debug, PartialEq)]
pub enum Child {
    Absent,
    Ok,
    Host(pdf_worker::HostError),
    /// The owner's stop races the child's own complete ok result.
    AbsentOrOk,
}
impl Child {
    pub fn of(outcome: &Option<pdf_worker::Outcome>) -> Self {
        match outcome {
            None => Self::Absent,
            Some(pdf_worker::Outcome::Ok(_)) => Self::Ok,
            Some(pdf_worker::Outcome::HostError { error, .. }) => Self::Host(error.clone()),
            Some(other) => panic!("unexpected child outcome {other:#?}"),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Refusal {
    pub kind: FailureKind,
    pub operation: &'static str,
    /// Exact bounded diagnostic text, when the control fixes one.
    pub diagnostic: Option<&'static str>,
    pub ready: bool,
    pub start_sent: bool,
    pub status: Status,
    pub child: Child,
}
impl Refusal {
    pub const fn new(kind: FailureKind, operation: &'static str, status: Status) -> Self {
        Self {
            kind,
            operation,
            diagnostic: None,
            ready: true,
            start_sent: true,
            status,
            child: Child::Absent,
        }
    }
    pub const fn before_start(mut self) -> Self {
        self.ready = false;
        self.start_sent = false;
        self
    }
    pub const fn diagnostic(mut self, text: &'static str) -> Self {
        self.diagnostic = Some(text);
        self
    }
    pub const fn child(mut self, child: Child) -> Self {
        self.child = child;
        self
    }
}

/// One owned job of `program` over `input`, run to its terminal progress.
pub struct Run {
    // Field order: the serial guard is released after the manager drops.
    pub root: Root,
    pub host: Host,
    pub launch: Launch,
    pub progress: JobProgress,
    descriptors: BTreeMap<i32, PathBuf>,
    _serial: MutexGuard<'static, ()>,
}
/// Takes the binary-wide serial guard; callers must not already hold it.
pub fn run(program: &str, input_name: impl AsRef<Path>, input: &[u8]) -> Run {
    let serial = serial();
    let root = Root::new(program);
    let input = root.file(input_name, input);
    let descriptors = descriptors();
    let mut host = host(&root, program);
    let (begun, launch) = begin(&mut host, &input, &CancellationFlag::new());
    begun.unwrap_or_else(|failure| panic!("{program}: {failure:#?}"));
    let progress = finish(&mut host.manager);
    Run {
        root,
        host,
        launch,
        progress,
        descriptors,
        _serial: serial,
    }
}
impl Run {
    /// Asserts owned closure and releases the manager, then checks that no
    /// descriptor it held remains open.
    pub fn close(self, receipt: &ClosedReceipt) {
        assert_closed(self.launch, receipt);
        drop(self.host);
        assert_eq!(descriptors(), self.descriptors);
    }
}

/// Runs a control that must refuse: no instance is returned, the failed job
/// stays retained, and the child is reaped by the owner. `inspect` sees the
/// retained job directory before closure is asserted.
pub fn refused(
    program: &str,
    input: &[u8],
    expected: Refusal,
    inspect: impl FnOnce(&ClosedFailure, &Path),
) {
    let run = run(program, "input.data", input);
    let (failure, job) = match &run.progress {
        JobProgress::FailedClosed { failure, retained } => (failure, retained.clone()),
        other => panic!("{program}: expected refusal, got {other:#?}"),
    };
    let actual = &failure.failure;
    eprintln!("{program}: {actual:#?} child={:?}", failure.child_outcome);
    assert_eq!(actual.kind, expected.kind, "{program}");
    assert_eq!(actual.operation, expected.operation, "{program}");
    if let Some(text) = expected.diagnostic {
        assert_eq!(actual.diagnostics.display().text(), text, "{program}");
    }
    let receipt = &failure.receipt;
    assert_eq!(receipt.ready, expected.ready, "{program} ready");
    assert_eq!(receipt.start_sent, expected.start_sent, "{program} start");
    expected.status.check(receipt.raw_wait_status);
    let child = Child::of(&failure.child_outcome);
    if expected.child == Child::AbsentOrOk {
        assert!(matches!(child, Child::Absent | Child::Ok), "{program}");
    } else {
        assert_eq!(child, expected.child, "{program}");
    }
    assert!(job.is_dir(), "{program} retains its failed job");
    assert_eq!(run.host.manager.evidence_paths(), vec![job.clone()]);
    inspect(failure, &job);
    let receipt = receipt.clone();
    run.close(&receipt);
}

/// Runs a control that must succeed with exactly `expected`.
pub fn accepted(
    program: &str,
    input: &[u8],
    expected: &ir::Instance,
    inspect: impl FnOnce(&ClosedReceipt, &Path),
) {
    let run = run(program, "input.data", input);
    let (instance, receipt) = match &run.progress {
        JobProgress::Succeeded { instance, receipt } => (instance, receipt.clone()),
        other => panic!("{program}: expected success, got {other:#?}"),
    };
    assert_eq!(instance, expected, "{program}");
    assert!(receipt.ready && receipt.start_sent);
    assert_eq!(receipt.effective_limits, Some(LIMITS));
    assert_eq!(receipt.raw_wait_status, Some(0));
    assert!(receipt.failure.is_none() && receipt.secondary_failures.is_empty());
    let job = run.host.manager.evidence_paths()[0].clone();
    inspect(&receipt, &job);
    run.close(&receipt);
}

pub fn file_len(path: &Path) -> u64 {
    std::fs::symlink_metadata(path).unwrap().len()
}
