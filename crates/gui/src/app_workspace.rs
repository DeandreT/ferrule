use super::*;

impl FerruleApp {
    pub(super) fn show_command_bar(
        &mut self,
        ui: &mut egui::Ui,
        editing_enabled: bool,
        layout_class: LayoutClass,
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
                    if ui
                        .add_enabled(self.run_report.is_some(), egui::Button::new("Run results"))
                        .clicked()
                    {
                        self.show_run_report = true;
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
            ui.label(self.document.display_path());
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
            if crate::icons::button(ui, true, lucide_icons::Icon::Palette, "Appearance").clicked() {
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
                "Show last run results",
            )
            .clicked()
            {
                self.show_run_report = true;
            }
            ui.separator();
            if crate::icons::button(
                ui,
                editing_enabled,
                lucide_icons::Icon::LayoutGrid,
                "Arrange canvas",
            )
            .clicked()
            {
                self.arrange_canvas();
            }
            if crate::icons::button(
                ui,
                editing_enabled,
                lucide_icons::Icon::Maximize2,
                "Fit canvas",
            )
            .clicked()
            {
                self.fit_canvas();
            }
        });
        if self.show_run_setup {
            ui.separator();
            ui.horizontal_wrapped(|ui| self.show_runtime_paths(ui));
        }
        self.show_workspace_tabs(ui, layout_class);
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
        arrange_snarl(
            &mut self.snarl,
            &self.canvas_node_sizes,
            *self.appearance.wire(),
        );
        self.reset_canvas_view();
        self.status = "canvas arranged with wire-aware spacing".to_string();
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

