use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};

use super::owned_worker::{OwnedWorker, WorkerExit};
use super::*;
use crate::preview::{
    BreakpointExpressionConditionDraft, BreakpointInputConditionDraft,
    BreakpointNodeConditionDraft, BreakpointPositionConditionDraft, BreakpointSourceConditionDraft,
    BreakpointValueConditionDraft, DebugExpressionCondition, DebugInputCondition,
    DebugNodeCondition, DebugPositionCondition, DebugScalarCondition, DebugSourceCondition,
    LoadedPreviewSource, PreviewBreakpoint, PreviewDraft, PreviewTarget, ScalarValueType,
};

fn show_preview_input(ui: &mut egui::Ui, draft: &mut PreviewDraft, input_size: usize) {
    let label = ui
        .horizontal(|ui| {
            let label = ui.strong("Primary input");
            ui.weak(format!(
                "{} / {}",
                format_preview_bytes(input_size),
                format_preview_bytes(cli::MAX_PAYLOAD_DOCUMENT_BYTES)
            ));
            crate::ui_kit::help_icon(ui, INPUT_TAB_HINT);
            label
        })
        .inner;
    let height = (ui.ctx().content_rect().height() * 0.3).clamp(
        crate::theme::ListHeight::Short.px(),
        crate::theme::ListHeight::Tall.px(),
    );
    egui::ScrollArea::both()
        .id_salt("preview_primary_input_scroll")
        .auto_shrink([false, false])
        .max_height(height)
        .show(ui, |ui| {
            let id = ui.make_persistent_id("preview_primary_input");
            if ui.memory(|memory| memory.has_focus(id))
                && ui.input_mut(|input| input.consume_key(egui::Modifiers::CTRL, egui::Key::Tab))
            {
                insert_tab(ui.ctx(), id, &mut draft.input_text);
            }
            ui.add_sized(
                egui::vec2(ui.available_width(), height),
                egui::TextEdit::multiline(&mut draft.input_text)
                    .id(id)
                    .code_editor()
                    .lock_focus(false)
                    .desired_width(f32::INFINITY),
            )
            .labelled_by(label.id);
            ui.ctx().accesskit_node_builder(id, |node| {
                node.set_description(INPUT_TAB_HINT);
            });
        });
}

/// Tab moves focus on like in any other field, so tab-separated input needs
/// another key for a literal tab.
const INPUT_TAB_HINT: &str = "Tab moves to the next control; Ctrl+Tab types a tab character.";

/// Replaces the selection of text field `id` with a tab character.
fn insert_tab(ctx: &egui::Context, id: egui::Id, text: &mut String) {
    use egui::TextBuffer as _;
    use egui::text::{CCursor, CCursorRange};

    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    let selection = state
        .cursor
        .char_range()
        .unwrap_or_else(|| CCursorRange::one(CCursor::new(text.chars().count())));
    let mut cursor = text.delete_selected(&selection);
    text.insert_text_at(&mut cursor, "\t", usize::MAX);
    state.cursor.set_char_range(Some(CCursorRange::one(cursor)));
    state.store(ctx, id);
}

pub(super) fn show_breakpoint_input_condition(
    ui: &mut egui::Ui,
    condition: &mut BreakpointInputConditionDraft,
    id: &str,
) -> bool {
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(
            &mut condition.enabled,
            "Pause when a value reaches input pin",
        );
        if condition.enabled {
            ui.label("Consumer node #");
            ui.add(
                egui::TextEdit::singleline(&mut condition.consumer_text)
                    .char_limit(10)
                    .desired_width(crate::theme::FieldWidth::Small.min()),
            );
            ui.label("Input #");
            ui.add(
                egui::TextEdit::singleline(&mut condition.input_number_text)
                    .char_limit(4)
                    .desired_width(crate::theme::METRICS.min_grid_col),
            );
        }
    });
    if condition.enabled {
        ui.horizontal_wrapped(|ui| {
            ui.label("Function # (blank = main graph)");
            ui.add(
                egui::TextEdit::singleline(&mut condition.function_text)
                    .char_limit(20)
                    .desired_width(crate::theme::FieldWidth::Medium.min())
                    .hint_text("Optional function ID"),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(
                &mut condition.value.enabled,
                "Only when delivered value equals",
            );
            if condition.value.enabled {
                egui::ComboBox::from_id_salt(id)
                    .selected_text(condition.value.value_type.label())
                    .show_ui(ui, |ui| {
                        for value_type in ScalarValueType::ALL {
                            ui.selectable_value(
                                &mut condition.value.value_type,
                                value_type,
                                value_type.label(),
                            );
                        }
                    });
                if condition.value.value_type.needs_text() {
                    ui.add(
                        egui::TextEdit::singleline(&mut condition.value.text)
                            .char_limit(160)
                            .hint_text("Exact delivered scalar"),
                    );
                }
            }
        });
        ui.weak("Input numbers are 1-based. This pauses only on graph or function-body inputs currently recorded in Node History; untaken branches, target bindings, and scope-control edges do not match. Step advances to the next recorded input delivery.");
    }
    match condition.compile() {
        Ok(_) => true,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            false
        }
    }
}

