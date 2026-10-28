//! XML nil is a retained occurrence, not an absent optional source value.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{Instance, Value};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    input_xml: String,
    expected_typed: serde_json::Value,
}

struct Package(PathBuf);
impl Package {
    fn new(repeating: bool) -> Result<Self, Box<dyn Error>> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule_nil_cloned_feeds_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path)?;
        let package = Self(path);
        let fixtures =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/optional_cloned_feeds");
        std::fs::copy(fixtures.join("mapping.mfd"), package.0.join("mapping.mfd"))?;
        std::fs::copy(
            fixtures.join("source-nil.xsd"),
            package.0.join("source.xsd"),
        )?;
        std::fs::copy(
            fixtures.join(if repeating {
                "target-nil-repeated.xsd"
            } else {
                "target-nil-singular.xsd"
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

fn cases() -> Vec<Case> {
    serde_json::from_str(include_str!(
        "fixtures/optional_cloned_feeds/nil_cases.json"
    ))
    .unwrap()
}

/// `null` is fixture notation for XML nil, never a JSON-boundary input.
fn typed(value: &serde_json::Value) -> Instance {
    match value {
        serde_json::Value::Object(fields) => Instance::Group(
            fields
                .iter()
                .map(|(name, value)| (name.clone(), typed(value)))
                .collect::<Vec<_>>()
                .into(),
        ),
        serde_json::Value::Array(values) => Instance::Repeated(values.iter().map(typed).collect()),
        serde_json::Value::Null => Instance::Scalar(Value::xml_nil()),
        serde_json::Value::Number(value) => Instance::Scalar(Value::Int(value.as_i64().unwrap())),
        serde_json::Value::String(value) => Instance::Scalar(Value::String(value.clone())),
        _ => panic!("unexpected typed fixture value"),
    }
}

#[test]
fn explicit_xml_nil_occurrences_survive_repeating_clones_and_roundtrip()
-> Result<(), Box<dyn Error>> {
    let package = Package::new(true)?;
    let imported = mfd::import_with_profile(
        &package.0.join("mapping.mfd"),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )?;
    assert!(imported.imported.warnings.is_empty());
    assert!(imported.report.executable && imported.report.issues.is_empty());
    let project = &imported.imported.project;
    assert!(project.source.child("First").unwrap().nillable);
    assert!(project.target.child("Amount").unwrap().nillable);
    let export = package.0.join("exported/mapping.mfd");
    std::fs::create_dir(export.parent().unwrap())?;
    assert!(mfd::export(project, &export)?.is_empty());
    let roundtrip = mfd::import(&export)?;
    assert!(roundtrip.warnings.is_empty());
    assert!(roundtrip.project.target.child("Amount").unwrap().nillable);
    let cases = cases();
    assert_eq!(
        cases
            .iter()
            .map(|case| case.name.as_str())
            .collect::<Vec<_>>(),
        [
            "both-values",
            "nil-first",
            "nil-second",
            "both-nil",
            "nil-and-absent"
        ]
    );
    for case in cases {
        let source = format_xml::from_str(&case.input_xml, &project.source)?;
        let expected = typed(&case.expected_typed);
        for project in [project, &roundtrip.project] {
            let actual = engine::run(project, &source)?;
            assert_eq!(actual, expected, "{}", case.name);
            let xml = format_xml::to_string(&project.target, &actual)?;
            assert_eq!(
                format_xml::from_str(&xml, &project.target)?,
                expected,
                "{}",
                case.name
            );
        }
    }
    Ok(())
}

#[test]
fn xml_nil_does_not_exempt_singular_duplicate_bindings() -> Result<(), Box<dyn Error>> {
    let package = Package::new(false)?;
    for case in cases() {
        std::fs::write(package.0.join("input.xml"), &case.input_xml)?;
        let Err(mfd::MfdError::IncompatibleImport(report)) = mfd::import_with_profile(
            &package.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        ) else {
            return Err(format!("{}: singular nil bindings admitted", case.name).into());
        };
        assert_eq!(
            *report,
            mfd::ImportReport {
                executable: false,
                issues: vec![mfd::ImportIssue {
                    kind: mfd::ImportIssueKind::Validation,
                    message: "root scope: target field `Amount` is bound more than once".to_owned(),
                }]
            }
        );
    }
    Ok(())
}
