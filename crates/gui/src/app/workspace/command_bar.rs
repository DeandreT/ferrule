use super::*;

use crate::ui_kit::{self, METRICS};
use lucide_icons::Icon;

/// Why editor commands are unavailable while another operation owns the
/// project.
const BUSY: &str = "Unavailable while another operation is running";
const NEEDS_MAPPING: &str = "Open the primary mapping or a named target mapping first";

struct Shortcuts<'a> {
    new: egui::KeyboardShortcut,
    open: egui::KeyboardShortcut,
    save: egui::KeyboardShortcut,
    run: egui::KeyboardShortcut,
    validate: egui::KeyboardShortcut,
    undo: &'a egui::KeyboardShortcut,
    redo: &'a egui::KeyboardShortcut,
}

impl Shortcuts<'_> {
    fn label(ctx: &egui::Context, action: &str, shortcut: &egui::KeyboardShortcut) -> String {
        format!("{action} ({})", ctx.format_shortcut(shortcut))
    }
}

/// A menu entry with an optional shortcut hint; `disabled_reason` disables
/// it and becomes its tooltip.
fn menu_item(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: Option<&egui::KeyboardShortcut>,
    disabled_reason: Option<&str>,
) -> bool {
    let mut button = egui::Button::new(label);
    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(ui.ctx().format_shortcut(shortcut));
    }
    let response = ui.add_enabled(disabled_reason.is_none(), button);
    let response = match disabled_reason {
        Some(reason) => response.on_disabled_hover_text(reason),
        None => response,
    };
    let clicked = response.clicked();
    if clicked {
        ui.close();
    }
    clicked
}

fn toolbar_divider(ui: &mut egui::Ui) {
    ui.add_space(ui_kit::space::XS);
    ui.separator();
    ui.add_space(ui_kit::space::XS);
}

