use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};

use super::*;
use crate::preview::{
    BreakpointPositionConditionDraft, BreakpointValueConditionDraft, DebugPositionCondition,
    DebugScalarCondition, LoadedPreviewSource, PreviewBreakpoint, PreviewDraft, PreviewTarget,
    ScalarValueType,
};

pub(super) fn show_breakpoint_position_condition(
    ui: &mut egui::Ui,
    condition: &mut BreakpointPositionConditionDraft,
) -> bool {
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut condition.enabled, "Only at innermost active item #");
        if condition.enabled {
            ui.add(
                egui::TextEdit::singleline(&mut condition.text)
                    .desired_width(96.0)
                    .hint_text("1-based active item number"),
            );
        }
    });
    if condition.enabled {
        ui.weak("Uses the shown active position after scope controls; root writes have none.");
    }
    match condition.compile() {
        Ok(_) => true,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            false
        }
    }
}

pub(super) fn show_breakpoint_value_condition(
    ui: &mut egui::Ui,
    condition: &mut BreakpointValueConditionDraft,
    id: &str,
) -> bool {
    ui.checkbox(&mut condition.enabled, "Only when pending scalar equals");
    if condition.enabled {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt(id)
                .selected_text(condition.value_type.label())
                .show_ui(ui, |ui| {
                    for value_type in ScalarValueType::ALL {
                        ui.selectable_value(
                            &mut condition.value_type,
                            value_type,
                            value_type.label(),
                        );
                    }
                });
            if condition.value_type.needs_text() {
                ui.add(
                    egui::TextEdit::singleline(&mut condition.text)
                        .char_limit(160)
                        .hint_text("Exact scalar value"),
                );
            }
        });
        ui.weak("Scalar type and complete value must match; strings over 160 characters and non-finite numbers cannot match.");
    }
    match condition.compile() {
        Ok(_) => true,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            false
        }
    }
}

enum PreviewAction {
    Cancel,
    Execute,
    Debug,
    Step,
    Continue,
    Pause,
}

#[derive(Clone, Copy)]
pub(super) enum PreviewCommand {
    Step,
    Continue,
    Pause,
    Cancel,
}

enum PreviewWorkerEvent {
    Paused(Box<engine::PendingTargetWrite>),
    Finished(
        Result<cli::PayloadRunOutcome, PreviewRunError>,
        crate::run_report::TraceReport,
    ),
}

enum PreviewRunError {
    Cancelled,
    Failed(String),
}

#[derive(Debug, Clone)]
pub(super) enum PreviewPhase {
    Running,
    Paused(Box<engine::PendingTargetWrite>),
    Stopping,
}

pub(super) struct PendingPreview {
    receiver: Receiver<PreviewWorkerEvent>,
    commands: Sender<PreviewCommand>,
    cancelled: Arc<AtomicBool>,
    started: Instant,
    input_path: PathBuf,
    target_label: String,
    debug: bool,
    pub(super) phase: PreviewPhase,
}

impl PendingPreview {
    fn command(&mut self, command: PreviewCommand) {
        match command {
            PreviewCommand::Step | PreviewCommand::Continue => {
                self.phase = PreviewPhase::Running;
            }
            PreviewCommand::Pause => {}
            PreviewCommand::Cancel => {
                self.cancelled.store(true, Ordering::Release);
                self.phase = PreviewPhase::Stopping;
            }
        }
        let _ = self.commands.send(command);
    }
}

impl Drop for PendingPreview {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.commands.send(PreviewCommand::Cancel);
    }
}

struct PreviewDebugHook {
    events: Sender<PreviewWorkerEvent>,
    commands: Receiver<PreviewCommand>,
    cancelled: Arc<AtomicBool>,
    pause_each_write: std::cell::Cell<bool>,
    breakpoint: Option<PreviewBreakpoint>,
    value_condition: Option<DebugScalarCondition>,
    position_condition: Option<DebugPositionCondition>,
}

