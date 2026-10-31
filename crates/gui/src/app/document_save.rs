//! Mapping document persistence: staged publication, save points, and the
//! relative-path basis retained by editor history.

use std::path::Path;

use super::{CanvasLayout, EditorSnapshot, FerruleApp};
use crate::diagnostics::Diagnostic;
use crate::document::DocumentLocation;
use crate::layout_store::{project_fingerprint, write_layout};

pub(super) struct DocumentSaveOutcome {
    pub(super) validation_issues: Vec<engine::ValidationIssue>,
    pub(super) layout_warning: Option<String>,
}

impl FerruleApp {
    /// Publishes the project, then its layout sidecar. Editor state adopts the
    /// rebased project and new save point only after the project file is
    /// completely replaced; a sidecar failure is reported as a warning because
    /// the published project reopens with a default layout.
    pub(super) fn save_document_to(&mut self, path: &Path) -> anyhow::Result<DocumentSaveOutcome> {
        anyhow::ensure!(
            !self.stage_document_actions_blocked(),
            "Finish the stage canvas before saving a mapping"
        );
        let previous_path = self.document.suggested_path().to_path_buf();
        let relocated = cli::project_paths_relocate(&previous_path, path)?;
        let mut project = self.project.clone();
        cli::rebase_project_paths(&mut project, &previous_path, path)?;
        let json = match &self.primary_document {
            Some(session) => {
                mapping::document_project_file::encode_pretty(&session.document(&project))?
            }
            None => mapping::project_file::encode_pretty(&project)?,
        };
        let layout =
            CanvasLayout::capture(&project, &self.main_canvas.snarl, &self.mapping_workspace);
        crate::document_file::replace(path, json.as_bytes())?;
        self.project = project;
        self.document = DocumentLocation::saved(path);
        crate::preferences::remember_recent_project(
            &mut self.recent_projects,
            path.display().to_string(),
        );
        self.mark_clean();
        let rebased = if relocated {
            self.rebase_retained_history(&previous_path, path)
        } else {
            // Within one directory rebasing only normalizes spelling, so retained
            // snapshots keep their meaning; the observation must still match
            // the normalized project so the save is not recorded as an edit.
            self.rebase_observed(&previous_path, path)
        };
        if rebased.is_err() {
            // A snapshot whose paths cannot be expressed from the new directory
            // must not be restorable there.
            self.rebase_history();
        }
        let layout_warning = write_layout(path, &layout)
            .err()
            .map(|error| format!("canvas layout was not saved: {error:#}"));
        Ok(DocumentSaveOutcome {
            validation_issues: cli::validate(&self.project),
            layout_warning,
        })
    }

    /// Re-expresses every snapshot that Undo, Redo, a pending edit, or the
    /// observer can restore against the new document directory, so restored
    /// relative paths keep their original input and output identities.
    fn rebase_retained_history(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        self.history
            .try_for_each_retained_snapshot_mut(|snapshot| rebase_snapshot(snapshot, from, to))?;
        if let Some(pending) = &mut self.pending_history {
            rebase_snapshot(&mut pending.before, from, to)?;
        }
        self.rebase_observed(from, to)
    }

    /// Rebases the observation without recording an edit; its revision
    /// still follows the rewritten content.
    fn rebase_observed(&mut self, from: &Path, to: &Path) -> std::io::Result<()> {
        let previous = self.observed_editor.state.clone();
        let rebased = rebase_snapshot(&mut self.observed_editor, from, to);
        self.record_observed_revision(&previous);
        rebased
    }

    pub(super) fn saved_status(path: &Path, outcome: &DocumentSaveOutcome) -> String {
        if !outcome.validation_issues.is_empty() {
            format!(
                "saved {} with {}",
                crate::ui_kit::file_label(path),
                crate::ui_kit::plural(outcome.validation_issues.len(), "validation issue")
            )
        } else if outcome.layout_warning.is_some() {
            format!(
                "saved {} without canvas layout",
                crate::ui_kit::file_label(path)
            )
        } else {
            format!("saved {}", crate::ui_kit::file_label(path))
        }
    }

    pub(super) fn apply_save_outcome(&mut self, path: &Path, outcome: DocumentSaveOutcome) {
        self.status = Self::saved_status(path, &outcome);
        let fingerprint = project_fingerprint(&self.project);
        let mut diagnostics = outcome
            .validation_issues
            .into_iter()
            .map(|issue| {
                Diagnostic::validation_with_fingerprint(&self.project, issue, &fingerprint)
            })
            .collect::<Vec<_>>();
        diagnostics.extend(outcome.layout_warning.map(Diagnostic::warning));
        if diagnostics.is_empty() {
            self.diagnostics.clear();
        } else {
            self.diagnostics.replace("Save", diagnostics);
        }
    }
}

/// Rebases one snapshot in place. A failure can leave it partially rebased;
/// callers then discard the history that holds it.
fn rebase_snapshot(snapshot: &mut EditorSnapshot, from: &Path, to: &Path) -> std::io::Result<()> {
    let before = static_paths(&snapshot.project);
    cli::rebase_project_paths(&mut snapshot.project, from, to)?;
    if static_paths(&snapshot.project) != before {
        snapshot.state.serialized_project =
            crate::project_state::project_snapshot_key(&snapshot.project);
        snapshot.state.layout.project_fingerprint = Some(project_fingerprint(&snapshot.project));
    }
    Ok(())
}

/// The document-relative paths [`cli::rebase_project_paths`] may rewrite,
/// retained so unchanged snapshots skip re-serialization.
fn static_paths(project: &mapping::Project) -> Vec<Option<String>> {
    [project.source_path.clone(), project.target_path.clone()]
        .into_iter()
        .chain(
            project
                .extra_sources
                .iter()
                .map(|source| Some(source.path.clone())),
        )
        .chain(
            project
                .extra_targets
                .iter()
                .map(|target| target.path.clone()),
        )
        .collect()
}

#[cfg(test)]
#[path = "document_save_tests.rs"]
mod tests;
