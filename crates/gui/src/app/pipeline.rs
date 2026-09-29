use super::*;

use std::cell::Cell;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use crate::preview::{
    DebugPositionCondition, DebugScalarCondition, DebugSourceCondition, PreviewBreakpoint,
    PreviewTarget,
};
use mapping::PipelineInput;

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
    ReadyToPublish,
    Finished(Result<cli::PipelineRunOutcome, PipelineRunError>),
}

enum PipelineRunError {
    Cancelled,
    Failed(String),
}

#[derive(Clone, Debug)]
pub(super) enum PipelineRunPhase {
    Running,
    Paused(String, Box<engine::PendingTargetWrite>),
    Publishing,
    Stopping,
}

pub(super) struct PendingPipelineRun {
    receiver: Receiver<PipelineRunEvent>,
    commands: Sender<PipelineRunCommand>,
    cancelled: Arc<AtomicBool>,
    started: Instant,
    path: PathBuf,
    debug: bool,
    pub(super) phase: PipelineRunPhase,
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
    fn drop(&mut self) {
        if !matches!(self.phase, PipelineRunPhase::Publishing) {
            self.cancelled.store(true, Ordering::Release);
            let _ = self.commands.send(PipelineRunCommand::Cancel);
        }
    }
}

struct PipelineRunHook {
    events: Sender<PipelineRunEvent>,
    commands: Receiver<PipelineRunCommand>,
    cancelled: Arc<AtomicBool>,
    pause_each_write: Cell<bool>,
    breakpoint: Option<PipelineBreakpoint>,
    value_condition: Option<DebugScalarCondition>,
    position_condition: Option<DebugPositionCondition>,
    source_condition: Option<DebugSourceCondition>,
}

impl PipelineRunHook {
    fn source_field_probe(&self) -> Option<(usize, String)> {
        self.source_condition
            .as_ref()
            .map(DebugSourceCondition::probe)
    }

