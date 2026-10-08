//! Browser playground application state and immediate-mode UI.

mod canvas;
mod history;
mod sample;

use eframe::egui;
use egui_snarl::Snarl;
use egui_snarl::ui::SnarlWidget;
use mapping::{NodeId, Project, TabularBoundaryKind};
use web_demo::browser_download::download_utf8_text;
use web_demo::project_document::{self, ProjectDocumentError};
use web_demo::runtime::{self, DataFormat, DataSide};

use canvas::{CanvasNode, DemoViewer, build_snarl, flat_bindings};
use history::ProjectHistory;
use sample::{SAMPLE_XML, demo_project};

#[derive(Clone, Copy, PartialEq, Eq)]
enum WorkspaceView {
    Input,
    Mapping,
    Output,
    Project,
}

pub(super) struct DemoApp {
    project: Project,
    bindings: Vec<(String, NodeId)>,
    snarl: Snarl<CanvasNode>,
    source_text: String,
    source_format: DataFormat,
    target_format: DataFormat,
    output: String,
    project_json: String,
    status: String,
    diagnostic: Option<String>,
    active_view: WorkspaceView,
    live_run: bool,
    run_pending: bool,
    project_changed: bool,
    canvas_view_generation: u64,
    canvas_compact: bool,
    history: ProjectHistory,
    history_notice: Option<String>,
    project_json_dirty: bool,
    edited_constant: Option<NodeId>,
    focused_constant: Option<NodeId>,
}

impl DemoApp {
    pub(super) fn new() -> Self {
        let project = demo_project();
        let mut bindings = Vec::new();
        flat_bindings(&project.root, "", &mut bindings);
        let snarl = build_snarl(&project, &bindings, false);
        let (project_json, diagnostic) = match project_document::to_json(&project) {
            Ok(json) => (json, None),
            Err(error) => (String::new(), Some(error.to_string())),
        };
        let mut history = ProjectHistory::default();
        let history_notice = history
            .capture_and_record(&project, None)
            .err()
            .map(|error| format!("History cleared: {error}"));
        Self {
            project,
            bindings,
            snarl,
            source_text: SAMPLE_XML.to_string(),
            source_format: DataFormat::Xml,
            target_format: DataFormat::Xml,
            output: String::new(),
            project_json,
            status: "Ready".to_string(),
            diagnostic,
            active_view: WorkspaceView::Mapping,
            live_run: true,
            run_pending: true,
            project_changed: false,
            canvas_view_generation: 0,
            canvas_compact: false,
            history,
            history_notice,
            project_json_dirty: false,
            edited_constant: None,
            focused_constant: None,
        }
    }

    fn run(&mut self) {
        self.run_pending = false;
        match runtime::run(
            &self.project,
            &self.source_text,
            self.source_format,
            self.target_format,
        ) {
            Ok(output) => {
                self.output = output;
                self.status = "Mapping completed".to_string();
                self.diagnostic = None;
            }
            Err(error) => {
                self.status = "Run failed".to_string();
                self.diagnostic = Some(error.to_string());
            }
        }
    }

