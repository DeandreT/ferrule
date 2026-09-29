use super::*;

use std::sync::mpsc::{Receiver, TryRecvError};
use std::time::Instant;

use mapping::PipelineInput;

pub(super) struct PendingPipelineRun {
    receiver: Receiver<Result<cli::PipelineRunOutcome, String>>,
    started: Instant,
    path: PathBuf,
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
        let running = self.pending_pipeline_run.is_some();
        let mut close = false;
        let mut run = false;
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
                ui.horizontal(|ui| {
                    let can_run = !running
                        && draft.issues.is_empty()
                        && draft.inputs.iter().all(|input| !input.path.trim().is_empty())
                        && draft.outputs.iter().any(|output| !output.path.trim().is_empty());
                    if ui.add_enabled(can_run, egui::Button::new("Run pipeline")).clicked() {
                        run = true;
                    }
                    if running {
                        ui.spinner();
                        ui.label("Running pipeline");
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                });
            });
        if close {
            self.pipeline_run_draft = None;
        } else if run {
            self.start_pipeline_run();
        }
    }

    pub(super) fn start_pipeline_run(&mut self) {
        if self.pending_pipeline_run.is_some() {
            return;
        }
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
        let (tx, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let result = cli::run_pipeline_file(&worker_path, &inputs, &outputs)
                .map_err(|error| format!("{error:#}"));
            let _ = tx.send(result);
        });
        self.pending_pipeline_run = Some(PendingPipelineRun {
            receiver,
            started: Instant::now(),
            path,
        });
        self.show_run_report = false;
        self.status = "running pipeline".into();
        self.diagnostics.clear();
    }

    pub(super) fn poll_pipeline_run(&mut self, ctx: &egui::Context) {
        let Some(pending) = &self.pending_pipeline_run else {
            return;
        };
        let result = match pending.receiver.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
                return;
            }
            Err(TryRecvError::Disconnected) => Err("pipeline worker stopped unexpectedly".into()),
        };
        let pending = self
            .pending_pipeline_run
            .take()
            .expect("pending run exists");
        match result {
            Ok(outcome) => {
                self.status = format!(
                    "pipeline completed {} stage(s) and wrote {} file(s)",
                    outcome.stages_executed.len(),
                    outcome.artifacts.len()
                );
                let report = crate::run_report::RunReport::from_pipeline_outcome(
                    outcome,
                    pending.path,
                    pending.started.elapsed(),
                );
                self.run_report = Some(crate::run_report::RunReportView::new(report));
                self.show_run_report = true;
                self.pipeline_run_draft = None;
            }
            Err(error) => {
                self.status = "pipeline run failed".into();
                self.diagnostics.error("Pipeline run failed", error);
            }
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
