//! The saved-file run worker's controls. [`FileRunControl`] carries pause and
//! publication events to the UI thread, its commands back, and the shared
//! cancellation flag. Cancellation and the publication gate need no debug
//! hook: plain Run attaches only them and its trace collector, while Debug Run
//! adds [`FileRunDebugHook`] for pauses and breakpoints.

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::Duration;

use super::{FileBreakpoint, FileRunCommand, FileRunEvent};
use crate::preview::{
    DebugExpressionCondition, DebugInputCondition, DebugNodeCondition, DebugPositionCondition,
    DebugScalarCondition, DebugSourceCondition,
};

pub(super) struct FileRunControl {
    pub(super) events: Sender<FileRunEvent>,
    pub(super) commands: Receiver<FileRunCommand>,
    /// Set by the UI before it sends Cancel; the runner checks it at every
    /// acquisition, interpreter control point and publication boundary.
    pub(super) cancelled: Arc<AtomicBool>,
}

impl FileRunControl {
    /// Options for one saved-file run. The trace collector feeds the run
    /// report; cancellation and the publication gate use this control; only
    /// Debug Run supplies `hook`.
    pub(super) fn run_options<'a>(
        &'a self,
        parameters: &'a engine::RuntimeParameters,
        trace: &'a dyn cli::TraceSink,
        gate: &'a dyn Fn() -> bool,
        hook: Option<&'a FileRunDebugHook<'_>>,
    ) -> cli::RunOptions<'a> {
        let options = cli::RunOptions::new()
            .with_runtime_parameters(parameters)
            .with_trace_sink(trace)
            .with_cancellation(&self.cancelled)
            .with_before_publish(gate);
        match hook {
            Some(hook) => options.with_debug_hook(hook),
            None => options,
        }
    }

    /// Asks the UI thread for permission to publish and blocks until it is
    /// granted or the run is cancelled.
    pub(super) fn before_publish(&self) -> bool {
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
pub(in crate::app) fn file_run_publication_gate_for_test(
    events: Sender<FileRunEvent>,
    commands: Receiver<FileRunCommand>,
    cancelled: Arc<AtomicBool>,
) -> bool {
    FileRunControl {
        events,
        commands,
        cancelled,
    }
    .before_publish()
}

pub(super) struct FileRunDebugHook<'a> {
    pub(super) control: &'a FileRunControl,
    pub(super) pause_each_write: Cell<bool>,
    pub(super) pause_next_node: Cell<bool>,
    pub(super) pause_next_input: Cell<bool>,
    pub(super) breakpoint: Option<FileBreakpoint>,
    pub(super) value_condition: Option<DebugScalarCondition>,
    pub(super) position_condition: Option<DebugPositionCondition>,
    pub(super) source_condition: Option<DebugSourceCondition>,
    pub(super) node_condition: Option<DebugNodeCondition>,
    pub(super) expression_condition: Option<DebugExpressionCondition>,
    pub(super) input_condition: Option<DebugInputCondition>,
    pub(super) pause_on_failure: bool,
}

