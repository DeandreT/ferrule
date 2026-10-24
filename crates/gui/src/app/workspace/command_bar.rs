use super::*;

impl FerruleApp {
    pub(in crate::app) fn show_command_bar(
        &mut self,
        ui: &mut egui::Ui,
        editing_enabled: bool,
        layout_class: LayoutClass,
        undo_shortcut: &egui::KeyboardShortcut,
        redo_shortcut: &egui::KeyboardShortcut,
    ) {
        let item_spacing_y = ui.spacing().item_spacing.y;
        ui.spacing_mut().item_spacing.y = 0.0;
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

        crate::theme::top_bar_row(ui, 22.0, |ui| {
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
                        if ui.button("Run Pipeline...").clicked() {
                            self.pending_dialog = Some((
                                DialogKind::OpenPipeline,
                                pick_file("ferrule pipeline", &["json"]),
                            ));
                            ui.close();
                        }
                        if ui.button("Edit Pipeline...").clicked() {
                            self.pending_dialog = Some((
                                DialogKind::OpenPipelineEditor,
                                pick_file("ferrule pipeline", &["json"]),
                            ));
                            ui.close();
                        }
                        if ui.button("New Pipeline...").clicked() {
                            self.pending_dialog = Some((
                                DialogKind::CreatePipeline,
                                save_file("ferrule pipeline", &["json"], "pipeline.json"),
                            ));
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
                        if ui.button("Generate Library...").clicked() {
                            self.begin_library_generation();
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
                        let _ = self.show_mfd_import_options(ui);
                        if ui.button("Import MFD as Pipeline...").clicked() {
                            self.begin_mfd_pipeline_import();
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
                        ui.menu_button("MFD package", |ui| {
                            if ui.button("Choose manifest...").clicked() {
                                self.pending_dialog = Some((
                                    DialogKind::BrowseMfdPackageManifest,
                                    pick_file("ferrule package manifest", &["json"]),
                                ));
                                ui.close();
                            }
                            if ui
                                .add_enabled(
                                    self.mfd_package_manifest.is_some(),
                                    egui::Button::new("Clear manifest"),
                                )
                                .clicked()
                            {
                                self.mfd_package_manifest = None;
                                self.status = "cleared MFD package manifest".to_string();
                                ui.close();
                            }
                            if let Some(path) = &self.mfd_package_manifest {
                                ui.separator();
                                ui.add(egui::Label::new(path).truncate())
                                    .on_hover_text(path);
                            }
                        });
                        if ui.button("Export MFD (Ferrule)...").clicked() {
                            self.pending_dialog = Some((
                                DialogKind::ExportMfd,
                                save_file("MFD design", &["mfd"], &self.mfd_export_suggestion()),
                            ));
                            ui.close();
                        }
                        if ui.button("Export native MFD...").clicked() {
                            self.pending_dialog = Some((
                                DialogKind::ExportNativeMfd,
                                save_file(
                                    "native MFD design",
                                    &["mfd"],
                                    &self.mfd_export_suggestion(),
                                ),
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
                                self.history.can_redo(),
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
                        if ui.button("Run JSON request...").clicked() {
                            self.begin_rest_run();
                            ui.close();
                        }
                        if ui.button("Debug Run...").clicked() {
                            self.debug_run(ui.ctx());
                            ui.close();
                        }
                        if ui
                            .add_enabled(self.can_preview(), egui::Button::new("Preview..."))
                            .on_disabled_hover_text(
                                "Open the primary mapping or a named target mapping first",
                            )
                            .clicked()
                        {
                            self.begin_preview();
                            ui.close();
                        }
                        let result_label = self.run_report.as_ref().map_or("Results", |result| {
                            match result.report.kind {
                                crate::run_report::RunReportKind::Run => "Run results",
                                crate::run_report::RunReportKind::Preview => "Preview results",
                                crate::run_report::RunReportKind::Pipeline => "Pipeline results",
                                crate::run_report::RunReportKind::PipelinePreview => {
                                    "Pipeline preview results"
                                }
                            }
                        });
                        if ui
                            .add_enabled(self.run_report.is_some(), egui::Button::new(result_label))
                            .clicked()
                        {
                            self.show_run_report = true;
                            ui.close();
                        }
                        if ui.button("Arrange left to right").clicked() {
                            self.arrange_focused_canvas(ArrangeMode::LeftToRight);
                            ui.close();
                        }
                        if ui.button("Arrange compact").clicked() {
                            self.arrange_focused_canvas(ArrangeMode::Compact);
                            ui.close();
                        }
                        if ui.button("Fit canvas").clicked() {
                            self.fit_focused_canvas();
                            ui.close();
                        }
                        if ui
                            .add_enabled(
                                self.can_auto_connect(),
                                egui::Button::new("Auto-connect fields..."),
                            )
                            .on_disabled_hover_text(
                                "Open the primary mapping or a named target mapping first",
                            )
                            .clicked()
                        {
                            self.begin_auto_connect();
                            ui.close();
                        }
                        ui.separator();
                        if ui.button("Add extra source...").clicked() {
                            self.begin_extra_source();
                            ui.close();
                        }
                    });
                    ui.menu_button("View", |ui| {
                        ui.checkbox(&mut self.show_source_panel, "Source schema");
                        ui.checkbox(&mut self.show_inspector_panel, "Inspector");
                        ui.checkbox(&mut self.show_minimap, "Canvas minimap");
                        ui.separator();
                        if ui.button("Appearance...").clicked() {
                            self.show_appearance_editor = true;
                            ui.close();
                        }
                    });
                });
                ui.separator();
                let path = self.document.display_path();
                ui.add(egui::Label::new(&path).truncate())
                    .on_hover_text(path);
            });
        });

        if self.pending_dialog.is_some() {
            crate::theme::top_bar_row(ui, crate::theme::METRICS.toolbar_height, |ui| {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label("Waiting for file dialog");
                    if ui.button("Cancel").clicked() {
                        self.cancel_pending_file_dialog();
                    }
                });
            });
            ui.spacing_mut().item_spacing.y = item_spacing_y;
            return;
        }

        crate::theme::top_bar_row(ui, crate::theme::METRICS.toolbar_height, |ui| {
            ui.horizontal_wrapped(|ui| {
                if crate::icons::button(
                    ui,
                    editing_enabled,
                    lucide_icons::Icon::FolderOpen,
                    format!("Open ({})", ui.ctx().format_shortcut(&open_shortcut)),
                )
                .clicked()
                    && let Some(action) =
                        self.request_destructive_action(DestructiveAction::OpenProject)
                {
                    self.perform_destructive_action(action, ui.ctx());
                }
                if crate::icons::button(
                    ui,
                    editing_enabled,
                    lucide_icons::Icon::Save,
                    format!("Save ({})", ui.ctx().format_shortcut(&save_shortcut)),
                )
                .clicked()
                {
                    self.save_with_continuation(None, ui.ctx());
                }
                if crate::icons::button(ui, true, lucide_icons::Icon::Palette, "Appearance")
                    .clicked()
                {
                    self.show_appearance_editor = true;
                }
                ui.separator();
                if crate::icons::button(
                    ui,
                    editing_enabled && self.can_undo(),
                    lucide_icons::Icon::Undo2,
                    format!("Undo ({})", ui.ctx().format_shortcut(undo_shortcut)),
                )
                .clicked()
                {
                    self.undo_project();
                }
                if crate::icons::button(
                    ui,
                    editing_enabled && self.history.can_redo(),
                    lucide_icons::Icon::Redo2,
                    format!("Redo ({})", ui.ctx().format_shortcut(redo_shortcut)),
                )
                .clicked()
                {
                    self.redo_project();
                }
                ui.separator();
                if crate::icons::button(
                    ui,
                    editing_enabled,
                    lucide_icons::Icon::CheckCircle2,
                    format!(
                        "Validate ({})",
                        ui.ctx().format_shortcut(&validate_shortcut)
                    ),
                )
                .clicked()
                {
                    self.validate_now();
                }
                if crate::icons::button(
                    ui,
                    editing_enabled,
                    lucide_icons::Icon::Play,
                    format!("Run ({})", ui.ctx().format_shortcut(&run_shortcut)),
                )
                .clicked()
                {
                    self.run(ui.ctx());
                }
                if crate::icons::button(
                    ui,
                    editing_enabled && self.can_preview(),
                    lucide_icons::Icon::Eye,
                    "Preview current target from in-memory input",
                )
                .clicked()
                {
                    self.begin_preview();
                }
                if crate::icons::button(
                    ui,
                    editing_enabled,
                    lucide_icons::Icon::Settings2,
                    "Run settings",
                )
                .clicked()
                {
                    self.show_run_setup = !self.show_run_setup;
                }
                if crate::icons::button(
                    ui,
                    self.run_report.is_some(),
                    lucide_icons::Icon::FileOutput,
                    "Show latest run or preview results",
                )
                .clicked()
                {
                    self.show_run_report = true;
                }
                ui.separator();
                ui.add_enabled_ui(editing_enabled, |ui| {
                    let button = egui::Button::new(crate::icons::text(
                        lucide_icons::Icon::LayoutGrid,
                        crate::theme::METRICS.icon_size,
                    ))
                    .min_size(egui::vec2(
                        crate::theme::METRICS.icon_button_size,
                        crate::theme::METRICS.icon_button_size,
                    ));
                    let (response, _) =
                        egui::containers::menu::MenuButton::from_button(button).ui(ui, |ui| {
                            if ui.button("Arrange left to right").clicked() {
                                self.arrange_focused_canvas(ArrangeMode::LeftToRight);
                                ui.close();
                            }
                            if ui.button("Arrange compact").clicked() {
                                self.arrange_focused_canvas(ArrangeMode::Compact);
                                ui.close();
                            }
                        });
                    response.on_hover_text("Arrange canvas");
                });
                if crate::icons::button(
                    ui,
                    editing_enabled,
                    lucide_icons::Icon::Maximize2,
                    "Fit canvas",
                )
                .clicked()
                {
                    self.fit_focused_canvas();
                }
            });
        });
        if self.show_run_setup {
            ui.spacing_mut().item_spacing.y = item_spacing_y;
            ui.separator();
            ui.add_enabled_ui(editing_enabled, |ui| {
                ui.horizontal_wrapped(|ui| self.show_runtime_paths(ui));
                // Document runs read CSV only and have no debug route.
                let documents = self.primary_document.is_some();
                if !documents {
                    self.show_owned_pdf_worker_toggle(ui);
                }
                self.host_parameters.show(ui);
                if !documents {
                    self.show_file_debug_controls(ui);
                }
            });
            ui.spacing_mut().item_spacing.y = 0.0;
        }
        self.show_mapping_tabs(ui, editing_enabled);
        ui.spacing_mut().item_spacing.y = item_spacing_y;
        self.show_workspace_tabs(ui, layout_class);
    }

    pub(in crate::app) fn show_owned_pdf_worker_toggle(&mut self, ui: &mut egui::Ui) {
        ui.checkbox(
            &mut self.owned_pdf_worker,
            "Extract file PDFs in an owned worker process",
        )
        .on_hover_text(
            "Runs and file previews read PDF inputs in the packaged ferrule-pdf-worker \
             beside this application (Linux x86_64). Cancelling or closing waits until \
             that process has been stopped and reaped. There is no in-process fallback.",
        );
    }

    fn show_runtime_paths(&mut self, ui: &mut egui::Ui) {
        if let Some(session) = &mut self.primary_document {
            ui.vertical(|ui| session.show(ui));
        } else {
            self.show_input_path(ui);
        }
        self.show_output_path(ui);
    }

    fn show_input_path(&mut self, ui: &mut egui::Ui) {
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
    }

    fn show_output_path(&mut self, ui: &mut egui::Ui) {
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

    pub(in crate::app) fn show_workspace_tabs(&mut self, ui: &mut egui::Ui, class: LayoutClass) {
        match class {
            LayoutClass::Wide => {}
            LayoutClass::Compact => {
                ui.horizontal(|ui| {
                    ui.toggle_value(&mut self.compact_dock_open, "Dock");
                    ui.add_enabled_ui(self.compact_dock_open, |ui| {
                        for dock in SideDock::ALL {
                            ui.selectable_value(&mut self.compact_dock, dock, dock.label());
                        }
                    });
                });
            }
            LayoutClass::Narrow => {
                ui.horizontal(|ui| {
                    for pane in WorkspacePane::ALL {
                        ui.selectable_value(&mut self.narrow_pane, pane, pane.label());
                    }
                });
            }
        }
    }

    pub(in crate::app) fn show_status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (label, color) = if self.observed_dirty() {
                ("Unsaved", self.palette.warning)
            } else {
                ("Saved", self.palette.success)
            };
            ui.colored_label(color, label);
            ui.separator();
            ui.label(self.document.display_name());
            if !self.diagnostics.is_empty() {
                ui.separator();
                ui.colored_label(
                    self.palette.error,
                    format!("{} issue(s)", self.diagnostics.len()),
                );
            }
            if !self.status.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.shrink_clip_rect(ui.available_rect_before_wrap());
                    let response = ui.add(
                        egui::Label::new(&self.status)
                            .truncate()
                            .show_tooltip_when_elided(false),
                    );
                    let tooltip_width = ui.ctx().global_style().spacing.tooltip_width.min(
                        (ui.ctx().content_rect().width()
                            - egui::Frame::popup(ui.style()).total_margin().sum().x)
                            .max(0.0),
                    );
                    let _ = egui::Tooltip::for_enabled(&response)
                        .width(tooltip_width)
                        .show(|ui| {
                            ui.set_max_width(tooltip_width);
                            ui.add(egui::Label::new(&self.status).wrap());
                        });
                });
            }
        });
    }
}
