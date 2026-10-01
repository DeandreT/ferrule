use super::*;

use crate::extra_targets::remove_extra_target;

#[path = "extra_targets/fixed_width.rs"]
mod fixed_width;

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
        if crate::new_mapping::is_flextext_configuration(&path) {
            match crate::new_mapping::FlexTextBoundaryDraft::from_configuration(path) {
                Ok(flextext) => {
                    let Some(draft) = self.extra_target_draft.as_mut() else {
                        return;
                    };
                    if draft.name.trim().is_empty() {
                        draft.name = flextext.layout().root_name().to_owned();
                    }
                    draft.flextext_draft = Some(Box::new(flextext));
                    draft.protobuf_draft = None;
                    draft.fixed_width_draft = None;
                    draft.csv_dialect_draft = None;
                    self.status = "loaded target FlexText layout".to_owned();
                    self.diagnostics.clear();
                }
                Err(error) => {
                    self.status = "failed to load target FlexText layout".to_owned();
                    self.diagnostics
                        .error("FlexText layout import failed", format!("{error:#}"));
                }
            }
            return;
        }
        if crate::new_mapping::is_protobuf_schema(&path) {
            match crate::new_mapping::ProtobufBoundaryDraft::from_schema(path) {
                Ok(protobuf) => {
                    let Some(draft) = self.extra_target_draft.as_mut() else {
                        return;
                    };
                    draft.protobuf_draft = Some(Box::new(protobuf));
                    draft.flextext_draft = None;
                    draft.fixed_width_draft = None;
                    draft.csv_dialect_draft = None;
                    self.status =
                        "loaded target Protocol Buffers schema; choose a root message".to_owned();
                    self.diagnostics.clear();
                }
                Err(error) => {
                    self.status = "failed to load target schema".to_owned();
                    self.diagnostics.error(
                        "Protocol Buffers schema import failed",
                        format!("{error:#}"),
                    );
                }
            }
            return;
        }
        let loaded = crate::new_mapping::import_schema(&path).and_then(|schema| {
            let pending_options = self
                .extra_target_draft
                .as_ref()
                .and_then(|draft| {
                    draft
                        .protobuf_draft
                        .as_ref()
                        .map(|protobuf| protobuf.options())
                        .or_else(|| {
                            draft
                                .flextext_draft
                                .as_ref()
                                .map(|flextext| flextext.options())
                        })
                })
                .transpose()?;
            if let Some(draft) = self.extra_target_draft.as_ref() {
                crate::new_mapping::validate_schema_replacement(
                    pending_options.as_ref().unwrap_or(&draft.options),
                    &schema,
                )?;
                if let Some(pending) = &draft.fixed_width_draft {
                    pending
                        .layout_for_schema(&schema)
                        .map_err(anyhow::Error::msg)?;
                } else if pending_options.is_none()
                    && let Some(layout) = &draft.options.fixed_width
                {
                    if draft.schema.as_ref() != Some(&schema) {
                        anyhow::bail!("the existing fixed-width widths belong to the current field order; choose From path before replacing this schema");
                    }
                    crate::extra_targets::validate_layout_for_schema(&schema, layout)
                        .map_err(anyhow::Error::msg)?;
                }
            }
            Ok((schema, pending_options))
        });
        match loaded {
            Ok((schema, pending_options)) => {
                let Some(draft) = self.extra_target_draft.as_mut() else {
                    return;
                };
                if draft.name.trim().is_empty() {
                    draft.name.clone_from(&schema.name);
                }
                draft.protobuf_draft = None;
                draft.flextext_draft = None;
                draft.schema = Some(schema);
                if let Some(options) = pending_options {
                    draft.options = options;
                }
                self.status = if draft.options.protobuf.is_some() {
                    "loaded matching target schema; kept Protocol Buffers format".to_owned()
                } else if draft.options.flextext.is_some() {
                    "loaded matching target schema; kept FlexText format".to_owned()
                } else {
                    format!("loaded target schema {}", path.display())
                };
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = "failed to load target schema".to_string();
                self.diagnostics
                    .error("Schema import failed", format!("{error:#}"));
            }
        }
    }

    pub(super) fn show_extra_target_setup(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.extra_target_draft.as_mut() else {
            return;
        };
        let editing = draft.editing.is_some();
        let dialog_idle = self.pending_dialog.is_none();
        let schema_label = draft.protobuf_draft.as_ref().map_or_else(
            || {
                draft.flextext_draft.as_ref().map_or_else(
                    || {
                        draft
                            .schema
                            .as_ref()
                            .map_or_else(|| "Not selected".to_owned(), |schema| schema.name.clone())
                    },
                    |flextext| {
                        flextext
                            .configuration_path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    },
                )
            },
            |protobuf| {
                protobuf
                    .schema_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            },
        );
        let can_save = !draft.name.trim().is_empty() && draft.schema_is_ready();
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
                if draft.protobuf_draft.is_some() {
                    show_named_target_protobuf(ui, draft);
                } else if draft.flextext_draft.is_some() {
                    show_named_target_flextext(ui, draft);
                } else if draft.fixed_width_draft.is_some() {
                    fixed_width::show_options(ui, draft);
                } else {
                    if show_target_format_options(ui, &draft.output_path, &mut draft.options) {
                        draft.csv_dialect_draft = None;
                    }
                    let csv_shape = draft.schema.as_ref().is_some_and(|schema| {
                        crate::extra_targets::flat_scalar_fields(schema).is_ok()
                    });
                    if let Some(pending) = draft.csv_dialect_draft.as_mut() {
                        super::csv_dialect_ui::show_fields(
                            ui,
                            pending,
                            &draft.output_path,
                            false,
                            "named_target",
                        );
                        ui.horizontal(|ui| {
                            if ui.button("Abandon CSV changes").clicked() {
                                draft.abandon_csv_dialect();
                            }
                            if ui.button("Use path format").clicked() {
                                draft.use_path_format();
                            }
                        });
                    } else if crate::new_mapping::uses_csv_format(
                        &draft.options,
                        &draft.output_path,
                    ) {
                        let mut editor = crate::new_mapping::CsvDialectDraft::from_options(
                            &draft.options,
                            &draft.output_path,
                        );
                        if super::csv_dialect_ui::show_fields(
                            ui,
                            &mut editor,
                            &draft.output_path,
                            false,
                            "named_target",
                        ) {
                            draft.csv_dialect_draft = Some(editor);
                        }
                    } else if csv_shape
                        && crate::new_mapping::csv_path_compatible(&draft.output_path)
                        && (crate::new_mapping::uses_path_format(
                            &draft.options,
                            &draft.output_path,
                        ) || draft.options.csv_text_repair_dependency.is_some())
                        && ui.button("Configure CSV output").clicked()
                        && let Err(error) = draft.begin_csv_dialect()
                    {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                    if draft.schema.as_ref().is_some_and(|schema| {
                        crate::extra_targets::flat_scalar_fields(schema).is_ok()
                    }) && ui
                        .button(if draft.options.fixed_width.is_some() {
                            "Edit fixed-width layout"
                        } else {
                            "Configure fixed-width output"
                        })
                        .clicked()
                        && let Err(error) = draft.begin_fixed_width()
                    {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                }
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
                    pick_file("schema or layout", &["xsd", "json", "proto", "mft"]),
                ));
            }
            Some(ExtraTargetAction::ChooseOutput) => {
                self.pending_dialog = Some((
                    DialogKind::BrowseExtraTargetOutput,
                    save_file(
                        "output data",
                        &[
                            "xml", "json", "jsonl", "csv", "xlsx", "edi", "txt", "bin", "dat",
                        ],
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
            self.diagnostics.validation(&self.project, issues);
        }
    }
}

fn show_named_target_flextext(ui: &mut egui::Ui, draft: &mut ExtraTargetDraft) {
    let Some(flextext) = &draft.flextext_draft else {
        return;
    };
    ui.strong("FlexText output");
    flextext.show_summary(ui);
    if ui.button("Use path format").clicked() {
        draft.use_path_format();
    }
}

fn show_named_target_protobuf(ui: &mut egui::Ui, draft: &mut ExtraTargetDraft) {
    let Some(protobuf) = draft.protobuf_draft.as_mut() else {
        return;
    };
    ui.strong("Protocol Buffers output");
    if crate::new_mapping::show_protobuf_root_message(ui, protobuf, "named_target")
        && draft.name.trim().is_empty()
        && let Ok(schema) = protobuf.schema()
    {
        draft.name = schema.name;
    }
    ui.weak("The project keeps the selected schema and its local imports for any data filename.");
    if let Err(error) = protobuf.validate() {
        ui.colored_label(ui.visuals().error_fg_color, format!("{error:#}"));
    }
    if ui.button("Use path format").clicked() {
        draft.use_path_format();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DocumentKind {
    ProtocolBuffers,
    Configured,
    Automatic,
    Xml,
    Json,
    JsonLines,
}

fn document_kind(options: &mapping::FormatOptions, output_path: &str) -> DocumentKind {
    if options.protobuf.is_some() {
        DocumentKind::ProtocolBuffers
    } else if crate::new_mapping::configured_layout_label_for_path(options, output_path).is_some() {
        DocumentKind::Configured
    } else if options.xml_document {
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
    if matches!(
        kind,
        DocumentKind::ProtocolBuffers | DocumentKind::Configured
    ) {
        return;
    }
    *options = mapping::FormatOptions::default();
    options.xml_document = kind == DocumentKind::Xml;
    options.json_document = kind == DocumentKind::Json;
    options.json_lines = kind == DocumentKind::JsonLines;
}

fn show_target_format_options(
    ui: &mut egui::Ui,
    output_path: &str,
    options: &mut mapping::FormatOptions,
) -> bool {
    let previous_kind = document_kind(options, output_path);
    let mut kind = previous_kind;
    ui.horizontal_wrapped(|ui| {
        if options.protobuf.is_some() {
            ui.selectable_value(&mut kind, DocumentKind::ProtocolBuffers, "Protocol Buffers");
        } else if let Some(label) =
            crate::new_mapping::configured_layout_label_for_path(options, output_path)
        {
            ui.selectable_value(&mut kind, DocumentKind::Configured, label);
        }
        ui.selectable_value(&mut kind, DocumentKind::Automatic, "From path");
        ui.selectable_value(&mut kind, DocumentKind::Xml, "XML");
        ui.selectable_value(&mut kind, DocumentKind::Json, "JSON");
        ui.selectable_value(&mut kind, DocumentKind::JsonLines, "JSON Lines");
    });
    let kind_changed = kind != previous_kind;
    if kind_changed {
        set_document_kind(options, kind);
    }
    if let Some(protobuf) = &options.protobuf {
        ui.label(format!("Root message: {}", protobuf.root_message));
        ui.weak("Binary output uses the embedded schema for any filename.");
        return kind_changed;
    }
    if let Some(label) = crate::new_mapping::configured_layout_label_for_path(options, output_path)
    {
        ui.label(format!("Configured format: {label}"));
        if label == "XLSX workbook"
            && crate::new_mapping::can_update_existing_workbook(options, output_path)
        {
            return ui
                .checkbox(
                    &mut options.xlsx_update_existing,
                    "Update existing workbook",
                )
                .changed()
                || kind_changed;
        }
        return kind_changed;
    }
    if !crate::new_mapping::uses_path_format(options, output_path) {
        return kind_changed;
    }
    if crate::new_mapping::can_update_existing_workbook(options, output_path) {
        return ui
            .checkbox(
                &mut options.xlsx_update_existing,
                "Update existing workbook",
            )
            .changed()
            || kind_changed;
    }
    kind_changed
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

        assert_eq!(document_kind(&options, "output.csv"), DocumentKind::Xml);
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

#[cfg(test)]
#[path = "extra_targets/protobuf_format_tests.rs"]
mod protobuf_format_tests;

#[cfg(test)]
#[path = "extra_targets/named_protobuf_tests.rs"]
mod named_protobuf_tests;

#[cfg(test)]
#[path = "extra_targets/fixed_width_tests.rs"]
mod fixed_width_tests;

#[cfg(test)]
#[path = "extra_targets/named_flextext_tests.rs"]
mod named_flextext_tests;
