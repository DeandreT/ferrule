use super::*;

use crate::extra_targets::remove_extra_target;

impl FerruleApp {
    pub(super) fn begin_extra_target(&mut self) {
        self.extra_target_draft = Some(ExtraTargetDraft::default());
    }

    pub(super) fn edit_extra_target(&mut self, index: usize) {
        let Some(target) = self.project.extra_targets.get(index) else {
            return;
        };
        self.extra_target_draft = Some(ExtraTargetDraft::from_target(index, target));
    }

    pub(super) fn stage_extra_target_schema(&mut self, path: PathBuf) {
        match crate::new_mapping::import_schema(&path) {
            Ok(schema) => {
                let Some(draft) = self.extra_target_draft.as_mut() else {
                    return;
                };
                if draft.name.trim().is_empty() {
                    draft.name.clone_from(&schema.name);
                }
                draft.schema = Some(schema);
                self.status = format!("loaded target schema {}", path.display());
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = "failed to load target schema".to_string();
                self.diagnostics
                    .error("Schema import failed", error.to_string());
            }
        }
    }

    pub(super) fn show_extra_target_setup(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.extra_target_draft.as_mut() else {
            return;
        };
        let editing = draft.editing.is_some();
        let dialog_idle = self.pending_dialog.is_none();
        let schema_label = draft
            .schema
            .as_ref()
            .map_or("Not selected", |schema| schema.name.as_str());
        let can_save = !draft.name.trim().is_empty() && draft.schema.is_some();
        let mut action = None;
        egui::Window::new(if editing { "Edit Target" } else { "Add Target" })
            .collapsible(false)
            .resizable(true)
            .default_width(540.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Grid::new("extra_target_fields")
                    .num_columns(3)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.strong("Name");
                        ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(280.0));
                        ui.end_row();

                        ui.strong("Output");
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.output_path)
                                .desired_width(280.0)
                                .hint_text("Optional stored output path"),
                        );
                        if ui
                            .add_enabled(dialog_idle, egui::Button::new("Choose..."))
                            .clicked()
                        {
                            action = Some(ExtraTargetAction::ChooseOutput);
                        }
                        ui.end_row();

                        ui.strong("Schema");
                        ui.label(schema_label);
                        if ui
                            .add_enabled(dialog_idle, egui::Button::new("Choose..."))
                            .clicked()
                        {
                            action = Some(ExtraTargetAction::ChooseSchema);
                        }
                        ui.end_row();
                    });
                ui.separator();
                ui.strong("Output format");
                show_target_format_options(ui, &draft.output_path, &mut draft.options);
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(dialog_idle, egui::Button::new("Cancel"))
                        .clicked()
                    {
                        action = Some(ExtraTargetAction::Cancel);
                    }
                    let label = if editing { "Save target" } else { "Add target" };
                    if ui
                        .add_enabled(dialog_idle && can_save, egui::Button::new(label))
                        .clicked()
                    {
                        action = Some(ExtraTargetAction::Save);
                    }
                });
            });

        match action {
            Some(ExtraTargetAction::ChooseSchema) => {
                self.pending_dialog = Some((
                    DialogKind::BrowseExtraTargetSchema,
                    pick_file("schema", &["xsd", "json"]),
                ));
            }
            Some(ExtraTargetAction::ChooseOutput) => {
                self.pending_dialog = Some((
                    DialogKind::BrowseExtraTargetOutput,
                    save_file(
                        "output data",
                        &["xml", "json", "jsonl", "csv", "xlsx", "edi", "txt"],
                        &draft.output_path,
                    ),
                ));
            }
            Some(ExtraTargetAction::Cancel) => {
                self.extra_target_draft = None;
                self.status = "target edit cancelled".to_string();
            }
            Some(ExtraTargetAction::Save) => self.finish_extra_target(),
            None => {}
        }
    }

    pub(super) fn finish_extra_target(&mut self) {
        let Some(draft) = self.extra_target_draft.as_ref() else {
            return;
        };
        match draft.clone().build(&self.project.extra_targets) {
            Ok((Some(index), target)) => {
                let saved_nodes = self
                    .mapping_workspace
                    .target_canvases
                    .get(&index)
                    .map(|canvas| CanvasLayout::capture_nodes(&canvas.snarl));
                self.project.extra_targets[index] = target;
                if let Some(saved_nodes) = saved_nodes
                    && let Some(canvas) = self.mapping_workspace.target_canvases.get_mut(&index)
                {
                    canvas.snarl = canvas_build::build_named_target_snarl(&self.project, index);
                    CanvasLayout::apply_nodes(&saved_nodes, &mut canvas.snarl);
                }
                self.extra_target_draft = None;
                self.open_target_tab(index);
                self.status = "target updated".to_string();
                self.diagnostics.clear();
            }
            Ok((None, target)) => {
                let index = self.project.extra_targets.len();
                self.project.extra_targets.push(target);
                self.extra_target_draft = None;
                self.open_target_tab(index);
                self.status = "target added".to_string();
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = "target is incomplete".to_string();
                self.diagnostics
                    .error("Target not saved", error.to_string());
            }
        }
    }

    pub(super) fn show_extra_target_removal_confirmation(&mut self, ctx: &egui::Context) {
        let Some(index) = self.pending_extra_target_removal else {
            return;
        };
        let Some(target) = self.project.extra_targets.get(index) else {
            self.pending_extra_target_removal = None;
            return;
        };
        let name = target.name.clone();
        let mut remove = None;
        egui::Window::new("Remove Target")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!(
                    "Remove {name} and its target scope? Shared graph nodes will be kept."
                ));
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        remove = Some(false);
                    }
                    if ui.button("Remove target").clicked() {
                        remove = Some(true);
                    }
                });
            });
        match remove {
            Some(true) => {
                self.pending_extra_target_removal = None;
                self.remove_extra_target_now(index);
            }
            Some(false) => self.pending_extra_target_removal = None,
            None => {}
        }
    }

    pub(super) fn remove_extra_target_now(&mut self, index: usize) {
        let Some(target) = remove_extra_target(&mut self.project.extra_targets, index) else {
            return;
        };
        self.mapping_workspace.remove_target(index);
        self.selected_scope.clear();
        self.status = format!("removed target {}", target.name);
        let issues = cli::validate(&self.project);
        if issues.is_empty() {
            self.diagnostics.clear();
        } else {
            self.diagnostics.validation(issues);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DocumentKind {
    Automatic,
    Xml,
    Json,
    JsonLines,
}

fn document_kind(options: &mapping::FormatOptions) -> DocumentKind {
    if options.xml_document {
        DocumentKind::Xml
    } else if options.json_lines {
        DocumentKind::JsonLines
    } else if options.json_document {
        DocumentKind::Json
    } else {
        DocumentKind::Automatic
    }
}

fn set_document_kind(options: &mut mapping::FormatOptions, kind: DocumentKind) {
    options.xml_document = kind == DocumentKind::Xml;
    options.json_document = kind == DocumentKind::Json;
    options.json_lines = kind == DocumentKind::JsonLines;
}

fn show_target_format_options(
    ui: &mut egui::Ui,
    output_path: &str,
    options: &mut mapping::FormatOptions,
) {
    let extension = std::path::Path::new(output_path.trim())
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mut kind = document_kind(options);
    ui.horizontal_wrapped(|ui| {
        ui.selectable_value(&mut kind, DocumentKind::Automatic, "From path");
        ui.selectable_value(&mut kind, DocumentKind::Xml, "XML");
        ui.selectable_value(&mut kind, DocumentKind::Json, "JSON");
        ui.selectable_value(&mut kind, DocumentKind::JsonLines, "JSON Lines");
    });
    set_document_kind(options, kind);
    if matches!(extension.as_str(), "csv" | "txt") {
        let headers = options.has_header_row.get_or_insert(true);
        ui.checkbox(headers, "Header row");
        let mut delimiter = options.delimiter.unwrap_or(',').to_string();
        ui.horizontal(|ui| {
            ui.label("Delimiter");
            if ui
                .add(egui::TextEdit::singleline(&mut delimiter).char_limit(1))
                .changed()
            {
                options.delimiter = delimiter.chars().next();
            }
        });
    }
    if extension == "xlsx" {
        ui.checkbox(
            &mut options.xlsx_update_existing,
            "Update existing workbook",
        );
    }
}

enum ExtraTargetAction {
    ChooseSchema,
    ChooseOutput,
    Cancel,
    Save,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_kind_selection_normalizes_mutually_exclusive_flags() {
        let mut options = mapping::FormatOptions {
            xml_document: true,
            json_document: true,
            json_lines: true,
            ..mapping::FormatOptions::default()
        };

        assert_eq!(document_kind(&options), DocumentKind::Xml);
        set_document_kind(&mut options, DocumentKind::JsonLines);
        assert!(!options.xml_document);
        assert!(!options.json_document);
        assert!(options.json_lines);

        set_document_kind(&mut options, DocumentKind::Automatic);
        assert!(!options.xml_document);
        assert!(!options.json_document);
        assert!(!options.json_lines);
    }
}