    pub(super) fn show_source_explorer(&mut self, ui: &mut egui::Ui, editing_enabled: bool) {
        let source_x12 = crate::x12_tooltips::boundary_has_x12(
            &self.project.source,
            self.project.source_path.as_deref(),
            &self.project.source_options,
        );
        ui.horizontal(|ui| {
            ui.strong("Source schema");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        editing_enabled,
                        egui::Button::new(crate::icons::text(
                            lucide_icons::Icon::CirclePlus,
                            crate::theme::METRICS.icon_size,
                        )),
                    )
                    .on_hover_text("Add source")
                    .clicked()
                {
                    self.begin_extra_source();
                }
            });
        });
        show_schema_search_input(ui, "source_schema_search", &mut self.source_schema_explorer);
        let source_matches = self
            .source_schema_explorer
            .match_count(&self.project.source)
            + self
                .project
                .extra_sources
                .iter()
                .map(|source| self.source_schema_explorer.match_count(&source.schema))
                .sum::<usize>();
        let source_fields = schema_field_count(&self.project.source)
            + self
                .project
                .extra_sources
                .iter()
                .map(|source| schema_field_count(&source.schema))
                .sum::<usize>();
        show_schema_result_count(
            ui,
            &self.source_schema_explorer,
            source_matches,
            source_fields,
        );
        let mut remove = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            if source_matches == 0 && self.source_schema_explorer.is_filtering() {
                ui.weak("No matching fields or groups");
            } else {
                let mut section_shown = show_schema_tree(
                    ui,
                    &self.project.source,
                    &self.source_schema_explorer,
                    "primary_source_schema",
                    source_x12,
                );
                for (index, extra) in self.project.extra_sources.iter().enumerate() {
                    if self.source_schema_explorer.is_filtering()
                        && self.source_schema_explorer.match_count(&extra.schema) == 0
                    {
                        continue;
                    }
                    if section_shown {
                        ui.separator();
                    }
                    ui.horizontal(|ui| {
                        ui.strong(format!("Extra: {}", extra.name));
                        if ui
                            .add_enabled(
                                editing_enabled,
                                egui::Button::new(crate::icons::text(
                                    lucide_icons::Icon::Trash2,
                                    crate::theme::METRICS.icon_size,
                                ))
                                .small(),
                            )
                            .on_hover_text("Remove source")
                            .clicked()
                        {
                            remove = Some(index);
                        }
                    });
                    show_schema_tree(
                        ui,
                        &extra.schema,
                        &self.source_schema_explorer,
                        ("extra_source_schema", index),
                        crate::x12_tooltips::boundary_has_x12(
                            &extra.schema,
                            Some(&extra.path),
                            &extra.options,
                        ),
                    );
                    section_shown = true;
                }
            }
        });
        if let Some(index) = remove {
            self.pending_extra_source_removal = Some(index);
        }
    }

    pub(super) fn show_inspector(&mut self, ui: &mut egui::Ui, editing_enabled: bool) {
        let target_x12 = crate::x12_tooltips::boundary_has_x12(
            &self.project.target,
            self.project.target_path.as_deref(),
            &self.project.target_options,
        );
        let source_paths =
            SourcePathCatalog::new(&self.project.source, &self.project.extra_sources);
        ui.add_enabled_ui(editing_enabled, |ui| {
            ui.strong("Target schema");
            show_schema_search_input(ui, "target_schema_search", &mut self.target_schema_explorer);
            let target_matches = self
                .target_schema_explorer
                .match_count(&self.project.target);
            show_schema_result_count(
                ui,
                &self.target_schema_explorer,
                target_matches,
                schema_field_count(&self.project.target),
            );
            egui::ScrollArea::vertical()
                .max_height(200.0)
                .show(ui, |ui| {
                    if target_matches == 0 && self.target_schema_explorer.is_filtering() {
                        ui.weak("No matching fields or groups");
                    } else {
                        show_schema_tree(
                            ui,
                            &self.project.target,
                            &self.target_schema_explorer,
                            "target_schema",
                            target_x12,
                        );
                    }
                });

            ui.separator();
            ui.strong("Scopes");
            egui::ScrollArea::vertical()
                .id_salt("scope_tree_scroll")
                .max_height(200.0)
                .show(ui, |ui| {
                    if let Some(new_selection) =
                        show_scope_tree(ui, &self.project.root, &self.selected_scope)
                    {
                        self.selected_scope = new_selection;
                    }
                });
            self.show_scope_controls(ui);

            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("scope_editor_scroll")
                .show(ui, |ui| {
                    let nested = !self.selected_scope.is_empty();
                    let target_chain = scope_target_chain(&self.project.root, &self.selected_scope);
                    let target_fields = binding_target_fields(&self.project.target, &target_chain);
                    let scope = scope_at_mut(&mut self.project.root, &self.selected_scope);
                    show_scope_editor(
                        ui,
                        scope,
                        &self.project.graph,
                        &source_paths,
                        &target_fields,
                        nested,
                    );
                });
        });
    }

    pub(super) fn show_canvas(&mut self, ui: &mut egui::Ui, editing_enabled: bool) {
        let source_paths =
            SourcePathCatalog::new(&self.project.source, &self.project.extra_sources);
        let source_x12 = crate::x12_tooltips::boundary_has_x12(
            &self.project.source,
            self.project.source_path.as_deref(),
            &self.project.source_options,
        );
        let target_x12 = crate::x12_tooltips::boundary_has_x12(
            &self.project.target,
            self.project.target_path.as_deref(),
            &self.project.target_options,
        );
        ui.add_enabled_ui(editing_enabled, |ui| {
            let source_blocks = source_blocks(&self.project.source);
            let target_blocks = target_blocks(&self.project.target);
            crate::app::sync_endpoint_wires(
                &self.project.graph,
                &self.project.root,
                &source_blocks,
                &target_blocks,
                &self.endpoint_scroll,
                &mut self.snarl,
            );
            let mut viewer = GraphViewer {
                graph: &mut self.project.graph,
                root_scope: &mut self.project.root,
                extra_targets: &self.project.extra_targets,
                source_blocks: &source_blocks,
                target_blocks: &target_blocks,
                source_x12,
                target_x12,
                source_paths: &source_paths,
                colors: self.appearance.resolved_colors(self.palette),
                wire_color_mode: self.appearance.wire().color_mode(),
                endpoint_scroll: &mut self.endpoint_scroll,
                endpoint_search_match: None,
                node_sizes: Some(&mut self.canvas_node_sizes),
                hovered_node: None,
                hovered_node_this_frame: None,
                camera_pan: egui::Vec2::ZERO,
                camera_focus: None,
                canvas_transform: None,
                pin_interaction_ids: Vec::new(),
                error: None,
            };
            crate::canvas_keyboard::show(
                &mut self.snarl,
                &mut viewer,
                &mut self.canvas_search,
                self.show_minimap,
                self.canvas_view_generation,
                self.appearance.to_snarl_style_with_palette(self.palette),
                ui,
            );
            if let Some(error) = viewer.error {
                self.status = "graph edit failed".to_string();
                self.diagnostics.error("Graph edit failed", error);
            }
        });
    }

    pub(super) fn show_workspace_tabs(&mut self, ui: &mut egui::Ui, class: LayoutClass) {
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

    pub(super) fn show_status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (label, color) = if self.is_dirty() {
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
                    ui.label(&self.status);
                });
            }
        });
    }
}

fn show_schema_search_input(
    ui: &mut egui::Ui,
    id_salt: impl egui::AsIdSalt,
    state: &mut SchemaExplorerState,
) {
    ui.horizontal(|ui| {
        let search_width = (ui.available_width()
            - crate::theme::METRICS.icon_button_size
            - ui.spacing().item_spacing.x)
            .max(64.0);
        ui.add(
            egui::TextEdit::singleline(state.query_mut())
                .id_source(id_salt)
                .hint_text("Search fields and groups")
                .desired_width(search_width),
        )
        .on_hover_text("Filter by field, group, path, or scalar type");
        if crate::icons::button(
            ui,
            state.is_filtering(),
            lucide_icons::Icon::SearchX,
            "Clear schema search",
        )
        .clicked()
        {
            state.clear();
        }
    });
}

fn show_schema_result_count(
    ui: &mut egui::Ui,
    state: &SchemaExplorerState,
    matches: usize,
    fields: usize,
) {
    let summary = if state.is_filtering() {
        match matches {
            1 => "1 match".to_string(),
            count => format!("{count} matches"),
        }
    } else {
        match fields {
            1 => "1 field".to_string(),
            count => format!("{count} fields"),
        }
    };
    ui.weak(summary);
}
