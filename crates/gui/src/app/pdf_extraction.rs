//! Opt-in owned PDF extraction for GUI file runs and file-backed previews.
//!
//! [`OwnedPdfReader`] is constructed on the background thread that executes
//! one run or preview, and it owns the PDF worker manager there. It installs
//! no signal handler. Each read polls its job until the child is reaped and
//! its captures are closed; a job whose closure is unresolved keeps being
//! serviced and is never reported as finished, so the thread, and every GUI
//! owner that waits for it, stays pending. The run's single cancellation flag
//! reaches the job; cancellation observed at any point wins over a late
//! extraction result.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

use anyhow::Context;
use pdf_worker::host::{
    BeginFailure, CancellationFlag, ExclusiveReaperAdmission, JobProgress, JobState,
    PdfWorkerManager, ScratchPolicy, WorkerProgram,
};

const POLL_INTERVAL: Duration = Duration::from_millis(20);
const UNRESOLVED_POLL_INTERVAL: Duration = Duration::from_millis(100);

/// Where the packaged worker and its scratch jobs live.
#[derive(Clone, Debug)]
pub(crate) struct PdfWorkerPackage {
    /// An executable path whose directory holds `ferrule-pdf-worker`; only
    /// that sibling is admitted. `None` uses this GUI's own executable.
    executable: Option<PathBuf>,
    /// Parent of the private job directories; `None` uses the temp directory.
    scratch: Option<PathBuf>,
}

impl PdfWorkerPackage {
    pub(crate) fn beside_current_executable() -> Self {
        Self {
            executable: None,
            scratch: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test(executable: PathBuf, scratch: PathBuf) -> Self {
        Self {
            executable: Some(executable),
            scratch: Some(scratch),
        }
    }

    fn admit(&self) -> anyhow::Result<PdfWorkerManager> {
        let executable = match &self.executable {
            Some(path) => path.clone(),
            None => std::env::current_exe().context("resolving the GUI executable")?,
        };
        let scratch = self.scratch.clone().unwrap_or_else(std::env::temp_dir);
        let program = WorkerProgram::admit_packaged_sibling(&executable)?;
        // Zero is this host's explicit external reserve beyond the fixed
        // per-job headroom; it is not a quota.
        let scratch = ScratchPolicy::admit(&scratch, 0)?;
        Ok(PdfWorkerManager::new(
            program,
            scratch,
            ExclusiveReaperAdmission::for_current_host(),
        )?)
    }
}

/// The owning thread's latest observation of its current extraction.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum ExtractionPhase {
    #[default]
    Idle,
    Starting,
    Running,
    Stopping,
    /// The stop allowance passed or ownership is unresolved; the job is
    /// still being serviced and nothing has been reported as closed.
    ClosureIncomplete,
}

/// A bounded snapshot replaced on every poll; there is no event queue.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ExtractionSnapshot {
    pub(crate) phase: ExtractionPhase,
    /// The current or most recently closed child.
    pub(crate) child_pid: Option<u32>,
    pub(crate) jobs_started: u32,
    pub(crate) jobs_closed: u32,
    /// Whether the last closed job's child was reaped by its owner.
    pub(crate) last_own_reap: Option<bool>,
}

#[derive(Debug, Default)]
pub(crate) struct ExtractionStatus(Mutex<ExtractionSnapshot>);

impl ExtractionStatus {
    pub(crate) fn snapshot(&self) -> ExtractionSnapshot {
        *self.lock()
    }

    fn lock(&self) -> MutexGuard<'_, ExtractionSnapshot> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn update(&self, change: impl FnOnce(&mut ExtractionSnapshot)) {
        change(&mut self.lock());
    }

    /// A short progress line while an extraction is pending.
    pub(crate) fn progress_label(&self) -> Option<String> {
        let snapshot = self.snapshot();
        let pid = snapshot
            .child_pid
            .map(|pid| format!(" (process {pid})"))
            .unwrap_or_default();
        match snapshot.phase {
            ExtractionPhase::Idle => None,
            ExtractionPhase::Starting => Some(format!("Starting the PDF worker{pid}")),
            ExtractionPhase::Running => Some(format!("Extracting PDF input{pid}")),
            ExtractionPhase::Stopping => Some(format!("Stopping the PDF worker{pid}")),
            ExtractionPhase::ClosureIncomplete => Some(format!(
                "The PDF worker{pid} has not been confirmed stopped; closing waits for it"
            )),
        }
    }
}

/// Test-only moments at which the run's own cancellation flag is set, as if
/// the user cancelled exactly then.
#[cfg(test)]
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct ReaderControls {
    /// Stop polling a started child before it is sent Start, until cancelled.
    pub(crate) hold_until_cancelled: bool,
    pub(crate) cancel_when_starting: bool,
    pub(crate) cancel_when_running: bool,
    /// After a closed, validated success and before it is returned.
    pub(crate) cancel_when_checked: bool,
}

/// A caller-owned filesystem PDF reader backed by an owned worker process.
pub(crate) struct OwnedPdfReader<'a> {
    package: &'a PdfWorkerPackage,
    cancelled: &'a AtomicBool,
    status: &'a ExtractionStatus,
    flag: CancellationFlag,
    manager: RefCell<Option<PdfWorkerManager>>,
    #[cfg(test)]
    controls: ReaderControls,
}