    fn validate(&mut self) {
        let issues = engine::validate(&self.project);
        if issues.is_empty() {
            self.status = "Project is valid".to_string();
            self.diagnostic = None;
        } else {
            self.status = format!("{} validation issue(s)", issues.len());
            self.diagnostic = Some(
                issues
                    .into_iter()
                    .map(|issue| issue.to_string())
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
    }

    fn apply_project_json(&mut self) {
        match project_document::parse_and_validate(&self.project_json) {
            Ok(project) => {
                self.install_project(project);
                self.project_json_dirty = false;
                self.record_project(None, false);
                self.status = "Project applied".to_string();
                self.diagnostic = None;
                self.active_view = WorkspaceView::Mapping;
            }
            Err(error) => {
                self.status = "Project not applied".to_string();
                self.diagnostic = Some(project_document_error(&error));
            }
        }
    }

    fn install_project(&mut self, project: Project) {
        let mut bindings = Vec::new();
        flat_bindings(&project.root, "", &mut bindings);
        self.snarl = build_snarl(&project, &bindings, self.canvas_compact);
        self.source_format = boundary_format(&project, DataSide::Source, self.source_format);
        self.target_format = boundary_format(&project, DataSide::Target, self.target_format);
        self.project = project;
        self.bindings = bindings;
        self.canvas_view_generation = self.canvas_view_generation.wrapping_add(1);
        self.project_changed = false;
        self.edited_constant = None;
        self.focused_constant = None;
        self.run_pending = true;
    }

    fn record_project(&mut self, constant: Option<NodeId>, sync_editor: bool) {
        self.history_notice = self
            .history
            .capture_and_record(&self.project, constant)
            .err()
            .map(|error| format!("History cleared: {error}"));
        if sync_editor && !self.project_json_dirty {
            self.sync_project_json();
        }
    }

    fn finish_project_edits(&mut self) {
        if self.project_changed {
            self.project_changed = false;
            self.record_project(self.edited_constant, true);
            self.edited_constant = None;
        }
        self.history.finish_focus(self.focused_constant);
    }

    fn restore_project(&mut self, redo: bool) {
        let snapshot = if redo {
            self.history.redo_json()
        } else {
            self.history.undo_json()
        };
        let Some(snapshot) = snapshot else {
            return;
        };
        // Canvas edits can be incomplete or invalid. Restore the complete typed
        // document without introducing Apply's separate schema-validation gate.
        let project = match mapping::project_file::decode_str(snapshot) {
            Ok(project) => project,
            Err(error) => {
                self.history_notice = Some(format!("History restore failed: {error}"));
                return;
            }
        };
        if redo {
            self.history.redo();
        } else {
            self.history.undo();
        }
        self.install_project(project);
        if !self.project_json_dirty {
            self.sync_project_json();
        }
        self.history_notice = None;
        self.status = if redo {
            "Project redone"
        } else {
            "Project undone"
        }
        .into();
        self.diagnostic = None;
    }

    fn handle_history_shortcuts(&mut self, context: &egui::Context) {
        if context.text_edit_focused() {
            return;
        }
        let undo = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
        let redo = egui::KeyboardShortcut::new(
            egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
            egui::Key::Z,
        );
        let alternate = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Y);
        if context
            .input_mut(|input| input.consume_shortcut(&redo) || input.consume_shortcut(&alternate))
        {
            self.restore_project(true);
        } else if context.input_mut(|input| input.consume_shortcut(&undo)) {
            self.restore_project(false);
        }
    }

    fn sync_project_json(&mut self) {
        match project_document::to_json(&self.project) {
            Ok(json) => self.project_json = json,
            Err(error) => {
                self.project_json.clear();
                self.status = "Project serialization failed".to_string();
                self.diagnostic = Some(error.to_string());
            }
        }
    }

    fn download_project(&mut self) {
        if self.project_json.is_empty() {
            self.status = "Project download unavailable".to_string();
            return;
        }
        match download_utf8_text("ferrule-project.json", &self.project_json) {
            Ok(()) => self.status = "Project download started".to_string(),
            Err(error) => {
                self.status = "Project download failed".to_string();
                self.diagnostic = Some(error.to_string());
            }
        }
    }

    fn download_output(&mut self) {
        let filename = format!("mapped-output.{}", format_extension(self.target_format));
        match download_utf8_text(&filename, &self.output) {
            Ok(()) => self.status = "Output download started".to_string(),
            Err(error) => {
                self.status = "Output download failed".to_string();
                self.diagnostic = Some(error.to_string());
            }
        }
    }

    fn accept_dropped_project(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|input| input.raw.dropped_files.clone());
        for file in dropped {
            let text = file
                .bytes
                .map(|bytes| String::from_utf8(bytes.to_vec()).map_err(|error| error.to_string()))
                .or_else(|| file.path.as_deref().and_then(read_native_drop));
            let Some(text) = text else {
                continue;
            };
            match text {
                Ok(text) => {
                    self.project_json = text;
                    self.project_json_dirty = true;
                    self.apply_project_json();
                }
                Err(error) => {
                    self.status = "Project drop failed".to_string();
                    self.diagnostic = Some(error);
                }
            }
            break;
        }
    }