impl engine::DebugHook for PreviewDebugHook {
    fn before_target_write(&self, write: &engine::PendingTargetWrite) -> engine::DebugDecision {
        if self.cancelled.load(Ordering::Acquire) {
            return engine::DebugDecision::Cancel;
        }
        loop {
            match self.commands.try_recv() {
                Ok(PreviewCommand::Pause | PreviewCommand::Step) => {
                    self.pause_each_write.set(true);
                }
                Ok(PreviewCommand::Continue) => self.pause_each_write.set(false),
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
        let has_breakpoint = self.breakpoint.is_some()
            || self.value_condition.is_some()
            || self.position_condition.is_some();
        if !self.pause_each_write.get()
            && !(has_breakpoint && matches_selection && matches_value && matches_position)
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

impl FerruleApp {
    pub(super) fn can_preview(&self) -> bool {
        self.pending_preview.is_none()
            && matches!(
                self.mapping_workspace.active,
                MappingDocument::Main | MappingDocument::Target(_)
            )
    }

    pub(super) fn begin_preview(&mut self) {
        let (target, output_identity) = match self.mapping_workspace.active {
            MappingDocument::Main => (
                PreviewTarget::Primary,
                self.project.target_path.clone().unwrap_or_default(),
            ),
            MappingDocument::Target(index) => {
                let Some(target) = self.project.extra_targets.get(index) else {
                    self.status = "preview unavailable".to_string();
                    self.diagnostics.error(
                        "Preview unavailable",
                        "the active named target no longer exists",
                    );
                    return;
                };
                (
                    PreviewTarget::Named(target.name.clone()),
                    target.path.clone().unwrap_or_default(),
                )
            }
            MappingDocument::Function(_) => {
                self.status = "preview unavailable".to_string();
                self.diagnostics.error(
                    "Preview unavailable",
                    "open the primary mapping or a named target mapping first",
                );
                return;
            }
        };
        let input_identity = nonempty_text(&self.input_path)
            .map(str::to_owned)
            .or_else(|| self.project.source_path.clone())
            .unwrap_or_default();
        self.preview_draft = Some(PreviewDraft::new(target, input_identity, output_identity));
    }

    pub(super) fn show_preview_setup(&mut self, ctx: &egui::Context) {
        let mut action = None;
        let breakpoint_candidates = self
            .preview_draft
            .as_ref()
            .map(|draft| crate::preview::breakpoint_candidates(&self.project, &draft.target))
            .unwrap_or_default();
        let phase = self
            .pending_preview
            .as_ref()
            .map(|pending| pending.phase.clone());
        let debug = self
            .pending_preview
            .as_ref()
            .is_some_and(|pending| pending.debug);
        let running = phase.is_some();
        let Some(draft) = &mut self.preview_draft else {
            return;
        };
        let input_size = draft.input_text.len();
        let input_too_large = input_size > cli::MAX_PAYLOAD_DOCUMENT_BYTES;
        let mut condition_valid = true;
        egui::Window::new("Preview mapping")
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(760.0, 620.0))
            .min_size(egui::vec2(520.0, 360.0))
            .show(ctx, |ui| {
                ui.add_enabled_ui(!running, |ui| {
                    egui::Grid::new("preview_identity_grid")
                        .num_columns(2)
                        .spacing(egui::vec2(12.0, 8.0))
                        .show(ui, |ui| {
                            ui.label("Target");
                            ui.strong(draft.target.label());
                            ui.end_row();
                            ui.label("Logical input identity");
                            ui.add(
                                egui::TextEdit::singleline(&mut draft.input_identity)
                                    .hint_text("input.json")
                                    .desired_width(f32::INFINITY),
                            )
                            .on_hover_text("Selects the input format; no file is opened");
                            ui.end_row();
                            ui.label("Logical output identity");
                            ui.add(
                                egui::TextEdit::singleline(&mut draft.output_identity)
                                    .hint_text("output.json")
                                    .desired_width(f32::INFINITY),
                            )
                            .on_hover_text("Selects the output format; no file is written");
                            ui.end_row();
                        });
                });
                ui.horizontal(|ui| {
                    ui.label("Debug breakpoint");
                    ui.add_enabled_ui(!running, |ui| {
                        egui::ComboBox::from_id_salt("preview_debug_breakpoint")
                            .selected_text(
                                draft
                                    .debug_breakpoint
                                    .as_ref()
                                    .map_or_else(|| "Every target-field write".to_owned(), PreviewBreakpoint::label),
                            )
                            .show_ui(ui, |ui| {
                                ui.selectable_value(
                                    &mut draft.debug_breakpoint,
                                    None,
                                    "Every target-field write",
                                );
                                for breakpoint in &breakpoint_candidates {
                                    ui.selectable_value(
                                        &mut draft.debug_breakpoint,
                                        Some(breakpoint.clone()),
                                        breakpoint.label(),
                                    );
                                }
                            });
                    });
                });
                ui.weak("The selector covers declared static scope writes; runtime-named fields are not listed.");
                ui.add_enabled_ui(!running, |ui| {
                    condition_valid = show_breakpoint_value_condition(
                        ui,
                        &mut self.preview_value_condition,
                        "preview_debug_value_type",
                    );
                    condition_valid &= show_breakpoint_position_condition(
                        ui,
                        &mut self.preview_position_condition,
                    );
                });
                if draft.input_identity.trim().is_empty() {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        "A logical input identity is required to select the input format.",
                    );
                } else if draft.input_identity.trim().len() > cli::MAX_PAYLOAD_PATH_BYTES {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!(
                            "Logical input identity exceeds {} UTF-8 bytes.",
                            cli::MAX_PAYLOAD_PATH_BYTES
                        ),
                    );
                }
                if draft.output_identity.trim().is_empty() {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        "A logical output identity is required to select the output format.",
                    );
                } else if draft.output_identity.trim().len() > cli::MAX_PAYLOAD_PATH_BYTES {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!(
                            "Logical output identity exceeds {} UTF-8 bytes.",
                            cli::MAX_PAYLOAD_PATH_BYTES
                        ),
                    );
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.strong("Primary input");
                    ui.weak(format!(
                        "{} / {}",
                        format_preview_bytes(input_size),
                        format_preview_bytes(cli::MAX_PAYLOAD_DOCUMENT_BYTES)
                    ));
                });
                if input_too_large {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        "Primary input exceeds the 64 MiB per-document limit.",
                    );
                }
                if let Some(phase) = &phase {
                    show_live_debug_state(ui, phase, debug);
                    ui.weak(
                        "The mapping and input are held as an in-memory snapshot for this preview.",
                    );
                } else {
                    let button_height = ui.spacing().interact_size.y;
                    let editor_height = (ui.available_height() - button_height - 18.0).max(140.0);
                    egui::ScrollArea::both()
                        .id_salt("preview_primary_input_scroll")
                        .auto_shrink([false, false])
                        .max_height(editor_height)
                        .show(ui, |ui| {
                            ui.add_sized(
                                egui::vec2(ui.available_width(), editor_height),
                                egui::TextEdit::multiline(&mut draft.input_text)
                                    .code_editor()
                                    .desired_width(f32::INFINITY),
                            );
                        });
                }
                ui.separator();
                ui.horizontal(|ui| match &phase {
                    Some(PreviewPhase::Paused(_)) => {
                        if ui.button("Step").clicked() {
                            action = Some(PreviewAction::Step);
                        }
                        if ui.button("Continue").clicked() {
                            action = Some(PreviewAction::Continue);
                        }
                        if ui.button("Cancel debug preview").clicked() {
                            action = Some(PreviewAction::Cancel);
                        }
                    }
                    Some(PreviewPhase::Running) => {
                        if debug && ui.button("Pause at next write").clicked() {
                            action = Some(PreviewAction::Pause);
                        }
                        if ui.button("Cancel preview").clicked() {
                            action = Some(PreviewAction::Cancel);
                        }
                    }
                    Some(PreviewPhase::Stopping) => {
                        ui.spinner();
                        ui.weak("Stopping preview");
                    }
                    None => {
                        if ui.button("Cancel").clicked() {
                            action = Some(PreviewAction::Cancel);
                        }
                        if ui
                            .add_enabled(draft.can_execute(), egui::Button::new("Preview"))
                            .clicked()
                        {
                            action = Some(PreviewAction::Execute);
                        }
                        if ui
                            .add_enabled(draft.can_execute() && condition_valid, egui::Button::new("Debug preview"))
                            .clicked()
                        {
                            action = Some(PreviewAction::Debug);
                        }
                    }
                });
            });

        if action.is_some() {
            ctx.request_repaint();
        }
        match action {
            Some(PreviewAction::Cancel) => {
                if let Some(pending) = &mut self.pending_preview {
                    pending.command(PreviewCommand::Cancel);
                    self.status = "stopping preview".into();
                } else {
                    self.preview_draft = None;
                }
            }
            Some(PreviewAction::Execute) => self.execute_preview(),
            Some(PreviewAction::Debug) => self.execute_debug_preview(),
            Some(PreviewAction::Step) => self.preview_command(PreviewCommand::Step),
            Some(PreviewAction::Continue) => self.preview_command(PreviewCommand::Continue),
            Some(PreviewAction::Pause) => self.preview_command(PreviewCommand::Pause),
            None => {}
        }
    }

    pub(super) fn execute_preview(&mut self) {
        self.start_preview(false);
    }

    pub(super) fn execute_debug_preview(&mut self) {
        self.start_preview(true);
    }

    fn start_preview(&mut self, debug: bool) {
        if self.pending_preview.is_some() {
            return;
        }
        let value_condition = if debug {
            match self.preview_value_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "preview blocked".into();
                    self.diagnostics.error("Preview blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let position_condition = if debug {
            match self.preview_position_condition.compile() {
                Ok(condition) => condition,
                Err(error) => {
                    self.status = "preview blocked".into();
                    self.diagnostics.error("Preview blocked", error);
                    return;
                }
            }
        } else {
            None
        };
        let issues = cli::validate(&self.project);
        if !issues.is_empty() {
            self.status = format!("preview blocked by {} validation issue(s)", issues.len());
            self.diagnostics.validation(&self.project, issues);
            return;
        }
        let Some(draft) = self.preview_draft.as_ref() else {
            return;
        };
        if !draft.can_execute() {
            self.status = "preview failed".to_string();
            self.diagnostics.error(
                "Preview failed",
                "preview input or logical format identity is invalid",
            );
            return;
        }
        let project_path = match self.preview_project_path() {
            Ok(path) => path,
            Err(error) => {
                self.status = "preview failed".to_string();
                self.diagnostics
                    .error("Preview failed", format!("{error:#}"));
                return;
            }
        };
        let draft = draft.clone();
        let input_path = PathBuf::from(draft.input_identity.trim());
        let target_label = draft.target.label().to_owned();
        let project = self.project.clone();
        let saved_path = self.document.saved_path().map(PathBuf::from);
        let (event_tx, receiver) = mpsc::channel();
        let (commands, command_rx) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        std::thread::spawn(move || {
            run_preview_worker(
                project,
                draft,
                project_path,
                saved_path,
                debug,
                value_condition,
                position_condition,
                event_tx,
                command_rx,
                worker_cancelled,
            );
        });
        self.pending_preview = Some(PendingPreview {
            receiver,
            commands,
            cancelled,
            started: Instant::now(),
            input_path,
            target_label,
            debug,
            phase: PreviewPhase::Running,
        });
        self.show_run_report = false;
        self.diagnostics.clear();
        self.status = if debug {
            "debug preview running".into()
        } else {
            "preview running".into()
        };
    }

    pub(super) fn preview_command(&mut self, command: PreviewCommand) {
        if let Some(pending) = &mut self.pending_preview {
            pending.command(command);
            self.status = match command {
                PreviewCommand::Cancel => "stopping preview",
                PreviewCommand::Pause => "pausing at next target write",
                PreviewCommand::Step | PreviewCommand::Continue => "debug preview running",
            }
            .into();
        }
    }

    pub(super) fn poll_preview(&mut self, ctx: &egui::Context) {
        let Some(pending) = &mut self.pending_preview else {
            return;
        };
        let event = match pending.receiver.try_recv() {
            Ok(event) => event,
            Err(TryRecvError::Empty) => {
                if !matches!(pending.phase, PreviewPhase::Paused(_)) {
                    ctx.request_repaint_after(Duration::from_millis(100));
                }
                return;
            }
            Err(TryRecvError::Disconnected) => PreviewWorkerEvent::Finished(
                Err(PreviewRunError::Failed(
                    "preview worker stopped unexpectedly".into(),
                )),
                crate::run_report::TraceReport::default(),
            ),
        };
        match event {
            PreviewWorkerEvent::Paused(write) => {
                if matches!(pending.phase, PreviewPhase::Stopping) {
                    pending.command(PreviewCommand::Cancel);
                } else {
                    self.status = format!("paused before target field `{}`", write.field);
                    pending.phase = PreviewPhase::Paused(write);
                }
                ctx.request_repaint();
            }
            PreviewWorkerEvent::Finished(result, trace) => {
                let pending = self.pending_preview.take().expect("preview worker exists");
                if matches!(pending.phase, PreviewPhase::Stopping)
                    || matches!(result, Err(PreviewRunError::Cancelled))
                {
                    self.status = "preview cancelled".into();
                    self.preview_draft = None;
                    self.diagnostics.clear();
                    return;
                }
                match result {
                    Ok(outcome) => {
                        let report = crate::run_report::RunReport::from_payload_with_trace(
                            outcome,
                            pending.input_path.clone(),
                            pending.started.elapsed(),
                            trace,
                        );
                        self.status = format!(
                            "previewed {} record(s) for {}",
                            report.records_written, pending.target_label
                        );
                        self.run_report = Some(crate::run_report::RunReportView::new(report));
                        self.show_run_report = true;
                        self.preview_draft = None;
                        self.diagnostics.clear();
                    }
                    Err(PreviewRunError::Failed(error)) => {
                        self.status = "preview failed".to_string();
                        self.diagnostics.error("Preview failed", error);
                    }
                    Err(PreviewRunError::Cancelled) => unreachable!("handled above"),
                }
            }
        }
    }

    fn preview_project_path(&self) -> anyhow::Result<PathBuf> {
        if let Some(path) = self.document.saved_path() {
            return Ok(path.to_path_buf());
        }
        let name = self
            .document
            .suggested_path()
            .file_name()
            .filter(|name| !name.is_empty())
            .context("untitled project has no logical mapping identity")?;
        Ok(std::env::current_dir()
            .context("resolving the current directory for Preview")?
            .join(name))
    }
}

