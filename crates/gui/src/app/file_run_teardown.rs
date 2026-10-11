//! One saved-file run remains owned after its app or desktop runner is gone.
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use super::run_ui::{FileRunCore, FileRunError, FileRunPhase, FileRunWorkerFailure};

#[derive(Debug)]
pub(crate) struct JoinedFileRun {
    pub(super) result: Result<cli::RunOutcome, FileRunError>,
    pub(super) trace: crate::run_report::TraceReport,
    pub(super) phase: FileRunPhase,
    pub(super) elapsed: Duration,
    pub(super) shutdown_cancel_attempts: u8,
    pub(super) shutdown_cancel_delivered: Option<bool>,
}

impl JoinedFileRun {
    // Surface a terminal category without pretending that closing prevented a
    // publication which had already been permitted. The complete tuple remains
    // available to the owner until this report is explicitly consumed.
    pub(crate) fn report(&self) {
        let category = match &self.result {
            Ok(_) => "reported success",
            Err(FileRunError::Cancelled) => "reported cancellation",
            Err(FileRunError::Failed(_)) => "reported failure",
            Err(FileRunError::Worker(FileRunWorkerFailure::Panicked)) => "worker panicked",
            Err(FileRunError::Worker(FileRunWorkerFailure::Unreported)) => "worker unreported",
        };
        eprintln!(
            "saved-file shutdown: {category}; phase {:?}; elapsed {:?}; shutdown cancel attempts {}; delivered {:?}; trace events {}, dropped {}, stages {}, event stages {}",
            self.phase,
            self.elapsed,
            self.shutdown_cancel_attempts,
            self.shutdown_cancel_delivered,
            self.trace.events.len(),
            self.trace.dropped,
            self.trace.stages.len(),
            self.trace.event_stages.len()
        );
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ShutdownProgress {
    Idle,
    PendingOwned,
    Joined,
}

enum Slot {
    Idle,
    Attached,
    Owned(FileRunCore),
    Servicing,
    Joined(JoinedFileRun),
}

struct OwnerState {
    slot: Slot,
    closing: bool,
    transfer_fault: bool,
}

impl Drop for OwnerState {
    fn drop(&mut self) {
        // Owned cores have their own exact-handle, sleeping drain fallback. A
        // retained terminal report must also be surfaced if its owner unwinds.
        if let Slot::Joined(terminal) = &self.slot {
            terminal.report();
        }
    }
}

struct SharedOwner(Mutex<OwnerState>);

impl SharedOwner {
    fn state(&self) -> MutexGuard<'_, OwnerState> {
        match self.0.lock() {
            Ok(state) => state,
            Err(poison) => {
                let mut state = poison.into_inner();
                state.transfer_fault = true;
                state
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct SavedFileRunTeardownPort {
    shared: Arc<SharedOwner>,
}

impl Default for SavedFileRunTeardownPort {
    fn default() -> Self {
        Self {
            shared: Arc::new(SharedOwner(Mutex::new(OwnerState {
                slot: Slot::Idle,
                closing: false,
                transfer_fault: false,
            }))),
        }
    }
}

impl SavedFileRunTeardownPort {
    pub(super) fn reserve(&self) -> Option<Reservation> {
        let mut state = self.shared.state();
        if state.closing || !matches!(state.slot, Slot::Idle) {
            return None;
        }
        state.slot = Slot::Attached;
        Some(Reservation {
            port: self.clone(),
            active: true,
        })
    }
}

pub(super) struct Reservation {
    port: SavedFileRunTeardownPort,
    active: bool,
}

impl Reservation {
    pub(super) fn handoff(mut self, mut core: FileRunCore) {
        // Cancellation and event commands occur outside the owner mutex.
        core.request_shutdown();
        let mut state = self.port.shared.state();
        if matches!(state.slot, Slot::Attached) {
            // A normally joined core no longer has a worker. No terminal tuple
            // is lost: the ordinary app consumer already took it.
            state.slot = if core.worker.is_some() {
                Slot::Owned(core)
            } else {
                Slot::Idle
            };
            self.active = false;
            return;
        }
        state.transfer_fault = true;
        self.active = false;
        drop(state);
        // A broken reservation cannot overwrite another exact owner. Keep this
        // core locally until its own Drop drains and joins, rather than detach.
        eprintln!("saved-file shutdown: unexpected reservation; exact core retained locally");
        drop(core);
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if self.active {
            let mut state = self.port.shared.state();
            if matches!(state.slot, Slot::Attached) {
                state.slot = Slot::Idle;
            } else {
                state.transfer_fault = true;
            }
        }
    }
}

pub(crate) struct SavedFileRunTeardownService {
    port: SavedFileRunTeardownPort,
}

impl SavedFileRunTeardownService {
    pub(crate) fn new() -> Self {
        Self {
            port: SavedFileRunTeardownPort::default(),
        }
    }

    pub(crate) fn port(&self) -> SavedFileRunTeardownPort {
        self.port.clone()
    }

    pub(crate) fn begin_shutdown(&mut self) {
        self.port.shared.state().closing = true;
    }

    pub(crate) fn poll_shutdown(&mut self) -> ShutdownProgress {
        let mut core = {
            let mut state = self.port.shared.state();
            match &state.slot {
                Slot::Idle => return ShutdownProgress::Idle,
                Slot::Joined(_) => return ShutdownProgress::Joined,
                Slot::Attached | Slot::Servicing => return ShutdownProgress::PendingOwned,
                Slot::Owned(_) => {}
            }
            match std::mem::replace(&mut state.slot, Slot::Servicing) {
                Slot::Owned(core) => core,
                _ => unreachable!("the owned slot was checked under the same lock"),
            }
        };
        // The exact core is now held by this service stack frame. A panic here
        // still runs the core's drain fallback. No lock spans channel polling,
        // join, a sleep, a GUI callback or publication work.
        core.request_shutdown();
        let terminal = core.poll_shutdown();
        let progress = if terminal.is_some() {
            ShutdownProgress::Joined
        } else {
            ShutdownProgress::PendingOwned
        };
        let mut state = self.port.shared.state();
        state.slot = match terminal {
            Some(terminal) => Slot::Joined(terminal),
            None => Slot::Owned(core),
        };
        progress
    }

    pub(crate) fn drain_for(&mut self, observation_budget: Duration) -> ShutdownProgress {
        self.begin_shutdown();
        let began = Instant::now();
        loop {
            let progress = self.poll_shutdown();
            if progress != ShutdownProgress::PendingOwned || began.elapsed() >= observation_budget {
                return progress;
            }
            std::thread::sleep(
                Duration::from_millis(10).min(observation_budget.saturating_sub(began.elapsed())),
            );
        }
    }

    pub(crate) fn take_terminal(&mut self) -> Option<JoinedFileRun> {
        let mut state = self.port.shared.state();
        if !matches!(state.slot, Slot::Joined(_)) {
            return None;
        }
        match std::mem::replace(&mut state.slot, Slot::Idle) {
            Slot::Joined(terminal) => Some(terminal),
            _ => unreachable!("the joined slot was checked under the same lock"),
        }
    }

    pub(crate) fn finish_after_runner<R>(
        &mut self,
        runner_result: R,
    ) -> (R, Option<JoinedFileRun>) {
        self.begin_shutdown();
        loop {
            match self.drain_for(Duration::from_secs(1)) {
                ShutdownProgress::Idle => return (runner_result, None),
                ShutdownProgress::Joined => return (runner_result, self.take_terminal()),
                ShutdownProgress::PendingOwned => {}
            }
        }
    }
}

impl Drop for SavedFileRunTeardownService {
    fn drop(&mut self) {
        let (_, terminal) = self.finish_after_runner(());
        if let Some(terminal) = terminal {
            terminal.report();
        }
        if self.port.shared.state().transfer_fault {
            eprintln!(
                "saved-file shutdown: ownership/lock fault retained; no clean-teardown claim"
            );
        }
    }
}

#[cfg(test)]
#[path = "file_run_teardown_tests.rs"]
mod tests;
