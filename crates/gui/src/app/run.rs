use std::cell::Cell;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use super::*;
use crate::preview::{DebugScalarCondition, PreviewBreakpoint, PreviewTarget};

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

enum FileRunEvent {
    Paused(engine::PendingTargetWrite),
    ReadyToPublish,
    Finished(
        Result<cli::RunOutcome, FileRunError>,
        crate::run_report::TraceReport,
    ),
}

enum FileRunError {
    Cancelled,
    Failed(String),
}

#[derive(Debug, Clone)]
pub(super) enum FileRunPhase {
    Running,
    Paused(Box<engine::PendingTargetWrite>),
    Publishing,
    Stopping,
}

pub(super) struct PendingFileRun {
    receiver: Receiver<FileRunEvent>,
    commands: Sender<FileRunCommand>,
    cancelled: Arc<AtomicBool>,
    started: Instant,
    debug: bool,
    pub(super) phase: FileRunPhase,
}

impl PendingFileRun {
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
}

impl Drop for PendingFileRun {
    fn drop(&mut self) {
        if !matches!(self.phase, FileRunPhase::Publishing) {
            self.cancelled.store(true, Ordering::Release);
            let _ = self.commands.send(FileRunCommand::Cancel);
        }
    }
}

struct FileRunDebugHook {
    events: Sender<FileRunEvent>,
    commands: Receiver<FileRunCommand>,
    cancelled: Arc<AtomicBool>,
    pause_each_write: Cell<bool>,
    breakpoint: Option<FileBreakpoint>,
    value_condition: Option<DebugScalarCondition>,
}

impl FileRunDebugHook {
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

impl engine::DebugHook for FileRunDebugHook {
    fn before_target_write(&self, write: &engine::PendingTargetWrite) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(FileRunCommand::Pause | FileRunCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(FileRunCommand::Continue) => self.pause_each_write.set(false),
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
        let has_breakpoint = self.breakpoint.is_some() || self.value_condition.is_some();
        if !self.pause_each_write.get() && !(has_breakpoint && matches_selection && matches_value) {
            return engine::DebugDecision::Resume;
        }
        if self
            .events
            .send(FileRunEvent::Paused(write.clone()))
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
    }

    pub(super) fn run(&mut self, ctx: &egui::Context) {
        self.start_saved_run(false, ctx);
    }

    pub(super) fn debug_run(&mut self, ctx: &egui::Context) {
        self.start_saved_run(true, ctx);
    }

    fn start_saved_run(&mut self, debug: bool, ctx: &egui::Context) {
        if self.pending_file_run.is_some() || self.pending_pipeline_run.is_some() {
            return;
        }
        if debug && let Err(error) = self.file_run_value_condition.compile() {
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
        self.diagnostics.clear();
        let continuation = if debug {
            SaveContinuation::DebugRun
        } else {
            SaveContinuation::Run
        };
        self.save_with_continuation(Some(continuation), ctx);
    }

    pub(super) fn run_saved(&mut self, debug: bool) {
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
        let (events, receiver) = mpsc::channel();
        let (commands, command_receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        std::thread::spawn(move || {
            let trace = crate::run_report::TraceCollector::new();
            let hook = FileRunDebugHook {
                events: events.clone(),
                commands: command_receiver,
                cancelled: worker_cancelled,
                pause_each_write: Cell::new(
                    debug && breakpoint.is_none() && value_condition.is_none(),
                ),
                breakpoint,
                value_condition,
            };
            let gate = || hook.before_publish();
            let mut options = cli::RunOptions::new()
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
        });
        self.pending_file_run = Some(PendingFileRun {
            receiver,
            commands,
            cancelled,
            started: Instant::now(),
            debug,
            phase: FileRunPhase::Running,
        });
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
        let event = match pending.receiver.try_recv() {
            Ok(event) => event,
            Err(TryRecvError::Empty) => {
                if !matches!(pending.phase, FileRunPhase::Paused(_)) {
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
                return;
            }
            Err(TryRecvError::Disconnected) => FileRunEvent::Finished(
                Err(FileRunError::Failed(
                    "run worker stopped unexpectedly".into(),
                )),
                crate::run_report::TraceReport::default(),
            ),
        };
        match event {
            FileRunEvent::Paused(write) => {
                if matches!(pending.phase, FileRunPhase::Stopping) {
                    let _ = pending.commands.send(FileRunCommand::Cancel);
                } else {
                    self.status = format!("paused before target field `{}`", write.field);
                    pending.phase = FileRunPhase::Paused(Box::new(write));
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
                let pending = self.pending_file_run.take().expect("run worker exists");
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
                    Err(FileRunError::Cancelled) => unreachable!("handled above"),
                }
                self.finish_deferred_close(ctx);
            }
        }
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
                    FileRunPhase::Paused(_) => {
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
        let condition_valid = preview_ui::show_breakpoint_value_condition(
            ui,
            &mut self.file_run_value_condition,
            "file_run_debug_value_type",
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
mod tests {
    use super::*;

    #[test]
    fn close_during_publication_waits_for_worker_to_finish() {
        let (events, receiver) = mpsc::channel();
        let (commands, _command_receiver) = mpsc::channel();
        let mut app = FerruleApp {
            pending_file_run: Some(PendingFileRun {
                receiver,
                commands,
                cancelled: Arc::new(AtomicBool::new(false)),
                started: Instant::now(),
                debug: false,
                phase: FileRunPhase::Publishing,
            }),
            ..FerruleApp::default()
        };
        let context = egui::Context::default();

        app.guard_app_close_requested(&context, true);
        assert!(app.close_after_file_run);
        let pending = app
            .pending_file_run
            .as_ref()
            .expect("worker remains active");
        assert!(matches!(pending.phase, FileRunPhase::Publishing));
        assert!(!pending.cancelled.load(Ordering::Acquire));

        events
            .send(FileRunEvent::Finished(
                Err(FileRunError::Cancelled),
                crate::run_report::TraceReport::default(),
            ))
            .expect("completion event is sent");
        app.poll_file_run(&context);
        assert!(app.pending_file_run.is_none());
        assert!(!app.close_after_file_run);
    }
}
