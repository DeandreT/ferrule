use std::fmt;

use serde::Serialize;

use super::{ImportOptions, Imported, import_with_options};
use crate::MfdError;

/// How much fidelity an `.mfd` import caller requires.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportProfile {
    /// Preserve the existing repair-oriented partial import behavior.
    #[default]
    BestEffort,
    /// Refuse an import with skipped constructs, missing runtime resources,
    /// or an invalid Ferrule project.
    Executable,
}

/// Why a design cannot currently execute as imported.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportIssueKind {
    ImportWarning,
    RuntimeDependency,
    Validation,
}

/// A deterministic finding from import and project validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ImportIssue {
    pub kind: ImportIssueKind,
    pub message: String,
}

/// Static Ferrule execution assessment, not a vendor behavioral certificate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ImportReport {
    pub executable: bool,
    pub issues: Vec<ImportIssue>,
}

impl fmt::Display for ImportReport {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.issues.is_empty() {
            return formatter.write_str("no known import blockers");
        }
        for (index, issue) in self.issues.iter().take(8).enumerate() {
            if index != 0 {
                formatter.write_str("; ")?;
            }
            formatter.write_str(&issue.message)?;
        }
        if self.issues.len() > 8 {
            write!(formatter, "; and {} more", self.issues.len() - 8)?;
        }
        Ok(())
    }
}

/// The project and the assessment derived from one import pass.
pub struct ImportOutcome {
    pub imported: Imported,
    pub report: ImportReport,
}

/// Assess an already imported design without reading source resources again.
pub fn assess_import(imported: &Imported) -> ImportReport {
    let mut issues = Vec::new();
    issues.extend(imported.warnings.iter().map(|message| ImportIssue {
        kind: ImportIssueKind::ImportWarning,
        message: message.clone(),
    }));
    issues.extend(
        imported
            .project
            .runtime_dependencies()
            .into_iter()
            .map(|dependency| ImportIssue {
                kind: ImportIssueKind::RuntimeDependency,
                message: dependency.to_string(),
            }),
    );
    issues.extend(
        engine::validate(&imported.project)
            .into_iter()
            .map(|finding| ImportIssue {
                kind: ImportIssueKind::Validation,
                message: finding.to_string(),
            }),
    );
    ImportReport {
        executable: issues.is_empty(),
        issues,
    }
}

/// Import once and apply a caller-selected fidelity policy. Strict mode
/// rejects before any project file is written by a host.
pub fn import_with_profile(
    path: &std::path::Path,
    options: &ImportOptions,
    profile: ImportProfile,
) -> Result<ImportOutcome, MfdError> {
    let imported = import_with_options(path, options)?;
    let report = assess_import(&imported);
    if profile == ImportProfile::Executable && !report.executable {
        return Err(MfdError::IncompatibleImport(Box::new(report)));
    }
    Ok(ImportOutcome { imported, report })
}