#[allow(clippy::too_many_arguments)]
fn run_preview_worker(
    project: Project,
    draft: PreviewDraft,
    project_path: PathBuf,
    saved_path: Option<PathBuf>,
    debug: bool,
    value_condition: Option<DebugScalarCondition>,
    position_condition: Option<DebugPositionCondition>,
    events: Sender<PreviewWorkerEvent>,
    commands: Receiver<PreviewCommand>,
    cancelled: Arc<AtomicBool>,
) {
    let trace = crate::run_report::TraceCollector::new();
    let breakpoint = if debug {
        draft.debug_breakpoint.clone()
    } else {
        None
    };
    let hook = PreviewDebugHook {
        events: events.clone(),
        commands,
        cancelled: Arc::clone(&cancelled),
        pause_each_write: std::cell::Cell::new(
            debug
                && breakpoint.is_none()
                && value_condition.is_none()
                && position_condition.is_none(),
        ),
        breakpoint,
        value_condition,
        position_condition,
    };
    let result = run_preview_payload(
        &project,
        &draft,
        &project_path,
        saved_path.as_deref(),
        &trace,
        &hook,
        &cancelled,
    )
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
    let _ = events.send(PreviewWorkerEvent::Finished(result, trace.finish()));
}

fn run_preview_payload(
    project: &Project,
    draft: &PreviewDraft,
    project_path: &std::path::Path,
    saved_path: Option<&std::path::Path>,
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
        .with_extra_sources(&named_inputs)
        .with_output_path(&output_path)
        .with_target(draft.target.selection())
        .with_trace_sink(trace)
        .with_debug_hook(debug_hook);
    cli::run_project_value_payloads(project, project_path, &options)
}

