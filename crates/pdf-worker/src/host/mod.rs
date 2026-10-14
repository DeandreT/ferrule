//! Opt-in filesystem host. A returned `ClosureIncomplete` is not a closed error:
//! the manager must remain on its launch thread and keep servicing its child.
use crate::{Diagnostics, Outcome};
use serde::Serialize;
#[cfg(not(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
use std::path::Path;
use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub const WORK_SECONDS: u64 = 30;
pub const STOP_SECONDS: u64 = 5;
pub const SCRATCH_HEADROOM_BYTES: u64 = 96 * 1024 * 1024;
pub const JOB_BYTES: u64 = 80 * 1024 * 1024;
pub const RECEIPT_BYTES: usize = 65536;

#[derive(Clone, Debug, Default)]
pub struct CancellationFlag(Arc<AtomicBool>);
impl CancellationFlag {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum FailureKind {
    UnsupportedHost,
    RepairDependency,
    Cancelled,
    TimedOut,
    SnapshotMismatch,
    Protocol,
    ResourceSetup,
    WorkerExit,
    Pdf,
    Receipt,
    Cleanup,
    Io,
    Ownership,
    InputTooLarge,
}
#[derive(Clone, Debug, Serialize, thiserror::Error)]
#[error("PDF worker {kind:?}: {operation}")]
pub struct Failure {
    pub kind: FailureKind,
    pub operation: &'static str,
    pub diagnostics: Diagnostics,
    pub errno: Option<i32>,
}
impl Failure {
    pub(crate) fn new(kind: FailureKind, operation: &'static str) -> Self {
        Self {
            kind,
            operation,
            diagnostics: Diagnostics::empty(),
            errno: None,
        }
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    fn input_too_large() -> Self {
        Self {
            kind: FailureKind::InputTooLarge,
            operation: "input byte cap",
            diagnostics: crate::error::capture(&format_pdf::PdfError::InputTooLarge),
            errno: None,
        }
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    pub(crate) fn io(operation: &'static str, e: std::io::Error) -> Self {
        Self {
            kind: FailureKind::Io,
            operation,
            diagnostics: crate::error::capture(&e),
            errno: e.raw_os_error(),
        }
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    pub(crate) fn classified_io(
        kind: FailureKind,
        operation: &'static str,
        e: std::io::Error,
    ) -> Self {
        let mut failure = Self::io(operation, e);
        failure.kind = kind;
        failure
    }
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    pub(crate) fn codec(operation: &'static str, e: crate::CodecError) -> Self {
        Self {
            kind: FailureKind::Protocol,
            operation,
            diagnostics: crate::error::capture(&e),
            errno: None,
        }
    }
}
pub type PrelaunchFailure = Failure;
pub type JobIdentity = crate::RequestIdentity;
pub type CleanupFailure = Failure;
#[derive(Clone, Debug, Serialize)]
pub struct CleanupReceipt {
    pub removed: Vec<String>,
    pub directory_removed: bool,
}
#[derive(Debug)]
pub enum BeginFailure {
    NoChild(Failure),
    OwnedStopping {
        failure: Box<Failure>,
        evidence: PathBuf,
    },
}
#[derive(Clone, Debug, Serialize)]
pub struct OwnedReapUsage {
    pub user_seconds: i64,
    pub user_microseconds: i64,
    pub system_seconds: i64,
    pub system_microseconds: i64,
    /// Linux own-reap accounting in KiB; not a sparse /proc maximum or RSS cap.
    pub maxrss_kib: i64,
    /// ixrss,idrss,isrss,minflt,majflt,nswap,inblock,oublock,msgsnd,msgrcv,nsignals,nvcsw,nivcsw.
    /// Unmaintained fields remain uninterpreted raw values.
    pub raw_counters: [i64; 13],
}
#[derive(Clone, Debug, Serialize)]
pub struct FileRecord {
    pub name: String,
    pub byte_len: u64,
    pub sha256: String,
    pub identity: [u64; 9],
}
#[derive(Clone, Debug, Serialize)]
pub struct Event {
    pub at_microseconds: u64,
    pub operation: &'static str,
    pub errno: Option<i32>,
}
#[derive(Clone, Debug, Serialize)]
pub struct ClosedReceipt {
    pub request_id: String,
    pub profile: &'static str,
    pub program: FileRecord,
    pub source_before: Option<[u64; 9]>,
    pub source_after: Option<[u64; 9]>,
    pub observed_unexpected_children: Vec<u32>,
    pub child_pid: Option<u32>,
    pub child_birth: Option<u64>,
    pub child_ppid: Option<u32>,
    pub pidfd_acquired: bool,
    pub ready: bool,
    pub start_sent: bool,
    pub effective_limits: Option<[u64; 5]>,
    pub work_seconds: u64,
    pub stop_seconds: u64,
    pub raw_wait_status: Option<i32>,
    pub reap_usage: Option<OwnedReapUsage>,
    pub own_reap: bool,
    pub stderr_observed_bytes: u64,
    pub stdout_observed_bytes: u64,
    pub stderr_overflow: bool,
    pub stdout_closed_before_eof: bool,
    pub stderr_closed_before_eof: bool,
    pub polls: u64,
    pub first_poll_microseconds: Option<u64>,
    pub last_poll_microseconds: Option<u64>,
    pub events: Vec<Event>,
    pub files: Vec<FileRecord>,
    pub cleanup_removed: Vec<String>,
    pub failure: Option<Failure>,
    pub secondary_failures: Vec<Failure>,
    pub stop_allowance_exceeded: bool,
    /// receipt.json is an immutable pre-cleanup snapshot. This returned value
    /// also records later cleanup and final-boundary facts; it is not asserted
    /// byte-identical to that earlier file.
    pub receipt_published_before_cleanup: bool,
    pub elapsed_microseconds: u64,
    pub successful_originals_retained: bool,
}
/// Parent-owned failure records are separate from `evidence_paths()` job paths.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum JobDirectoryState {
    Present,
    Removed,
    Unknown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum TerminalPersistence {
    Incomplete,
    Durable,
}
#[derive(Clone, Debug, Serialize)]
pub struct TerminalLeaf {
    pub part_name: String,
    pub final_name: String,
    /// A full stable read of an owned reached leaf; incomplete is not durable.
    pub record: Option<FileRecord>,
    pub persistence: TerminalPersistence,
    pub last_completed_phase: &'static str,
}
#[derive(Clone, Debug, Serialize)]
pub struct TerminalEvidence {
    pub parent: PathBuf,
    pub request_id: String,
    pub job_directory: JobDirectoryState,
    pub pre_cleanup: Option<TerminalLeaf>,
    pub versions: Vec<TerminalLeaf>,
    pub cleanup_intent: Option<TerminalLeaf>,
    pub latest_sha256: Option<String>,
    pub persistence_failure: Option<Failure>,
    pub cleanup_failure: Option<Failure>,
    pub secondary_cap_reached: bool,
    /// Returned afterfacts, not a rewrite of the immutable cleanup intent.
    pub cleanup_removed: Vec<String>,
}
#[derive(Debug, thiserror::Error)]
#[error("{failure}")]
pub struct ClosedFailure {
    #[source]
    pub failure: Failure,
    pub child_outcome: Option<Outcome>,
    pub receipt: ClosedReceipt,
}
#[derive(Clone, Debug)]
pub struct ClosureSnapshot {
    pub evidence: PathBuf,
    pub child_pid: Option<u32>,
    pub primary: Failure,
    pub own_reap: bool,
    pub stop_allowance_exceeded: bool,
}
#[derive(Clone, Copy, Debug)]
pub enum JobState {
    Starting,
    Running,
    Stopping,
}
#[derive(Clone, Debug)]
pub struct PendingSnapshot {
    pub child_pid: Option<u32>,
    pub own_reap: bool,
    pub elapsed_microseconds: u64,
}
#[derive(Debug)]
pub enum JobProgress {
    Pending {
        state: JobState,
        snapshot: PendingSnapshot,
    },
    Succeeded {
        instance: ir::Instance,
        receipt: Box<ClosedReceipt>,
    },
    FailedClosed {
        failure: Box<ClosedFailure>,
        retained: PathBuf,
    },
    ClosureIncomplete {
        snapshot: ClosureSnapshot,
    },
}
/// The embedding host promises it will not independently wait for this child.
/// This token is an explicit precondition, not a kernel-verified global fact.
#[derive(Clone, Copy, Debug)]
pub struct ExclusiveReaperAdmission;
impl ExclusiveReaperAdmission {
    pub fn for_current_host() -> Self {
        Self
    }
}

#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod fs_job;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod linux;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod owner;
#[cfg(all(test, target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod ownership_fault351;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod terminal;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod worker;
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
pub use fs_job::{ScratchPolicy, WorkerProgram};
#[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
pub use owner::PdfWorkerManager;
pub fn worker_entry() -> u8 {
    #[cfg(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
    {
        worker::entry()
    }
    #[cfg(not(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
    {
        2
    }
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
mod unsupported {
    use super::*;
    pub struct WorkerProgram;
    pub struct ScratchPolicy;
    pub struct PdfWorkerManager;
    fn refused() -> Failure {
        Failure::new(
            FailureKind::UnsupportedHost,
            "Linux x86_64 GNU host required",
        )
    }
    impl WorkerProgram {
        pub fn admit_packaged_sibling(_: &Path) -> Result<Self, Failure> {
            Err(refused())
        }
    }
    impl ScratchPolicy {
        pub fn admit(_: &Path, _: u64) -> Result<Self, Failure> {
            Err(refused())
        }
    }
    impl PdfWorkerManager {
        pub fn new(
            _: WorkerProgram,
            _: ScratchPolicy,
            _: ExclusiveReaperAdmission,
        ) -> Result<Self, Failure> {
            Err(refused())
        }
        pub fn begin(
            &mut self,
            _: &Path,
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
            if cancel.is_cancelled() {
                return Err(BeginFailure::NoChild(Failure::new(
                    FailureKind::Cancelled,
                    "cancellation observed",
                )));
            }
            Err(BeginFailure::NoChild(refused()))
        }
        pub fn poll(&mut self) -> JobProgress {
            JobProgress::ClosureIncomplete {
                snapshot: ClosureSnapshot {
                    evidence: PathBuf::new(),
                    child_pid: None,
                    primary: refused(),
                    own_reap: false,
                    stop_allowance_exceeded: false,
                },
            }
        }
        pub fn request_cancel(&mut self) {}
        pub fn evidence_paths(&self) -> Vec<PathBuf> {
            Vec::new()
        }
        pub fn terminal_evidence(&self) -> Option<&TerminalEvidence> {
            None
        }
        pub fn retain_successful_originals(&mut self, _: bool) -> Result<(), Failure> {
            Err(refused())
        }
        pub fn cleanup_closed_failure(&mut self) -> Result<CleanupReceipt, CleanupFailure> {
            Err(refused())
        }
    }
}
#[cfg(not(all(target_os = "linux", target_arch = "x86_64", target_env = "gnu")))]
pub use unsupported::{PdfWorkerManager, ScratchPolicy, WorkerProgram};

/// Test-fixture-only policy entry; absent from ordinary package APIs/binaries.
#[cfg(all(
    feature = "host-tests",
    target_os = "linux",
    target_arch = "x86_64",
    target_env = "gnu"
))]
pub fn fixture_install_policy(parent: u32) -> std::io::Result<()> {
    linux::setup_worker(parent)
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
mod input_limit382 {
    use super::{Failure, FailureKind};

    #[test]
    fn input_too_large_has_frozen_complete_projection() {
        let failure = Failure::input_too_large();
        let original = serde_json::to_value(&failure).expect("input-limit failure must serialize");
        eprintln!("input_limit382 original {original:#?}");
        assert_eq!(failure.kind, FailureKind::InputTooLarge);
        assert_eq!(
            original,
            serde_json::json!({
                "kind": "InputTooLarge",
                "operation": "input byte cap",
                "diagnostics": {
                    "display": {
                        "text": "PDF input exceeds the 8388608-byte limit",
                        "truncated": false
                    },
                    "causes": [],
                    "cause_chain_truncated": false
                },
                "errno": null
            })
        );
    }
}
