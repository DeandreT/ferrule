use super::*;

impl FerruleApp {
    pub(super) fn begin_extra_source(&mut self) {
        self.extra_source_draft = Some(ExtraSourceDraft::default());
    }

    pub(super) fn stage_extra_source_schema(&mut self, path: PathBuf) {
        match crate::new_mapping::import_schema(&path) {
            Ok(schema) => {
                let Some(draft) = self.extra_source_draft.as_mut() else {
                    return;
                };
                if draft.name.trim().is_empty() {
                    draft.name.clone_from(&schema.name);
                }
                draft.schema = Some(schema);
                self.status = format!("loaded extra source schema {}", path.display());
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = "failed to load extra source schema".to_string();
                self.diagnostics
                    .error("Schema import failed", error.to_string());
            }
        }
    }

    pub(super) fn show_extra_source_setup(&mut self, ctx: &egui::Context) {
        let Some(draft) = self.extra_source_draft.as_mut() else {
            return;
        };
        let dialog_idle = self.pending_dialog.is_none();
        let schema_label = draft
            .schema
            .as_ref()
            .map_or("Not selected", |schema| schema.name.as_str());
        let can_add = !draft.name.trim().is_empty()
            && !draft.instance_path.trim().is_empty()
            && draft.schema.is_some();
        let mut action = None;
        egui::Window::new("Add Extra Source")
            .collapsible(false)
            .resizable(false)
            .default_width(520.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Grid::new("extra_source_fields")
                    .num_columns(3)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.strong("Name");
                        ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(260.0));
                        ui.end_row();

                        ui.strong("Instance");
                        ui.add(
                            egui::TextEdit::singleline(&mut draft.instance_path)
                                .desired_width(260.0),
                        );
                        if ui
                            .add_enabled(dialog_idle, egui::Button::new("Choose..."))
                            .clicked()
                        {
                            action = Some(ExtraSourceAction::ChooseInstance);
                        }
                        ui.end_row();

                        ui.strong("Schema");
                        ui.label(schema_label);
                        if ui
                            .add_enabled(dialog_idle, egui::Button::new("Choose..."))
                            .clicked()
                        {
                            action = Some(ExtraSourceAction::ChooseSchema);
                        }
                        ui.end_row();
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(dialog_idle, egui::Button::new("Cancel"))
                        .clicked()
                    {
                        action = Some(ExtraSourceAction::Cancel);
                    }
                    if ui
                        .add_enabled(dialog_idle && can_add, egui::Button::new("Add source"))
                        .clicked()
                    {
                        action = Some(ExtraSourceAction::Add);
                    }
                });
            });

        match action {
            Some(ExtraSourceAction::ChooseInstance) => {
                self.pending_dialog = Some((
                    DialogKind::BrowseExtraSourceInstance,
                    pick_file(
                        "input data",
                        &[
                            "csv", "xml", "json", "db", "sqlite", "edi", "x12", "edifact",
                        ],
                    ),
                ));
            }
            Some(ExtraSourceAction::ChooseSchema) => {
                self.pending_dialog = Some((
                    DialogKind::BrowseExtraSourceSchema,
                    pick_file("schema", &["xsd", "json"]),
                ));
            }
            Some(ExtraSourceAction::Cancel) => {
                self.extra_source_draft = None;
                self.status = "extra source cancelled".to_string();
            }
            Some(ExtraSourceAction::Add) => self.finish_extra_source(),
            None => {}
        }
    }

    fn finish_extra_source(&mut self) {
        let Some(draft) = self.extra_source_draft.as_ref() else {
            return;
        };
        match draft.clone().build(&self.project.extra_sources) {
            Ok(source) => {
                self.project.extra_sources.push(source);
                self.extra_source_draft = None;
                self.status = "extra source added".to_string();
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = "extra source is incomplete".to_string();
                self.diagnostics
                    .error("Extra source not added", error.to_string());
            }
        }
    }

    pub(super) fn show_extra_source_removal_confirmation(&mut self, ctx: &egui::Context) {
        let Some(index) = self.pending_extra_source_removal else {
            return;
        };
        let Some(source) = self.project.extra_sources.get(index) else {
            self.pending_extra_source_removal = None;
            return;
        };
        let name = source.name.clone();
        let mut remove = None;
        egui::Window::new("Remove Extra Source")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.label(format!("Remove {name} from this mapping?"));
                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        remove = Some(false);
                    }
                    if ui.button("Remove").clicked() {
                        remove = Some(true);
                    }
                });
            });
        match remove {
            Some(true) => {
                let removed = remove_extra_source(&mut self.project.extra_sources, index);
                self.pending_extra_source_removal = None;
                if let Some(source) = removed {
                    self.status = format!("removed extra source {}", source.name);
                    let issues = cli::validate(&self.project);
                    if issues.is_empty() {
                        self.diagnostics.clear();
                    } else {
                        self.diagnostics.validation(issues);
                    }
                }
            }
            Some(false) => self.pending_extra_source_removal = None,
            None => {}
        }
    }
}

enum ExtraSourceAction {
    ChooseInstance,
    ChooseSchema,
    Cancel,
    Add,
}
