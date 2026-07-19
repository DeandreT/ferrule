use super::*;

impl FerruleApp {
    pub(super) fn begin_new_mapping(&mut self) {
        self.new_mapping_setup = Some(NewMappingSetup::default());
    }

    pub(super) fn stage_mapping_schema(&mut self, side: SchemaSide, path: PathBuf) {
        match crate::new_mapping::import_schema(&path) {
            Ok(schema) => {
                let imported = crate::new_mapping::ImportedSchema { path, schema };
                let Some(setup) = self.new_mapping_setup.as_mut() else {
                    return;
                };
                match side {
                    SchemaSide::Source => setup.source = Some(imported),
                    SchemaSide::Target => setup.target = Some(imported),
                }
                self.status = format!("loaded {} schema", side.label().to_lowercase());
                self.diagnostics.clear();
            }
            Err(error) => {
                self.status = format!("failed to load {} schema", side.label().to_lowercase());
                self.diagnostics
                    .error("Schema import failed", error.to_string());
            }
        }
    }

    pub(super) fn show_new_mapping_setup(&mut self, ctx: &egui::Context) {
        let Some(setup) = self.new_mapping_setup.as_ref() else {
            return;
        };
        let source = setup
            .source
            .as_ref()
            .map_or("Not selected".to_string(), |item| {
                item.path.display().to_string()
            });
        let target = setup
            .target
            .as_ref()
            .map_or("Not selected".to_string(), |item| {
                item.path.display().to_string()
            });
        let can_create = setup.source.is_some() && setup.target.is_some();
        let dialog_idle = self.pending_dialog.is_none();
        let mut action = None;

        egui::Window::new("New Mapping")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                egui::Grid::new("new_mapping_schemas")
                    .num_columns(3)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.strong("Source schema");
                        ui.label(source).on_hover_text("Source schema file");
                        if ui
                            .add_enabled(dialog_idle, egui::Button::new("Choose..."))
                            .clicked()
                        {
                            action = Some(NewMappingAction::Choose(SchemaSide::Source));
                        }
                        ui.end_row();

                        ui.strong("Target schema");
                        ui.label(target).on_hover_text("Target schema file");
                        if ui
                            .add_enabled(dialog_idle, egui::Button::new("Choose..."))
                            .clicked()
                        {
                            action = Some(NewMappingAction::Choose(SchemaSide::Target));
                        }
                        ui.end_row();
                    });
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(dialog_idle, egui::Button::new("Cancel"))
                        .clicked()
                    {
                        action = Some(NewMappingAction::Cancel);
                    }
                    if ui
                        .add_enabled(
                            dialog_idle && can_create,
                            egui::Button::new("Create mapping"),
                        )
                        .clicked()
                    {
                        action = Some(NewMappingAction::Create);
                    }
                });
            });

        match action {
            Some(NewMappingAction::Choose(side)) => {
                let kind = match side {
                    SchemaSide::Source => DialogKind::BrowseSourceSchema,
                    SchemaSide::Target => DialogKind::BrowseTargetSchema,
                };
                self.pending_dialog = Some((kind, pick_file("schema", &["xsd", "json"])));
            }
            Some(NewMappingAction::Cancel) => {
                self.new_mapping_setup = None;
                self.status = "new mapping cancelled".to_string();
            }
            Some(NewMappingAction::Create) => self.finish_new_mapping(),
            None => {}
        }
    }

    pub(super) fn finish_new_mapping(&mut self) {
        let Some(mut setup) = self.new_mapping_setup.take() else {
            return;
        };
        let (Some(source), Some(target)) = (setup.source.take(), setup.target.take()) else {
            self.new_mapping_setup = Some(setup);
            return;
        };
        self.project = Project {
            source: source.schema,
            target: target.schema,
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            extra_targets: Vec::new(),
            failure_rules: Vec::new(),
            graph: Graph::default(),
            root: Scope::default(),
        };
        self.snarl = build_snarl(&self.project);
        self.reset_canvas_view();
        self.document = DocumentLocation::untitled("mapping.json");
        self.history.mark_unsaved();
        self.selected_scope.clear();
        self.rebase_history();
        self.diagnostics.clear();
        self.status = "new mapping ready".to_string();
    }
}

#[derive(Clone, Copy)]
enum NewMappingAction {
    Choose(SchemaSide),
    Cancel,
    Create,
}
