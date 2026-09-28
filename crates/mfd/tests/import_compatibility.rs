use std::error::Error;
use std::path::{Path, PathBuf};

use mfd::{ImportIssueKind, ImportOptions, ImportProfile, MfdError};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn strict_profile_accepts_a_complete_engine_valid_import() -> Result<(), Box<dyn Error>> {
    let outcome = mfd::import_with_profile(
        &fixture("people.mfd"),
        &ImportOptions::default(),
        ImportProfile::Executable,
    )?;
    assert!(outcome.report.executable);
    assert!(outcome.report.issues.is_empty());
    assert!(outcome.imported.warnings.is_empty());
    Ok(())
}

#[test]
fn best_effort_retains_partial_design_while_strict_reports_its_warning()
-> Result<(), Box<dyn Error>> {
    let path = fixture("edi-unsupported.mfd");
    let outcome =
        mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::BestEffort)?;
    assert!(!outcome.report.executable);
    assert!(!outcome.imported.warnings.is_empty());
    assert!(
        outcome
            .report
            .issues
            .iter()
            .any(|issue| issue.kind == ImportIssueKind::ImportWarning)
    );

    let error =
        match mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable)
        {
            Err(MfdError::IncompatibleImport(report)) => report,
            Err(other) => return Err(format!("wrong error: {other}").into()),
            Ok(_) => return Err("strict import unexpectedly accepted a partial design".into()),
        };
    assert!(!error.executable);
    assert!(
        error
            .issues
            .iter()
            .any(|issue| issue.kind == ImportIssueKind::ImportWarning)
    );
    Ok(())
}

#[test]
fn assessment_catches_dependencies_and_invalid_project_after_warning_free_reimport()
-> Result<(), Box<dyn Error>> {
    let mut imported = mfd::import(&fixture("people.mfd"))?;
    assert!(imported.warnings.is_empty());
    imported.project.source_options.edi_config_reference = Some("missing/850.Config".into());
    imported.project.graph.nodes.clear();
    let report = mfd::assess_import(&imported);
    assert!(!report.executable);
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.kind == ImportIssueKind::RuntimeDependency)
    );
    assert!(
        report
            .issues
            .iter()
            .any(|issue| issue.kind == ImportIssueKind::Validation)
    );
    Ok(())
}