    fn show_top_bar(&mut self, ui: &mut egui::Ui, compact: bool) {
        egui::Panel::top("top").show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong("ferrule");
                let views: &[(WorkspaceView, &str)] = if compact {
                    &[
                        (WorkspaceView::Input, "Input"),
                        (WorkspaceView::Mapping, "Mapping"),
                        (WorkspaceView::Output, "Output"),
                        (WorkspaceView::Project, "Project"),
                    ]
                } else {
                    &[
                        (WorkspaceView::Mapping, "Mapping"),
                        (WorkspaceView::Project, "Project"),
                    ]
                };
                for &(view, label) in views {
                    ui.selectable_value(&mut self.active_view, view, label);
                }
                ui.separator();
                if ui
                    .add_enabled(
                        self.history.undo_json().is_some(),
                        egui::Button::new("Undo"),
                    )
                    .on_hover_text("Undo project edit (Ctrl/Command+Z); text fields keep text undo")
                    .clicked()
                {
                    self.restore_project(false);
                }
                if ui
                    .add_enabled(
                        self.history.redo_json().is_some(),
                        egui::Button::new("Redo"),
                    )
                    .on_hover_text("Redo project edit (Ctrl/Command+Shift+Z or Ctrl/Command+Y)")
                    .clicked()
                {
                    self.restore_project(true);
                }
                if ui.button("Run").clicked() {
                    self.run();
                }
                ui.checkbox(&mut self.live_run, "Live");
                if ui.button("Validate").clicked() {
                    self.validate();
                }
                if ui.button("Reset").clicked() {
                    *self = Self::new();
                }
                if ui.button("Fit").clicked() {
                    self.canvas_view_generation = self.canvas_view_generation.wrapping_add(1);
                }
                ui.hyperlink_to("GitHub", "https://github.com/DeandreT/ferrule");
            });
            ui.horizontal_wrapped(|ui| {
                ui.label("Input format");
                if format_picker(ui, "source_format", &mut self.source_format) {
                    self.run_pending = true;
                }
                ui.label("Output format");
                if format_picker(ui, "target_format", &mut self.target_format) {
                    self.run_pending = true;
                }
                if ui.button("Download output").clicked() {
                    self.download_output();
                }
                ui.separator();
                ui.label(&self.status);
                if let Some(notice) = &self.history_notice {
                    ui.label(notice);
                }
            });
        });
    }

    fn show_input(&mut self, ui: &mut egui::Ui) {
        ui.strong(format!("Input ({})", self.source_format));
        egui::ScrollArea::vertical().show(ui, |ui| {
            if ui
                .add(
                    egui::TextEdit::multiline(&mut self.source_text)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(28),
                )
                .changed()
            {
                self.run_pending = true;
            }
        });
    }

    fn show_output(&mut self, ui: &mut egui::Ui) {
        ui.strong(format!("Output ({})", self.target_format));
        egui::ScrollArea::vertical().show(ui, |ui| {
            let mut text = self.output.as_str();
            ui.add(
                egui::TextEdit::multiline(&mut text)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(28),
            );
        });
    }

    fn show_project(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.strong("Project JSON");
            if self.project_json_dirty {
                ui.label("Unapplied JSON");
            }
            if ui.button("Apply").clicked() {
                self.apply_project_json();
            }
            if ui
                .add_enabled(!self.project_json.is_empty(), egui::Button::new("Download"))
                .clicked()
            {
                self.download_project();
            }
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            if ui
                .add(
                    egui::TextEdit::multiline(&mut self.project_json)
                        .code_editor()
                        .desired_width(f32::INFINITY)
                        .desired_rows(30),
                )
                .changed()
            {
                self.project_json_dirty = true;
            }
        });
    }

    fn show_mapping(&mut self, ui: &mut egui::Ui) {
        self.edited_constant = None;
        let mut viewer = DemoViewer::new(
            &mut self.project.graph,
            &self.bindings,
            &mut self.run_pending,
            &mut self.project_changed,
            &mut self.edited_constant,
            &mut self.focused_constant,
        );
        SnarlWidget::new()
            .id(egui::Id::new((
                "web_mapping_canvas",
                self.canvas_view_generation,
            )))
            .show(&mut self.snarl, &mut viewer, ui);
    }

    fn show_workspace(&mut self, ui: &mut egui::Ui) {
        self.accept_dropped_project(ui.ctx());
        self.handle_history_shortcuts(ui.ctx());
        self.focused_constant = None;
        let compact = ui.available_width() < 900.0;
        if compact != self.canvas_compact {
            self.canvas_compact = compact;
            self.snarl = build_snarl(&self.project, &self.bindings, compact);
            self.canvas_view_generation = self.canvas_view_generation.wrapping_add(1);
        }
        if !compact
            && matches!(
                self.active_view,
                WorkspaceView::Input | WorkspaceView::Output
            )
        {
            self.active_view = WorkspaceView::Mapping;
        }
        self.show_top_bar(ui, compact);

        if let Some(diagnostic) = &self.diagnostic {
            egui::Panel::bottom("diagnostic")
                .resizable(true)
                .default_size(80.0)
                .show(ui, |ui| {
                    ui.strong("Diagnostics");
                    egui::ScrollArea::vertical().show(ui, |ui| ui.monospace(diagnostic));
                });
        }

        if compact {
            egui::CentralPanel::default().show(ui, |ui| match self.active_view {
                WorkspaceView::Input => self.show_input(ui),
                WorkspaceView::Mapping => self.show_mapping(ui),
                WorkspaceView::Output => self.show_output(ui),
                WorkspaceView::Project => self.show_project(ui),
            });
        } else if self.active_view == WorkspaceView::Project {
            egui::CentralPanel::default().show(ui, |ui| self.show_project(ui));
        } else {
            egui::Panel::left("source")
                .default_size(300.0)
                .min_size(220.0)
                .max_size(420.0)
                .show(ui, |ui| self.show_input(ui));
            egui::Panel::right("output")
                .default_size(300.0)
                .min_size(220.0)
                .max_size(420.0)
                .show(ui, |ui| self.show_output(ui));
            egui::CentralPanel::default().show(ui, |ui| self.show_mapping(ui));
        }

        self.finish_project_edits();
        if self.run_pending && self.live_run {
            self.run();
        }
    }
}

