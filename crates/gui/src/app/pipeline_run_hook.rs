//! The pipeline worker's debug hook. It relays pause and publication events
//! to the UI thread and waits for its commands; plain runs use only its
//! publication gate and the shared cancellation flag.

use super::*;

pub(super) struct PipelineRunHook {
    pub(super) events: Sender<PipelineRunEvent>,
    pub(super) commands: Receiver<PipelineRunCommand>,
    pub(super) cancelled: Arc<AtomicBool>,
    pub(super) pause_each_write: Cell<bool>,
    pub(super) pause_next_node: Cell<bool>,
    pub(super) pause_next_input: Cell<bool>,
    pub(super) breakpoint: Option<PipelineBreakpoint>,
    pub(super) value_condition: Option<DebugScalarCondition>,
    pub(super) position_condition: Option<DebugPositionCondition>,
    pub(super) source_condition: Option<DebugSourceCondition>,
    pub(super) node_condition: Option<DebugNodeCondition>,
    pub(super) expression_condition: Option<DebugExpressionCondition>,
    pub(super) expression_stage: Option<String>,
    pub(super) input_condition: Option<DebugInputCondition>,
    pub(super) input_stage: Option<String>,
    pub(super) pause_on_failure: bool,
    pub(super) failure_stage: Option<String>,
}

