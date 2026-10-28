//! Missing optional source values do not collapse distinct target occurrences.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    input_xml: String,
    expected_json: serde_json::Value,
}

struct Package(PathBuf);

impl Package {
    fn new(repeating: bool) -> Result<Self, Box<dyn Error>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "ferrule_optional_cloned_feeds_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root)?;
        let package = Self(root);
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/optional_cloned_feeds");
        for name in ["mapping.mfd", "source.xsd"] {
            std::fs::copy(fixtures.join(name), package.0.join(name))?;
        }
        std::fs::copy(
            fixtures.join(if repeating {
                "target-repeated.xsd"
            } else {
                "target-singular.xsd"
            }),
            package.0.join("target.xsd"),
        )?;
        Ok(package)
    }
}

impl Drop for Package {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn cases() -> Result<Vec<Case>, Box<dyn Error>> {
    Ok(serde_json::from_str(include_str!(
        "fixtures/optional_cloned_feeds/cases.json"
    ))?)
}

#[test]
fn optional_repeating_feeds_keep_complete_values_and_empty_results() -> Result<(), Box<dyn Error>> {
    let package = Package::new(true)?;
    let cases = cases()?;
    assert_eq!(
        cases
            .iter()
            .map(|case| case.name.as_str())
            .collect::<Vec<_>>(),
        ["both", "first-only", "second-only", "neither"]
    );
    let imported = mfd::import_with_profile(
        &package.0.join("mapping.mfd"),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )?;
    assert!(imported.imported.warnings.is_empty());
    assert!(imported.report.executable && imported.report.issues.is_empty());
    let project = &imported.imported.project;
    assert_eq!(
        project
            .root
            .bindings
            .iter()
            .filter(|binding| binding.target_field == "Amount")
            .count(),
        2
    );
    let exported = package.0.join("exported/mapping.mfd");
    std::fs::create_dir(exported.parent().unwrap())?;
    assert!(mfd::export(project, &exported)?.is_empty());
    let roundtrip = mfd::import(&exported)?;
    assert!(roundtrip.warnings.is_empty());
    for case in cases {
        let source = format_xml::from_str(&case.input_xml, &project.source)?;
        let expected = format_json::from_str(&case.expected_json.to_string(), &project.target)?;
        assert_eq!(engine::run(project, &source)?, expected, "{}", case.name);
        assert_eq!(
            engine::run(&roundtrip.project, &source)?,
            expected,
            "{}",
            case.name
        );
    }
    Ok(())
}

#[test]
fn optional_singular_feeds_refuse_even_when_only_one_value_is_present() -> Result<(), Box<dyn Error>>
{
    let package = Package::new(false)?;
    for case in cases()? {
        std::fs::write(package.0.join("input.xml"), &case.input_xml)?;
        let imported = mfd::import(&package.0.join("mapping.mfd"))?;
        assert!(imported.warnings.is_empty());
        format_xml::from_str(&case.input_xml, &imported.project.source)?;
        let issues = engine::validate(&imported.project);
        assert_eq!(issues.len(), 1, "{}", case.name);
        assert_eq!(issues[0].location, "root scope");
        assert_eq!(
            issues[0].message,
            "target field `Amount` is bound more than once"
        );
        assert_eq!(
            codegen::lower(&imported.project).unwrap_err().diagnostics(),
            vec![codegen::Diagnostic::Validation {
                location: "root scope".to_owned(),
                message: "target field `Amount` is bound more than once".to_owned(),
            }]
        );
        let Err(mfd::MfdError::IncompatibleImport(report)) = mfd::import_with_profile(
            &package.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        ) else {
            return Err(format!("{}: singular duplicate bindings admitted", case.name).into());
        };
        assert_eq!(
            *report,
            mfd::ImportReport {
                executable: false,
                issues: vec![mfd::ImportIssue {
                    kind: mfd::ImportIssueKind::Validation,
                    message: "root scope: target field `Amount` is bound more than once".to_owned(),
                }],
            }
        );
    }
    Ok(())
}