impl FerruleApp {
    pub(in crate::app) fn show_command_bar(
        &mut self,
        ui: &mut egui::Ui,
        editing_enabled: bool,
        layout_class: LayoutClass,
        undo_shortcut: &egui::KeyboardShortcut,
        redo_shortcut: &egui::KeyboardShortcut,
    ) {
        let shortcuts = Shortcuts {
            new: egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::N),
            open: egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::O),
            save: egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S),
            run: egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::R),
            validate: egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::V,
            ),
            undo: undo_shortcut,
            redo: redo_shortcut,
        };
        self.handle_command_shortcuts(ui.ctx(), editing_enabled, &shortcuts);

        // The bars stack flush; each one pads itself.
        ui.scope(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            crate::theme::top_bar_row(ui, METRICS.menu_bar_height, |ui| {
                egui::MenuBar::new().ui(ui, |ui| {
                    ui.add_enabled_ui(editing_enabled, |ui| self.show_menus(ui, &shortcuts));
                    self.show_document_identity(ui);
                });
            });
            crate::theme::top_bar_row(ui, METRICS.toolbar_height, |ui| {
                if self.pending_dialog.is_some() {
                    ui.spinner();
                    ui.label("Waiting for file dialog");
                    if ui.button("Cancel").clicked() {
                        self.cancel_pending_file_dialog();
                    }
                } else {
                    self.show_toolbar(ui, editing_enabled, &shortcuts);
                }
            });
            // The start view has no mapping to switch between.
            if self.pending_dialog.is_none() && !self.is_start_state() {
                self.show_mapping_tabs(ui, editing_enabled);
            }
        });
        if self.pending_dialog.is_none() {
            self.show_workspace_tabs(ui, layout_class);
        }
    }

    fn handle_command_shortcuts(
        &mut self,
        ctx: &egui::Context,
        editing_enabled: bool,
        shortcuts: &Shortcuts<'_>,
    ) {
        if !editing_enabled {
            return;
        }
        let pressed = |shortcut: &egui::KeyboardShortcut| {
            ctx.input_mut(|input| input.consume_shortcut(shortcut))
        };
        if pressed(&shortcuts.new) {
            self.request_new_project(ctx);
        }
        if pressed(&shortcuts.open) {
            self.request_open_project(ctx);
        }
        if pressed(&shortcuts.save) {
            self.save_with_continuation(None, ctx);
        }
        if pressed(&shortcuts.run) {
            self.run(ctx);
        }
        if pressed(&shortcuts.validate) {
            self.validate_now();
        }
    }

    fn request_new_project(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.request_destructive_action(DestructiveAction::NewProject) {
            self.perform_destructive_action(action, ctx);
        }
    }

    fn request_open_project(&mut self, ctx: &egui::Context) {
        if let Some(action) = self.request_destructive_action(DestructiveAction::OpenProject) {
            self.perform_destructive_action(action, ctx);
        }
    }

    fn results_label(&self) -> &'static str {
        self.run_report
            .as_ref()
            .map_or("Results", |result| match result.report.kind {
                crate::run_report::RunReportKind::Run => "Run results",
                crate::run_report::RunReportKind::Preview => "Preview results",
                crate::run_report::RunReportKind::Pipeline => "Pipeline results",
                crate::run_report::RunReportKind::PipelinePreview => "Pipeline preview results",
            })
    }

    fn show_menus(&mut self, ui: &mut egui::Ui, shortcuts: &Shortcuts<'_>) {
        let ctx = ui.ctx().clone();
        ui.menu_button("File", |ui| {
            if menu_item(ui, "New", Some(&shortcuts.new), None) {
                self.request_new_project(&ctx);
            }
            if menu_item(ui, "Open…", Some(&shortcuts.open), None) {
                self.request_open_project(&ctx);
            }
            if menu_item(ui, "Save", Some(&shortcuts.save), None) {
                self.save_with_continuation(None, &ctx);
            }
            if menu_item(ui, "Save as…", None, None) {
                self.start_save_as(None);
            }
            ui.separator();
            ui.menu_button("Import", |ui| {
                if menu_item(ui, "MFD as mapping…", None, None) {
                    self.mfd_import_dialog = Some(MfdImportTarget::Mapping);
                }
                if menu_item(ui, "MFD as pipeline…", None, None) {
                    self.mfd_import_dialog = Some(MfdImportTarget::Pipeline);
                }
            });
            ui.menu_button("Export", |ui| {
                let response = ui.button("MFD with ferrule extensions…").on_hover_text(
                    "Keeps every ferrule feature so the design round-trips through ferrule.",
                );
                if response.clicked() {
                    self.pending_dialog = Some((
                        DialogKind::ExportMfd,
                        save_file("MFD design", &["mfd"], &self.mfd_export_suggestion()),
                    ));
                    ui.close();
                }
                let response = ui.button("Native MFD…").on_hover_text(
                    "Refuses to export features that native MFD tools cannot represent.",
                );
                if response.clicked() {
                    self.pending_dialog = Some((
                        DialogKind::ExportNativeMfd,
                        save_file("native MFD design", &["mfd"], &self.mfd_export_suggestion()),
                    ));
                    ui.close();
                }
            });
            ui.separator();
            if menu_item(ui, "Generate library…", None, None) {
                self.begin_library_generation();
            }
        });
        ui.menu_button("Edit", |ui| {
            let undo_blocked = (!self.can_undo()).then_some("Nothing to undo");
            if menu_item(ui, "Undo", Some(shortcuts.undo), undo_blocked) {
                self.undo_project();
            }
            let redo_blocked = (!self.history.can_redo()).then_some("Nothing to redo");
            if menu_item(ui, "Redo", Some(shortcuts.redo), redo_blocked) {
                self.redo_project();
            }
        });
        ui.menu_button("Mapping", |ui| {
            if menu_item(ui, "Validate", Some(&shortcuts.validate), None) {
                self.validate_now();
            }
            let auto_connect_blocked = (!self.can_auto_connect()).then_some(NEEDS_MAPPING);
            if menu_item(ui, "Auto-connect fields…", None, auto_connect_blocked) {
                self.begin_auto_connect();
            }
            ui.separator();
            if menu_item(ui, "Add source…", None, None) {
                self.begin_extra_source();
            }
            if menu_item(ui, "Add target…", None, None) {
                self.begin_extra_target();
            }
        });
        ui.menu_button("Run", |ui| {
            if menu_item(ui, "Run", Some(&shortcuts.run), None) {
                self.run(&ctx);
            }
            if menu_item(ui, "Debug run…", None, None) {
                self.debug_run(&ctx);
            }
            let preview_blocked = (!self.can_preview()).then_some(NEEDS_MAPPING);
            if menu_item(ui, "Preview…", None, preview_blocked) {
                self.begin_preview();
            }
            let request_unavailable = self.rest_run_unavailable_reason();
            if menu_item(
                ui,
                "Run JSON request…",
                None,
                request_unavailable.as_deref(),
            ) {
                self.begin_rest_run();
            }
            ui.separator();
            let results_blocked = self
                .run_report
                .is_none()
                .then_some("Run or preview the mapping first");
            if menu_item(ui, self.results_label(), None, results_blocked) {
                self.show_run_report = true;
            }
            if ui
                .checkbox(&mut self.show_run_setup, "Run settings")
                .clicked()
            {
                ui.close();
            }
        });
        ui.menu_button("Pipeline", |ui| {
            if menu_item(ui, "New pipeline…", None, None) {
                self.pending_dialog = Some((
                    DialogKind::CreatePipeline,
                    save_file("ferrule pipeline", &["json"], "pipeline.json"),
                ));
            }
            if menu_item(ui, "Edit pipeline…", None, None) {
                self.pending_dialog = Some((
                    DialogKind::OpenPipelineEditor,
                    pick_file("ferrule pipeline", &["json"]),
                ));
            }
            if menu_item(ui, "Run pipeline…", None, None) {
                self.pending_dialog = Some((
                    DialogKind::OpenPipeline,
                    pick_file("ferrule pipeline", &["json"]),
                ));
            }
        });
        ui.menu_button("View", |ui| {
            ui.checkbox(&mut self.show_source_panel, "Source schema");
            ui.checkbox(&mut self.show_inspector_panel, "Inspector");
            ui.checkbox(&mut self.show_minimap, "Canvas minimap");
            ui.separator();
            ui.menu_button("Arrange", |ui| self.show_arrange_items(ui));
            if menu_item(ui, "Fit canvas", None, None) {
                self.fit_focused_canvas();
            }
            ui.separator();
            if menu_item(ui, "Appearance…", None, None) {
                self.show_appearance_editor = true;
            }
        });
    }

    fn show_arrange_items(&mut self, ui: &mut egui::Ui) {
        if menu_item(ui, "Left to right", None, None) {
            self.arrange_focused_canvas(ArrangeMode::LeftToRight);
        }
        if menu_item(ui, "Compact", None, None) {
            self.arrange_focused_canvas(ArrangeMode::Compact);
        }
    }

    /// The document name, marked while it has unsaved changes; the full
    /// location is in the tooltip.
    fn show_document_identity(&self, ui: &mut egui::Ui) {
        ui.separator();
        let name = self.document.title();
        let text = if self.observed_dirty() {
            format!("{name} •")
        } else {
            name
        };
        let hint = if self.observed_dirty() {
            format!("{}\nUnsaved changes", self.document.location_hint())
        } else {
            self.document.location_hint()
        };
        ui.add(
            egui::Label::new(text)
                .truncate()
                .show_tooltip_when_elided(false),
        )
        .on_hover_text(hint);
    }

    fn show_toolbar(
        &mut self,
        ui: &mut egui::Ui,
        editing_enabled: bool,
        shortcuts: &Shortcuts<'_>,
    ) {
        let ctx = ui.ctx().clone();
        let busy = (!editing_enabled).then_some(BUSY);
        let label = |action: &str, shortcut: &egui::KeyboardShortcut| {
            Shortcuts::label(&ctx, action, shortcut)
        };
        // File
        if ui_kit::icon_button(ui, Icon::FilePlus2, &label("New", &shortcuts.new), busy).clicked() {
            self.request_new_project(&ctx);
        }
        if ui_kit::icon_button(ui, Icon::FolderOpen, &label("Open", &shortcuts.open), busy)
            .clicked()
        {
            self.request_open_project(&ctx);
        }
        if ui_kit::icon_button(ui, Icon::Save, &label("Save", &shortcuts.save), busy).clicked() {
            self.save_with_continuation(None, &ctx);
        }
        toolbar_divider(ui);
        // History
        let undo_blocked = busy.or((!self.can_undo()).then_some("Nothing to undo"));
        if ui_kit::icon_button(
            ui,
            Icon::Undo2,
            &label("Undo", shortcuts.undo),
            undo_blocked,
        )
        .clicked()
        {
            self.undo_project();
        }
        let redo_blocked = busy.or((!self.history.can_redo()).then_some("Nothing to redo"));
        if ui_kit::icon_button(
            ui,
            Icon::Redo2,
            &label("Redo", shortcuts.redo),
            redo_blocked,
        )
        .clicked()
        {
            self.redo_project();
        }
        toolbar_divider(ui);
        // Run
        if ui_kit::icon_button(
            ui,
            Icon::CheckCircle2,
            &label("Validate", &shortcuts.validate),
            busy,
        )
        .clicked()
        {
            self.validate_now();
        }
        if ui_kit::icon_button(ui, Icon::Play, &label("Run", &shortcuts.run), busy).clicked() {
            self.run(&ctx);
        }
        let preview_blocked = busy.or((!self.can_preview()).then_some(NEEDS_MAPPING));
        if ui_kit::icon_button(
            ui,
            Icon::Eye,
            "Preview the current target from in-memory input",
            preview_blocked,
        )
        .clicked()
        {
            self.begin_preview();
        }
        if ui_kit::icon_toggle(
            ui,
            Icon::Settings2,
            "Run settings",
            self.show_run_setup,
            busy,
        )
        .clicked()
        {
            self.show_run_setup = !self.show_run_setup;
        }
        let results_blocked = self
            .run_report
            .is_none()
            .then_some("Run or preview the mapping first");
        if ui_kit::icon_button(
            ui,
            Icon::ClipboardList,
            self.results_label(),
            results_blocked,
        )
        .clicked()
        {
            self.show_run_report = true;
        }
        toolbar_divider(ui);
        // View
        ui.add_enabled_ui(editing_enabled, |ui| {
            let button = egui::Button::new(crate::icons::text(Icon::LayoutGrid, METRICS.icon_size))
                .min_size(egui::Vec2::splat(METRICS.icon_button_size));
            let (response, _) = egui::containers::menu::MenuButton::from_button(button)
                .ui(ui, |ui| self.show_arrange_items(ui));
            let response = response.on_hover_text("Arrange canvas");
            if !editing_enabled {
                response.on_disabled_hover_text(format!("Arrange canvas: {BUSY}"));
            }
        });
        if ui_kit::icon_button(ui, Icon::Maximize2, "Fit canvas", busy).clicked() {
            self.fit_focused_canvas();
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui_kit::icon_button(ui, Icon::Palette, "Appearance", None).clicked() {
                self.show_appearance_editor = true;
            }
        });
    }

    /// The run settings drawer: run files, run values and debugging.
    pub(in crate::app) fn show_run_settings(&mut self, ctx: &egui::Context, editing_enabled: bool) {
        if !self.show_run_setup {
            return;
        }
        let response = ui_kit::dialog(ctx, ui_kit::DialogKind::Docked, "Run settings")
            .width(ui_kit::DialogWidth::Medium)
            .show_body(|ui| {
                ui.add_enabled_ui(editing_enabled, |ui| {
                    // Document runs read CSV only and have no debug route.
                    let documents = self.primary_document.is_some();
                    ui_kit::section(ui, "Files", |ui| {
                        self.show_runtime_paths(ui);
                        if !documents {
                            self.show_owned_pdf_worker_toggle(ui);
                        }
                    });
                    ui.add_space(METRICS.section_gap);
                    self.host_parameters.show(ui);
                    if !documents {
                        ui_kit::collapsible_section(
                            ui,
                            "run_settings_debugging",
                            "Debugging",
                            false,
                            |ui| {
                                self.show_file_debug_controls(ui);
                            },
                        );
                    }
                });
            });
        if response.close_requested {
            self.show_run_setup = false;
        }
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
        self.show_primary_document_settings(ui);
        let documents = self.primary_document.is_some();
        let mut browse = None;
        ui_kit::form_grid(ui, "run_files", |form| {
            if !documents
                && form.row("Input file", |ui| {
                    path_field(ui, &mut self.input_path, "Data file read by Run")
                })
            {
                browse = Some(DialogKind::BrowseInput);
            }
            if form.row("Output file", |ui| {
                path_field(ui, &mut self.output_path, "File written by Run")
            }) {
                browse = Some(DialogKind::BrowseOutput);
            }
        });
        let extensions = [
            "csv", "xml", "json", "db", "sqlite", "edi", "x12", "edifact",
        ];
        match browse {
            Some(DialogKind::BrowseInput) => {
                self.pending_dialog = Some((
                    DialogKind::BrowseInput,
                    pick_file("input data", &extensions),
                ));
            }
            Some(DialogKind::BrowseOutput) => {
                self.pending_dialog = Some((
                    DialogKind::BrowseOutput,
                    save_file("output data", &extensions, &self.output_path),
                ));
            }
            _ => {}
        }
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
                let previous = self.narrow_pane;
                ui.horizontal(|ui| {
                    for pane in WorkspacePane::ALL {
                        ui.selectable_value(&mut self.narrow_pane, pane, pane.label());
                    }
                });
                if previous != self.narrow_pane && self.narrow_pane == WorkspacePane::Canvas {
                    // The canvas was laid out for another width while hidden.
                    self.reset_canvas_view();
                }
            }
        }
    }

    pub(in crate::app) fn show_status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (label, color) = if self.observed_dirty() {
                ("Unsaved changes", self.palette.warning)
            } else if self.document.saved_path().is_none() {
                ("Not saved", ui.visuals().weak_text_color())
            } else {
                ("Saved", self.palette.success)
            };
            ui.colored_label(color, label);
            if !self.diagnostics.is_empty() {
                ui.separator();
                ui.colored_label(
                    self.palette.error,
                    ui_kit::plural(self.diagnostics.len(), "issue"),
                );
            }
            if !self.status.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.shrink_clip_rect(ui.available_rect_before_wrap());
                    let status = ui_kit::sentence(&self.status);
                    let elided = egui::WidgetText::from(status.as_ref())
                        .into_galley(
                            ui,
                            Some(egui::TextWrapMode::Extend),
                            f32::INFINITY,
                            egui::TextStyle::Body,
                        )
                        .size()
                        .x
                        > ui.available_width();
                    let response = ui.add(
                        egui::Label::new(status.as_ref())
                            .truncate()
                            .show_tooltip_when_elided(false),
                    );
                    if !elided {
                        return;
                    }
                    let tooltip_width = ui.ctx().global_style().spacing.tooltip_width.min(
                        (ui.ctx().content_rect().width()
                            - egui::Frame::popup(ui.style()).total_margin().sum().x)
                            .max(0.0),
                    );
                    let _ = egui::Tooltip::for_enabled(&response)
                        .width(tooltip_width)
                        .show(|ui| {
                            ui.set_max_width(tooltip_width);
                            ui.add(egui::Label::new(status.as_ref()).wrap());
                        });
                });
            }
        });
    }
}

/// A path field that fills its row, followed by a Browse button; returns
/// whether Browse was clicked.
fn path_field(ui: &mut egui::Ui, path: &mut String, hint: &str) -> bool {
    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
        let browse = ui.button("Browse…").clicked();
        ui.add(
            egui::TextEdit::singleline(path)
                .hint_text(hint)
                .min_size(egui::vec2(crate::theme::FieldWidth::Medium.min(), 0.0))
                .desired_width(ui.available_width()),
        )
        .on_hover_text(path.as_str());
        browse
    })
    .inner
}
