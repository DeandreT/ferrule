use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use mfd::{ImportIssueKind, ImportOptions, ImportProfile, MfdError};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    design: String,
    accepted: bool,
    expected_warnings: Vec<String>,
    expected_json: serde_json::Value,
    reference: String,
}

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/input_guard")
}

fn retain(directory: &Path, name: &str, value: impl std::fmt::Debug) -> std::io::Result<()> {
    std::fs::write(directory.join(name), format!("{value:#?}\n"))
}

fn expected_output(value: &serde_json::Value) -> Result<ir::Instance, Box<dyn Error>> {
    let object = value
        .as_object()
        .ok_or("expected output must be an object")?;
    if object.keys().any(|name| name != "Value" && name != "Safe") {
        return Err("unexpected field in the complete output literal".into());
    }
    let mut fields = Vec::new();
    for name in ["Value", "Safe"] {
        let Some(value) = object.get(name) else {
            continue;
        };
        let instance = match value {
            serde_json::Value::String(value) => {
                ir::Instance::Scalar(ir::Value::String(value.clone()))
            }
            serde_json::Value::Array(values) => ir::Instance::Repeated(
                values
                    .iter()
                    .map(|value| {
                        value
                            .as_str()
                            .map(|value| ir::Instance::Scalar(ir::Value::String(value.to_owned())))
                            .ok_or("expected repeated output must contain strings")
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            ),
            _ => return Err("expected output must contain only strings or string arrays".into()),
        };
        fields.push((name.to_owned(), instance));
    }
    Ok(ir::Instance::Group(fields.into()))
}

/// Removes a per-test directory on drop, including after a panic.
struct TempDir(std::path::PathBuf);

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn physical_input_conflicts_never_select_a_feed() -> Result<(), Box<dyn Error>> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let root = std::env::temp_dir().join(format!(
        "ferrule_input_guard335_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&root)?;
    let _cleanup = TempDir(root.clone());
    eprintln!("INPUT_GUARD335_ORIGINALS={}", root.display());
    let cases: Vec<Case> = serde_json::from_str(include_str!("fixtures/input_guard/cases.json"))?;
    assert_eq!(cases.len(), 29);
    for case in cases {
        let directory = root.join(&case.name);
        std::fs::create_dir(&directory)?;
        for name in ["source.xsd", "target.xsd", "target-repeat.xsd"] {
            std::fs::copy(fixtures().join(name), directory.join(name))?;
        }
        std::fs::copy(fixtures().join(&case.design), directory.join("design.mfd"))?;
        std::fs::copy(
            fixtures().join(&case.reference),
            directory.join("reference.mfd"),
        )?;
        std::fs::write(
            directory.join("expected-before-import.json"),
            serde_json::to_vec_pretty(&serde_json::json!({
                "warnings": case.expected_warnings,
                "output": case.expected_json,
                "executable": case.accepted,
            }))?,
        )?;
        let best = mfd::import_with_profile(
            &directory.join("design.mfd"),
            &ImportOptions::default(),
            ImportProfile::BestEffort,
        );
        retain(
            &directory,
            "best.original.txt",
            best.as_ref().map(|outcome| {
                (
                    &outcome.imported.project,
                    &outcome.imported.warnings,
                    &outcome.imported.mapping_path,
                    &outcome.report,
                )
            }),
        )?;
        let strict = mfd::import_with_profile(
            &directory.join("design.mfd"),
            &ImportOptions::default(),
            ImportProfile::Executable,
        );
        retain(
            &directory,
            "strict.original.txt",
            strict.as_ref().map(|outcome| {
                (
                    &outcome.imported.project,
                    &outcome.imported.warnings,
                    &outcome.imported.mapping_path,
                    &outcome.report,
                )
            }),
        )?;
        let reference = mfd::import(&directory.join("reference.mfd"));
        retain(
            &directory,
            "reference.original.txt",
            reference.as_ref().map(|imported| {
                (
                    &imported.project,
                    &imported.warnings,
                    &imported.mapping_path,
                )
            }),
        )?;
        let best = best?;
        let reference = reference?;
        std::fs::write(
            directory.join("project.original.json"),
            serde_json::to_vec_pretty(&best.imported.project)?,
        )?;
        assert_eq!(
            best.imported.warnings, case.expected_warnings,
            "{}",
            case.name
        );
        assert_eq!(best.report.executable, case.accepted, "{}", case.name);
        assert_eq!(
            serde_json::to_value(&best.imported.project)?,
            serde_json::to_value(&reference.project)?,
            "{}: complete repair/identity project",
            case.name
        );
        assert!(reference.warnings.is_empty(), "{}: reference", case.name);
        if case.accepted {
            assert_eq!(
                serde_json::to_value(&strict?.imported.project)?,
                serde_json::to_value(&best.imported.project)?,
                "{}",
                case.name
            );
        } else {
            let Err(MfdError::IncompatibleImport(report)) = strict else {
                return Err(format!("{}: executable import did not refuse", case.name).into());
            };
            assert_eq!(*report, best.report, "{}", case.name);
            assert_eq!(
                report
                    .issues
                    .iter()
                    .map(|issue| issue.kind)
                    .collect::<Vec<_>>(),
                vec![ImportIssueKind::ImportWarning],
                "{}",
                case.name
            );
        }
        let source = format_xml::from_str(
            "<Seed><Token>kept</Token></Seed>",
            &best.imported.project.source,
        )?;
        let expected = expected_output(&case.expected_json)?;
        retain(&directory, "output.expected.txt", &expected)?;
        let actual = engine::run(&best.imported.project, &source);
        retain(&directory, "output.original.txt", &actual)?;
        let actual = actual?;
        assert_eq!(actual, expected, "{}: complete typed output", case.name);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&format_json::to_string(
                &best.imported.project.target,
                &actual,
            )?)?,
            case.expected_json,
            "{}: complete serialized output",
            case.name
        );
    }
    Ok(())
}

#[test]
fn distinct_ports_competing_for_a_singular_target_remain_refused() -> Result<(), Box<dyn Error>> {
    let path = fixtures().join("competing-singular.mfd");
    let imported = mfd::import(&path)?;
    assert!(imported.warnings.is_empty());
    let issues = engine::validate(&imported.project);
    assert_eq!(issues.len(), 1);
    assert_eq!(
        issues[0].message,
        "target field `Value` is bound more than once"
    );
    assert!(matches!(
        mfd::import_with_profile(&path, &ImportOptions::default(), ImportProfile::Executable),
        Err(MfdError::IncompatibleImport(_))
    ));
    Ok(())
}
