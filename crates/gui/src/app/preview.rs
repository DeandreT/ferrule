use std::path::PathBuf;

use anyhow::{Context as _, bail};

use super::*;
use crate::preview::{LoadedPreviewSource, PreviewDraft, PreviewTarget};

enum PreviewAction {
    Cancel,
    Execute,
}

impl FerruleApp {
    pub(super) fn can_preview(&self) -> bool {
        matches!(
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
        let Some(draft) = &mut self.preview_draft else {
            return;
        };
        let input_size = draft.input_text.len();
        let input_too_large = input_size > cli::MAX_PAYLOAD_DOCUMENT_BYTES;
        egui::Window::new("Preview mapping")
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(760.0, 620.0))
            .min_size(egui::vec2(520.0, 360.0))
            .show(ctx, |ui| {
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
                ui.separator();
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        action = Some(PreviewAction::Cancel);
                    }
                    if ui
                        .add_enabled(draft.can_execute(), egui::Button::new("Preview"))
                        .clicked()
                    {
                        action = Some(PreviewAction::Execute);
                    }
                });
            });

        match action {
            Some(PreviewAction::Cancel) => self.preview_draft = None,
            Some(PreviewAction::Execute) => self.execute_preview(),
            None => {}
        }
    }

    pub(super) fn execute_preview(&mut self) {
        let issues = cli::validate(&self.project);
        if !issues.is_empty() {
            self.status = format!("preview blocked by {} validation issue(s)", issues.len());
            self.diagnostics.validation(issues);
            return;
        }
        let Some(draft) = self.preview_draft.as_ref() else {
            return;
        };
        if draft.input_text.len() > cli::MAX_PAYLOAD_DOCUMENT_BYTES {
            self.status = "preview failed".to_string();
            self.diagnostics.error(
                "Preview failed",
                format!(
                    "primary input exceeds the {} MiB per-document limit",
                    cli::MAX_PAYLOAD_DOCUMENT_BYTES / (1024 * 1024)
                ),
            );
            return;
        }
        let draft = draft.clone();
        let started = std::time::Instant::now();
        let trace = crate::run_report::TraceCollector::new();
        let result = self.run_preview_payload(&draft, &trace);
        match result {
            Ok(outcome) => {
                let report = crate::run_report::RunReport::from_payload_with_trace(
                    outcome,
                    PathBuf::from(draft.input_identity.trim()),
                    started.elapsed(),
                    trace.finish(),
                );
                self.status = format!(
                    "previewed {} record(s) for {}",
                    report.records_written,
                    draft.target.label()
                );
                self.run_report = Some(crate::run_report::RunReportView::new(report));
                self.show_run_report = true;
                self.preview_draft = None;
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = "preview failed".to_string();
                self.diagnostics
                    .error("Preview failed", format!("{error:#}"));
            }
        }
    }

    fn run_preview_payload(
        &self,
        draft: &PreviewDraft,
        trace: &crate::run_report::TraceCollector,
    ) -> anyhow::Result<cli::PayloadRunOutcome> {
        if !draft.can_execute() {
            bail!("preview input or logical format identity is invalid");
        }
        let project_path = self.preview_project_path()?;
        let loaded_sources = crate::preview::load_required_sources(
            &self.project,
            self.document.saved_path(),
            &draft.target,
        )?;
        let named_inputs = named_payload_inputs(&loaded_sources)?;
        let input_path = PathBuf::from(draft.input_identity.trim());
        let output_path = PathBuf::from(draft.output_identity.trim());
        let primary = cli::PayloadDocument::new(&input_path, draft.input_text.as_bytes())?;
        let options = cli::PayloadRunOptions::new(primary)
            .with_extra_sources(&named_inputs)
            .with_output_path(&output_path)
            .with_target(draft.target.selection())
            .with_trace_sink(trace);
        cli::run_project_value_payloads(&self.project, &project_path, &options)
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
