use super::*;

impl FerruleApp {
    pub(super) fn show_command_bar(
        &mut self,
        ui: &mut egui::Ui,
        editing_enabled: bool,
        undo_shortcut: &egui::KeyboardShortcut,
        redo_shortcut: &egui::KeyboardShortcut,
    ) {
        let open_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O);
        let save_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
        let run_shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::R);
        let validate_shortcut = egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::V,
        );

        if editing_enabled
            && ui
                .ctx()
                .input_mut(|input| input.consume_shortcut(&open_shortcut))
            && let Some(action) = self.request_destructive_action(DestructiveAction::OpenProject)
        {
            self.perform_destructive_action(action, ui.ctx());
        }
        if editing_enabled
            && ui
                .ctx()
                .input_mut(|input| input.consume_shortcut(&save_shortcut))
        {
            self.save_with_continuation(None, ui.ctx());
        }
        if editing_enabled
            && ui
                .ctx()
                .input_mut(|input| input.consume_shortcut(&run_shortcut))
        {
            self.run(ui.ctx());
        }
        if editing_enabled
            && ui
                .ctx()
                .input_mut(|input| input.consume_shortcut(&validate_shortcut))
        {
            self.validate_now();
        }

        egui::MenuBar::new().ui(ui, |ui| {
            ui.add_enabled_ui(editing_enabled, |ui| {
                ui.menu_button("File", |ui| {
                    if ui
                        .add(
                            egui::Button::new("Open...")
                                .shortcut_text(ui.ctx().format_shortcut(&open_shortcut)),
                        )
                        .clicked()
                    {
                        if let Some(action) =
                            self.request_destructive_action(DestructiveAction::OpenProject)
                        {
                            self.perform_destructive_action(action, ui.ctx());
                        }
                        ui.close();
                    }
                    if ui
                        .add(
                            egui::Button::new("Save")
                                .shortcut_text(ui.ctx().format_shortcut(&save_shortcut)),
                        )
                        .clicked()
                    {
                        self.save_with_continuation(None, ui.ctx());
                        ui.close();
                    }
                    if ui.button("Save As...").clicked() {
                        self.start_save_as(None);
                        ui.close();
                    }
                    ui.separator();
                    if ui.button("New").clicked() {
                        if let Some(action) =
                            self.request_destructive_action(DestructiveAction::NewProject)
                        {
                            self.perform_destructive_action(action, ui.ctx());
                        }
                        ui.close();
                    }
                    if ui.button("Import MFD...").clicked() {
                        if let Some(action) =
                            self.request_destructive_action(DestructiveAction::ImportMfd)
                        {
                            self.perform_destructive_action(action, ui.ctx());
                        }
                        ui.close();
                    }
                    if ui.button("Export MFD...").clicked() {
                        self.pending_dialog = Some((
                            DialogKind::ExportMfd,
                            save_file("MapForce design", &["mfd"], &self.document.display_path()),
                        ));
                        ui.close();
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui
                        .add_enabled(
                            self.can_undo(),
                            egui::Button::new("Undo")
                                .shortcut_text(ui.ctx().format_shortcut(undo_shortcut)),
                        )
                        .clicked()
                    {
                        self.undo_project();
                        ui.close();
                    }
                    if ui
                        .add_enabled(
                            !self.redo_history.is_empty(),
                            egui::Button::new("Redo")
                                .shortcut_text(ui.ctx().format_shortcut(redo_shortcut)),
                        )
                        .clicked()
                    {
                        self.redo_project();
                        ui.close();
                    }
                });
                ui.menu_button("Mapping", |ui| {
                    if ui
                        .add(
                            egui::Button::new("Validate")
                                .shortcut_text(ui.ctx().format_shortcut(&validate_shortcut)),
                        )
                        .clicked()
                    {
                        self.validate_now();
                        ui.close();
                    }
                    if ui
                        .add(
                            egui::Button::new("Run")
                                .shortcut_text(ui.ctx().format_shortcut(&run_shortcut)),
                        )
                        .clicked()
                    {
                        self.run(ui.ctx());
                        ui.close();
                    }
                    if ui.button("Arrange canvas").clicked() {
                        self.arrange_canvas();
                        ui.close();
                    }
                    if ui.button("Fit canvas").clicked() {
                        self.fit_canvas();
                        ui.close();
                    }
                });
            });
            ui.separator();
            ui.label(self.document.display_path());
            if !self.status.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(&self.status);
                });
            }
        });

        if self.pending_dialog.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Waiting for file dialog");
                if ui.button("Cancel").clicked() {
                    self.pending_dialog = None;
                    self.pending_save_continuation = None;
                    self.status = "file dialog cancelled".to_string();
                }
            });
            return;
        }

        ui.add_enabled_ui(editing_enabled, |ui| {
            let compact = ui.available_width() < 1050.0;
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled(self.can_undo(), egui::Button::new("Undo"))
                    .on_hover_text(ui.ctx().format_shortcut(undo_shortcut))
                    .clicked()
                {
                    self.undo_project();
                }
                if ui
                    .add_enabled(!self.redo_history.is_empty(), egui::Button::new("Redo"))
                    .on_hover_text(ui.ctx().format_shortcut(redo_shortcut))
                    .clicked()
                {
                    self.redo_project();
                }
                ui.separator();
                if ui.button("Validate").clicked() {
                    self.validate_now();
                }
                if ui.button("Run").clicked() {
                    self.run(ui.ctx());
                }
                if ui.button("Arrange").clicked() {
                    self.arrange_canvas();
                }
                if ui.button("Fit").clicked() {
                    self.fit_canvas();
                }
                if !compact {
                    ui.separator();
                    self.show_runtime_paths(ui);
                }
            });
            if compact {
                ui.horizontal_wrapped(|ui| self.show_runtime_paths(ui));
            }
        });
    }

    fn show_runtime_paths(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Input");
            ui.add(egui::TextEdit::singleline(&mut self.input_path).desired_width(190.0));
            if ui.button("Browse...").clicked() {
                self.pending_dialog = Some((
                    DialogKind::BrowseInput,
                    pick_file(
                        "input data",
                        &[
                            "csv", "xml", "json", "db", "sqlite", "edi", "x12", "edifact",
                        ],
                    ),
                ));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Output");
            ui.add(egui::TextEdit::singleline(&mut self.output_path).desired_width(190.0));
            if ui.button("Browse...").clicked() {
                self.pending_dialog = Some((
                    DialogKind::BrowseOutput,
                    save_file(
                        "output data",
                        &[
                            "csv", "xml", "json", "db", "sqlite", "edi", "x12", "edifact",
                        ],
                        &self.output_path,
                    ),
                ));
            }
        });
    }

    fn arrange_canvas(&mut self) {
        self.snarl = arrange_snarl(&self.project, &self.snarl);
        self.reset_canvas_view();
        self.status = "canvas arranged".to_string();
    }

    fn fit_canvas(&mut self) {
        self.reset_canvas_view();
        self.status = "canvas fitted".to_string();
    }

    pub(super) fn reset_canvas_view(&mut self) {
        self.canvas_view_generation = self.canvas_view_generation.wrapping_add(1);
    }

    fn validate_now(&mut self) {
        let issues = cli::validate(&self.project);
        if issues.is_empty() {
            self.status = "project is valid".to_string();
            self.diagnostics.clear();
        } else {
            self.status = format!("{} validation issue(s)", issues.len());
            self.diagnostics.validation(issues);
        }
    }
}
