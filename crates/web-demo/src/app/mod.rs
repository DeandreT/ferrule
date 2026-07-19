//! Browser playground application state and immediate-mode UI.

mod canvas;
mod sample;

use eframe::egui;
use egui_snarl::Snarl;
use egui_snarl::ui::SnarlWidget;
use mapping::{NodeId, Project, TabularBoundaryKind};
use web_demo::browser_download::download_utf8_text;
use web_demo::project_document::{self, ProjectDocumentError};
use web_demo::runtime::{self, DataFormat, DataSide};

use canvas::{CanvasNode, DemoViewer, build_snarl, flat_bindings};
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
                let mut bindings = Vec::new();
                flat_bindings(&project.root, "", &mut bindings);
                self.snarl = build_snarl(&project, &bindings, self.canvas_compact);
                self.source_format =
                    boundary_format(&project, DataSide::Source, self.source_format);
                self.target_format =
                    boundary_format(&project, DataSide::Target, self.target_format);
                self.project = project;
                self.bindings = bindings;
                self.canvas_view_generation = self.canvas_view_generation.wrapping_add(1);
                self.project_changed = false;
                self.run_pending = true;
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

    fn sync_project_json(&mut self) {
        match project_document::to_json(&self.project) {
            Ok(json) => self.project_json = json,
            Err(error) => {
                self.status = "Project serialization failed".to_string();
                self.diagnostic = Some(error.to_string());
            }
        }
    }

    fn download_project(&mut self) {
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
            if ui.button("Apply").clicked() {
                self.apply_project_json();
            }
            if ui.button("Download").clicked() {
                self.download_project();
            }
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(&mut self.project_json)
                    .code_editor()
                    .desired_width(f32::INFINITY)
                    .desired_rows(30),
            );
        });
    }

    fn show_mapping(&mut self, ui: &mut egui::Ui) {
        let mut viewer = DemoViewer::new(
            &mut self.project.graph,
            &self.bindings,
            &mut self.run_pending,
            &mut self.project_changed,
        );
        SnarlWidget::new()
            .id(egui::Id::new((
                "web_mapping_canvas",
                self.canvas_view_generation,
            )))
            .show(&mut self.snarl, &mut viewer, ui);
    }
}

impl eframe::App for DemoApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.accept_dropped_project(ui.ctx());
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

        if self.project_changed {
            self.project_changed = false;
            self.sync_project_json();
        }
        if self.run_pending && self.live_run {
            self.run();
        }
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
    } else if options.json_document || options.json_lines {
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
