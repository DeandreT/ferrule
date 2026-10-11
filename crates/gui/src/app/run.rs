use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::*;
use crate::preview::{
    DebugExpressionCondition, DebugInputCondition, DebugNodeCondition, DebugPositionCondition,
    DebugScalarCondition, DebugSourceCondition, PreviewBreakpoint, PreviewTarget,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct FileBreakpoint {
    target: engine::TraceTarget,
    field: PreviewBreakpoint,
}

impl FileBreakpoint {
    fn label(&self) -> String {
        let target = match &self.target {
            engine::TraceTarget::Primary => "Primary",
            engine::TraceTarget::Named(name) => name,
        };
        format!("{target} / {}", self.field.label())
    }

    fn matches(&self, write: &engine::PendingTargetWrite) -> bool {
        self.target == write.scope.target && self.field.matches(write)
    }
}

fn file_breakpoint_candidates(project: &Project) -> Vec<FileBreakpoint> {
    const MAX_CANDIDATES: usize = 512;
    let mut result = Vec::new();
    for field in crate::preview::breakpoint_candidates(project, &PreviewTarget::Primary) {
        result.push(FileBreakpoint {
            target: engine::TraceTarget::Primary,
            field,
        });
    }
    for target in &project.extra_targets {
        for field in crate::preview::breakpoint_candidates(
            project,
            &PreviewTarget::Named(target.name.clone()),
        ) {
            result.push(FileBreakpoint {
                target: engine::TraceTarget::Named(target.name.clone()),
                field,
            });
            if result.len() >= MAX_CANDIDATES {
                return result;
            }
        }
    }
    result.truncate(MAX_CANDIDATES);
    result
}

#[derive(Clone, Copy)]
pub(super) enum FileRunCommand {
    Step,
    Continue,
    Pause,
    Cancel,
    Publish,
}

pub(super) enum FileRunEvent {
    Paused(Box<engine::PendingTargetWrite>),
    PausedNode(Box<engine::PendingNodeValue>),
    PausedFunctionNode(Box<engine::PendingFunctionNodeValue>),
    PausedInput(Box<engine::PendingNodeInput>),
    PausedFunctionInput(Box<engine::PendingFunctionNodeInput>),
    PausedFailure(Box<engine::PendingNodeFailure>),
    PausedFunctionFailure(Box<engine::PendingFunctionNodeFailure>),
    ReadyToPublish,
    Finished(
        Result<cli::RunOutcome, FileRunError>,
        crate::run_report::TraceReport,
    ),
}

#[derive(Debug)]
pub(super) enum FileRunError {
    Cancelled,
    Failed(String),
    Worker(FileRunWorkerFailure),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FileRunWorkerFailure {
    Panicked,
    Unreported,
}

impl FileRunWorkerFailure {
    fn message(self) -> &'static str {
        match self {
            Self::Panicked => "Run worker panicked.",
            Self::Unreported => "Run worker stopped without reporting a result.",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum FileRunPhase {
    Running,
    Paused(Box<engine::PendingTargetWrite>),
    PausedNode(Box<engine::PendingNodeValue>),
    PausedFunctionNode(Box<engine::PendingFunctionNodeValue>),
    PausedInput(Box<engine::PendingNodeInput>),
    PausedFunctionInput(Box<engine::PendingFunctionNodeInput>),
    PausedFailure(Box<engine::PendingNodeFailure>),
    PausedFunctionFailure(Box<engine::PendingFunctionNodeFailure>),
    Publishing,
    Stopping,
}

pub(super) type FileRunCompletion = (
    Result<cli::RunOutcome, FileRunError>,
    crate::run_report::TraceReport,
);

#[derive(Default)]
pub(super) struct ShutdownIntent {
    pub(super) requested: bool,
    pub(super) attempts: u8,
    pub(super) delivered: Option<bool>,
    #[cfg(test)]
    pub(super) join_observer: Option<Arc<std::sync::atomic::AtomicUsize>>,
}

pub(super) struct FileRunCore {
    pub(super) receiver: Receiver<FileRunEvent>,
    pub(super) worker: Option<JoinHandle<()>>,
    pub(super) completed: Option<FileRunCompletion>,
    pub(super) commands: Sender<FileRunCommand>,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) started: Instant,
    pub(super) debug: bool,
    pub(super) phase: FileRunPhase,
    pub(super) shutdown: ShutdownIntent,
}

pub(super) struct PendingFileRun {
    core: Option<FileRunCore>,
    reservation: Option<super::file_run_teardown::Reservation>,
}

impl std::ops::Deref for PendingFileRun {
    type Target = FileRunCore;
    fn deref(&self) -> &Self::Target {
        self.core
            .as_ref()
            .expect("attached saved-file core retained")
    }
}

impl std::ops::DerefMut for PendingFileRun {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.core
            .as_mut()
            .expect("attached saved-file core retained")
    }
}

impl PendingFileRun {
    pub(super) fn new(
        core: FileRunCore,
        reservation: super::file_run_teardown::Reservation,
    ) -> Self {
        Self {
            core: Some(core),
            reservation: Some(reservation),
        }
    }

    #[cfg(test)]
    fn from_test_core(core: FileRunCore) -> Self {
        let port = super::file_run_teardown::SavedFileRunTeardownPort::default();
        let reservation = port.reserve().expect("fresh test saved-file owner slot");
        Self::new(core, reservation)
    }

    pub(super) fn take_completion_if_finished(&mut self) -> Option<FileRunCompletion> {
        self.core.as_mut()?.take_completion_if_finished()
    }
}

impl Drop for PendingFileRun {
    fn drop(&mut self) {
        if let Some(core) = self.core.take() {
            if let Some(reservation) = self.reservation.take() {
                reservation.handoff(core);
            } else {
                // The core itself drains on Drop; an unexpected missing ticket
                // never drops a live JoinHandle or pretends that it completed.
                drop(core);
            }
        }
    }
}

impl FileRunCore {
    pub(super) fn take_completion_if_finished(
        &mut self,
    ) -> Option<(
        Result<cli::RunOutcome, FileRunError>,
        crate::run_report::TraceReport,
    )> {
        if !self.worker.as_ref().is_some_and(JoinHandle::is_finished) {
            return None;
        }
        // Only the exact retained handle is joined, after it reports completion.
        let joined = self
            .worker
            .take()
            .expect("run worker handle retained")
            .join();
        #[cfg(test)]
        if let Some(observer) = &self.shutdown.join_observer {
            observer.fetch_add(1, Ordering::AcqRel);
        }
        let (result, trace) = self.completed.take().unwrap_or_else(|| {
            (
                Err(FileRunError::Worker(FileRunWorkerFailure::Unreported)),
                crate::run_report::TraceReport::default(),
            )
        });
        let result = if joined.is_err() {
            Err(FileRunError::Worker(FileRunWorkerFailure::Panicked))
        } else {
            result
        };
        Some((result, trace))
    }

    fn command(&mut self, command: FileRunCommand) {
        if matches!(
            self.phase,
            FileRunPhase::Publishing | FileRunPhase::Stopping
        ) {
            return;
        }
        match command {
            FileRunCommand::Step | FileRunCommand::Continue => {
                self.phase = FileRunPhase::Running;
            }
            FileRunCommand::Pause => {}
            FileRunCommand::Cancel => {
                self.cancelled.store(true, Ordering::Release);
                self.phase = FileRunPhase::Stopping;
            }
            FileRunCommand::Publish => return,
        }
        let _ = self.commands.send(command);
    }

    pub(super) fn request_shutdown(&mut self) {
        if self.worker.is_none() || self.shutdown.requested {
            return;
        }
        self.shutdown.requested = true;
        if matches!(self.phase, FileRunPhase::Publishing) {
            return;
        }
        if matches!(self.phase, FileRunPhase::Stopping) || self.cancelled.load(Ordering::Acquire) {
            return;
        }
        self.cancelled.store(true, Ordering::Release);
        self.phase = FileRunPhase::Stopping;
        self.shutdown.attempts = 1;
        self.shutdown.delivered = Some(self.commands.send(FileRunCommand::Cancel).is_ok());
    }

    pub(super) fn poll_shutdown(&mut self) -> Option<super::file_run_teardown::JoinedFileRun> {
        const EVENT_BATCH: usize = 32;
        self.worker.as_ref()?;
        // A queued result remains held even if the sender is disconnected or
        // the worker keeps running after sending it.
        if self.completed.is_none() {
            // Observe completion before probing the queue. A worker may send
            // Finished and exit between an Empty probe and a later predicate.
            let finished_before_probe = self.worker.as_ref().is_some_and(JoinHandle::is_finished);
            let mut exhausted = true;
            for _ in 0..EVENT_BATCH {
                match self.receiver.try_recv() {
                    Ok(FileRunEvent::Finished(result, trace)) => {
                        self.completed = Some((result, trace));
                        exhausted = false;
                        break;
                    }
                    Ok(_) => {} // Never grant Publish/resume after detachment.
                    Err(TryRecvError::Empty | TryRecvError::Disconnected) => {
                        if !finished_before_probe {
                            return None;
                        }
                        exhausted = false;
                        break;
                    }
                }
            }
            if exhausted {
                return None;
            }
        }
        // Empty/disconnected is not completion. This exact-handle predicate is
        // also required when a complete result has already been received.
        let (result, trace) = self.take_completion_if_finished()?;
        Some(super::file_run_teardown::JoinedFileRun {
            result,
            trace,
            phase: self.phase.clone(),
            elapsed: self.started.elapsed(),
            shutdown_cancel_attempts: self.shutdown.attempts,
            shutdown_cancel_delivered: self.shutdown.delivered,
        })
    }
}

impl Drop for FileRunCore {
    fn drop(&mut self) {
        self.request_shutdown();
        while self.worker.is_some() {
            if let Some(terminal) = self.poll_shutdown() {
                terminal.report();
                break;
            }
            // An uncooperative in-process parser can keep this owner alive
            // indefinitely. There is no timeout/drop-as-completion shortcut.
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

struct FileRunDebugHook {
    events: Sender<FileRunEvent>,
    commands: Receiver<FileRunCommand>,
    cancelled: Arc<AtomicBool>,
    pause_each_write: Cell<bool>,
    pause_next_node: Cell<bool>,
    pause_next_input: Cell<bool>,
    breakpoint: Option<FileBreakpoint>,
    value_condition: Option<DebugScalarCondition>,
    position_condition: Option<DebugPositionCondition>,
    source_condition: Option<DebugSourceCondition>,
    node_condition: Option<DebugNodeCondition>,
    expression_condition: Option<DebugExpressionCondition>,
    input_condition: Option<DebugInputCondition>,
    pause_on_failure: bool,
}

impl FileRunDebugHook {
    fn after_failure(&self, event: FileRunEvent) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(FileRunCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(
                    FileRunCommand::Step
                    | FileRunCommand::Continue
                    | FileRunCommand::Pause
                    | FileRunCommand::Publish,
                ) => {}
                Err(TryRecvError::Empty) => break,
            }
        }
        if self.events.send(event).is_err() {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(FileRunCommand::Continue) => return engine::DebugDecision::Resume,
                Ok(FileRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(FileRunCommand::Pause | FileRunCommand::Step | FileRunCommand::Publish)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }

    fn before_publish(&self) -> bool {
        if self.cancelled.load(Ordering::Acquire)
            || self.events.send(FileRunEvent::ReadyToPublish).is_err()
        {
            return false;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return false;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(FileRunCommand::Publish) => {
                    return !self.cancelled.load(Ordering::Acquire);
                }
                Ok(FileRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return false;
                }
                Ok(FileRunCommand::Step | FileRunCommand::Continue | FileRunCommand::Pause)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

#[cfg(test)]
pub(super) fn file_run_publication_gate_for_test(
    events: Sender<FileRunEvent>,
    commands: Receiver<FileRunCommand>,
    cancelled: Arc<AtomicBool>,
) -> bool {
    FileRunDebugHook {
        events,
        commands,
        cancelled,
        pause_each_write: Cell::new(false),
        pause_next_node: Cell::new(false),
        pause_next_input: Cell::new(false),
        breakpoint: None,
        value_condition: None,
        position_condition: None,
        source_condition: None,
        node_condition: None,
        expression_condition: None,
        input_condition: None,
        pause_on_failure: false,
    }
    .before_publish()
}

impl engine::DebugHook for FileRunDebugHook {
    fn wants_node_values(&self) -> bool {
        self.expression_condition.is_some()
            || self.pause_next_node.get()
            || self.cancelled.load(Ordering::Acquire)
    }

    fn wants_function_node_values(&self) -> bool {
        self.expression_condition
            .as_ref()
            .is_some_and(DebugExpressionCondition::targets_function)
            || self.pause_next_node.get()
            || self.cancelled.load(Ordering::Acquire)
    }

    fn wants_node_inputs(&self) -> bool {
        self.input_condition.is_some()
            || self.pause_next_input.get()
            || self.cancelled.load(Ordering::Acquire)
    }

    fn wants_function_node_inputs(&self) -> bool {
        self.input_condition
            .as_ref()
            .is_some_and(DebugInputCondition::targets_function)
            || self.pause_next_input.get()
            || self.cancelled.load(Ordering::Acquire)
    }

    fn wants_node_failures(&self) -> bool {
        self.pause_on_failure
    }

    fn wants_function_node_failures(&self) -> bool {
        self.pause_on_failure
    }

    fn after_node_failure(&self, failure: &engine::PendingNodeFailure) -> engine::DebugDecision {
        self.after_failure(FileRunEvent::PausedFailure(Box::new(failure.clone())))
    }

    fn after_function_node_failure(
        &self,
        failure: &engine::PendingFunctionNodeFailure,
    ) -> engine::DebugDecision {
        self.after_failure(FileRunEvent::PausedFunctionFailure(Box::new(
            failure.clone(),
        )))
    }

    fn source_field_probe(&self) -> Option<(usize, String)> {
        self.source_condition
            .as_ref()
            .map(DebugSourceCondition::probe)
    }

    fn before_target_write(&self, write: &engine::PendingTargetWrite) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(FileRunCommand::Pause | FileRunCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(FileRunCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(FileRunCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(FileRunCommand::Publish) | Err(TryRecvError::Empty) => break,
            }
        }
        let matches_selection = self
            .breakpoint
            .as_ref()
            .is_none_or(|breakpoint| breakpoint.matches(write));
        let matches_value = self
            .value_condition
            .as_ref()
            .is_none_or(|condition| condition.matches(write));
        let matches_position = self
            .position_condition
            .as_ref()
            .is_none_or(|condition| condition.matches(write));
        let matches_source = self
            .source_condition
            .as_ref()
            .is_none_or(|condition| condition.matches(write));
        let matches_node = self
            .node_condition
            .as_ref()
            .is_none_or(|condition| condition.matches(write));
        let has_breakpoint = self.breakpoint.is_some()
            || self.value_condition.is_some()
            || self.position_condition.is_some()
            || self.source_condition.is_some()
            || self.node_condition.is_some();
        if !self.pause_each_write.get()
            && !(has_breakpoint
                && matches_selection
                && matches_value
                && matches_position
                && matches_source
                && matches_node)
        {
            return engine::DebugDecision::Resume;
        }
        if self
            .events
            .send(FileRunEvent::Paused(Box::new(write.clone())))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(FileRunCommand::Step) => {
                    self.pause_each_write.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(FileRunCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(FileRunCommand::Pause | FileRunCommand::Publish)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(FileRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
            }
        }
    }

    fn after_node_value(&self, value: &engine::PendingNodeValue) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(FileRunCommand::Pause | FileRunCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(FileRunCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(FileRunCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(FileRunCommand::Publish) | Err(TryRecvError::Empty) => break,
            }
        }
        let matches_expression = self
            .expression_condition
            .as_ref()
            .is_some_and(|condition| condition.matches(value));
        if !self.pause_next_node.replace(false) && !matches_expression {
            return engine::DebugDecision::Resume;
        }
        if self
            .events
            .send(FileRunEvent::PausedNode(Box::new(value.clone())))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(FileRunCommand::Step) => {
                    self.pause_next_node.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(FileRunCommand::Continue) => {
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                    self.pause_each_write.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(FileRunCommand::Pause | FileRunCommand::Publish)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(FileRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
            }
        }
    }

    fn after_function_node_value(
        &self,
        value: &engine::PendingFunctionNodeValue,
    ) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(FileRunCommand::Pause | FileRunCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(FileRunCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(FileRunCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(FileRunCommand::Publish) | Err(TryRecvError::Empty) => break,
            }
        }
        let matches_expression = self
            .expression_condition
            .as_ref()
            .is_some_and(|condition| condition.matches_function(value));
        if !self.pause_next_node.replace(false) && !matches_expression {
            return engine::DebugDecision::Resume;
        }
        if self
            .events
            .send(FileRunEvent::PausedFunctionNode(Box::new(value.clone())))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(FileRunCommand::Step) => {
                    self.pause_next_node.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(FileRunCommand::Continue) => {
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                    self.pause_each_write.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(FileRunCommand::Pause | FileRunCommand::Publish)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(FileRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
            }
        }
    }

    fn after_node_input(&self, input: &engine::PendingNodeInput) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(FileRunCommand::Pause | FileRunCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(FileRunCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(FileRunCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(FileRunCommand::Publish) | Err(TryRecvError::Empty) => break,
            }
        }
        let matches_pin = self
            .input_condition
            .as_ref()
            .is_some_and(|condition| condition.matches(input));
        if !self.pause_next_input.replace(false) && !matches_pin {
            return engine::DebugDecision::Resume;
        }
        if self
            .events
            .send(FileRunEvent::PausedInput(Box::new(input.clone())))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(FileRunCommand::Step) => {
                    self.pause_next_input.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(FileRunCommand::Continue) => {
                    self.pause_next_input.set(false);
                    self.pause_next_node.set(false);
                    self.pause_each_write.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(FileRunCommand::Pause | FileRunCommand::Publish)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(FileRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
            }
        }
    }

    fn after_function_node_input(
        &self,
        input: &engine::PendingFunctionNodeInput,
    ) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(FileRunCommand::Pause | FileRunCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(FileRunCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(FileRunCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(FileRunCommand::Publish) | Err(TryRecvError::Empty) => break,
            }
        }
        let matches_pin = self
            .input_condition
            .as_ref()
            .is_some_and(|condition| condition.matches_function(input));
        if !self.pause_next_input.replace(false) && !matches_pin {
            return engine::DebugDecision::Resume;
        }
        if self
            .events
            .send(FileRunEvent::PausedFunctionInput(Box::new(input.clone())))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(FileRunCommand::Step) => {
                    self.pause_next_input.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(FileRunCommand::Continue) => {
                    self.pause_next_input.set(false);
                    self.pause_next_node.set(false);
                    self.pause_each_write.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(FileRunCommand::Pause | FileRunCommand::Publish)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(FileRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
            }
        }
    }
}

impl FerruleApp {
    pub(super) fn clear_run_report(&mut self) {
        self.run_report = None;
        self.show_run_report = false;
        self.clear_rest_run_session();
    }

    pub(super) fn run(&mut self, ctx: &egui::Context) {
        self.start_saved_run(false, ctx);
    }

    pub(super) fn debug_run(&mut self, ctx: &egui::Context) {
        self.start_saved_run(true, ctx);
    }

    fn start_saved_run(&mut self, debug: bool, ctx: &egui::Context) {
        if self.rest_run_busy() {
            return;
        }
        if self.pending_file_run.is_some() || self.pending_pipeline_run.is_some() {
            return;
        }
        if let Err(error) = self.host_parameters.compile() {
            self.status = "run blocked".into();
            self.diagnostics.error("Run blocked", error);
            return;
        }
        if debug && let Err(error) = self.file_run_value_condition.compile() {
            self.status = "debug run blocked".into();
            self.diagnostics.error("Debug Run blocked", error);
            return;
        }
        if debug && let Err(error) = self.file_run_position_condition.compile() {
            self.status = "debug run blocked".into();
            self.diagnostics.error("Debug Run blocked", error);
            return;
        }
        if debug && let Err(error) = self.file_run_source_condition.compile() {
            self.status = "debug run blocked".into();
            self.diagnostics.error("Debug Run blocked", error);
            return;
        }
        if debug && let Err(error) = self.file_run_node_condition.compile() {
            self.status = "debug run blocked".into();
            self.diagnostics.error("Debug Run blocked", error);
            return;
        }
        if debug && let Err(error) = self.file_run_expression_condition.compile() {
            self.status = "debug run blocked".into();
            self.diagnostics.error("Debug Run blocked", error);
            return;
        }
        if debug && let Err(error) = self.file_run_input_condition.compile() {
            self.status = "debug run blocked".into();
            self.diagnostics.error("Debug Run blocked", error);
            return;
        }
        let issues = cli::validate(&self.project);
        if !issues.is_empty() {
            self.status = format!("run blocked by {} validation issue(s)", issues.len());
            self.diagnostics.validation(&self.project, issues);
            return;
        }
        self.show_run_report = false;
        self.clear_rest_run_session();
        self.diagnostics.clear();
        let continuation = if debug {
            SaveContinuation::DebugRun
        } else {
            SaveContinuation::Run
        };
        self.save_with_continuation(Some(continuation), ctx);
    }

    pub(super) fn run_saved(&mut self, debug: bool) {
        if self.rest_run_busy() {
            return;
        }
        let runtime_parameters = match self.host_parameters.compile() {
            Ok(parameters) => parameters,
            Err(error) => {
                self.status = "run blocked".into();
                self.diagnostics.error("Run blocked", error);
                return;
            }
        };
        let value_condition = if debug {
            match self.file_run_value_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "debug run blocked".into();
                    self.diagnostics.error("Debug Run blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let position_condition = if debug {
            match self.file_run_position_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "debug run blocked".into();
                    self.diagnostics.error("Debug Run blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let source_condition = if debug {
            match self.file_run_source_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "debug run blocked".into();
                    self.diagnostics.error("Debug Run blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let node_condition = if debug {
            match self.file_run_node_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "debug run blocked".into();
                    self.diagnostics.error("Debug Run blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let expression_condition = if debug {
            match self.file_run_expression_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "debug run blocked".into();
                    self.diagnostics.error("Debug Run blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let input_condition = if debug {
            match self.file_run_input_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "debug run blocked".into();
                    self.diagnostics.error("Debug Run blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let Some(project_path) = self.document.saved_path().map(PathBuf::from) else {
            self.status = "run failed".to_string();
            self.diagnostics
                .error("Run failed", "project has no saved file");
            return;
        };
        let input_path = nonempty_path(&self.input_path);
        let output_path = nonempty_path(&self.output_path);
        let breakpoint = if debug {
            self.file_run_breakpoint
                .clone()
                .filter(|selected| file_breakpoint_candidates(&self.project).contains(selected))
        } else {
            None
        };
        let Some(reservation) = self.file_run_teardown.reserve() else {
            self.status = "run blocked".into();
            self.diagnostics
                .error("Run blocked", "a saved-file run is still finishing");
            return;
        };
        let (events, receiver) = mpsc::channel();
        let (commands, command_receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let pause_on_failure = debug && self.file_run_pause_on_failure;
        let mut core = FileRunCore {
            receiver,
            worker: None,
            completed: None,
            commands,
            cancelled,
            started: Instant::now(),
            debug,
            phase: FileRunPhase::Running,
            shutdown: ShutdownIntent::default(),
        };
        core.worker = Some(std::thread::spawn(move || {
            let trace = crate::run_report::TraceCollector::new();
            let hook = FileRunDebugHook {
                events: events.clone(),
                commands: command_receiver,
                cancelled: worker_cancelled,
                pause_each_write: Cell::new(
                    debug
                        && breakpoint.is_none()
                        && value_condition.is_none()
                        && position_condition.is_none()
                        && source_condition.is_none()
                        && node_condition.is_none()
                        && expression_condition.is_none()
                        && input_condition.is_none()
                        && !pause_on_failure,
                ),
                pause_next_node: Cell::new(false),
                pause_next_input: Cell::new(false),
                breakpoint,
                value_condition,
                position_condition,
                source_condition,
                node_condition,
                expression_condition,
                input_condition,
                pause_on_failure,
            };
            let gate = || hook.before_publish();
            let mut options = cli::RunOptions::new()
                .with_runtime_parameters(&runtime_parameters)
                .with_trace_sink(&trace)
                .with_debug_hook(&hook)
                .with_before_publish(&gate);
            if let Some(path) = input_path.as_deref() {
                options = options.with_input_path(path);
            }
            if let Some(path) = output_path.as_deref() {
                options = options.with_output_path(path);
            }
            let result = cli::run_project_with_options(&project_path, &options).map_err(|error| {
                if error.chain().any(|cause| {
                    matches!(
                        cause.downcast_ref::<engine::EngineError>(),
                        Some(engine::EngineError::DebugCancelled)
                    )
                }) {
                    FileRunError::Cancelled
                } else {
                    FileRunError::Failed(format!("{error:#}"))
                }
            });
            let _ = events.send(FileRunEvent::Finished(result, trace.finish()));
        }));
        // Preserve the existing report clock's start after worker creation.
        core.started = Instant::now();
        self.pending_file_run = Some(PendingFileRun::new(core, reservation));
        self.status = if debug {
            "debug run running".into()
        } else {
            "run running".into()
        };
    }

    pub(super) fn file_run_command(&mut self, command: FileRunCommand) {
        if let Some(pending) = &mut self.pending_file_run {
            pending.command(command);
            self.status = match command {
                FileRunCommand::Cancel => "stopping run",
                FileRunCommand::Pause => "pausing at next target write",
                FileRunCommand::Step | FileRunCommand::Continue | FileRunCommand::Publish => {
                    "debug run running"
                }
            }
            .into();
        }
    }

    pub(super) fn poll_file_run(&mut self, ctx: &egui::Context) {
        let Some(pending) = &mut self.pending_file_run else {
            return;
        };
        if pending.completed.is_none() {
            // Observe thread completion before probing its queued result. An
            // Empty probe must not race a result sent just before thread exit.
            let worker_finished = pending.worker.as_ref().is_some_and(JoinHandle::is_finished);
            let event = match pending.receiver.try_recv() {
                Ok(event) => event,
                Err(TryRecvError::Empty) if worker_finished => FileRunEvent::Finished(
                    Err(FileRunError::Worker(FileRunWorkerFailure::Unreported)),
                    crate::run_report::TraceReport::default(),
                ),
                Err(TryRecvError::Empty) => {
                    if !matches!(
                        pending.phase,
                        FileRunPhase::Paused(_)
                            | FileRunPhase::PausedNode(_)
                            | FileRunPhase::PausedFunctionNode(_)
                            | FileRunPhase::PausedInput(_)
                            | FileRunPhase::PausedFunctionInput(_)
                            | FileRunPhase::PausedFailure(_)
                            | FileRunPhase::PausedFunctionFailure(_)
                    ) {
                        ctx.request_repaint_after(Duration::from_millis(100));
                    }
                    return;
                }
                Err(TryRecvError::Disconnected) => {
                    if !matches!(pending.phase, FileRunPhase::Publishing) {
                        pending.cancelled.store(true, Ordering::Release);
                        let _ = pending.commands.send(FileRunCommand::Cancel);
                    }
                    FileRunEvent::Finished(
                        Err(FileRunError::Worker(FileRunWorkerFailure::Unreported)),
                        crate::run_report::TraceReport::default(),
                    )
                }
            };
            match event {
                FileRunEvent::Paused(write) => {
                    if matches!(pending.phase, FileRunPhase::Stopping) {
                        let _ = pending.commands.send(FileRunCommand::Cancel);
                    } else {
                        self.status = format!("paused before target field `{}`", write.field);
                        pending.phase = FileRunPhase::Paused(write);
                    }
                    ctx.request_repaint();
                }
                FileRunEvent::PausedNode(value) => {
                    if matches!(pending.phase, FileRunPhase::Stopping) {
                        let _ = pending.commands.send(FileRunCommand::Cancel);
                    } else {
                        self.status = format!("paused after graph node #{}", value.node);
                        pending.phase = FileRunPhase::PausedNode(value);
                    }
                    ctx.request_repaint();
                }
                FileRunEvent::PausedFunctionNode(value) => {
                    if matches!(pending.phase, FileRunPhase::Stopping) {
                        let _ = pending.commands.send(FileRunCommand::Cancel);
                    } else {
                        self.status = format!(
                            "paused after function #{} node #{}",
                            value.function.get(),
                            value.node
                        );
                        pending.phase = FileRunPhase::PausedFunctionNode(value);
                    }
                    ctx.request_repaint();
                }
                FileRunEvent::PausedInput(input) => {
                    if matches!(pending.phase, FileRunPhase::Stopping) {
                        let _ = pending.commands.send(FileRunCommand::Cancel);
                    } else {
                        self.status = format!(
                            "paused at node #{} input #{}",
                            input.consumer,
                            input.input_index + 1
                        );
                        pending.phase = FileRunPhase::PausedInput(input);
                    }
                    ctx.request_repaint();
                }
                FileRunEvent::PausedFunctionInput(input) => {
                    if matches!(pending.phase, FileRunPhase::Stopping) {
                        let _ = pending.commands.send(FileRunCommand::Cancel);
                    } else {
                        self.status = format!(
                            "paused at function #{} node #{} input #{}",
                            input.function.get(),
                            input.consumer,
                            input.input_index + 1
                        );
                        pending.phase = FileRunPhase::PausedFunctionInput(input);
                    }
                    ctx.request_repaint();
                }
                FileRunEvent::PausedFailure(failure) => {
                    if matches!(pending.phase, FileRunPhase::Stopping) {
                        let _ = pending.commands.send(FileRunCommand::Cancel);
                    } else {
                        self.status = format!("paused on graph node #{} error", failure.node);
                        pending.phase = FileRunPhase::PausedFailure(failure);
                    }
                    ctx.request_repaint();
                }
                FileRunEvent::PausedFunctionFailure(failure) => {
                    if matches!(pending.phase, FileRunPhase::Stopping) {
                        let _ = pending.commands.send(FileRunCommand::Cancel);
                    } else {
                        self.status = format!(
                            "paused on function #{} node #{} error",
                            failure.function.get(),
                            failure.node
                        );
                        pending.phase = FileRunPhase::PausedFunctionFailure(failure);
                    }
                    ctx.request_repaint();
                }
                FileRunEvent::ReadyToPublish => {
                    if matches!(pending.phase, FileRunPhase::Stopping) {
                        let _ = pending.commands.send(FileRunCommand::Cancel);
                    } else {
                        pending.phase = FileRunPhase::Publishing;
                        self.status = "publishing run outputs".into();
                        let _ = pending.commands.send(FileRunCommand::Publish);
                    }
                    ctx.request_repaint();
                }
                FileRunEvent::Finished(result, trace) => {
                    pending.completed = Some((result, trace));
                }
            }
        }
        // A finished thread can still have earlier pause/publication events
        // queued before Finished. Preserve their order until a result is read.
        if pending.completed.is_none() {
            return;
        }
        let Some((result, trace)) = pending.take_completion_if_finished() else {
            ctx.request_repaint_after(Duration::from_millis(100));
            return;
        };
        let pending = self.pending_file_run.take().expect("run worker exists");
        // Thread faults stay visible even when cancellation was requested.
        if let Err(FileRunError::Worker(failure)) = &result {
            self.status = "run failed".into();
            self.diagnostics.error("Run failed", failure.message());
            self.finish_deferred_close(ctx);
            return;
        }
        if matches!(pending.phase, FileRunPhase::Stopping)
            || matches!(result, Err(FileRunError::Cancelled))
        {
            self.status = "run cancelled".into();
            self.diagnostics.clear();
            self.finish_deferred_close(ctx);
            return;
        }
        match result {
            Ok(outcome) => {
                self.status = format!(
                    "wrote {} record(s) to {}",
                    outcome.records_written,
                    outcome.output_path.display()
                );
                let report = crate::run_report::RunReport::from_outcome_with_trace(
                    outcome,
                    pending.started.elapsed(),
                    trace,
                );
                self.run_report = Some(crate::run_report::RunReportView::new(report));
                self.show_run_report = true;
                self.diagnostics.clear();
            }
            Err(FileRunError::Failed(error)) => {
                self.status = "run failed".into();
                self.diagnostics.error("Run failed", error);
            }
            Err(FileRunError::Cancelled | FileRunError::Worker(_)) => {
                unreachable!("handled above")
            }
        }
        self.finish_deferred_close(ctx);
    }

    fn finish_deferred_close(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.close_after_file_run) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    pub(super) fn show_file_run_progress(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_file_run else {
            return;
        };
        let phase = pending.phase.clone();
        let debug = pending.debug;
        let mut action = None;
        egui::Window::new(if debug { "Debug Run" } else { "Run" })
            .collapsible(false)
            .resizable(true)
            .default_width(600.0)
            .show(ctx, |ui| {
                match &phase {
                    FileRunPhase::Running => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.strong(if debug {
                                "Debug Run is running"
                            } else {
                                "Run is running"
                            });
                        });
                    }
                    FileRunPhase::Paused(write) => {
                        preview_ui::show_live_debug_state(
                            ui,
                            &preview_ui::PreviewPhase::Paused(write.clone()),
                            true,
                        );
                    }
                    FileRunPhase::PausedNode(value) => {
                        preview_ui::show_live_node_debug_state(ui, value);
                    }
                    FileRunPhase::PausedFunctionNode(value) => {
                        preview_ui::show_live_function_node_debug_state(ui, value);
                    }
                    FileRunPhase::PausedInput(input) => {
                        preview_ui::show_live_input_debug_state(ui, input);
                    }
                    FileRunPhase::PausedFunctionInput(input) => {
                        preview_ui::show_live_function_input_debug_state(ui, input);
                    }
                    FileRunPhase::PausedFailure(failure) => {
                        preview_ui::show_live_failure_debug_state(ui, failure);
                    }
                    FileRunPhase::PausedFunctionFailure(failure) => {
                        preview_ui::show_live_function_failure_debug_state(ui, failure);
                    }
                    FileRunPhase::Publishing => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.strong("Publishing completed mapping outputs");
                        });
                    }
                    FileRunPhase::Stopping => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.strong("Stopping run");
                        });
                    }
                }
                ui.separator();
                ui.horizontal(|ui| match &phase {
                    FileRunPhase::Paused(_)
                    | FileRunPhase::PausedNode(_)
                    | FileRunPhase::PausedFunctionNode(_)
                    | FileRunPhase::PausedInput(_)
                    | FileRunPhase::PausedFunctionInput(_) => {
                        if ui.button("Step").clicked() {
                            action = Some(FileRunCommand::Step);
                        }
                        if ui.button("Continue").clicked() {
                            action = Some(FileRunCommand::Continue);
                        }
                        if ui.button("Cancel run").clicked() {
                            action = Some(FileRunCommand::Cancel);
                        }
                    }
                    FileRunPhase::PausedFailure(_) | FileRunPhase::PausedFunctionFailure(_) => {
                        if ui.button("Continue to error").clicked() {
                            action = Some(FileRunCommand::Continue);
                        }
                        if ui.button("Cancel run").clicked() {
                            action = Some(FileRunCommand::Cancel);
                        }
                    }
                    FileRunPhase::Running => {
                        if debug && ui.button("Pause at next write").clicked() {
                            action = Some(FileRunCommand::Pause);
                        }
                        if ui.button("Cancel run").clicked() {
                            action = Some(FileRunCommand::Cancel);
                        }
                    }
                    FileRunPhase::Publishing | FileRunPhase::Stopping => {}
                });
            });
        if let Some(command) = action {
            self.file_run_command(command);
            ctx.request_repaint();
        }
    }

    pub(super) fn show_file_debug_controls(&mut self, ui: &mut egui::Ui) {
        let candidates = file_breakpoint_candidates(&self.project);
        if self
            .file_run_breakpoint
            .as_ref()
            .is_some_and(|selected| !candidates.contains(selected))
        {
            self.file_run_breakpoint = None;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("Debug breakpoint");
            egui::ComboBox::from_id_salt("file_run_debug_breakpoint")
                .selected_text(self.file_run_breakpoint.as_ref().map_or_else(
                    || "Every target-field write".to_owned(),
                    FileBreakpoint::label,
                ))
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut self.file_run_breakpoint,
                        None,
                        "Every target-field write",
                    );
                    for candidate in candidates {
                        ui.selectable_value(
                            &mut self.file_run_breakpoint,
                            Some(candidate.clone()),
                            candidate.label(),
                        );
                    }
                });
        });
        let mut condition_valid = preview_ui::show_breakpoint_value_condition(
            ui,
            &mut self.file_run_value_condition,
            "file_run_debug_value_type",
        );
        condition_valid &= preview_ui::show_breakpoint_position_condition(
            ui,
            &mut self.file_run_position_condition,
        );
        condition_valid &= preview_ui::show_breakpoint_source_condition(
            ui,
            &mut self.file_run_source_condition,
            "file_run_debug_source_type",
        );
        condition_valid &=
            preview_ui::show_breakpoint_node_condition(ui, &mut self.file_run_node_condition);
        condition_valid &= preview_ui::show_breakpoint_expression_condition(
            ui,
            &mut self.file_run_expression_condition,
            "file_run_debug_expression_value_type",
        );
        condition_valid &= preview_ui::show_breakpoint_input_condition(
            ui,
            &mut self.file_run_input_condition,
            "file_run_debug_input_value_type",
        );
        ui.checkbox(
            &mut self.file_run_pause_on_failure,
            "Pause at the first graph or function evaluation error",
        );
        if ui
            .add_enabled(condition_valid, egui::Button::new("Debug Run"))
            .clicked()
        {
            self.debug_run(ui.ctx());
        }
        ui.weak("Breakpoints cover declared static fields in primary and named targets.");
    }
}

fn nonempty_path(value: &str) -> Option<PathBuf> {
    (!value.trim().is_empty()).then(|| PathBuf::from(value.trim()))
}

#[cfg(test)]
#[path = "file_run_ownership_tests.rs"]
mod file_run_ownership_tests;

#[cfg(test)]
mod tests {
    #[test]
    fn close_during_publication_waits_for_worker_to_finish() {
        super::file_run_ownership_tests::close_during_publication();
    }
}