pub(super) fn show_live_debug_state(ui: &mut egui::Ui, phase: &PreviewPhase, debug: bool) {
    match phase {
        PreviewPhase::Running => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.strong(if debug {
                    "Debug preview is running"
                } else {
                    "Preview is running"
                });
            });
            if debug {
                ui.weak("Execution pauses before the next ordinary target-field write.");
            }
        }
        PreviewPhase::Stopping => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.strong("Stopping preview");
            });
        }
        PreviewPhase::Paused(write) => {
            ui.strong("Paused before target-field insertion");
            egui::ScrollArea::vertical()
                .id_salt("preview_live_debug_details")
                .max_height(280.0)
                .show(ui, |ui| {
                    let target = match &write.scope.target {
                        engine::TraceTarget::Primary => "Primary",
                        engine::TraceTarget::Named(name) => name,
                    };
                    ui.label(format!("Target: {target}"));
                    let scope = if write.scope.target_path.is_empty() {
                        "<root>".to_string()
                    } else {
                        write.scope.target_path.join(" / ")
                    };
                    ui.label(format!("Scope: {scope}"));
                    ui.label(format!("Pending field: {}", write.field));
                    if write.field_truncated {
                        ui.weak("Field name preview was truncated.");
                    }
                    ui.label(format!("Binding: {:?}", write.binding));
                    ui.label(format!("Pending value: {}", debug_value(&write.pending)));
                    if write.positions.is_empty() {
                        ui.weak("Source position: <root>");
                    } else {
                        for position in &write.positions {
                            let collection = if position.collection.is_empty() {
                                "<current>".to_string()
                            } else {
                                position.collection.join(" / ")
                            };
                            let mut label =
                                format!("Source position: {collection} #{}", position.index);
                            if position.grouped {
                                label.push_str(" (grouped)");
                            }
                            if let Some(path) = &position.document_path {
                                label.push_str(&format!(" — {path}"));
                            }
                            ui.label(label);
                        }
                    }
                    ui.separator();
                    ui.strong("Active source frames (outer to inner)");
                    if write.source.omitted_outer_frames > 0 {
                        ui.weak(format!(
                            "{} outer frame(s) omitted from this bounded view",
                            write.source.omitted_outer_frames
                        ));
                    }
                    if write.source.frames.is_empty() {
                        ui.weak("No active source frame.");
                    }
                    for (index, frame) in write.source.frames.iter().enumerate() {
                        ui.label(format!(
                            "Frame {}: {}",
                            write.source.omitted_outer_frames + index + 1,
                            debug_value(&frame.preview)
                        ));
                        ui.indent(("debug_source_frame", index), |ui| {
                            for field in &frame.fields {
                                let suffix = if field.name_truncated { "…" } else { "" };
                                ui.monospace(format!(
                                    "{}{suffix}: {}",
                                    field.name,
                                    debug_value(&field.preview)
                                ));
                            }
                            if frame.omitted_fields > 0 {
                                ui.weak(format!("{} more field(s) omitted", frame.omitted_fields));
                            }
                        });
                    }
                    ui.separator();
                    ui.strong("Already inserted in this scope");
                    if write.draft.fields.is_empty() {
                        ui.weak("No fields yet.");
                    }
                    for field in &write.draft.fields {
                        ui.monospace(format!("{}: {}", field.name, debug_value(&field.preview)));
                    }
                    if write.draft.omitted_fields > 0 {
                        ui.weak(format!(
                            "{} more field(s) omitted from this bounded view",
                            write.draft.omitted_fields
                        ));
                    }
                });
            ui.weak("The pending value has been computed but is not yet in the target. Step inserts it and pauses before the next write.");
        }
    }
}

fn debug_value(value: &engine::DebugInstancePreview) -> String {
    if let Some(scalar) = &value.value {
        let suffix = if scalar.truncated { "…" } else { "" };
        format!("{}: {}{suffix}", scalar.value_type, scalar.preview)
    } else if let Some(length) = value.length {
        format!("{:?} ({length} immediate item(s))", value.kind)
    } else {
        format!("{:?}", value.kind)
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

fn nonempty_text(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty()).then_some(value)
}

fn format_preview_bytes(bytes: usize) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= MIB {
        format!("{:.1} MiB", bytes / MIB)
    } else if bytes >= KIB {
        format!("{:.1} KiB", bytes / KIB)
    } else {
        format!("{} B", bytes as u64)
    }
}