pub(super) fn show_breakpoint_expression_condition(
    ui: &mut egui::Ui,
    condition: &mut BreakpointExpressionConditionDraft,
    id: &str,
) -> bool {
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(
            &mut condition.enabled,
            "Pause after graph expression node #",
        );
        if condition.enabled {
            ui.add(
                egui::TextEdit::singleline(&mut condition.node_text)
                    .char_limit(10)
                    .desired_width(crate::theme::FieldWidth::Small.min())
                    .hint_text("Graph node ID"),
            );
        }
    });
    if condition.enabled {
        ui.horizontal_wrapped(|ui| {
            ui.label("Function # (blank = main graph)");
            ui.add(
                egui::TextEdit::singleline(&mut condition.function_text)
                    .char_limit(20)
                    .desired_width(crate::theme::FieldWidth::Medium.min())
                    .hint_text("Optional function ID"),
            );
        });
        ui.horizontal_wrapped(|ui| {
            ui.checkbox(&mut condition.value.enabled, "Only when result equals");
            if condition.value.enabled {
                egui::ComboBox::from_id_salt(id)
                    .selected_text(condition.value.value_type.label())
                    .show_ui(ui, |ui| {
                        for value_type in ScalarValueType::ALL {
                            ui.selectable_value(
                                &mut condition.value.value_type,
                                value_type,
                                value_type.label(),
                            );
                        }
                    });
                if condition.value.value_type.needs_text() {
                    ui.add(
                        egui::TextEdit::singleline(&mut condition.value.text)
                            .char_limit(160)
                            .hint_text("Exact scalar result"),
                    );
                }
            }
        });
        ui.weak("Pauses after a successful graph evaluation, including filters that write nothing. Enter a function ID to select a node inside that reusable function. Step advances to the next evaluated node. Expression pauses do not carry a target identity because the same node can run outside target construction.");
    }
    match condition.compile() {
        Ok(_) => true,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            false
        }
    }
}

pub(super) fn show_breakpoint_node_condition(
    ui: &mut egui::Ui,
    condition: &mut BreakpointNodeConditionDraft,
) -> bool {
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(
            &mut condition.enabled,
            "Only when target write uses value node #",
        );
        if condition.enabled {
            ui.add(
                egui::TextEdit::singleline(&mut condition.text)
                    .desired_width(crate::theme::FieldWidth::Small.min())
                    .hint_text("Graph node ID"),
            );
        }
    });
    if condition.enabled {
        ui.weak("Pauses before a target write driven by this node, not during intermediate graph evaluation. Child-scope writes have no value node.");
    }
    match condition.compile() {
        Ok(_) => true,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            false
        }
    }
}

