//! Duplicated target entries fed by one interned expression keep one binding
//! per authored entry: repeating scalars emit every entry's value in
//! declaration order, and singular duplicates refuse like distinct feeds.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use mapping::{NodeId, Scope};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    design: String,
    input_xml: String,
    expected_json: serde_json::Value,
}

fn fixtures(directory: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(directory)
}

/// Copies one authored design and its schemas into a fresh package directory.
fn package(directory: &str, schemas: &[&str], design: &str) -> Result<PathBuf, Box<dyn Error>> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let package = std::env::temp_dir().join(format!(
        "ferrule_duplicate_binding_occurrences_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&package)?;
    for name in schemas {
        std::fs::copy(fixtures(directory).join(name), package.join(name))?;
    }
    std::fs::copy(
        fixtures(directory).join(design),
        package.join("mapping.mfd"),
    )?;
    Ok(package)
}

fn repeating_package(design: &str) -> Result<PathBuf, Box<dyn Error>> {
    package(
        "nested_clone_owners",
        &["source.xsd", "target-lines.xsd"],
        design,
    )
}

fn bound_nodes(scope: &Scope, field: &str) -> Vec<NodeId> {
    scope
        .bindings
        .iter()
        .filter(|binding| binding.target_field == field)
        .map(|binding| binding.node)
        .collect()
}

#[test]
fn repeating_scalar_entries_sharing_one_expression_emit_every_value_in_order()
-> Result<(), Box<dyn Error>> {
    let cases: Vec<Case> = serde_json::from_str(include_str!(
        "fixtures/nested_clone_owners/binding_occurrence_cases.json"
    ))?;
    assert_eq!(
        cases
            .iter()
            .map(|case| case.name.as_str())
            .collect::<Vec<_>>(),
        ["shared_repeating_values"]
    );
    for case in cases {
        let directory = repeating_package(&case.design)?;
        let mapping = directory.join("mapping.mfd");
        let imported = mfd::import(&mapping)?;
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        let validation = engine::validate(&imported.project);
        assert!(validation.is_empty(), "{validation:?}");

        // Lines: shared, between, shared. Tags: concat, value, concat.
        let lines = bound_nodes(&imported.project.root, "Line");
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], lines[2]);
        assert_ne!(lines[0], lines[1]);
        let row = imported
            .project
            .root
            .children
            .iter()
            .find(|child| child.target_field == "Row")
            .ok_or("Row scope")?;
        let tags = bound_nodes(row, "Tag");
        assert_eq!(tags.len(), 3);
        assert_eq!(tags[0], tags[2]);
        assert_ne!(tags[0], tags[1]);

        let strict = mfd::import_with_profile(
            &mapping,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        )?;
        assert_eq!(
            strict.report,
            mfd::ImportReport {
                executable: true,
                issues: Vec::new()
            }
        );
        assert_eq!(
            serde_json::to_value(&strict.imported.project)?,
            serde_json::to_value(&imported.project)?
        );
        let source = format_xml::from_str(&case.input_xml, &imported.project.source)?;
        let expected =
            format_json::from_str(&case.expected_json.to_string(), &imported.project.target)?;
        assert_eq!(engine::run(&imported.project, &source)?, expected);

        // Export writes one entry per binding; re-import keeps each of them.
        let exported = directory.join("exported").join("mapping.mfd");
        std::fs::create_dir_all(directory.join("exported"))?;
        assert!(mfd::export(&imported.project, &exported)?.is_empty());
        let reimported = mfd::import(&exported)?;
        assert!(reimported.warnings.is_empty(), "{:?}", reimported.warnings);
        assert_eq!(engine::run(&reimported.project, &source)?, expected);
        std::fs::remove_dir_all(directory)?;
    }
    Ok(())
}

#[test]
fn one_authored_entry_with_repeated_edges_remains_one_binding() -> Result<(), Box<dyn Error>> {
    let directory = repeating_package("shared_repeating_values.mfd")?;
    let mapping = directory.join("mapping.mfd");
    let original = mfd::import(&mapping)?;
    let design = std::fs::read_to_string(&mapping)?;
    let repeated = design.replace(
        "<edge from=\"80\" to=\"20\"/>",
        "<edge from=\"80\" to=\"20\"/><edge from=\"80\" to=\"20\"/>",
    );
    assert_ne!(repeated, design);
    std::fs::write(&mapping, repeated)?;
    let imported = mfd::import(&mapping)?;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(bound_nodes(&imported.project.root, "Line").len(), 3);
    assert_eq!(
        serde_json::to_value(&imported.project)?,
        serde_json::to_value(&original.project)?
    );
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn identical_singular_feeds_refuse_like_distinct_singular_feeds() -> Result<(), Box<dyn Error>> {
    let schemas = ["source.xsd", "target.xsd"];
    let mut outcomes = Vec::new();
    // `competing-singular.mfd` is the unchanged #241 distinct-constant control.
    for design in ["shared-singular.mfd", "competing-singular.mfd"] {
        let directory = package("input_guard", &schemas, design)?;
        let mapping = directory.join("mapping.mfd");
        let imported = mfd::import(&mapping)?;
        assert!(imported.warnings.is_empty(), "{design}");
        let validation = engine::validate(&imported.project);
        let Err(mfd::MfdError::IncompatibleImport(report)) = mfd::import_with_profile(
            &mapping,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        ) else {
            return Err(format!("{design}: executable import did not refuse").into());
        };
        outcomes.push((validation, report));
        std::fs::remove_dir_all(directory)?;
    }
    let (shared_validation, shared_report) = &outcomes[0];
    assert_eq!(
        shared_validation
            .iter()
            .map(|issue| issue.message.as_str())
            .collect::<Vec<_>>(),
        ["target field `Value` is bound more than once"]
    );
    assert_eq!(outcomes[1].0, *shared_validation);
    assert_eq!(outcomes[1].1, *shared_report);
    Ok(())
}