    fn before_target_write(
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
                Ok(PipelineRunCommand::Continue) => self.pause_each_write.set(false),
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
        let has_breakpoint = self.breakpoint.is_some()
            || self.value_condition.is_some()
            || self.position_condition.is_some()
            || self.source_condition.is_some();
        if !self.pause_each_write.get()
            && !(has_breakpoint
                && matches_selection
                && matches_value
                && matches_position
                && matches_source)
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

    fn before_publish(&self) -> bool {
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

impl FerruleApp {
    pub(super) fn load_pipeline_for_run(&mut self, path: &Path) {
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

    pub(super) fn show_pipeline_run_setup(&mut self, ctx: &egui::Context) {
        let Some(draft) = &mut self.pipeline_run_draft else {
            return;
        };
        let phase = self
            .pending_pipeline_run
            .as_ref()
            .map(|run| run.phase.clone());
        let debug = self
            .pending_pipeline_run
            .as_ref()
            .is_some_and(|run| run.debug);
        let running = phase.is_some();
        let candidates = breakpoint_candidates(&draft.pipeline);
        if self
            .pipeline_run_breakpoint
            .as_ref()
            .is_some_and(|selected| !candidates.contains(selected))
        {
            self.pipeline_run_breakpoint = None;
        }
        let mut close = false;
        let mut run = None;
        let mut command = None;
        let mut condition_valid = true;
        egui::Window::new("Run pipeline")
            .default_width(650.0)
            .min_width(480.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.label(draft.path.display().to_string());
                ui.small("Input and output paths are relative to the pipeline file.");
                ui.separator();
                ui.strong("Stages");
                for stage in &draft.pipeline.stages {
                    ui.label(format!(
                        "{}  ←  {}",
                        stage.id,
                        input_label(&stage.source)
                    ));
                    for binding in &stage.extra_sources {
                        ui.indent(("pipeline_binding", &stage.id, &binding.name), |ui| {
                            ui.weak(format!(
                                "{}  ←  {}",
                                binding.name,
                                input_label(&binding.from)
                            ));
                        });
                    }
                }
                if !draft.issues.is_empty() {
                    ui.separator();
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("{} validation issue(s)", draft.issues.len()),
                    );
                    for issue in &draft.issues {
                        ui.label(format!("• {issue}"));
                    }
                }
                ui.separator();
                ui.strong("Host inputs");
                if draft.inputs.is_empty() {
                    ui.weak("No host files are required.");
                }
                egui::Grid::new("pipeline_host_inputs")
                    .num_columns(2)
                    .show(ui, |ui| {
                        for input in &mut draft.inputs {
                            ui.label(&input.name);
                            ui.add_enabled(
                                !running,
                                egui::TextEdit::singleline(&mut input.path)
                                    .hint_text("Input file path"),
                            );
                            ui.end_row();
                        }
                    });
                ui.separator();
                ui.strong("Publish outputs");
                ui.small("Leave an output blank to keep it in memory only. Dynamic document targets use a base directory.");
                egui::ScrollArea::vertical()
                    .max_height(210.0)
                    .show(ui, |ui| {
                        egui::Grid::new("pipeline_output_paths")
                            .num_columns(2)
                            .show(ui, |ui| {
                                for output in &mut draft.outputs {
                                    let label = match &output.target {
                                        None => format!("{} / Primary", output.stage),
                                        Some(name) => format!("{} / {name}", output.stage),
                                    };
                                    ui.label(label);
                                    ui.add_enabled(
                                        !running,
                                        egui::TextEdit::singleline(&mut output.path)
                                            .hint_text("Output path or base directory"),
                                    );
                                    ui.end_row();
                                }
                            });
                    });
                ui.separator();
                if !running {
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Debug breakpoint");
                        egui::ComboBox::from_id_salt("pipeline_debug_breakpoint")
                            .selected_text(self.pipeline_run_breakpoint.as_ref().map_or_else(
                                || "Every target-field write".to_owned(),
                                PipelineBreakpoint::label,
                            ))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut self.pipeline_run_breakpoint,
                                    None,
                                    "Every target-field write",
                                );
                                for candidate in candidates {
                                    ui.selectable_value(
                                        &mut self.pipeline_run_breakpoint,
                                        Some(candidate.clone()),
                                        candidate.label(),
                                    );
                                }
                            });
                    });
                    ui.weak("Breakpoints cover declared static fields in each stage and target.");
                    condition_valid = preview_ui::show_breakpoint_value_condition(
                        ui,
                        &mut self.pipeline_run_value_condition,
                        "pipeline_debug_value_type",
                    );
                    condition_valid &= preview_ui::show_breakpoint_position_condition(
                        ui,
                        &mut self.pipeline_run_position_condition,
                    );
                    condition_valid &= preview_ui::show_breakpoint_source_condition(
                        ui,
                        &mut self.pipeline_run_source_condition,
                        "pipeline_debug_source_type",
                    );
                }
                match &phase {
                    Some(PipelineRunPhase::Running) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.strong(if debug {
                                "Debug pipeline is running"
                            } else {
                                "Pipeline is running"
                            });
                        });
                    }
                    Some(PipelineRunPhase::Paused(stage, write)) => {
                        ui.strong(format!("Paused in stage `{stage}`"));
                        preview_ui::show_live_debug_state(
                            ui,
                            &preview_ui::PreviewPhase::Paused(write.clone()),
                            true,
                        );
                    }
                    Some(PipelineRunPhase::Publishing) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.strong("Publishing completed pipeline outputs");
                        });
                    }
                    Some(PipelineRunPhase::Stopping) => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.strong("Stopping pipeline");
                        });
                    }
                    None => {}
                }
                ui.separator();
                ui.horizontal(|ui| {
                    match &phase {
                        None => {
                            let can_run = draft.issues.is_empty()
                                && draft.inputs.iter().all(|input| !input.path.trim().is_empty())
                                && draft.outputs.iter().any(|output| !output.path.trim().is_empty());
                            if ui.add_enabled(can_run, egui::Button::new("Run pipeline")).clicked() {
                                run = Some(false);
                            }
                            if ui.add_enabled(can_run && condition_valid, egui::Button::new("Debug pipeline")).clicked() {
                                run = Some(true);
                            }
                        }
                        Some(PipelineRunPhase::Running) => {
                            if debug && ui.button("Pause at next write").clicked() {
                                command = Some(PipelineRunCommand::Pause);
                            }
                            if ui.button("Cancel pipeline").clicked() {
                                command = Some(PipelineRunCommand::Cancel);
                            }
                        }
                        Some(PipelineRunPhase::Paused(..)) => {
                            if ui.button("Step").clicked() {
                                command = Some(PipelineRunCommand::Step);
                            }
                            if ui.button("Continue").clicked() {
                                command = Some(PipelineRunCommand::Continue);
                            }
                            if ui.button("Cancel pipeline").clicked() {
                                command = Some(PipelineRunCommand::Cancel);
                            }
                        }
                        Some(PipelineRunPhase::Publishing | PipelineRunPhase::Stopping) => {}
                    }
                    if ui.add_enabled(!running, egui::Button::new("Close")).clicked() {
                        close = true;
                    }
                });
            });
        if close {
            self.pipeline_run_draft = None;
        } else if let Some(debug) = run {
            self.start_pipeline_run_mode(debug);
        } else if let Some(command) = command {
            self.pipeline_run_command(command);
        }
    }

    #[cfg(test)]
    pub(super) fn start_pipeline_run(&mut self) {
        self.start_pipeline_run_mode(false);
    }

    #[cfg(test)]
    pub(super) fn start_pipeline_debug_run(&mut self) {
        self.start_pipeline_run_mode(true);
    }

    fn start_pipeline_run_mode(&mut self, debug: bool) {
        if self.pending_pipeline_run.is_some() || self.pending_file_run.is_some() {
            return;
        }
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
        let Some(draft) = self.pipeline_run_draft.as_ref() else {
            return;
        };
        let prepared = draft.ensure_unchanged().and_then(|()| draft.requests());
        let (inputs, outputs) = match prepared {
            Ok(requests) => requests,
            Err(error) => {
                self.status = "pipeline run blocked".into();
                self.diagnostics
                    .error("Pipeline run blocked", format!("{error:#}"));
                return;
            }
        };
        let path = draft.path.clone();
        let worker_path = path.clone();
        let breakpoint = if debug {
            self.pipeline_run_breakpoint
                .clone()
                .filter(|choice| breakpoint_candidates(&draft.pipeline).contains(choice))
        } else {
            None
        };
        let (events, receiver) = mpsc::channel();
        let (commands, command_receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        std::thread::spawn(move || {
            let hook = PipelineRunHook {
                events: events.clone(),
                commands: command_receiver,
                cancelled: worker_cancelled,
                pause_each_write: Cell::new(
                    debug
                        && breakpoint.is_none()
                        && value_condition.is_none()
                        && position_condition.is_none()
                        && source_condition.is_none(),
                ),
                breakpoint,
                value_condition,
                position_condition,
                source_condition,
            };
            let stage_hook = |stage: &str, write: &engine::PendingTargetWrite| {
                hook.before_target_write(stage, write)
            };
            let source_probe = |_stage: &str| hook.source_field_probe();
            let gate = || hook.before_publish();
            let options = cli::PipelineRunOptions::default()
                .with_stage_debug_hook(&stage_hook)
                .with_stage_source_field_probe(&source_probe)
                .with_before_publish(&gate);
            let result =
                cli::run_pipeline_file_with_options(&worker_path, &inputs, &outputs, &options)
                    .map_err(|error| {
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
            let _ = events.send(PipelineRunEvent::Finished(result));
        });
        self.pending_pipeline_run = Some(PendingPipelineRun {
            receiver,
            commands,
            cancelled,
            started: Instant::now(),
            path,
            debug,
            phase: PipelineRunPhase::Running,
        });
        self.show_run_report = false;
        self.status = if debug {
            "debug pipeline running".into()
        } else {
            "running pipeline".into()
        };
        self.diagnostics.clear();
    }

    pub(super) fn pipeline_run_command(&mut self, command: PipelineRunCommand) {
        if let Some(pending) = &mut self.pending_pipeline_run {
            pending.command(command);
            self.status = match command {
                PipelineRunCommand::Cancel => "stopping pipeline",
                PipelineRunCommand::Pause => "pausing pipeline at next target write",
                PipelineRunCommand::Step
                | PipelineRunCommand::Continue
                | PipelineRunCommand::Publish => "debug pipeline running",
            }
            .into();
        }
    }

    pub(super) fn poll_pipeline_run(&mut self, ctx: &egui::Context) {
        let Some(pending) = &mut self.pending_pipeline_run else {
            return;
        };
        let event = match pending.receiver.try_recv() {
            Ok(event) => event,
            Err(TryRecvError::Empty) => {
                if !matches!(pending.phase, PipelineRunPhase::Paused(..)) {
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
                return;
            }
            Err(TryRecvError::Disconnected) => PipelineRunEvent::Finished(Err(
                PipelineRunError::Failed("pipeline worker stopped unexpectedly".into()),
            )),
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
            PipelineRunEvent::ReadyToPublish => {
                if matches!(pending.phase, PipelineRunPhase::Stopping) {
                    let _ = pending.commands.send(PipelineRunCommand::Cancel);
                } else {
                    pending.phase = PipelineRunPhase::Publishing;
                    self.status = "publishing pipeline outputs".into();
                    let _ = pending.commands.send(PipelineRunCommand::Publish);
                }
                ctx.request_repaint();
            }
            PipelineRunEvent::Finished(result) => {
                let pending = self
                    .pending_pipeline_run
                    .take()
                    .expect("pending run exists");
                if matches!(pending.phase, PipelineRunPhase::Stopping)
                    || matches!(result, Err(PipelineRunError::Cancelled))
                {
                    self.status = "pipeline cancelled".into();
                    self.diagnostics.clear();
                    self.finish_deferred_pipeline_close(ctx);
                    return;
                }
                match result {
                    Ok(outcome) => {
                        self.status = format!(
                            "pipeline completed {} stage(s) and wrote {} file(s)",
                            outcome.stages_executed.len(),
                            outcome.artifacts.len()
                        );
                        let report = crate::run_report::RunReport::from_pipeline_outcome(
                            outcome,
                            pending.path.clone(),
                            pending.started.elapsed(),
                        );
                        self.run_report = Some(crate::run_report::RunReportView::new(report));
                        self.show_run_report = true;
                        self.pipeline_run_draft = None;
                    }
                    Err(PipelineRunError::Failed(error)) => {
                        self.status = "pipeline run failed".into();
                        self.diagnostics.error("Pipeline run failed", error);
                    }
                    Err(PipelineRunError::Cancelled) => unreachable!("handled above"),
                }
                self.finish_deferred_pipeline_close(ctx);
            }
        }
    }

    fn finish_deferred_pipeline_close(&mut self, ctx: &egui::Context) {
        if std::mem::take(&mut self.close_after_pipeline_run) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

fn input_label(input: &PipelineInput) -> String {
    match input {
        PipelineInput::Host { name } => format!("host `{name}`"),
        PipelineInput::StageTarget { stage, target } => match target {
            Some(target) => format!("{stage} / {target}"),
            None => format!("{stage} / Primary"),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_during_pipeline_publication_waits_for_worker() {
        let (events, receiver) = mpsc::channel();
        let (commands, _command_receiver) = mpsc::channel();
        let mut app = FerruleApp {
            pending_pipeline_run: Some(PendingPipelineRun {
                receiver,
                commands,
                cancelled: Arc::new(AtomicBool::new(false)),
                started: Instant::now(),
                path: PathBuf::from("pipeline.json"),
                debug: false,
                phase: PipelineRunPhase::Publishing,
            }),
            ..FerruleApp::default()
        };
        let context = egui::Context::default();

        app.guard_app_close_requested(&context, true);
        assert!(app.close_after_pipeline_run);
        let pending = app
            .pending_pipeline_run
            .as_ref()
            .expect("worker remains active");
        assert!(matches!(pending.phase, PipelineRunPhase::Publishing));
        assert!(!pending.cancelled.load(Ordering::Acquire));

        events
            .send(PipelineRunEvent::Finished(Err(PipelineRunError::Cancelled)))
            .expect("completion event is sent");
        app.poll_pipeline_run(&context);
        assert!(app.pending_pipeline_run.is_none());
        assert!(!app.close_after_pipeline_run);
    }
}
