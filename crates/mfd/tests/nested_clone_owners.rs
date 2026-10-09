use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    design: String,
    input_xml: String,
    expected_json: serde_json::Value,
}

struct Evidence(PathBuf);

impl Evidence {
    fn new(label: &str) -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let parent = std::env::var_os("FERRULE_NESTED_CLONES_EVIDENCE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(std::env::temp_dir);
        let path = parent.join(format!(
            "ferrule_nested_clones239_{label}_{}_{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path)?;
        eprintln!("NESTED_CLONES239_ORIGINALS={}", path.display());
        for name in [
            "source.xsd",
            "target.xsd",
            "source-deep.xsd",
            "target-deep.xsd",
        ] {
            std::fs::copy(fixtures().join(name), path.join(name))?;
        }
        Ok(Self(path))
    }

    fn record(&self, name: &str, value: impl std::fmt::Debug) -> Result<(), std::io::Error> {
        std::fs::write(self.0.join(name), format!("{value:#?}"))
    }

    fn import(&self, design: &str) -> Result<mfd::Imported, Box<dyn Error>> {
        std::fs::copy(fixtures().join(design), self.0.join("mapping.mfd"))?;
        let observed = mfd::import(&self.0.join("mapping.mfd"));
        let snapshot = match &observed {
            Ok(imported) => {
                serde_json::json!({"project": imported.project, "warnings": imported.warnings})
            }
            Err(error) => serde_json::json!({"error": error.to_string()}),
        };
        std::fs::write(
            self.0.join("import.original.json"),
            serde_json::to_vec_pretty(&snapshot)?,
        )?;
        Ok(observed?)
    }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nested_clone_owners")
}

#[test]
fn declared_nested_clone_owners_preserve_complete_ordered_values() -> Result<(), Box<dyn Error>> {
    let cases: Vec<Case> =
        serde_json::from_str(include_str!("fixtures/nested_clone_owners/cases.json"))?;
    assert_eq!(cases.len(), 8, "frozen full-value corpus");
    for case in cases {
        let evidence = Evidence::new(&case.name)?;
        std::fs::write(evidence.0.join("input.original.xml"), &case.input_xml)?;
        std::fs::write(
            evidence.0.join("expected-before-run.json"),
            serde_json::to_vec_pretty(&case.expected_json)?,
        )?;
        let imported = evidence.import(&case.design)?;
        let validation = engine::validate(&imported.project);
        evidence.record("validation.original.txt", &validation)?;
        assert!(
            imported.warnings.is_empty(),
            "{}: {:?}",
            case.name,
            imported.warnings
        );
        assert!(validation.is_empty(), "{}: {validation:?}", case.name);
        let source = format_xml::from_str(&case.input_xml, &imported.project.source)?;
        let expected =
            format_json::from_str(&case.expected_json.to_string(), &imported.project.target)?;
        let observed = engine::run(&imported.project, &source);
        evidence.record("output.original.txt", &observed)?;
        if let Ok(value) = &observed {
            std::fs::write(
                evidence.0.join("output.original.xml"),
                format_xml::to_string(&imported.project.target, value)?,
            )?;
        }
        assert_eq!(observed?, expected, "{}", case.name);
    }
    Ok(())
}

#[test]
fn ambiguous_owners_and_distinct_singular_feeds_remain_refusals() -> Result<(), Box<dyn Error>> {
    for (design, expected) in [
        (
            "same_source_owners.mfd",
            vec![
                (
                    "scope `Group`",
                    "target field `Side` is bound more than once",
                ),
                (
                    "scope `Group/Row`",
                    "target field `Value` is bound more than once",
                ),
                (
                    "scope `Group/Row`",
                    "target field `Value` is bound more than once",
                ),
                (
                    "scope `Group/Row`",
                    "target field `Value` is bound more than once",
                ),
            ],
        ),
        (
            "distinct_scalar_feeds.mfd",
            vec![(
                "scope `Group/<segment 1>`",
                "target field `Side` is bound more than once",
            )],
        ),
    ] {
        let evidence = Evidence::new(design)?;
        std::fs::copy(
            fixtures().join("refusals.json"),
            evidence.0.join("expected-before-run.json"),
        )?;
        let imported = evidence.import(design)?;
        let validation = engine::validate(&imported.project);
        evidence.record("validation.original.txt", &validation)?;
        let strict = mfd::import_with_profile(
            &evidence.0.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        );
        evidence.record(
            "strict.original.txt",
            strict.as_ref().map(|outcome| &outcome.report),
        )?;
        assert_eq!(
            validation
                .iter()
                .map(|issue| (issue.location.as_str(), issue.message.as_str()))
                .collect::<Vec<_>>(),
            expected,
            "{design}"
        );
        assert!(
            matches!(strict, Err(mfd::MfdError::IncompatibleImport(_))),
            "{design}"
        );
    }
    let evidence = Evidence::new("ownerless")?;
    let original = include_str!("fixtures/nested_clone_owners/ambiguous-ownerless.json");
    std::fs::write(evidence.0.join("project.original.json"), original)?;
    std::fs::copy(
        fixtures().join("refusals.json"),
        evidence.0.join("expected-before-run.json"),
    )?;
    let project: mapping::Project = serde_json::from_str(original)?;
    let validation = engine::validate(&project);
    evidence.record("validation.original.txt", &validation)?;
    assert_eq!(
        validation
            .iter()
            .map(|issue| (issue.location.as_str(), issue.message.as_str()))
            .collect::<Vec<_>>(),
        [(
            "scope `Group`",
            "concatenated scope wrapper cannot contain construction, controls, bindings, or child content"
        )]
    );
    Ok(())
}
