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
fn json5_instance_path_is_visible_in_best_effort_and_rejected_by_executable_profile()
-> Result<(), Box<dyn Error>> {
    let directory =
        std::env::temp_dir().join(format!("ferrule_mfd_json5_profile_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory)?;
    for name in ["inventory.schema.json", "inventory-target.xsd"] {
        std::fs::copy(fixture(name), directory.join(name))?;
    }
    let design = directory.join("inventory.mfd");
    let xml = std::fs::read_to_string(fixture("inventory.mfd"))?.replace(
        "inputinstance=\"inventory.json\"",
        "inputinstance=\"inventory.JSON5\"",
    );
    std::fs::write(&design, xml)?;

    let best_effort = mfd::import_with_profile(
        &design,
        &ImportOptions::default(),
        ImportProfile::BestEffort,
    )?;
    assert!(
        best_effort
            .imported
            .warnings
            .iter()
            .any(|warning| warning.contains("JSON5 setting is unverified")),
        "{:?}",
        best_effort.imported.warnings
    );
    assert!(!best_effort.imported.project.source_options.json5);

    let strict = mfd::import_with_profile(
        &design,
        &ImportOptions::default(),
        ImportProfile::Executable,
    );
    let Err(MfdError::IncompatibleImport(report)) = strict else {
        panic!("executable import should reject an unverified JSON5 boundary");
    };
    assert!(report.issues.iter().any(|issue| {
        issue.kind == ImportIssueKind::ImportWarning
            && issue.message.contains("JSON5 setting is unverified")
    }));
    std::fs::remove_dir_all(directory)?;
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
