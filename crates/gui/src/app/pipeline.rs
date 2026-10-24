use super::owned_worker::{OwnedWorker, WorkerExit};
use super::*;

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use crate::preview::{
    DebugExpressionCondition, DebugInputCondition, DebugNodeCondition, DebugPositionCondition,
    DebugScalarCondition, DebugSourceCondition, PreviewBreakpoint, PreviewTarget,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PipelineBreakpoint {
    pub(super) stage: String,
    pub(super) target: engine::TraceTarget,
    pub(super) field: PreviewBreakpoint,
}

impl PipelineBreakpoint {
    fn label(&self) -> String {
        let target = match &self.target {
            engine::TraceTarget::Primary => "Primary",
            engine::TraceTarget::Named(name) => name,
        };
        format!("{} / {target} / {}", self.stage, self.field.label())
    }

    fn matches(&self, stage: &str, write: &engine::PendingTargetWrite) -> bool {
        self.stage == stage && self.target == write.scope.target && self.field.matches(write)
    }
}

pub(super) fn breakpoint_candidates(pipeline: &mapping::Pipeline) -> Vec<PipelineBreakpoint> {
    const MAX_CANDIDATES: usize = 512;
    let mut result = Vec::new();
    for stage in &pipeline.stages {
        for field in crate::preview::breakpoint_candidates(&stage.project, &PreviewTarget::Primary)
        {
            result.push(PipelineBreakpoint {
                stage: stage.id.clone(),
                target: engine::TraceTarget::Primary,
                field,
            });
            if result.len() >= MAX_CANDIDATES {
                return result;
            }
        }
        for target in &stage.project.extra_targets {
            for field in crate::preview::breakpoint_candidates(
                &stage.project,
                &PreviewTarget::Named(target.name.clone()),
            ) {
                result.push(PipelineBreakpoint {
                    stage: stage.id.clone(),
                    target: engine::TraceTarget::Named(target.name.clone()),
                    field,
                });
                if result.len() >= MAX_CANDIDATES {
                    return result;
                }
            }
        }
    }
    result
}

#[derive(Clone, Copy)]
pub(super) enum PipelineRunCommand {
    Step,
    Continue,
    Pause,
    Cancel,
    Publish,
}

enum PipelineRunEvent {
    Paused(String, Box<engine::PendingTargetWrite>),
    PausedNode(String, Box<engine::PendingNodeValue>),
    PausedFunctionNode(String, Box<engine::PendingFunctionNodeValue>),
    PausedInput(String, Box<engine::PendingNodeInput>),
    PausedFunctionInput(String, Box<engine::PendingFunctionNodeInput>),
    PausedFailure(String, Box<engine::PendingNodeFailure>),
    PausedFunctionFailure(String, Box<engine::PendingFunctionNodeFailure>),
    ReadyToPublish,
}

/// What the pipeline worker returns when its thread finishes.
type PipelineRunResult = (
    Result<PipelineCompletion, PipelineRunError>,
    crate::run_report::TraceReport,
);

enum PipelineCompletion {
    Run(cli::PipelineRunOutcome),
    Preview(cli::PipelinePreviewOutcome),
}

enum PipelineRunError {
    Cancelled,
    Failed(String),
}

#[derive(Clone, Debug)]
pub(super) enum PipelineRunPhase {
    Running,
    Paused(String, Box<engine::PendingTargetWrite>),
    PausedNode(String, Box<engine::PendingNodeValue>),
    PausedFunctionNode(String, Box<engine::PendingFunctionNodeValue>),
    PausedInput(String, Box<engine::PendingNodeInput>),
    PausedFunctionInput(String, Box<engine::PendingFunctionNodeInput>),
    PausedFailure(String, Box<engine::PendingNodeFailure>),
    PausedFunctionFailure(String, Box<engine::PendingFunctionNodeFailure>),
    Publishing,
    Stopping,
}

/// A pipeline run or preview owned by its exact worker thread. Pause and
/// publication events arrive on `receiver`; the outcome is reported only
/// after that thread was joined.
pub(super) struct PendingPipelineRun {
    worker: OwnedWorker<PipelineRunResult>,
    receiver: Receiver<PipelineRunEvent>,
    commands: Sender<PipelineRunCommand>,
    cancelled: Arc<AtomicBool>,
    started: Instant,
    path: PathBuf,
    debug: bool,
    preview: bool,
    pub(super) phase: PipelineRunPhase,
    /// The opted-in owned PDF extraction's latest state, if any.
    pub(super) extraction: Option<Arc<super::pdf_extraction::ExtractionStatus>>,
    /// Set for previews that may own a PDF worker: dropping this owner hands
    /// the exact thread to the preview service instead of blocking on it.
    pub(super) service: Option<super::PreviewServicePort>,
}

impl PendingPipelineRun {
    fn command(&mut self, command: PipelineRunCommand) {
        if matches!(
            self.phase,
            PipelineRunPhase::Publishing | PipelineRunPhase::Stopping
        ) {
            return;
        }
        match command {
            PipelineRunCommand::Step | PipelineRunCommand::Continue => {
                self.phase = PipelineRunPhase::Running;
            }
            PipelineRunCommand::Pause => {}
            PipelineRunCommand::Cancel => {
                self.cancelled.store(true, Ordering::Release);
                self.phase = PipelineRunPhase::Stopping;
            }
            PipelineRunCommand::Publish => return,
        }
        let _ = self.commands.send(command);
    }
}

impl Drop for PendingPipelineRun {
    /// Cancels unless outputs are already being published, then drains the
    /// exact worker: the `worker` field is dropped after this runs, while the
    /// command channel is still connected.
    fn drop(&mut self) {
        if !matches!(self.phase, PipelineRunPhase::Publishing) {
            self.cancelled.store(true, Ordering::Release);
            let _ = self.commands.send(PipelineRunCommand::Cancel);
        }
        // A finished thread is joined at once by the field's own drop.
        if let Some(service) = &self.service
            && !self.worker.is_finished()
        {
            service.retain(Box::new(self.worker.take_ownership()));
        }
    }
}

#[cfg(test)]
impl PendingPipelineRun {
    /// Every owner of the cancellation flag, including the worker's own.
    pub(super) fn cancellation_for_test(&self) -> std::sync::Weak<AtomicBool> {
        Arc::downgrade(&self.cancelled)
    }
}

impl FerruleApp {
    pub(super) fn load_pipeline_for_run(&mut self, path: &Path) {
        if self.stage_document_actions_blocked()
            || self.pipeline_editor.as_ref().is_some_and(|editor| {
                editor.document.path == path
                    && (editor.document.is_dirty() || editor.has_unapplied_text())
            })
        {
            return;
        }
        match crate::pipeline_run::PipelineRunDraft::load(path) {
            Ok(draft) => {
                self.status = format!("inspecting pipeline {}", path.display());
                self.pipeline_run_draft = Some(draft);
            }
            Err(error) => {
                self.status = "failed to open pipeline".into();
                self.diagnostics
                    .error("Pipeline open failed", format!("{error:#}"));
            }
        }
    }

    #[cfg(test)]
    pub(super) fn start_pipeline_run(&mut self) {
        self.start_pipeline_execution_mode(false, false);
    }

    #[cfg(test)]
    pub(super) fn start_pipeline_debug_run(&mut self) {
        self.start_pipeline_execution_mode(false, true);
    }

    #[cfg(test)]
    pub(super) fn start_pipeline_preview(&mut self) {
        self.start_pipeline_execution_mode(true, false);
    }

    #[cfg(test)]
    pub(super) fn start_pipeline_debug_preview(&mut self) {
        self.start_pipeline_execution_mode(true, true);
    }

    fn start_pipeline_execution_mode(&mut self, preview: bool, debug: bool) {
        if self.rest_run_busy() {
            return;
        }
        if self.pending_pipeline_run.is_some()
            || self.pending_file_run.is_some()
            || self.pending_preview.is_some()
        {
            return;
        }
        let Some(draft) = self.pipeline_run_draft.as_ref() else {
            return;
        };
        let runtime_parameters = match draft.host_parameters.compile() {
            Ok(parameters) => parameters,
            Err(error) => {
                let title = if preview {
                    "Pipeline preview blocked"
                } else {
                    "Pipeline run blocked"
                };
                self.status = title.to_lowercase();
                self.diagnostics.error(title, error);
                return;
            }
        };
        let value_condition = if debug {
            match self.pipeline_run_value_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "debug pipeline blocked".into();
                    self.diagnostics.error("Debug pipeline blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let position_condition = if debug {
            match self.pipeline_run_position_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "debug pipeline blocked".into();
                    self.diagnostics.error("Debug pipeline blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let source_condition = if debug {
            match self.pipeline_run_source_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "debug pipeline blocked".into();
                    self.diagnostics.error("Debug pipeline blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let node_condition = if debug {
            match self.pipeline_run_node_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "debug pipeline blocked".into();
                    self.diagnostics.error("Debug pipeline blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let expression_condition = if debug {
            match self.pipeline_run_expression_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "debug pipeline blocked".into();
                    self.diagnostics.error("Debug pipeline blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let input_condition = if debug {
            match self.pipeline_run_input_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "debug pipeline blocked".into();
                    self.diagnostics.error("Debug pipeline blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let Some(draft) = self.pipeline_run_draft.as_ref() else {
            return;
        };
        let prepared = draft.ensure_unchanged().and_then(|()| {
            if preview {
                let inputs = draft.input_requests()?;
                let identities = draft
                    .preview_paths()?
                    .into_iter()
                    .map(|(stage, target, path)| cli::PipelinePreviewOutputIdentity {
                        stage,
                        target,
                        path,
                    })
                    .collect();
                Ok((inputs, Vec::new(), identities))
            } else {
                let (inputs, outputs) = draft.requests()?;
                Ok((inputs, outputs, Vec::new()))
            }
        });
        let (inputs, outputs, preview_identities) = match prepared {
            Ok(requests) => requests,
            Err(error) => {
                let title = if preview {
                    "Pipeline preview blocked"
                } else {
                    "Pipeline run blocked"
                };
                self.status = title.to_lowercase();
                self.diagnostics.error(title, format!("{error:#}"));
                return;
            }
        };
        let path = draft.path.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        // Capability preflight, before any host input is acquired.
        let preflight = if preview {
            super::route_preflight::pipeline_preview(
                &draft.pipeline,
                &path,
                &inputs,
                &preview_identities,
                &runtime_parameters,
                &cancelled,
            )
        } else {
            super::route_preflight::pipeline_run(
                &path,
                &inputs,
                &outputs,
                &runtime_parameters,
                &cancelled,
            )
        };
        if let Err(error) = preflight {
            let title = if preview {
                "Pipeline preview blocked"
            } else {
                "Pipeline run blocked"
            };
            self.status = title.to_lowercase();
            self.diagnostics.error(title, format!("{error:#}"));
            return;
        }
        let worker_path = path.clone();
        let worker_pipeline = preview.then(|| Arc::clone(&draft.pipeline));
        let breakpoint = if debug {
            self.pipeline_run_breakpoint
                .clone()
                .filter(|choice| breakpoint_candidates(&draft.pipeline).contains(choice))
        } else {
            None
        };
        let expression_stage = self
            .pipeline_run_expression_stage
            .clone()
            .filter(|selected| {
                draft
                    .pipeline
                    .stages
                    .iter()
                    .any(|stage| stage.id == selected.as_str())
            });
        let input_stage = self.pipeline_run_input_stage.clone().filter(|selected| {
            draft
                .pipeline
                .stages
                .iter()
                .any(|stage| stage.id == selected.as_str())
        });
        let failure_stage = self.pipeline_run_failure_stage.clone().filter(|selected| {
            draft
                .pipeline
                .stages
                .iter()
                .any(|stage| stage.id == selected.as_str())
        });
        let pause_on_failure = debug && self.pipeline_run_pause_on_failure;
        let (events, receiver) = mpsc::channel();
        let (commands, command_receiver) = mpsc::channel();
        let worker_cancelled = Arc::clone(&cancelled);
        let extraction = self
            .owned_pdf_worker
            .then(|| Arc::new(super::pdf_extraction::ExtractionStatus::default()));
        let pdf_worker = extraction
            .as_ref()
            .map(|status| (self.pdf_worker_package.clone(), Arc::clone(status)));
        #[cfg(test)]
        let reader_controls = self.pdf_reader_controls;
        let service = (preview && extraction.is_some()).then(|| self.preview_service.clone());
        let worker = OwnedWorker::spawn("ferrule-pipeline-run", move || {
            let trace = crate::run_report::PipelineTraceCollector::new();
            let hook = PipelineRunHook {
                events,
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
                expression_stage,
                input_condition,
                input_stage,
                pause_on_failure,
                failure_stage,
            };
            // Owned by this thread; each read returns only after its
            // extraction is closed, so the thread outlives any worker child.
            let reader = pdf_worker.as_ref().map(|(package, status)| {
                let reader =
                    super::pdf_extraction::OwnedPdfReader::new(package, &hook.cancelled, status);
                #[cfg(test)]
                let reader = reader.with_controls(reader_controls);
                reader
            });
            let stage_hook = |stage: &str, write: &engine::PendingTargetWrite| {
                hook.before_target_write(stage, write)
            };
            let stage_node_hook =
                |stage: &str, value: &engine::PendingNodeValue| hook.after_node_value(stage, value);
            let stage_function_node_hook =
                |stage: &str, value: &engine::PendingFunctionNodeValue| {
                    hook.after_function_node_value(stage, value)
                };
            let stage_input_hook =
                |stage: &str, input: &engine::PendingNodeInput| hook.after_node_input(stage, input);
            let stage_function_input_hook =
                |stage: &str, input: &engine::PendingFunctionNodeInput| {
                    hook.after_function_node_input(stage, input)
                };
            let stage_node_failure_hook = |stage: &str, failure: &engine::PendingNodeFailure| {
                hook.after_node_failure(stage, failure)
            };
            let stage_function_failure_hook =
                |stage: &str, failure: &engine::PendingFunctionNodeFailure| {
                    hook.after_function_node_failure(stage, failure)
                };
            let source_probe = |_stage: &str| hook.source_field_probe();
            let stage_trace = |stage: &str, event: cli::TraceEvent| trace.record(stage, event);
            let source_row_capacity = || trace.can_retain_event();
            let gate = || hook.before_publish();
            // Cancellation and the publication gate need no stage hook; only
            // a debug run attaches pause and breakpoint callbacks.
            let mut options = cli::PipelineRunOptions::default()
                .with_stage_trace_sink(&stage_trace)
                .with_stage_trace_source_row_capacity(&source_row_capacity)
                .with_cancellation(hook.cancelled.as_ref())
                .with_before_publish(&gate);
            options.runtime_parameters = Some(&runtime_parameters);
            if debug {
                options = options
                    .with_stage_debug_hook(&stage_hook)
                    .with_stage_source_field_probe(&source_probe);
            }
            if hook.expression_condition.is_some() {
                options = options
                    .with_stage_node_debug_hook(&stage_node_hook)
                    .with_stage_function_node_debug_hook(&stage_function_node_hook);
            }
            if hook.input_condition.is_some() {
                options = options
                    .with_stage_input_debug_hook(&stage_input_hook)
                    .with_stage_function_input_debug_hook(&stage_function_input_hook);
            }
            if hook.pause_on_failure {
                options = options
                    .with_stage_node_failure_debug_hook(&stage_node_failure_hook)
                    .with_stage_function_failure_debug_hook(&stage_function_failure_hook);
            }
            let result: anyhow::Result<PipelineCompletion> = if preview {
                (|| {
                    // With the owned worker opted in, hosts are acquired once
                    // each from their files; otherwise they become payloads.
                    let loaded = match reader {
                        Some(_) => Vec::new(),
                        None => crate::pipeline_run::load_preview_hosts(&inputs, || {
                            hook.cancelled.load(Ordering::Acquire)
                        })?,
                    };
                    let hosts = loaded
                        .iter()
                        .map(|host| {
                            let document = cli::PayloadDocument::new(&host.path, &host.bytes)?;
                            cli::PipelineHostPayload::new(&host.name, document)
                        })
                        .collect::<anyhow::Result<Vec<_>>>()?;
                    let preview_options = match &reader {
                        Some(reader) => {
                            cli::PipelinePreviewOptions::from_files_with_pdf_reader(&inputs, reader)
                        }
                        None => cli::PipelinePreviewOptions::new(&hosts),
                    };
                    let mut preview_options = preview_options
                        .with_output_identities(&preview_identities)
                        .with_runtime_parameters(&runtime_parameters)
                        .with_stage_trace_sink(&stage_trace)
                        .with_stage_trace_source_row_capacity(&source_row_capacity)
                        .with_cancellation(hook.cancelled.as_ref());
                    if debug {
                        preview_options = preview_options
                            .with_stage_debug_hook(&stage_hook)
                            .with_stage_source_field_probe(&source_probe);
                    }
                    if hook.expression_condition.is_some() {
                        preview_options = preview_options
                            .with_stage_node_debug_hook(&stage_node_hook)
                            .with_stage_function_node_debug_hook(&stage_function_node_hook);
                    }
                    if hook.input_condition.is_some() {
                        preview_options = preview_options
                            .with_stage_input_debug_hook(&stage_input_hook)
                            .with_stage_function_input_debug_hook(&stage_function_input_hook);
                    }
                    if hook.pause_on_failure {
                        preview_options = preview_options
                            .with_stage_node_failure_debug_hook(&stage_node_failure_hook)
                            .with_stage_function_failure_debug_hook(&stage_function_failure_hook);
                    }
                    let outcome = cli::preview_pipeline_value_payloads(
                        worker_pipeline.as_ref().expect("preview pipeline snapshot"),
                        &worker_path,
                        &preview_options,
                    )?;
                    if hook.cancelled.load(Ordering::Acquire) {
                        return Err(engine::EngineError::DebugCancelled.into());
                    }
                    Ok(PipelineCompletion::Preview(outcome))
                })()
            } else {
                match &reader {
                    Some(reader) => cli::run_pipeline_file_with_options_and_pdf_reader(
                        &worker_path,
                        &inputs,
                        &outputs,
                        &options,
                        reader,
                    ),
                    None => cli::run_pipeline_file_with_options(
                        &worker_path,
                        &inputs,
                        &outputs,
                        &options,
                    ),
                }
                .map(PipelineCompletion::Run)
            };
            let result = result.map_err(|error| {
                if error.chain().any(|cause| {
                    matches!(
                        cause.downcast_ref::<engine::EngineError>(),
                        Some(engine::EngineError::DebugCancelled)
                    )
                }) {
                    PipelineRunError::Cancelled
                } else {
                    PipelineRunError::Failed(format!("{error:#}"))
                }
            });
            (result, trace.finish())
        });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                let title = if preview {
                    "Pipeline preview failed"
                } else {
                    "Pipeline run failed"
                };
                self.status = title.to_lowercase();
                self.diagnostics.error(
                    title,
                    format!("Could not start the pipeline worker: {error}"),
                );
                return;
            }
        };
        self.pending_pipeline_run = Some(PendingPipelineRun {
            worker,
            receiver,
            commands,
            cancelled,
            started: Instant::now(),
            path,
            debug,
            preview,
            phase: PipelineRunPhase::Running,
            extraction,
            service,
        });
        self.run_report = None;
        self.show_run_report = false;
        self.status = match (preview, debug) {
            (true, true) => "debug pipeline preview running",
            (true, false) => "previewing pipeline",
            (false, true) => "debug pipeline running",
            (false, false) => "running pipeline",
        }
        .into();
        self.diagnostics.clear();
    }

    pub(super) fn pipeline_run_command(&mut self, command: PipelineRunCommand) {
        if let Some(pending) = &mut self.pending_pipeline_run {
            pending.command(command);
            self.status = match (pending.preview, command) {
                (true, PipelineRunCommand::Cancel) => "stopping pipeline preview",
                (false, PipelineRunCommand::Cancel) => "stopping pipeline",
                (true, PipelineRunCommand::Pause) => {
                    "pausing pipeline preview at next target write"
                }
                (false, PipelineRunCommand::Pause) => "pausing pipeline at next target write",
                (true, _) => "debug pipeline preview running",
                (false, _) => "debug pipeline running",
            }
            .into();
        }
    }

    pub(super) fn poll_pipeline_run(&mut self, ctx: &egui::Context) {
        let Some(pending) = &mut self.pending_pipeline_run else {
            return;
        };
        // Pause and publication events are relayed only while the exact
        // worker runs; its outcome is the joined thread, never a message or a
        // disconnected channel.
        let event = if pending.worker.is_finished() {
            None
        } else {
            pending.receiver.try_recv().ok()
        };
        let Some(event) = event else {
            match pending.worker.poll() {
                Some(exit) => self.finish_pipeline_run(exit, ctx),
                None => {
                    if !matches!(
                        pending.phase,
                        PipelineRunPhase::Paused(..)
                            | PipelineRunPhase::PausedNode(..)
                            | PipelineRunPhase::PausedFunctionNode(..)
                            | PipelineRunPhase::PausedInput(..)
                            | PipelineRunPhase::PausedFunctionInput(..)
                            | PipelineRunPhase::PausedFailure(..)
                            | PipelineRunPhase::PausedFunctionFailure(..)
                    ) {
                        ctx.request_repaint_after(Duration::from_millis(100));
                    }
                }
            }
            return;
        };
        match event {
            PipelineRunEvent::Paused(stage, write) => {
                if matches!(pending.phase, PipelineRunPhase::Stopping) {
                    let _ = pending.commands.send(PipelineRunCommand::Cancel);
                } else {
                    self.status = format!(
                        "paused in stage `{stage}` before target field `{}`",
                        write.field
                    );
                    pending.phase = PipelineRunPhase::Paused(stage, write);
                }
                ctx.request_repaint();
            }
            PipelineRunEvent::PausedNode(stage, value) => {
                if matches!(pending.phase, PipelineRunPhase::Stopping) {
                    let _ = pending.commands.send(PipelineRunCommand::Cancel);
                } else {
                    self.status =
                        format!("paused in stage `{stage}` after graph node #{}", value.node);
                    pending.phase = PipelineRunPhase::PausedNode(stage, value);
                }
                ctx.request_repaint();
            }
            PipelineRunEvent::PausedFunctionNode(stage, value) => {
                if matches!(pending.phase, PipelineRunPhase::Stopping) {
                    let _ = pending.commands.send(PipelineRunCommand::Cancel);
                } else {
                    self.status = format!(
                        "paused in stage `{stage}` after function #{} node #{}",
                        value.function.get(),
                        value.node
                    );
                    pending.phase = PipelineRunPhase::PausedFunctionNode(stage, value);
                }
                ctx.request_repaint();
            }
            PipelineRunEvent::PausedInput(stage, input) => {
                if matches!(pending.phase, PipelineRunPhase::Stopping) {
                    let _ = pending.commands.send(PipelineRunCommand::Cancel);
                } else {
                    self.status = format!(
                        "paused in stage `{stage}` at node #{} input #{}",
                        input.consumer,
                        input.input_index + 1
                    );
                    pending.phase = PipelineRunPhase::PausedInput(stage, input);
                }
                ctx.request_repaint();
            }
            PipelineRunEvent::PausedFunctionInput(stage, input) => {
                if matches!(pending.phase, PipelineRunPhase::Stopping) {
                    let _ = pending.commands.send(PipelineRunCommand::Cancel);
                } else {
                    self.status = format!(
                        "paused in stage `{stage}` at function #{} node #{} input #{}",
                        input.function.get(),
                        input.consumer,
                        input.input_index + 1
                    );
                    pending.phase = PipelineRunPhase::PausedFunctionInput(stage, input);
                }
                ctx.request_repaint();
            }
            PipelineRunEvent::PausedFailure(stage, failure) => {
                if matches!(pending.phase, PipelineRunPhase::Stopping) {
                    let _ = pending.commands.send(PipelineRunCommand::Cancel);
                } else {
                    self.status = format!(
                        "paused in stage `{stage}` on graph node #{} error",
                        failure.node
                    );
                    pending.phase = PipelineRunPhase::PausedFailure(stage, failure);
                }
                ctx.request_repaint();
            }
            PipelineRunEvent::PausedFunctionFailure(stage, failure) => {
                if matches!(pending.phase, PipelineRunPhase::Stopping) {
                    let _ = pending.commands.send(PipelineRunCommand::Cancel);
                } else {
                    self.status = format!(
                        "paused in stage `{stage}` on function #{} node #{} error",
                        failure.function.get(),
                        failure.node
                    );
                    pending.phase = PipelineRunPhase::PausedFunctionFailure(stage, failure);
                }
                ctx.request_repaint();
            }
            PipelineRunEvent::ReadyToPublish => {
                debug_assert!(!pending.preview, "preview cannot publish outputs");
                if matches!(pending.phase, PipelineRunPhase::Stopping) {
                    let _ = pending.commands.send(PipelineRunCommand::Cancel);
                } else {
                    pending.phase = PipelineRunPhase::Publishing;
                    self.status = "publishing pipeline outputs".into();
                    let _ = pending.commands.send(PipelineRunCommand::Publish);
                }
                ctx.request_repaint();
            }
        }
    }

    /// Applies the outcome of a joined pipeline worker.
    fn finish_pipeline_run(&mut self, exit: WorkerExit<PipelineRunResult>, ctx: &egui::Context) {
        let Some(pending) = self.pending_pipeline_run.take() else {
            return;
        };
        let (result, trace) = match exit {
            WorkerExit::Reported(completion) => completion,
            WorkerExit::Unreported | WorkerExit::Panicked => (
                Err(PipelineRunError::Failed(
                    "pipeline worker stopped unexpectedly".into(),
                )),
                crate::run_report::TraceReport::default(),
            ),
        };
        if matches!(pending.phase, PipelineRunPhase::Stopping)
            || matches!(result, Err(PipelineRunError::Cancelled))
        {
            self.status = if pending.preview {
                "pipeline preview cancelled"
            } else {
                "pipeline cancelled"
            }
            .into();
            self.diagnostics.clear();
            self.finish_deferred_pipeline_close(ctx);
            return;
        }
        match result {
            Ok(PipelineCompletion::Run(outcome)) => {
                self.status = format!(
                    "pipeline completed {} stage(s) and wrote {} file(s)",
                    outcome.stages_executed.len(),
                    outcome.artifacts.len()
                );
                let report = crate::run_report::RunReport::from_pipeline_outcome(
                    outcome,
                    pending.path.clone(),
                    pending.started.elapsed(),
                    trace,
                );
                self.run_report = Some(crate::run_report::RunReportView::new(report));
                self.show_run_report = true;
                self.pipeline_run_draft = None;
            }
            Ok(PipelineCompletion::Preview(outcome)) => {
                self.status = format!(
                    "pipeline preview completed {} stage(s) and {} artifact(s)",
                    outcome.stages_executed.len(),
                    outcome.artifacts.len()
                );
                let report = crate::run_report::RunReport::from_pipeline_preview_outcome(
                    outcome,
                    pending.path.clone(),
                    pending.started.elapsed(),
                    trace,
                );
                self.run_report = Some(crate::run_report::RunReportView::new(report));
                self.show_run_report = true;
                self.pipeline_run_draft = None;
            }
            Err(PipelineRunError::Failed(error)) => {
                let title = if pending.preview {
                    "Pipeline preview failed"
                } else {
                    "Pipeline run failed"
                };
                self.status = title.to_lowercase();
                self.diagnostics.error(title, error);
            }
            Err(PipelineRunError::Cancelled) => {}
        }
        self.finish_deferred_pipeline_close(ctx);
    }

    fn finish_deferred_pipeline_close(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.close_after_pipeline_run) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

#[path = "pipeline_run_hook.rs"]
mod pipeline_run_hook;
use pipeline_run_hook::PipelineRunHook;

#[path = "pipeline_run_window.rs"]
mod pipeline_run_window;

#[cfg(test)]
#[path = "pipeline_worker_tests.rs"]
mod pipeline_worker_tests;
