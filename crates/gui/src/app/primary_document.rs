//! Saved primary filename document projects in the editor. The document's
//! filename plan is retained beside the edited project and saved with it; Run
//! and Preview read documents only beneath the explicit input root, through
//! the same local host and limits as `ferrule run-document`.

use std::path::Path;

use anyhow::bail;
use mapping::{DocumentProject, EmptyFileSelection, PrimaryFileSelection, PrimarySourcePlan};

use super::FerruleApp;

pub(super) struct PrimaryDocumentSession {
    pub(super) plan: PrimarySourcePlan,
    pub(super) input_root: String,
    pub(super) max_document_bytes: u64,
    pub(super) max_documents: usize,
}

impl PrimaryDocumentSession {
    /// Documents resolve beneath the project's folder until the root is edited.
    pub(super) fn new(plan: PrimarySourcePlan, project_path: &Path) -> Self {
        let folder = project_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let defaults = cli::PrimaryDocumentPolicy::new(folder);
        Self {
            plan,
            input_root: folder.display().to_string(),
            max_document_bytes: defaults.max_document_bytes,
            max_documents: defaults.max_documents,
        }
    }

    pub(super) fn document(&self, project: &mapping::Project) -> DocumentProject {
        DocumentProject::new(project.clone(), self.plan.clone())
    }

    pub(super) fn policy(&self) -> anyhow::Result<cli::PrimaryDocumentPolicy> {
        let root = self.input_root.trim();
        if root.is_empty() {
            bail!("enter the input root that primary documents are read beneath");
        }
        Ok(cli::PrimaryDocumentPolicy {
            input_root: root.into(),
            max_document_bytes: self.max_document_bytes,
            max_documents: self.max_documents,
        })
    }

    pub(super) fn selection_label(&self) -> &'static str {
        match self.plan.selection {
            PrimaryFileSelection::Direct => "One document named by the primary filename",
            PrimaryFileSelection::FileNameWildcard {
                empty: EmptyFileSelection::Refuse,
            } => "Every file matching the primary filename wildcard; no match is an error",
            PrimaryFileSelection::FileNameWildcard {
                empty: EmptyFileSelection::MapNoDocuments,
            } => "Every file matching the primary filename wildcard; no match maps no documents",
        }
    }

    /// The input root, limits and selection, in place of the input file.
    pub(super) fn show(&mut self, ui: &mut egui::Ui) -> bool {
        let mut browse = false;
        crate::ui_kit::form_grid(ui, "primary_document_settings", |form| {
            form.row("Input folder", |ui| {
                ui.add(crate::ui_kit::text_field(
                    &mut self.input_root,
                    crate::ui_kit::FieldWidth::Fill,
                ))
                .on_hover_text(
                    "Primary documents are read only beneath this folder. URIs, rooted \
                     or parent paths and symbolic links are refused.",
                );
                browse = ui.button("Browse…").clicked();
            });
            form.row("Maximum documents", |ui| {
                ui.add(egui::DragValue::new(&mut self.max_documents).range(1..=1_000_000));
            });
            form.row("Maximum size per document", |ui| {
                ui.add(
                    egui::DragValue::new(&mut self.max_document_bytes)
                        .range(1..=u64::MAX)
                        .speed(65_536.0)
                        .suffix(" MiB")
                        .custom_formatter(|bytes, _| format_document_mebibytes(bytes))
                        .custom_parser(parse_document_mebibytes),
                )
                .on_hover_text(format!(
                    "{} bytes. One MiB is 1,048,576 bytes.",
                    self.max_document_bytes
                ));
            });
        });
        crate::ui_kit::help(ui, self.selection_label());
        browse
    }
}

fn format_document_mebibytes(bytes: f64) -> String {
    let value = format!("{:.6}", bytes / 1_048_576.0);
    value.trim_end_matches('0').trim_end_matches('.').to_owned()
}

