//! The preview worker's body: it runs one payload or configured-file preview
//! with the debug hook that relays pause events to the UI thread and waits
//! for its commands. The worker returns its completion; [`super::PendingPreview`]
//! owns the thread.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn run_preview_worker(
    project: Project,
    draft: PreviewDraft,
    project_path: PathBuf,
    saved_path: Option<PathBuf>,
    runtime_parameters: engine::RuntimeParameters,
    debug: bool,
    value_condition: Option<DebugScalarCondition>,
    position_condition: Option<DebugPositionCondition>,
    source_condition: Option<DebugSourceCondition>,
    node_condition: Option<DebugNodeCondition>,
    expression_condition: Option<DebugExpressionCondition>,
    input_condition: Option<DebugInputCondition>,
    pause_on_failure: bool,
    events: Sender<PreviewWorkerEvent>,
    commands: Receiver<PreviewCommand>,
    cancelled: Arc<AtomicBool>,
    acquisition: super::preview_service::PreviewAcquisition,
) -> PreviewCompletion {
    let trace = crate::run_report::TraceCollector::new();
    let breakpoint = if debug {
        draft.debug_breakpoint.clone()
    } else {
        None
    };
    let hook = PreviewDebugHook {
        events,
        commands,
        cancelled: Arc::clone(&cancelled),
        pause_each_write: std::cell::Cell::new(
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
        pause_next_node: std::cell::Cell::new(false),
        pause_next_input: std::cell::Cell::new(false),
        breakpoint,
        value_condition,
        position_condition,
        source_condition,
        node_condition,
        expression_condition,
        input_condition,
        pause_on_failure,
    };
    let result = match acquisition.source {
        super::preview_service::PreviewInputSource::EditorText => run_preview_payload(
            &project,
            &draft,
            &project_path,
            saved_path.as_deref(),
            &runtime_parameters,
            &trace,
            &hook,
            &cancelled,
        ),
        super::preview_service::PreviewInputSource::ConfiguredFiles => run_preview_files(
            &project,
            &draft,
            &project_path,
            &runtime_parameters,
            &trace,
            &hook,
            &cancelled,
            &acquisition,
        ),
    }
    .map_err(|error| {
        if error.chain().any(|cause| {
            matches!(
                cause.downcast_ref::<engine::EngineError>(),
                Some(engine::EngineError::DebugCancelled)
            )
        }) {
            PreviewRunError::Cancelled
        } else {
            PreviewRunError::Failed(format!("{error:#}"))
        }
    });
    (result, trace.finish())
}

#[allow(clippy::too_many_arguments)]
fn run_preview_payload(
    project: &Project,
    draft: &PreviewDraft,
    project_path: &std::path::Path,
    saved_path: Option<&std::path::Path>,
    runtime_parameters: &engine::RuntimeParameters,
    trace: &crate::run_report::TraceCollector,
    debug_hook: &dyn engine::DebugHook,
    cancelled: &AtomicBool,
) -> anyhow::Result<cli::PayloadRunOutcome> {
    if !draft.can_execute() {
        bail!("preview input or logical format identity is invalid");
    }
    if cancelled.load(Ordering::Acquire) {
        return Err(engine::EngineError::DebugCancelled.into());
    }
    let loaded_sources = crate::preview::load_required_sources(project, saved_path, &draft.target)?;
    if cancelled.load(Ordering::Acquire) {
        return Err(engine::EngineError::DebugCancelled.into());
    }
    let named_inputs = named_payload_inputs(&loaded_sources)?;
    let input_path = PathBuf::from(draft.input_identity.trim());
    let output_path = PathBuf::from(draft.output_identity.trim());
    let primary = cli::PayloadDocument::new(&input_path, draft.input_text.as_bytes())?;
    let options = cli::PayloadRunOptions::new(primary)
        .with_execution_purpose(engine::ExecutionPurpose::Preview)
        .with_extra_sources(&named_inputs)
        .with_output_path(&output_path)
        .with_target(draft.target.selection())
        .with_runtime_parameters(runtime_parameters)
        .with_trace_sink(trace)
        .with_debug_hook(debug_hook);
    cli::run_project_value_payloads(project, project_path, &options)
}

/// Previews the project's configured filesystem inputs. The primary is the
/// draft's input path, or the stored source path when they are equal; only
/// selected static sources and requested dynamic sources are read, and an
/// opted-in owned reader extracts PDFs on this thread.
#[allow(clippy::too_many_arguments)]
fn run_preview_files(
    project: &Project,
    draft: &PreviewDraft,
    project_path: &std::path::Path,
    runtime_parameters: &engine::RuntimeParameters,
    trace: &crate::run_report::TraceCollector,
    debug_hook: &dyn engine::DebugHook,
    cancelled: &AtomicBool,
    acquisition: &super::preview_service::PreviewAcquisition,
) -> anyhow::Result<cli::PayloadRunOutcome> {
    if !draft.can_execute() {
        bail!("preview input or output identity is invalid");
    }
    let input = draft.input_identity.trim();
    let stored = project.source_path.as_deref().unwrap_or_default().trim();
    let input_path = (input != stored).then(|| PathBuf::from(input));
    let output_path = PathBuf::from(draft.output_identity.trim());
    let mut options = cli::RunOptions::new()
        .with_target(draft.target.selection())
        .with_output_path(&output_path)
        .with_runtime_parameters(runtime_parameters)
        .with_trace_sink(trace)
        .with_debug_hook(debug_hook)
        .with_cancellation(cancelled);
    if let Some(path) = input_path.as_deref() {
        options = options.with_input_path(path);
    }
    let reader = acquisition.reader(cancelled);
    cli::preview_project_value(
        project,
        project_path,
        cli::ProjectPreviewInputs::Files {
            options: &options,
            pdf_reader: reader
                .as_ref()
                .map(|reader| reader as &dyn cli::FilesystemPdfReader),
        },
    )
}

struct PreviewDebugHook {
    events: Sender<PreviewWorkerEvent>,
    commands: Receiver<PreviewCommand>,
    cancelled: Arc<AtomicBool>,
    pause_each_write: std::cell::Cell<bool>,
    pause_next_node: std::cell::Cell<bool>,
    pause_next_input: std::cell::Cell<bool>,
    breakpoint: Option<PreviewBreakpoint>,
    value_condition: Option<DebugScalarCondition>,
    position_condition: Option<DebugPositionCondition>,
    source_condition: Option<DebugSourceCondition>,
    node_condition: Option<DebugNodeCondition>,
    expression_condition: Option<DebugExpressionCondition>,
    input_condition: Option<DebugInputCondition>,
    pause_on_failure: bool,
}

impl PreviewDebugHook {
    fn after_failure(&self, event: PreviewWorkerEvent) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(PreviewCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(PreviewCommand::Step | PreviewCommand::Continue | PreviewCommand::Pause) => {}
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
                Ok(PreviewCommand::Continue) => return engine::DebugDecision::Resume,
                Ok(PreviewCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Ok(PreviewCommand::Pause | PreviewCommand::Step)
                | Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

impl engine::DebugHook for PreviewDebugHook {
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
        self.after_failure(PreviewWorkerEvent::PausedFailure(Box::new(failure.clone())))
    }

    fn after_function_node_failure(
        &self,
        failure: &engine::PendingFunctionNodeFailure,
    ) -> engine::DebugDecision {
        self.after_failure(PreviewWorkerEvent::PausedFunctionFailure(Box::new(
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
                Ok(PreviewCommand::Pause | PreviewCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(PreviewCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(PreviewCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Err(TryRecvError::Empty) => break,
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
            .send(PreviewWorkerEvent::Paused(Box::new(write.clone())))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(PreviewCommand::Step) => {
                    self.pause_each_write.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(PreviewCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(PreviewCommand::Pause) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(PreviewCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
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
                Ok(PreviewCommand::Pause | PreviewCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(PreviewCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(PreviewCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Err(TryRecvError::Empty) => break,
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
            .send(PreviewWorkerEvent::PausedNode(Box::new(value.clone())))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(PreviewCommand::Step) => {
                    self.pause_next_node.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(PreviewCommand::Continue) => {
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                    self.pause_each_write.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(PreviewCommand::Pause) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(PreviewCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
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
                Ok(PreviewCommand::Pause | PreviewCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(PreviewCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(PreviewCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Err(TryRecvError::Empty) => break,
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
            .send(PreviewWorkerEvent::PausedFunctionNode(Box::new(
                value.clone(),
            )))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(PreviewCommand::Step) => {
                    self.pause_next_node.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(PreviewCommand::Continue) => {
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                    self.pause_each_write.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(PreviewCommand::Pause) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(PreviewCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
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
                Ok(PreviewCommand::Pause | PreviewCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(PreviewCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(PreviewCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Err(TryRecvError::Empty) => break,
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
            .send(PreviewWorkerEvent::PausedInput(Box::new(input.clone())))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(PreviewCommand::Step) => {
                    self.pause_next_input.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(PreviewCommand::Continue) => {
                    self.pause_next_input.set(false);
                    self.pause_next_node.set(false);
                    self.pause_each_write.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(PreviewCommand::Pause) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(PreviewCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
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
                Ok(PreviewCommand::Pause | PreviewCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(PreviewCommand::Continue) => {
                    self.pause_each_write.set(false);
                    self.pause_next_node.set(false);
                    self.pause_next_input.set(false);
                }
                Ok(PreviewCommand::Cancel) | Err(TryRecvError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
                Err(TryRecvError::Empty) => break,
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
            .send(PreviewWorkerEvent::PausedFunctionInput(Box::new(
                input.clone(),
            )))
            .is_err()
        {
            return engine::DebugDecision::Cancel;
        }
        loop {
            if self.cancelled.load(Ordering::Acquire) {
                return engine::DebugDecision::Cancel;
            }
            match self.commands.recv_timeout(Duration::from_millis(100)) {
                Ok(PreviewCommand::Step) => {
                    self.pause_next_input.set(true);
                    return engine::DebugDecision::Resume;
                }
                Ok(PreviewCommand::Continue) => {
                    self.pause_next_input.set(false);
                    self.pause_next_node.set(false);
                    self.pause_each_write.set(false);
                    return engine::DebugDecision::Resume;
                }
                Ok(PreviewCommand::Pause) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Ok(PreviewCommand::Cancel) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                    return engine::DebugDecision::Cancel;
                }
            }
        }
    }
}

fn named_payload_inputs<'a>(
    sources: &'a [LoadedPreviewSource],
) -> anyhow::Result<Vec<cli::NamedPayloadInput<'a>>> {
    sources
        .iter()
        .map(|source| {
            let document = cli::PayloadDocument::new(&source.path, &source.bytes)?;
            cli::NamedPayloadInput::new(&source.name, document)
        })
        .collect()
}
