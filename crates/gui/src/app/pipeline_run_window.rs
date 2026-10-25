//! The "Run or preview pipeline" window: host inputs, output paths, debug
//! breakpoints, the paused state, and an action footer that stays visible
//! however long the settings above it grow.

use super::*;

use mapping::PipelineInput;

/// Space kept between the window and the viewport edges.
const VIEWPORT_MARGIN: f32 = 16.0;
/// Below this the body would only show a sliver; the window may then extend
/// past a very short viewport rather than hide the footer's actions.
const MIN_WINDOW_HEIGHT: f32 = 240.0;
const DEFAULT_WINDOW_HEIGHT: f32 = 900.0;

impl FerruleApp {
    pub(in crate::app) fn show_pipeline_run_setup(&mut self, ctx: &egui::Context) {
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
        let previewing = self
            .pending_pipeline_run
            .as_ref()
            .is_some_and(|run| run.preview);
        let cancel_label = if previewing {
            "Cancel preview"
        } else {
            "Cancel pipeline"
        };
        let running = phase.is_some();
        let extraction = self
            .pending_pipeline_run
            .as_ref()
            .and_then(|run| run.extraction.as_ref())
            .and_then(|status| status.progress_label());
        let closing = self.close_after_pipeline_run;
        let candidates = breakpoint_candidates(&draft.pipeline);
        if self
            .pipeline_run_breakpoint
            .as_ref()
            .is_some_and(|selected| !candidates.contains(selected))
        {
            self.pipeline_run_breakpoint = None;
        }
        let mut open = true;
        let mut run = None;
        let mut command = None;
        let mut condition_valid = true;
        let mut host_parameters_valid = true;
        let max_height =
            (ctx.content_rect().height() - 2.0 * VIEWPORT_MARGIN).max(MIN_WINDOW_HEIGHT);
        let window = egui::Window::new("Run or preview pipeline")
            .collapsible(false)
            .default_width(crate::theme::DialogWidth::Wide.px())
            .min_width(crate::theme::DialogWidth::Narrow.px())
            .default_height(DEFAULT_WINDOW_HEIGHT.min(max_height))
            .max_height(max_height)
            .resizable(true);
        // A running pipeline is stopped with its cancel action, never by
        // closing the window that shows its pause state.
        let window = if running {
            window
        } else {
            window.open(&mut open)
        };
        window.show(ctx, |ui| {
            let footer_id = egui::Id::unique("pipeline_run_footer_height");
            let footer_height = ui
                .data(|data| data.get_temp::<f32>(footer_id))
                .unwrap_or_else(|| {
                    ui.spacing().interact_size.y + 2.0 * ui.spacing().item_spacing.y + 6.0
                });
            egui::ScrollArea::vertical()
                .id_salt("pipeline_run_setup_body")
                .auto_shrink([false, false])
                .max_height((ui.available_height() - footer_height).max(0.0))
                .show(ui, |ui| {
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
                                path_field(ui, !running, &mut input.path, "Input file path");
                                ui.end_row();
                            }
                        });
                    ui.separator();
                    ui.add_enabled_ui(!running, |ui| {
                        host_parameters_valid = draft.host_parameters.show(ui);
                    });
                    ui.separator();
                    ui.strong("Preview formats");
                    ui.small("These paths identify output formats for Preview. Preview does not write files. Dynamic document targets use a base directory.");
                    egui::ScrollArea::vertical()
                        .id_salt("pipeline_preview_formats")
                        .max_height(crate::theme::ListHeight::Medium.px())
                        .show(ui, |ui| {
                            egui::Grid::new("pipeline_preview_paths")
                                .num_columns(2)
                                .show(ui, |ui| {
                                    for output in &mut draft.outputs {
                                        ui.label(output_label(output));
                                        path_field(
                                            ui,
                                            !running,
                                            &mut output.preview_path,
                                            "Format path or base directory",
                                        );
                                        ui.end_row();
                                    }
                                });
                        });
                    ui.separator();
                    ui.strong("Publish outputs on Run");
                    ui.small("Choose files to publish after every stage succeeds. Blank outputs are not saved. Dynamic document targets use a base directory.");
                    egui::ScrollArea::vertical()
                        .id_salt("pipeline_publish_outputs")
                        .max_height(crate::theme::ListHeight::Medium.px())
                        .show(ui, |ui| {
                            egui::Grid::new("pipeline_output_paths")
                                .num_columns(2)
                                .show(ui, |ui| {
                                    for output in &mut draft.outputs {
                                        ui.label(output_label(output));
                                        path_field(
                                            ui,
                                            !running,
                                            &mut output.path,
                                            "Output path or base directory",
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
                        condition_valid &= preview_ui::show_breakpoint_node_condition(
                            ui,
                            &mut self.pipeline_run_node_condition,
                        );
                        condition_valid &= preview_ui::show_breakpoint_expression_condition(
                            ui,
                            &mut self.pipeline_run_expression_condition,
                            "pipeline_debug_expression_value_type",
                        );
                        if self.pipeline_run_expression_condition.enabled {
                            if self
                                .pipeline_run_expression_stage
                                .as_ref()
                                .is_some_and(|selected| {
                                    !draft
                                        .pipeline
                                        .stages
                                        .iter()
                                        .any(|stage| stage.id == selected.as_str())
                                })
                            {
                                self.pipeline_run_expression_stage = None;
                            }
                            egui::ComboBox::from_id_salt("pipeline_debug_expression_stage")
                                .selected_text(
                                    self.pipeline_run_expression_stage
                                        .as_deref()
                                        .unwrap_or("Every stage"),
                                )
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.pipeline_run_expression_stage,
                                        None,
                                        "Every stage",
                                    );
                                    for stage in &draft.pipeline.stages {
                                        ui.selectable_value(
                                            &mut self.pipeline_run_expression_stage,
                                            Some(stage.id.clone()),
                                            &stage.id,
                                        );
                                    }
                                });
                        }
                        condition_valid &= preview_ui::show_breakpoint_input_condition(
                            ui,
                            &mut self.pipeline_run_input_condition,
                            "pipeline_debug_input_value_type",
                        );
                        if self.pipeline_run_input_condition.enabled {
                            if self.pipeline_run_input_stage.as_ref().is_some_and(|selected| {
                                !draft
                                    .pipeline
                                    .stages
                                    .iter()
                                    .any(|stage| stage.id == selected.as_str())
                            }) {
                                self.pipeline_run_input_stage = None;
                            }
                            egui::ComboBox::from_id_salt("pipeline_debug_input_stage")
                                .selected_text(
                                    self.pipeline_run_input_stage
                                        .as_deref()
                                        .unwrap_or("Every stage"),
                                )
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.pipeline_run_input_stage,
                                        None,
                                        "Every stage",
                                    );
                                    for stage in &draft.pipeline.stages {
                                        ui.selectable_value(
                                            &mut self.pipeline_run_input_stage,
                                            Some(stage.id.clone()),
                                            &stage.id,
                                        );
                                    }
                                });
                        }
                        ui.checkbox(
                            &mut self.pipeline_run_pause_on_failure,
                            "Pause at the first graph or function evaluation error",
                        );
                        if self.pipeline_run_pause_on_failure {
                            if self.pipeline_run_failure_stage.as_ref().is_some_and(|selected| {
                                !draft.pipeline.stages.iter().any(|stage| stage.id == *selected)
                            }) {
                                self.pipeline_run_failure_stage = None;
                            }
                            egui::ComboBox::from_id_salt("pipeline_debug_failure_stage")
                                .selected_text(
                                    self.pipeline_run_failure_stage
                                        .as_deref()
                                        .unwrap_or("Every stage"),
                                )
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(
                                        &mut self.pipeline_run_failure_stage,
                                        None,
                                        "Every stage",
                                    );
                                    for stage in &draft.pipeline.stages {
                                        ui.selectable_value(
                                            &mut self.pipeline_run_failure_stage,
                                            Some(stage.id.clone()),
                                            &stage.id,
                                        );
                                    }
                                });
                        }
                    }
                    ui.add_enabled(
                        !running,
                        egui::Checkbox::new(
                            &mut self.owned_pdf_worker,
                            "Extract file PDFs in an owned worker process",
                        ),
                    );
                    match &phase {
                        Some(PipelineRunPhase::Running) => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.strong(match (previewing, debug) {
                                    (true, true) => "Debug pipeline preview is running",
                                    (true, false) => "Pipeline preview is running",
                                    (false, true) => "Debug pipeline is running",
                                    (false, false) => "Pipeline is running",
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
                        Some(PipelineRunPhase::PausedNode(stage, value)) => {
                            ui.strong(format!("Paused in stage `{stage}`"));
                            preview_ui::show_live_node_debug_state(ui, value);
                        }
                        Some(PipelineRunPhase::PausedFunctionNode(stage, value)) => {
                            ui.strong(format!("Paused in stage `{stage}`"));
                            preview_ui::show_live_function_node_debug_state(ui, value);
                        }
                        Some(PipelineRunPhase::PausedInput(stage, input)) => {
                            ui.strong(format!("Paused in stage `{stage}`"));
                            preview_ui::show_live_input_debug_state(ui, input);
                        }
                        Some(PipelineRunPhase::PausedFunctionInput(stage, input)) => {
                            ui.strong(format!("Paused in stage `{stage}`"));
                            preview_ui::show_live_function_input_debug_state(ui, input);
                        }
                        Some(PipelineRunPhase::PausedFailure(stage, failure)) => {
                            ui.strong(format!("Paused in stage `{stage}`"));
                            preview_ui::show_live_failure_debug_state(ui, failure);
                        }
                        Some(PipelineRunPhase::PausedFunctionFailure(stage, failure)) => {
                            ui.strong(format!("Paused in stage `{stage}`"));
                            preview_ui::show_live_function_failure_debug_state(ui, failure);
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
                                ui.strong(if previewing {
                                    "Stopping pipeline preview"
                                } else {
                                    "Stopping pipeline"
                                });
                            });
                        }
                        None => {}
                    }
                    if let Some(extraction) = &extraction {
                        ui.weak(extraction);
                    }
                    if closing {
                        ui.weak("The application closes after this pipeline has stopped.");
                    }
                });
            // The footer sits below the body's scroll area, so its actions
            // stay in view; its measured height sizes the body next frame.
            let footer_top = ui.cursor().top();
            ui.separator();
            ui.horizontal(|ui| match &phase {
                None => {
                    let can_run = draft.issues.is_empty()
                        && draft.inputs.iter().all(|input| !input.path.trim().is_empty())
                        && draft.outputs.iter().any(|output| !output.path.trim().is_empty())
                        && host_parameters_valid;
                    let can_preview = draft.issues.is_empty()
                        && draft.inputs.iter().all(|input| !input.path.trim().is_empty())
                        && draft
                            .outputs
                            .iter()
                            .all(|output| !output.preview_path.trim().is_empty())
                        && host_parameters_valid;
                    if ui.add_enabled(can_run, egui::Button::new("Run pipeline")).clicked() {
                        run = Some((false, false));
                    }
                    if ui
                        .add_enabled(can_run && condition_valid, egui::Button::new("Debug pipeline"))
                        .clicked()
                    {
                        run = Some((false, true));
                    }
                    if ui
                        .add_enabled(can_preview, egui::Button::new("Preview pipeline"))
                        .clicked()
                    {
                        run = Some((true, false));
                    }
                    if ui
                        .add_enabled(
                            can_preview && condition_valid,
                            egui::Button::new("Debug preview"),
                        )
                        .clicked()
                    {
                        run = Some((true, true));
                    }
                }
                Some(PipelineRunPhase::Running) => {
                    if debug && ui.button("Pause at next write").clicked() {
                        command = Some(PipelineRunCommand::Pause);
                    }
                    if ui.button(cancel_label).clicked() {
                        command = Some(PipelineRunCommand::Cancel);
                    }
                }
                Some(
                    PipelineRunPhase::Paused(..)
                    | PipelineRunPhase::PausedNode(..)
                    | PipelineRunPhase::PausedFunctionNode(..)
                    | PipelineRunPhase::PausedInput(..)
                    | PipelineRunPhase::PausedFunctionInput(..),
                ) => {
                    if ui.button("Step").clicked() {
                        command = Some(PipelineRunCommand::Step);
                    }
                    if ui.button("Continue").clicked() {
                        command = Some(PipelineRunCommand::Continue);
                    }
                    if ui.button(cancel_label).clicked() {
                        command = Some(PipelineRunCommand::Cancel);
                    }
                }
                Some(
                    PipelineRunPhase::PausedFailure(..) | PipelineRunPhase::PausedFunctionFailure(..),
                ) => {
                    if ui.button("Continue to error").clicked() {
                        command = Some(PipelineRunCommand::Continue);
                    }
                    if ui.button(cancel_label).clicked() {
                        command = Some(PipelineRunCommand::Cancel);
                    }
                }
                Some(PipelineRunPhase::Publishing | PipelineRunPhase::Stopping) => {}
            });
            let measured = ui.min_rect().bottom() - footer_top;
            ui.data_mut(|data| data.insert_temp(footer_id, measured));
        });
        if !open {
            self.pipeline_run_draft = None;
        } else if let Some((preview, debug)) = run {
            self.start_pipeline_execution_mode(preview, debug);
        } else if let Some(command) = command {
            self.pipeline_run_command(command);
        }
    }
}

/// A path field that fills its grid column and shows the whole path on hover,
/// since long absolute paths do not fit even a wide window.
fn path_field(ui: &mut egui::Ui, enabled: bool, path: &mut String, hint: &str) {
    let response = ui.add_enabled(
        enabled,
        egui::TextEdit::singleline(path)
            .hint_text(hint)
            .desired_width(f32::INFINITY),
    );
    if !path.is_empty() {
        response
            .on_hover_text(path.as_str())
            .on_disabled_hover_text(path.as_str());
    }
}

fn output_label(output: &crate::pipeline_run::PipelineOutputDraft) -> String {
    match &output.target {
        None => format!("{} / Primary", output.stage),
        Some(name) => format!("{} / {name}", output.stage),
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