impl PipelineRunHook {
    fn after_failure(&self, event: PipelineRunEvent) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(PipelineRunCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(
                    PipelineRunCommand::Step
                    | PipelineRunCommand::Continue
                    | PipelineRunCommand::Pause
                    | PipelineRunCommand::Publish,
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
                Ok(PipelineRunCommand::Continue) => return engine::DebugDecision::Resume,
                Ok(PipelineRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(
                    PipelineRunCommand::Pause
                    | PipelineRunCommand::Step
                    | PipelineRunCommand::Publish,
                )
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }

    pub(super) fn after_node_failure(
        &self,
        stage: &str,
        failure: &engine::PendingNodeFailure,
    ) -> engine::DebugDecision {
        if !self.pause_on_failure
            || !self
                .failure_stage
                .as_deref()
                .is_none_or(|selected| selected == stage)
        {
            return engine::DebugDecision::Resume;
        }
        self.after_failure(PipelineRunEvent::PausedFailure(
            stage.into(),
            Box::new(failure.clone()),
        ))
    }

    pub(super) fn after_function_node_failure(
        &self,
        stage: &str,
        failure: &engine::PendingFunctionNodeFailure,
    ) -> engine::DebugDecision {
        if !self.pause_on_failure
            || !self
                .failure_stage
                .as_deref()
                .is_none_or(|selected| selected == stage)
        {
            return engine::DebugDecision::Resume;
        }
        self.after_failure(PipelineRunEvent::PausedFunctionFailure(
            stage.into(),
            Box::new(failure.clone()),
        ))
    }

    pub(super) fn source_field_probe(&self) -> Option<(usize, String)> {
        self.source_condition
            .as_ref()
            .map(DebugSourceCondition::probe)
    }

    pub(super) fn before_target_write(
        &self,
        stage: &str,
        write: &engine::PendingTargetWrite,
    ) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(PipelineRunCommand::Pause | PipelineRunCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(PipelineRunCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(PipelineRunCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(PipelineRunCommand::Publish) | Err(TryRecvError::Empty) => break,
            }
        }
        let matches_selection = self
            .breakpoint
            .as_ref()
            .is_none_or(|breakpoint| breakpoint.matches(stage, write));
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
            .send(PipelineRunEvent::Paused(
                stage.into(),
                Box::new(write.clone()),
            ))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(PipelineRunCommand::Step) => {
                    self.pause_each_write.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(PipelineRunCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(PipelineRunCommand::Pause | PipelineRunCommand::Publish)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(PipelineRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
            }
        }
    }

    pub(super) fn after_node_value(
        &self,
        stage: &str,
        value: &engine::PendingNodeValue,
    ) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(PipelineRunCommand::Pause | PipelineRunCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(PipelineRunCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(PipelineRunCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(PipelineRunCommand::Publish) | Err(TryRecvError::Empty) => break,
            }
        }
        let matches_expression = self.expression_condition.as_ref().is_some_and(|condition| {
            self.expression_stage
                .as_deref()
                .is_none_or(|id| id == stage)
                && condition.matches(value)
        });
        if !self.pause_next_node.replace(false) && !matches_expression {
            return engine::DebugDecision::Resume;
        }
        if self
            .events
            .send(PipelineRunEvent::PausedNode(
                stage.into(),
                Box::new(value.clone()),
            ))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(PipelineRunCommand::Step) => {
                    self.pause_next_node.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(PipelineRunCommand::Continue) => {
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                    self.pause_each_write.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(PipelineRunCommand::Pause | PipelineRunCommand::Publish)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(PipelineRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
            }
        }
    }

    pub(super) fn after_function_node_value(
        &self,
        stage: &str,
        value: &engine::PendingFunctionNodeValue,
    ) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(PipelineRunCommand::Pause | PipelineRunCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(PipelineRunCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(PipelineRunCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(PipelineRunCommand::Publish) | Err(TryRecvError::Empty) => break,
            }
        }
        let matches_expression = self.expression_condition.as_ref().is_some_and(|condition| {
            self.expression_stage
                .as_deref()
                .is_none_or(|id| id == stage)
                && condition.matches_function(value)
        });
        if !self.pause_next_node.replace(false) && !matches_expression {
            return engine::DebugDecision::Resume;
        }
        if self
            .events
            .send(PipelineRunEvent::PausedFunctionNode(
                stage.into(),
                Box::new(value.clone()),
            ))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(PipelineRunCommand::Step) => {
                    self.pause_next_node.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(PipelineRunCommand::Continue) => {
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                    self.pause_each_write.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(PipelineRunCommand::Pause | PipelineRunCommand::Publish)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(PipelineRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
            }
        }
    }

    pub(super) fn after_node_input(
        &self,
        stage: &str,
        input: &engine::PendingNodeInput,
    ) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(PipelineRunCommand::Pause | PipelineRunCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(PipelineRunCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(PipelineRunCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(PipelineRunCommand::Publish) | Err(TryRecvError::Empty) => break,
            }
        }
        let matches_pin = self.input_condition.as_ref().is_some_and(|condition| {
            self.input_stage.as_deref().is_none_or(|id| id == stage) && condition.matches(input)
        });
        if !self.pause_next_input.replace(false) && !matches_pin {
            return engine::DebugDecision::Resume;
        }
        if self
            .events
            .send(PipelineRunEvent::PausedInput(
                stage.into(),
                Box::new(input.clone()),
            ))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(PipelineRunCommand::Step) => {
                    self.pause_next_input.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(PipelineRunCommand::Continue) => {
                    self.pause_next_input.set(false);
                    self.pause_next_node.set(false);
                    self.pause_each_write.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(PipelineRunCommand::Pause | PipelineRunCommand::Publish)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(PipelineRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
            }
        }
    }

    pub(super) fn after_function_node_input(
        &self,
        stage: &str,
        input: &engine::PendingFunctionNodeInput,
    ) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(PipelineRunCommand::Pause | PipelineRunCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(PipelineRunCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(PipelineRunCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(PipelineRunCommand::Publish) | Err(TryRecvError::Empty) => break,
            }
        }
        let matches_pin = self.input_condition.as_ref().is_some_and(|condition| {
            self.input_stage.as_deref().is_none_or(|id| id == stage)
                && condition.matches_function(input)
        });
        if !self.pause_next_input.replace(false) && !matches_pin {
            return engine::DebugDecision::Resume;
        }
        if self
            .events
            .send(PipelineRunEvent::PausedFunctionInput(
                stage.into(),
                Box::new(input.clone()),
            ))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(PipelineRunCommand::Step) => {
                    self.pause_next_input.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(PipelineRunCommand::Continue) => {
                    self.pause_next_input.set(false);
                    self.pause_next_node.set(false);
                    self.pause_each_write.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(PipelineRunCommand::Pause | PipelineRunCommand::Publish)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(PipelineRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
            }
        }
    }

    pub(super) fn before_publish(&self) -> bool {
        if self.cancelled.load(Ordering::Acquire)
            || self.events.send(PipelineRunEvent::ReadyToPublish).is_err()
        {
            return false;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return false;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(PipelineRunCommand::Publish) => {
                    return !self.cancelled.load(Ordering::Acquire);
                }
                Ok(PipelineRunCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return false;
                }
                Ok(
                    PipelineRunCommand::Step
                    | PipelineRunCommand::Continue
                    | PipelineRunCommand::Pause,
                )
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}
