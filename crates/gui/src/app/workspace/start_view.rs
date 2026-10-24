use super::*;

use crate::ui_kit::{self, METRICS, TextRole};
use lucide_icons::Icon;

/// Snapshot key of the project a fresh editor starts with.
static BLANK_PROJECT_KEY: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    crate::project_state::project_snapshot_key(&crate::new_mapping::blank_project())
});

impl FerruleApp {
    /// Nothing is open yet: an untitled, unchanged, empty project.
    pub(in crate::app) fn is_start_state(&self) -> bool {
        self.document.saved_path().is_none()
            && self.primary_document.is_none()
            && self.embedded_stage_namespace.is_none()
            && !self.observed_dirty()
            && self.observed_editor.state.serialized_project == *BLANK_PROJECT_KEY
    }

    /// The first screen: start a mapping, open a project or import a design.
    pub(in crate::app) fn show_start_view(&mut self, ui: &mut egui::Ui) {
        let editing_enabled = self.ui_project_editing_enabled();
        let busy = (!editing_enabled).then_some("Unavailable while another operation is running");
        let width = ui_kit::DialogWidth::Narrow.px().min(ui.available_width());
        let top_space = (ui.available_height() * 0.25).max(ui_kit::space::XXL);
        egui::ScrollArea::vertical()
            .id_salt("start_view_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| self.show_start_actions(ui, width, top_space, busy));
    }

    fn show_start_actions(
        &mut self,
        ui: &mut egui::Ui,
        width: f32,
        top_space: f32,
        busy: Option<&str>,
    ) {
        ui.vertical_centered(|ui| {
            ui.add_space(top_space);
            ui.label(TextRole::Title.text("Start a mapping"));
            ui.add_space(ui_kit::space::SM);
            ui_kit::help(ui, "Create a mapping from source and target schemas, open a saved project or import an MFD design.");
            ui.add_space(METRICS.section_gap);
            let mut action = None;
            for (icon, label, choice) in [
                (Icon::FilePlus2, "New mapping…", StartAction::New),
                (Icon::FolderOpen, "Open project…", StartAction::Open),
                (Icon::Import, "Import MFD…", StartAction::ImportMfd),
                (Icon::Workflow, "Open pipeline…", StartAction::OpenPipeline),
            ] {
                let button =
                    egui::Button::new((crate::icons::text(icon, METRICS.icon_size), label))
                        .min_size(egui::vec2(width, METRICS.control_height + ui_kit::space::MD));
                let response = ui.add_enabled(busy.is_none(), button);
                let response = match busy {
                    Some(reason) => response.on_disabled_hover_text(reason),
                    None => response,
                };
                if response.clicked() {
                    action = Some(choice);
                }
            }
            let mut open_recent = None;
            let recent: Vec<&String> = self
                .recent_projects
                .iter()
                .filter(|path| std::path::Path::new(path).is_file())
                .collect();
            if !recent.is_empty() {
                ui.add_space(METRICS.section_gap);
                ui.label(TextRole::Section.text("Recent"));
                for path in recent {
                    let response = ui.add_enabled(
                        busy.is_none(),
                        egui::Button::new(ui_kit::file_label(path))
                            .frame(false)
                            .min_size(egui::vec2(width, METRICS.control_height)),
                    );
                    if response.on_hover_text(path.as_str()).clicked() {
                        open_recent = Some(std::path::PathBuf::from(path));
                    }
                }
            }
            if let Some(path) = open_recent {
                self.load_project_from(&path);
            }
            match action {
                Some(StartAction::New) => {
                    if let Some(action) =
                        self.request_destructive_action(DestructiveAction::NewProject)
                    {
                        self.perform_destructive_action(action, ui.ctx());
                    }
                }
                Some(StartAction::Open) => {
                    if let Some(action) =
                        self.request_destructive_action(DestructiveAction::OpenProject)
                    {
                        self.perform_destructive_action(action, ui.ctx());
                    }
                }
                Some(StartAction::ImportMfd) => {
                    self.mfd_import_dialog = Some(MfdImportTarget::Mapping);
                }
                Some(StartAction::OpenPipeline) => {
                    self.pending_dialog = Some((
                        DialogKind::OpenPipelineEditor,
                        pick_file("ferrule pipeline", &["json"]),
                    ));
                }
                None => {}
            }
        });
    }
}

#[derive(Clone, Copy)]
enum StartAction {
    New,
    Open,
    ImportMfd,
    OpenPipeline,
}
