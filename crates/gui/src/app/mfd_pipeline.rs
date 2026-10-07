//! Explicit connected-design import and current-snapshot pipeline export.

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use anyhow::{Context as _, bail};
use mfd::{ExportProfile, MfdError};

use super::{DialogKind, FerruleApp, pipeline_editor_ui};
use crate::diagnostics::Diagnostic;

pub(super) struct PendingMfdPipelineImport {
    options: mfd::ImportOptions,
    imported: Option<mfd::ImportedPipeline>,
}

pub(super) struct PipelineMfdExport {
    pipeline: mapping::Pipeline,
    document_path: PathBuf,
    profile: ExportProfile,
}

impl PipelineMfdExport {
    fn capture(
        editor: &pipeline_editor_ui::PipelineEditorUi,
        profile: ExportProfile,
    ) -> anyhow::Result<Self> {
        if editor.has_unapplied_text() {
            bail!("Apply staged text edits before exporting the pipeline");
        }
        let issues = editor.document.issues();
        if !issues.is_empty() {
            bail!("Pipeline validation failed: {}", issues.join("; "));
        }
        Ok(Self {
            pipeline: editor.document.pipeline.clone(),
            document_path: editor.document.path.clone(),
            profile,
        })
    }

    fn stage_context(&self) -> String {
        self.pipeline
            .stages
            .iter()
            .map(|stage| stage.id.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn publish(&self, path: &Path) -> anyhow::Result<mfd::ExportReport> {
        let mut pipeline = self.pipeline.clone();
        // Match the saved CLI facade's path policy without reloading a file.
        for stage in &mut pipeline.stages {
            cli::rebase_project_paths(&mut stage.project, &self.document_path, path)?;
        }
        mfd::export_pipeline_with_profile(&pipeline, path, self.profile)
            .map_err(anyhow::Error::from)
    }
}

impl FerruleApp {
    pub(super) fn pipeline_mfd_busy(&self) -> bool {
        self.pending_mfd_pipeline_import.is_some()
            || self.pending_pipeline_mfd_export.is_some()
            || matches!(
                self.pending_pipeline_editor_action.as_ref(),
                Some(pipeline_editor_ui::PipelineEditorAction::ImportMfd(_))
            )
    }

    pub(super) fn begin_mfd_pipeline_import(&mut self) {
        if !self.project_editing_enabled()
            || self.pipeline_mfd_busy()
            || self.pipeline_run_draft.is_some()
            || self.pending_pipeline_editor_action.is_some()
        {
            return;
        }
        let options = self.mfd_import_options();
        match options {
            Ok(options) => self.request_pipeline_editor_action(
                pipeline_editor_ui::PipelineEditorAction::ImportMfd(options),
            ),
            Err(error) => {
                self.status = "invalid MFD package manifest".into();
                self.diagnostics
                    .error("MFD pipeline import failed", error.to_string());
            }
        }
    }

    pub(super) fn start_mfd_pipeline_import(&mut self, options: mfd::ImportOptions) {
        self.pending_mfd_pipeline_import = Some(PendingMfdPipelineImport {
            options,
            imported: None,
        });
        let receiver = self.pipeline_mfd_dialog(DialogKind::ImportMfdPipeline, "");
        self.pending_dialog = Some((DialogKind::ImportMfdPipeline, receiver));
    }

    pub(super) fn stage_mfd_pipeline_source(&mut self, path: PathBuf) -> Result<(), MfdError> {
        let Some(mut draft) = self.pending_mfd_pipeline_import.take() else {
            return Err(MfdError::UnsupportedImport(
                "Pipeline import settings are unavailable".into(),
            ));
        };
        // There is no fallback to single-project import when the chain refuses.
        let imported = mfd::import_pipeline_with_options(&path, &draft.options)?;
        let suggested = imported
            .mapping_path
            .with_extension("pipeline.json")
            .display()
            .to_string();
        draft.imported = Some(imported);
        self.pending_mfd_pipeline_import = Some(draft);
        let receiver =
            self.pipeline_mfd_dialog(DialogKind::ChooseImportedPipelineDestination, &suggested);
        self.pending_dialog = Some((DialogKind::ChooseImportedPipelineDestination, receiver));
        self.status =
            "choose a new pipeline location; no file is written until Save pipeline".into();
        Ok(())
    }

    pub(super) fn finish_mfd_pipeline_import(&mut self, destination: &Path) -> anyhow::Result<()> {
        let draft = self
            .pending_mfd_pipeline_import
            .take()
            .context("Pipeline import settings are unavailable")?;
        let imported = draft
            .imported
            .context("Choose a connected MFD design first")?;
        let mapping_path = imported.mapping_path.clone();
        let (document, warnings) =
            crate::pipeline_edit::PipelineEditorDocument::from_imported_mfd(destination, imported)?;
        let count = document.pipeline.stages.len();
        let stages = document
            .pipeline
            .stages
            .iter()
            .map(|stage| stage.id.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        self.pipeline_editor = Some(pipeline_editor_ui::PipelineEditorUi::new(document));
        if warnings.is_empty() {
            self.diagnostics.clear();
        } else {
            self.diagnostics.replace(
                format!("MFD pipeline import: {stages}"),
                warnings
                    .into_iter()
                    .map(|warning| Diagnostic::import_warning(warning, &mapping_path)),
            );
        }
        self.status = format!(
            "imported {count} stages; pipeline is unsaved at {}",
            destination.display()
        );
        Ok(())
    }

    pub(super) fn begin_pipeline_mfd_export(&mut self, profile: ExportProfile) {
        if !self.project_editing_enabled()
            || self.pipeline_mfd_busy()
            || self.pipeline_run_draft.is_some()
            || self.pending_pipeline_editor_action.is_some()
        {
            return;
        }
        let snapshot = self
            .pipeline_editor
            .as_ref()
            .context("Open a pipeline first")
            .and_then(|editor| PipelineMfdExport::capture(editor, profile));
        match snapshot {
            Ok(snapshot) => {
                let suggested = snapshot
                    .document_path
                    .with_extension("mfd")
                    .display()
                    .to_string();
                self.pending_pipeline_mfd_export = Some(snapshot);
                let receiver = self.pipeline_mfd_dialog(DialogKind::ExportPipelineMfd, &suggested);
                self.pending_dialog = Some((DialogKind::ExportPipelineMfd, receiver));
            }
            Err(error) => {
                self.status = "pipeline export unavailable".into();
                self.diagnostics
                    .error("Pipeline MFD export", format!("{error:#}"));
            }
        }
    }

    pub(super) fn finish_pipeline_mfd_export(&mut self, path: &Path) {
        let Some(snapshot) = self.pending_pipeline_mfd_export.take() else {
            return;
        };
        let stages = snapshot.stage_context();
        match snapshot.publish(path) {
            Ok(report) => {
                self.status = format!("exported pipeline {}", path.display());
                if report.warnings.is_empty() {
                    self.diagnostics.clear();
                } else {
                    self.diagnostics.replace(
                        format!("Pipeline MFD export: {stages}"),
                        report.warnings.into_iter().map(Diagnostic::warning),
                    );
                }
            }
            Err(error) => {
                self.status = format!("failed to export pipeline {}", path.display());
                if let Some(MfdError::IncompatibleExport(report)) = error.downcast_ref::<MfdError>()
                {
                    let issues = report
                        .issues
                        .iter()
                        .cloned()
                        .map(Diagnostic::export_compatibility_issue);
                    let warnings = report
                        .warnings
                        .iter()
                        .map(|warning| Diagnostic::error(format!("Export warning: {warning}")));
                    self.diagnostics.replace(
                        format!("Native pipeline MFD export rejected: {stages}"),
                        issues.chain(warnings),
                    );
                } else {
                    self.diagnostics.error(
                        format!("Pipeline MFD export: {stages}"),
                        format!("{error:#}"),
                    );
                }
            }
        }
    }

    pub(super) fn cancel_pipeline_mfd_dialog(&mut self, kind: DialogKind) {
        match kind {
            DialogKind::ImportMfdPipeline | DialogKind::ChooseImportedPipelineDestination => {
                self.pending_mfd_pipeline_import = None;
            }
            DialogKind::ExportPipelineMfd => self.pending_pipeline_mfd_export = None,
            _ => {}
        }
    }

    pub(super) fn cancel_pipeline_mfd_for_app_close(&mut self) {
        if self.pending_dialog.as_ref().is_some_and(|(kind, _)| {
            matches!(
                kind,
                DialogKind::ImportMfdPipeline
                    | DialogKind::ChooseImportedPipelineDestination
                    | DialogKind::ExportPipelineMfd
            )
        }) {
            self.cancel_pending_file_dialog();
        }
        self.pending_mfd_pipeline_import = None;
        self.pending_pipeline_mfd_export = None;
        if matches!(
            self.pending_pipeline_editor_action.as_ref(),
            Some(pipeline_editor_ui::PipelineEditorAction::ImportMfd(_))
        ) {
            self.pending_pipeline_editor_action = None;
        }
    }

    fn pipeline_mfd_dialog(
        &mut self,
        kind: DialogKind,
        suggested: &str,
    ) -> Receiver<Option<String>> {
        #[cfg(test)]
        if let Some(receiver) = self.pipeline_mfd_dialog_override.take() {
            return receiver;
        }
        match kind {
            DialogKind::ImportMfdPipeline => super::pick_file("connected MFD design", &["mfd"]),
            DialogKind::ChooseImportedPipelineDestination => {
                super::save_file("new pipeline location (saved later)", &["json"], suggested)
            }
            DialogKind::ExportPipelineMfd => {
                super::save_file("pipeline MFD design", &["mfd"], suggested)
            }
            _ => unreachable!("only pipeline MFD actions use this dialog"),
        }
    }
}

#[cfg(test)]
mod tests;