impl eframe::App for DemoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.show_workspace(ui);
    }
}

fn format_picker(ui: &mut egui::Ui, id: &str, format: &mut DataFormat) -> bool {
    let before = *format;
    egui::ComboBox::from_id_salt(id)
        .selected_text(format.to_string())
        .show_ui(ui, |ui| {
            for choice in [
                DataFormat::Xml,
                DataFormat::Json,
                DataFormat::Csv,
                DataFormat::Xbrl,
            ] {
                ui.selectable_value(format, choice, choice.to_string());
            }
        });
    *format != before
}

fn format_extension(format: DataFormat) -> &'static str {
    match format {
        DataFormat::Xml => "xml",
        DataFormat::Json => "json",
        DataFormat::Csv => "csv",
        DataFormat::Xbrl => "xbrl",
    }
}

fn boundary_format(project: &Project, side: DataSide, current: DataFormat) -> DataFormat {
    let options = match side {
        DataSide::Source => &project.source_options,
        DataSide::Target => &project.target_options,
    };
    if options.xbrl.is_some() {
        DataFormat::Xbrl
    } else if options.xml_document {
        DataFormat::Xml
    } else if options.json_document || options.json5 || options.json_lines {
        DataFormat::Json
    } else if options.tabular_kind == Some(TabularBoundaryKind::Csv) {
        DataFormat::Csv
    } else if current == DataFormat::Xbrl {
        DataFormat::Xml
    } else {
        current
    }
}

fn project_document_error(error: &ProjectDocumentError) -> String {
    match error {
        ProjectDocumentError::Validation(issues) => issues
            .iter()
            .map(|issue| issue.to_string())
            .collect::<Vec<_>>()
            .join("\n"),
        ProjectDocumentError::Serialize(_) | ProjectDocumentError::Parse(_) => error.to_string(),
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn read_native_drop(path: &std::path::Path) -> Option<Result<String, String>> {
    (!path.as_os_str().is_empty())
        .then(|| std::fs::read_to_string(path).map_err(|error| error.to_string()))
}

#[cfg(target_arch = "wasm32")]
fn read_native_drop(_path: &std::path::Path) -> Option<Result<String, String>> {
    None
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[cfg(test)]
mod history_tests;
