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
    pub(super) fn show(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Input root");
            ui.add(egui::TextEdit::singleline(&mut self.input_root).desired_width(190.0))
                .on_hover_text(
                    "Primary documents are read only beneath this folder. URIs, rooted \
                     or parent paths and symbolic links are refused.",
                );
        });
        ui.horizontal(|ui| {
            ui.label("Max documents");
            ui.add(egui::DragValue::new(&mut self.max_documents).range(1..=1_000_000));
            ui.label("Max bytes per document");
            ui.add(egui::DragValue::new(&mut self.max_document_bytes).range(1..=u64::MAX));
        });
        ui.weak(self.selection_label());
    }
}

impl FerruleApp {
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