fn parse_document_mebibytes(text: &str) -> Option<f64> {
    let mebibytes = text.trim().strip_suffix("MiB").unwrap_or(text).trim();
    let bytes = mebibytes.parse::<f64>().ok()? * 1_048_576.0;
    (bytes.is_finite() && bytes >= 1.0 && bytes <= u64::MAX as f64).then_some(bytes.round())
}

#[cfg(test)]
mod size_tests {
    use super::{format_document_mebibytes, parse_document_mebibytes};

    #[test]
    fn document_sizes_use_mebibytes_without_changing_the_stored_unit() {
        assert_eq!(format_document_mebibytes(8_388_608.0), "8");
        assert_eq!(format_document_mebibytes(1_572_864.0), "1.5");
        assert_eq!(parse_document_mebibytes("8"), Some(8_388_608.0));
        assert_eq!(parse_document_mebibytes("1.5 MiB"), Some(1_572_864.0));
    }

    #[test]
    fn document_size_entry_rejects_invalid_or_nonfinite_limits() {
        for text in ["", "zero", "0", "-1", "NaN", "inf", "1e100"] {
            assert_eq!(parse_document_mebibytes(text), None, "{text}");
        }
    }

    #[test]
    fn choosing_an_input_folder_updates_only_the_document_run_setting() {
        use super::super::{DialogKind, FerruleApp};
        use super::PrimaryDocumentSession;

        let mut app = FerruleApp::default();
        app.primary_document = Some(PrimaryDocumentSession::new(
            mapping::PrimarySourcePlan::new(0),
            std::path::Path::new("/demo/project.json"),
        ));
        let original_project = serde_json::to_value(&app.project).unwrap();
        let original_input = app.input_path.clone();
        let (send, receive) = std::sync::mpsc::channel();
        app.pending_dialog = Some((DialogKind::BrowsePrimaryInputRoot, receive));
        send.send(Some("/demo/selected".to_owned())).unwrap();
        app.poll_dialog(&egui::Context::default());
        assert_eq!(
            app.primary_document.as_ref().unwrap().input_root,
            "/demo/selected"
        );
        assert_eq!(app.input_path, original_input);
        assert_eq!(
            serde_json::to_value(&app.project).unwrap(),
            original_project
        );
        assert!(app.pending_dialog.is_none());
    }

    #[test]
    fn cancelling_the_input_folder_picker_keeps_the_current_folder() {
        use super::super::{DialogKind, FerruleApp};
        use super::PrimaryDocumentSession;

        let mut app = FerruleApp::default();
        app.primary_document = Some(PrimaryDocumentSession::new(
            mapping::PrimarySourcePlan::new(0),
            std::path::Path::new("/demo/project.json"),
        ));
        let (send, receive) = std::sync::mpsc::channel();
        app.pending_dialog = Some((DialogKind::BrowsePrimaryInputRoot, receive));
        send.send(None).unwrap();
        app.poll_dialog(&egui::Context::default());
        assert_eq!(app.primary_document.as_ref().unwrap().input_root, "/demo");
        assert!(app.pending_dialog.is_none());
    }
}

impl FerruleApp {
    pub(super) fn show_primary_document_settings(&mut self, ui: &mut egui::Ui) {
        let dialog_idle = self.pending_dialog.is_none();
        let Some(session) = &mut self.primary_document else {
            return;
        };
        let browse = ui.add_enabled_ui(dialog_idle, |ui| session.show(ui)).inner;
        if browse {
            self.browse_primary_input_root();
        }
    }

    pub(super) fn browse_primary_input_root(&mut self) {
        if self.pending_dialog.is_some() {
            return;
        }
        if let Some(session) = &self.primary_document {
            self.pending_dialog = Some((
                super::DialogKind::BrowsePrimaryInputRoot,
                super::library_generation::pick_folder(Path::new(&session.input_root)),
            ));
        }
    }

    /// The run-time refusals that need no input: the document route, the
    /// filename plan and the mapping.
    pub(super) fn primary_document_preflight(&self) -> anyhow::Result<()> {
        let Some(session) = &self.primary_document else {
            return Ok(());
        };
        session.policy()?;
        cli::check_primary_document(&session.document(&self.project))
    }
}