pub(super) fn show_breakpoint_source_condition(
    ui: &mut egui::Ui,
    condition: &mut BreakpointSourceConditionDraft,
    id: &str,
) -> bool {
    ui.checkbox(
        &mut condition.enabled,
        "Only when active source field equals",
    );
    if condition.enabled {
        ui.horizontal_wrapped(|ui| {
            ui.label("Frame from innermost");
            ui.add(
                egui::TextEdit::singleline(&mut condition.frame_from_inner)
                    .desired_width(crate::theme::METRICS.min_grid_col)
                    .hint_text("0–3"),
            );
            ui.label("Field");
            ui.add(
                egui::TextEdit::singleline(&mut condition.field)
                    .desired_width(crate::theme::FieldWidth::Medium.min())
                    .hint_text("Immediate field name"),
            );
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
                    egui::TextEdit::singleline(&mut condition.text).hint_text("Exact scalar value"),
                );
            }
        });
        ui.weak("Matches one immediate field in an active source frame. Frame 0 is innermost; absent fields and truncated scalar previews never match.");
    }
    match condition.compile() {
        Ok(_) => true,
        Err(error) => {
            ui.colored_label(ui.visuals().error_fg_color, error);
            false
        }
    }
}

pub(super) fn show_breakpoint_position_condition(
    ui: &mut egui::Ui,
    condition: &mut BreakpointPositionConditionDraft,
) -> bool {
    ui.horizontal_wrapped(|ui| {
        ui.checkbox(&mut condition.enabled, "Only at innermost active item #");
        if condition.enabled {
            ui.add(
                egui::TextEdit::singleline(&mut condition.text)
                    .desired_width(crate::theme::FieldWidth::Small.min())
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
    PausedNode(Box<engine::PendingNodeValue>),
    PausedFunctionNode(Box<engine::PendingFunctionNodeValue>),
    PausedInput(Box<engine::PendingNodeInput>),
    PausedFunctionInput(Box<engine::PendingFunctionNodeInput>),
    PausedFailure(Box<engine::PendingNodeFailure>),
    PausedFunctionFailure(Box<engine::PendingFunctionNodeFailure>),
}

/// What the preview worker returns when its thread finishes.
type PreviewCompletion = (
    Result<cli::PayloadRunOutcome, PreviewRunError>,
    crate::run_report::TraceReport,
);

enum PreviewRunError {
    Cancelled,
    Failed(String),
}

#[derive(Debug, Clone)]
pub(super) enum PreviewPhase {
    Running,
    Paused(Box<engine::PendingTargetWrite>),
    PausedNode(Box<engine::PendingNodeValue>),
    PausedFunctionNode(Box<engine::PendingFunctionNodeValue>),
    PausedInput(Box<engine::PendingNodeInput>),
    PausedFunctionInput(Box<engine::PendingFunctionNodeInput>),
    PausedFailure(Box<engine::PendingNodeFailure>),
    PausedFunctionFailure(Box<engine::PendingFunctionNodeFailure>),
    Stopping,
}

/// A preview owned by its exact worker thread. Pause events arrive on
/// `receiver`; the outcome is reported only after that thread was joined.
pub(super) struct PendingPreview {
    worker: OwnedWorker<PreviewCompletion>,
    receiver: Receiver<PreviewWorkerEvent>,
    commands: Sender<PreviewCommand>,
    cancelled: Arc<AtomicBool>,
    started: Instant,
    input_path: PathBuf,
    target_label: String,
    debug: bool,
    pub(super) phase: PreviewPhase,
    /// The opted-in owned PDF extraction of a file preview, if any.
    pub(super) extraction: Option<Arc<super::pdf_extraction::ExtractionStatus>>,
    /// Set for previews that may own a PDF worker: dropping this owner hands
    /// the exact thread to the service instead of blocking on it.
    service: Option<super::PreviewServicePort>,
}

impl PendingPreview {
    pub(super) fn is_service_owned(&self) -> bool {
        self.service.is_some()
    }

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
    /// Cancels, then drains the exact worker: the `worker` field is dropped
    /// after this runs, while the command channel is still connected. A
    /// service-owned preview hands its thread to the service instead.
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        let _ = self.commands.send(PreviewCommand::Cancel);
        // A finished thread is joined at once by the field's own drop.
        if let Some(service) = &self.service
            && !self.worker.is_finished()
        {
            service.retain(Box::new(self.worker.take_ownership()));
        }
    }
}

#[cfg(test)]
impl PendingPreview {
    /// Every owner of the cancellation flag, including the worker's own.
    pub(super) fn cancellation_for_test(&self) -> std::sync::Weak<AtomicBool> {
        Arc::downgrade(&self.cancelled)
    }
}

impl FerruleApp {
    pub(super) fn can_preview(&self) -> bool {
        !self.rest_run_busy()
            && self.pending_preview.is_none()
            && matches!(
                self.mapping_workspace.active,
                MappingDocument::Main | MappingDocument::Target(_)
            )
    }

    pub(super) fn begin_preview(&mut self) {
        if self.rest_run_busy() {
            return;
        }
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
        let input_identity = match &self.primary_document {
            // A document project never previews its saved source path.
            Some(session) => session.input_root.clone(),
            None => nonempty_text(&self.input_path)
                .map(str::to_owned)
                .or_else(|| self.project.source_path.clone())
                .unwrap_or_default(),
        };
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
        let extraction = self
            .pending_preview
            .as_ref()
            .and_then(|pending| pending.extraction.as_ref())
            .and_then(|status| status.progress_label());
        let closing = self.close_after_preview;
        let Some(draft) = &mut self.preview_draft else {
            return;
        };
        let documents = self.primary_document.is_some();
        let documents_ready = self
            .primary_document
            .as_ref()
            .is_none_or(|session| session.policy().is_ok());
        let files = !documents
            && self.preview_input_source
                == super::preview_service::PreviewInputSource::ConfiguredFiles;
        let input_size = draft.input_text.len();
        let input_too_large = input_size > cli::MAX_PAYLOAD_DOCUMENT_BYTES;
        let conditions_valid = self.preview_value_condition.compile().is_ok()
            && self.preview_position_condition.compile().is_ok()
            && self.preview_source_condition.compile().is_ok()
            && self.preview_node_condition.compile().is_ok()
            && self.preview_expression_condition.compile().is_ok()
            && self.preview_input_condition.compile().is_ok();
        let footer_ready = std::cell::Cell::new((false, false, false));
        let dialog_idle = self.pending_dialog.is_none();
        let mut browse_primary_root = false;
        let response = crate::ui_kit::dialog(
            ctx,
            crate::ui_kit::DialogKind::Docked,
            "Preview mapping",
        )
            .id_salt("preview_mapping")
            .width(crate::ui_kit::DialogWidth::Wide)
            .show(|ui| {
                let mut condition_valid = conditions_valid;
                let mut host_parameters_valid = self.host_parameters.compile().is_ok();
                if let Some(session) = &mut self.primary_document {
                    ui.add_enabled_ui(!running && dialog_idle, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Target");
                            ui.strong(draft.target.label());
                        });
                        ui.strong("Primary documents");
                        browse_primary_root = session.show(ui);
                    });
                }
                ui.add_enabled_ui(!running, |ui| {
                    if documents {
                        return;
                    }
                    ui.horizontal(|ui| {
                        ui.label("Primary input");
                        ui.radio_value(
                            &mut self.preview_input_source,
                            super::preview_service::PreviewInputSource::EditorText,
                            "Editor text",
                        );
                        ui.radio_value(
                            &mut self.preview_input_source,
                            super::preview_service::PreviewInputSource::ConfiguredFiles,
                            "Project files",
                        )
                        .on_hover_text(
                            "Reads the configured input files like Run, without writing outputs",
                        );
                    });
                    egui::Grid::new("preview_identity_grid")
                        .num_columns(2)
                        .spacing(egui::vec2(12.0, 8.0))
                        .show(ui, |ui| {
                            ui.label("Target");
                            ui.strong(draft.target.label());
                            ui.end_row();
                            ui.label(if files {
                                "Input file"
                            } else {
                                "Logical input identity"
                            });
                            ui.add(
                                crate::ui_kit::text_field(
                                    &mut draft.input_identity,
                                    crate::ui_kit::FieldWidth::Fill,
                                )
                                .hint_text("input.json"),
                            )
                            .on_hover_text(if files {
                                "Read when the preview starts; the project's source path is \
                                 resolved from the project folder"
                            } else {
                                "Selects the input format; no file is opened"
                            });
                            ui.end_row();
                            ui.label("Logical output identity");
                            ui.add(
                                crate::ui_kit::text_field(
                                    &mut draft.output_identity,
                                    crate::ui_kit::FieldWidth::Fill,
                                )
                                .hint_text("output.json"),
                            )
                            .on_hover_text("Selects the output format; no file is written");
                            ui.end_row();
                        });
                });
                if !documents && !files && phase.is_none() {
                    show_preview_input(ui, draft, input_size);
                }
                crate::ui_kit::collapsible_section(
                    ui, "preview_run_values", "Run values", false, |ui| {
                        ui.add_enabled_ui(!running, |ui| {
                            host_parameters_valid = self.host_parameters.show(ui);
                        });
                    },
                );
                // Document previews have no debug route.
                if !documents {
                    crate::ui_kit::collapsible_section(
                        ui, "preview_debugging", "Debugging", false, |ui| {
                    ui.horizontal(|ui| {
                        let breakpoint_label = ui.label("Debug breakpoint");
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
                                }).response.labelled_by(breakpoint_label.id);
                        });
                    });
                    ui.weak("The selector covers declared static scope writes; runtime-named fields are not listed.");
                    ui.add_enabled_ui(!running, |ui| {
                        condition_valid &= show_breakpoint_value_condition(
                            ui,
                            &mut self.preview_value_condition,
                            "preview_debug_value_type",
                        );
                        condition_valid &= show_breakpoint_position_condition(
                            ui,
                            &mut self.preview_position_condition,
                        );
                        condition_valid &= show_breakpoint_source_condition(
                            ui,
                            &mut self.preview_source_condition,
                            "preview_debug_source_type",
                        );
                        condition_valid &= show_breakpoint_node_condition(
                            ui,
                            &mut self.preview_node_condition,
                        );
                        condition_valid &= show_breakpoint_expression_condition(
                            ui,
                            &mut self.preview_expression_condition,
                            "preview_debug_expression_value_type",
                        );
                        condition_valid &= show_breakpoint_input_condition(
                            ui,
                            &mut self.preview_input_condition,
                            "preview_debug_input_value_type",
                        );
                        ui.checkbox(
                            &mut self.preview_pause_on_failure,
                            "Pause at the first graph or function evaluation error",
                        );
                    });
                    });
                }
                if documents {
                    if !documents_ready {
                        ui.colored_label(
                            ui.visuals().error_fg_color,
                            "An input root is required to read primary documents.",
                        );
                    }
                } else if draft.input_identity.trim().is_empty() {
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
                if documents {
                } else if draft.output_identity.trim().is_empty() {
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
                if documents {
                    ui.weak(
                        "Documents are read beneath the input root when the preview \
                         starts. The output stays in memory; no file is written.",
                    );
                } else if files {
                    ui.add_enabled(
                        !running,
                        egui::Checkbox::new(
                            &mut self.owned_pdf_worker,
                            "Extract file PDFs in an owned worker process",
                        ),
                    );
                }
                if input_too_large && !files && !documents {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        "Primary input exceeds the 64 MiB per-document limit.",
                    );
                }
                if let Some(phase) = &phase {
                    show_live_debug_state(ui, phase, debug);
                    if let Some(extraction) = &extraction {
                        ui.weak(extraction);
                    }
                    if closing {
                        ui.weak("The application closes after this preview has stopped.");
                    }
                    ui.weak(if files {
                        "The mapping is held as an in-memory snapshot; inputs are read from files."
                    } else {
                        "The mapping and input are held as an in-memory snapshot for this preview."
                    });
                } else if files {
                    ui.weak(
                        "Inputs are read from the configured files when the preview starts. \
                         Outputs stay in memory; no file is written.",
                    );
                }
                let ready = if documents { documents_ready } else { draft.can_execute() };
                footer_ready.set((ready, host_parameters_valid, condition_valid));
            }, |ui| {
                let (ready, host_parameters_valid, condition_valid) = footer_ready.get();
                ui.horizontal(|ui| match &phase {
                    Some(
                        PreviewPhase::Paused(_)
                        | PreviewPhase::PausedNode(_)
                        | PreviewPhase::PausedFunctionNode(_)
                        | PreviewPhase::PausedInput(_)
                        | PreviewPhase::PausedFunctionInput(_),
                    ) => {
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
                    Some(
                        PreviewPhase::PausedFailure(_)
                        | PreviewPhase::PausedFunctionFailure(_),
                    ) => {
                        if ui.button("Continue to error").clicked() {
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
                        let palette = crate::theme::active_palette(ui.ctx());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add_enabled(ready && host_parameters_valid,
                                egui::Button::new(egui::RichText::new("Preview").color(palette.on_action)).fill(palette.action)).clicked()
                            {
                                action = Some(PreviewAction::Execute);
                            }
                            if !documents && ui.add_enabled(ready && host_parameters_valid && condition_valid,
                                egui::Button::new("Debug preview")).clicked()
                            {
                                action = Some(PreviewAction::Debug);
                            }
                            if ui.button("Cancel").clicked() {
                                action = Some(PreviewAction::Cancel);
                            }
                        });
                    }
                });
            });

        if response.close_requested {
            action = Some(PreviewAction::Cancel);
        }
        if browse_primary_root {
            self.browse_primary_input_root();
        }
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
        let runtime_parameters = match self.host_parameters.compile() {
            Ok(parameters) => parameters,
            Err(error) => {
                self.status = "preview blocked".into();
                self.diagnostics.error("Preview blocked", error);
                return;
            }
        };
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
        let source_condition = if debug {
            match self.preview_source_condition.compile() {
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
        let node_condition = if debug {
            match self.preview_node_condition.compile() {
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
        let expression_condition = if debug {
            match self.preview_expression_condition.compile() {
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
        let input_condition = if debug {
            match self.preview_input_condition.compile() {
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
            self.status = format!(
                "preview blocked by {}",
                crate::ui_kit::plural(issues.len(), "validation issue")
            );
            self.diagnostics.validation(&self.project, issues);
            return;
        }
        if self.primary_document.is_some() {
            self.start_primary_document_preview(debug, runtime_parameters);
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
        let pause_on_failure = debug && self.preview_pause_on_failure;
        let source = self.preview_input_source;
        let extraction = (source == super::preview_service::PreviewInputSource::ConfiguredFiles
            && self.owned_pdf_worker)
            .then(|| Arc::new(super::pdf_extraction::ExtractionStatus::default()));
        let acquisition = super::preview_service::PreviewAcquisition {
            source,
            pdf_worker: extraction
                .as_ref()
                .map(|status| (self.pdf_worker_package.clone(), Arc::clone(status))),
            #[cfg(test)]
            controls: self.pdf_reader_controls,
        };
        let service = extraction.is_some().then(|| self.preview_service.clone());
        let worker = OwnedWorker::spawn("ferrule-preview", move || {
            run_preview_worker(
                project,
                draft,
                project_path,
                saved_path,
                runtime_parameters,
                debug,
                value_condition,
                position_condition,
                source_condition,
                node_condition,
                expression_condition,
                input_condition,
                pause_on_failure,
                event_tx,
                command_rx,
                worker_cancelled,
                acquisition,
            )
        });
        let worker = match worker {
            Ok(worker) => worker,
            Err(error) => {
                self.status = "preview failed".to_string();
                self.diagnostics.error(
                    "Preview failed",
                    format!("Could not start the preview worker: {error}"),
                );
                return;
            }
        };
        self.pending_preview = Some(PendingPreview {
            worker,
            receiver,
            commands,
            cancelled,
            started: Instant::now(),
            input_path,
            target_label,
            debug,
            phase: PreviewPhase::Running,
            extraction,
            service,
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
        // Pause events are relayed only while the exact worker runs; its
        // outcome is the joined thread, never a message or a closed channel.
        let event = if pending.worker.is_finished() {
            None
        } else {
            pending.receiver.try_recv().ok()
        };
        let Some(event) = event else {
            match pending.worker.poll() {
                Some(exit) => self.finish_preview(exit),
                None => {
                    if !matches!(
                        pending.phase,
                        PreviewPhase::Paused(_)
                            | PreviewPhase::PausedNode(_)
                            | PreviewPhase::PausedFunctionNode(_)
                            | PreviewPhase::PausedInput(_)
                            | PreviewPhase::PausedFunctionInput(_)
                            | PreviewPhase::PausedFailure(_)
                            | PreviewPhase::PausedFunctionFailure(_)
                    ) {
                        ctx.request_repaint_after(Duration::from_millis(100));
                    }
                }
            }
            return;
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
            PreviewWorkerEvent::PausedNode(value) => {
                if matches!(pending.phase, PreviewPhase::Stopping) {
                    pending.command(PreviewCommand::Cancel);
                } else {
                    self.status = format!("paused after graph node #{}", value.node);
                    pending.phase = PreviewPhase::PausedNode(value);
                }
                ctx.request_repaint();
            }
            PreviewWorkerEvent::PausedFunctionNode(value) => {
                if matches!(pending.phase, PreviewPhase::Stopping) {
                    pending.command(PreviewCommand::Cancel);
                } else {
                    self.status = format!(
                        "paused after function #{} node #{}",
                        value.function.get(),
                        value.node
                    );
                    pending.phase = PreviewPhase::PausedFunctionNode(value);
                }
                ctx.request_repaint();
            }
            PreviewWorkerEvent::PausedInput(input) => {
                if matches!(pending.phase, PreviewPhase::Stopping) {
                    pending.command(PreviewCommand::Cancel);
                } else {
                    self.status = format!(
                        "paused at node #{} input #{}",
                        input.consumer,
                        input.input_index + 1
                    );
                    pending.phase = PreviewPhase::PausedInput(input);
                }
                ctx.request_repaint();
            }
            PreviewWorkerEvent::PausedFunctionInput(input) => {
                if matches!(pending.phase, PreviewPhase::Stopping) {
                    pending.command(PreviewCommand::Cancel);
                } else {
                    self.status = format!(
                        "paused at function #{} node #{} input #{}",
                        input.function.get(),
                        input.consumer,
                        input.input_index + 1
                    );
                    pending.phase = PreviewPhase::PausedFunctionInput(input);
                }
                ctx.request_repaint();
            }
            PreviewWorkerEvent::PausedFailure(failure) => {
                if matches!(pending.phase, PreviewPhase::Stopping) {
                    pending.command(PreviewCommand::Cancel);
                } else {
                    self.status = format!("paused on graph node #{} error", failure.node);
                    pending.phase = PreviewPhase::PausedFailure(failure);
                }
                ctx.request_repaint();
            }
            PreviewWorkerEvent::PausedFunctionFailure(failure) => {
                if matches!(pending.phase, PreviewPhase::Stopping) {
                    pending.command(PreviewCommand::Cancel);
                } else {
                    self.status = format!(
                        "paused on function #{} node #{} error",
                        failure.function.get(),
                        failure.node
                    );
                    pending.phase = PreviewPhase::PausedFunctionFailure(failure);
                }
                ctx.request_repaint();
            }
        }
    }

    /// Applies the outcome of a joined preview worker.
    fn finish_preview(&mut self, exit: WorkerExit<PreviewCompletion>) {
        let Some(pending) = self.pending_preview.take() else {
            return;
        };
        let (result, trace) = match exit {
            WorkerExit::Reported(completion) => completion,
            WorkerExit::Unreported | WorkerExit::Panicked => (
                Err(PreviewRunError::Failed(
                    "preview worker stopped unexpectedly".into(),
                )),
                crate::run_report::TraceReport::default(),
            ),
        };
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
                    "previewed {} for {}",
                    crate::ui_kit::plural(report.records_written, "record"),
                    pending.target_label
                );
                self.run_report = Some(
                    crate::run_report::RunReportView::new(report)
                        .with_project_labels(&self.project),
                );
                self.show_run_report = true;
                self.preview_draft = None;
                self.diagnostics.clear();
            }
            Err(PreviewRunError::Failed(error)) => {
                self.status = "preview failed".to_string();
                self.diagnostics.error("Preview failed", error);
            }
            Err(PreviewRunError::Cancelled) => {}
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

#[path = "preview_debug_view.rs"]
mod debug_view;
pub(super) use debug_view::{
    show_live_debug_state, show_live_failure_debug_state, show_live_function_failure_debug_state,
    show_live_function_input_debug_state, show_live_function_node_debug_state,
    show_live_input_debug_state, show_live_node_debug_state,
};

#[path = "preview_documents.rs"]
mod documents;

#[path = "preview_worker.rs"]
mod preview_worker;
use preview_worker::run_preview_worker;

#[cfg(test)]
#[path = "preview_worker_tests.rs"]
mod preview_worker_tests;