impl<'a> OwnedPdfReader<'a> {
    pub(crate) fn new(
        package: &'a PdfWorkerPackage,
        cancelled: &'a AtomicBool,
        status: &'a ExtractionStatus,
    ) -> Self {
        Self {
            package,
            cancelled,
            status,
            flag: CancellationFlag::new(),
            manager: RefCell::new(None),
            #[cfg(test)]
            controls: ReaderControls::default(),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_controls(mut self, controls: ReaderControls) -> Self {
        self.controls = controls;
        self
    }

    fn observe_cancellation(&self) -> bool {
        let cancelled = self.cancelled.load(Ordering::Acquire);
        if cancelled {
            self.flag.cancel();
        }
        cancelled
    }

    #[cfg(test)]
    fn test_control(&self, state: JobState) {
        let cancel = match state {
            JobState::Starting => {
                while self.controls.hold_until_cancelled && !self.observe_cancellation() {
                    std::thread::sleep(Duration::from_millis(5));
                }
                self.controls.cancel_when_starting
            }
            JobState::Running => self.controls.cancel_when_running,
            JobState::Stopping => false,
        };
        if cancel {
            self.cancelled.store(true, Ordering::Release);
        }
    }

    fn extract(
        &self,
        manager: &mut PdfWorkerManager,
        path: &Path,
        layout: &mapping::PdfLayout,
    ) -> anyhow::Result<ir::Instance> {
        match manager.begin(path, layout, &self.flag) {
            Ok(_) | Err(BeginFailure::OwnedStopping { .. }) => {}
            Err(BeginFailure::NoChild(failure)) => {
                return Err(anyhow::Error::new(failure).context("PDF worker job was not started"));
            }
        }
        self.status.update(|snapshot| {
            snapshot.phase = ExtractionPhase::Starting;
            snapshot.child_pid = None;
            snapshot.jobs_started = snapshot.jobs_started.saturating_add(1);
        });
        loop {
            self.observe_cancellation();
            match manager.poll() {
                JobProgress::Pending { state, snapshot } => {
                    self.status.update(|status| {
                        status.phase = match state {
                            JobState::Starting => ExtractionPhase::Starting,
                            JobState::Running => ExtractionPhase::Running,
                            JobState::Stopping => ExtractionPhase::Stopping,
                        };
                        status.child_pid = snapshot.child_pid.or(status.child_pid);
                    });
                    #[cfg(test)]
                    if snapshot.child_pid.is_some() {
                        self.test_control(state);
                    }
                    std::thread::sleep(POLL_INTERVAL);
                }
                JobProgress::Succeeded { instance, receipt } => {
                    self.closed(receipt.child_pid, receipt.own_reap);
                    #[cfg(test)]
                    if self.controls.cancel_when_checked {
                        self.cancelled.store(true, Ordering::Release);
                    }
                    return Ok(instance);
                }
                JobProgress::FailedClosed { failure, retained } => {
                    self.closed(failure.receipt.child_pid, failure.receipt.own_reap);
                    return Err(anyhow::Error::new(*failure).context(format!(
                        "closed PDF worker evidence retained at {}",
                        retained.display()
                    )));
                }
                JobProgress::ClosureIncomplete { snapshot } => {
                    // Never a terminal result: keep servicing the owned child.
                    self.status.update(|status| {
                        status.phase = ExtractionPhase::ClosureIncomplete;
                        status.child_pid = snapshot.child_pid.or(status.child_pid);
                    });
                    std::thread::sleep(UNRESOLVED_POLL_INTERVAL);
                }
            }
        }
    }

    fn closed(&self, child_pid: Option<u32>, own_reap: bool) {
        self.status.update(|status| {
            status.phase = ExtractionPhase::Idle;
            status.child_pid = child_pid.or(status.child_pid);
            status.jobs_closed = status.jobs_closed.saturating_add(1);
            status.last_own_reap = Some(own_reap);
        });
    }
}

impl cli::FilesystemPdfReader for OwnedPdfReader<'_> {
    fn read_pdf(&self, path: &Path, layout: &mapping::PdfLayout) -> anyhow::Result<ir::Instance> {
        if self.observe_cancellation() {
            return Err(engine::EngineError::DebugCancelled.into());
        }
        let mut slot = self
            .manager
            .try_borrow_mut()
            .context("reentrant PDF worker acquisition")?;
        if slot.is_none() {
            // A refused admission never falls back to the in-process parser.
            *slot = Some(
                self.package
                    .admit()
                    .context("the packaged PDF worker was not admitted")?,
            );
        }
        let Some(manager) = slot.as_mut() else {
            anyhow::bail!("the PDF worker manager is unavailable");
        };
        let result = self.extract(manager, path, layout);
        if result.is_err() {
            // The failed job is closed; its evidence stays on disk and a
            // later read starts from a fresh manager.
            *slot = None;
        }
        if self.observe_cancellation() {
            let closed = match &result {
                Ok(_) => "a closed PDF extraction completed after cancellation".to_owned(),
                Err(error) => format!("{error:#}"),
            };
            return Err(anyhow::Error::new(engine::EngineError::DebugCancelled).context(closed));
        }
        result
    }
}

#[cfg(all(test, target_os = "linux", target_arch = "x86_64", target_env = "gnu"))]
#[path = "pdf_extraction_tests.rs"]
pub(crate) mod tests;