impl FileRunDebugHook<'_> {
    fn after_failure(&self, event: FileRunEvent) -> engine::DebugDecision {
        if self.control.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.control.commands.try_recv() {
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
        if self.control.events.send(event).is_err() {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.control.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self
                .control
                .commands
                .recv_timeout(Duration::from_millis(100))
            {
                Ok(FileRunCommand::Continue) => return engine::DebugDecision::Resume,
                Ok(FileRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(FileRunCommand::Pause | FileRunCommand::Step | FileRunCommand::Publish)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

impl engine::DebugHook for FileRunDebugHook<'_> {
    fn wants_node_values(&self) -> bool {
        self.expression_condition.is_some() || self.pause_next_node.get()
    }

    fn wants_function_node_values(&self) -> bool {
        self.expression_condition
            .as_ref()
            .is_some_and(DebugExpressionCondition::targets_function)
            || self.pause_next_node.get()
    }

    fn wants_node_inputs(&self) -> bool {
        self.input_condition.is_some() || self.pause_next_input.get()
    }

    fn wants_function_node_inputs(&self) -> bool {
        self.input_condition
            .as_ref()
            .is_some_and(DebugInputCondition::targets_function)
            || self.pause_next_input.get()
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
        if self.control.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.control.commands.try_recv() {
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
            .control
            .events
            .send(FileRunEvent::Paused(Box::new(write.clone())))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.control.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self
                .control
                .commands
                .recv_timeout(Duration::from_millis(100))
            {
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
        if self.control.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.control.commands.try_recv() {
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
            .control
            .events
            .send(FileRunEvent::PausedNode(Box::new(value.clone())))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.control.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self
                .control
                .commands
                .recv_timeout(Duration::from_millis(100))
            {
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
        if self.control.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.control.commands.try_recv() {
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
            .control
            .events
            .send(FileRunEvent::PausedFunctionNode(Box::new(value.clone())))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.control.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self
                .control
                .commands
                .recv_timeout(Duration::from_millis(100))
            {
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
        if self.control.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.control.commands.try_recv() {
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
            .control
            .events
            .send(FileRunEvent::PausedInput(Box::new(input.clone())))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.control.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self
                .control
                .commands
                .recv_timeout(Duration::from_millis(100))
            {
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
        if self.control.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.control.commands.try_recv() {
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
            .control
            .events
            .send(FileRunEvent::PausedFunctionInput(Box::new(input.clone())))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.control.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self
                .control
                .commands
                .recv_timeout(Duration::from_millis(100))
            {
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

#[cfg(test)]
mod tests {
    use super::*;

    fn control() -> (
        FileRunControl,
        Receiver<FileRunEvent>,
        Sender<FileRunCommand>,
    ) {
        let (events, event_receiver) = mpsc::channel();
        let (commands, command_receiver) = mpsc::channel();
        let control = FileRunControl {
            events,
            commands: command_receiver,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        (control, event_receiver, commands)
    }

    #[test]
    fn plain_run_cancels_through_control_without_a_debug_hook() {
        let (control, events, commands) = control();
        let parameters = engine::RuntimeParameters::new();
        let trace = crate::run_report::TraceCollector::new();
        let gate = || control.before_publish();
        let options = control.run_options(&parameters, &trace, &gate, None);
        assert!(options.debug_hook.is_none());
        assert!(
            options
                .cancellation
                .is_some_and(|flag| std::ptr::eq(flag, &*control.cancelled))
        );
        // Cancellation is admitted as interpreter control, not as an observer
        // the host must supply.
        assert_eq!(
            options.request().observers,
            cli::SourceOwnershipObservers {
                tracing: true,
                debugging: true,
            }
        );

        control.cancelled.store(true, Ordering::Release);
        assert!(!(options.before_publish.expect("publication gate"))());
        assert!(events.try_recv().is_err(), "a cancelled gate sends nothing");
        drop(commands);
    }

    #[test]
    fn debug_run_adds_its_hook_beside_the_same_cancellation() {
        let (control, _events, _commands) = control();
        let parameters = engine::RuntimeParameters::new();
        let trace = crate::run_report::TraceCollector::new();
        let gate = || control.before_publish();
        let hook = FileRunDebugHook {
            control: &control,
            pause_each_write: Cell::new(true),
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
        };
        let options = control.run_options(&parameters, &trace, &gate, Some(&hook));
        assert!(options.debug_hook.is_some());
        assert!(
            options
                .cancellation
                .is_some_and(|flag| std::ptr::eq(flag, &*control.cancelled))
        );
        // The hook no longer requests control points for cancellation itself.
        control.cancelled.store(true, Ordering::Release);
        assert!(!engine::DebugHook::wants_node_values(&hook));
        assert!(!engine::DebugHook::wants_node_inputs(&hook));
    }
}
