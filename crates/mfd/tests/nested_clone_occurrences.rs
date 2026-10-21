//! Identical interned bindings in distinct declared clone occurrences keep
//! distinct provenance, while one occurrence that aliases several authored
//! edges remains refused in the established warning order.

use std::error::Error;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use mapping::Scope;
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    design: String,
    input_xml: String,
    expected_json: serde_json::Value,
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/nested_clone_owners")
}

/// Copies one authored design and its schemas into a fresh package directory.
fn package(design: &str) -> Result<PathBuf, Box<dyn Error>> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let directory = std::env::temp_dir().join(format!(
        "ferrule_nested_clone_occurrences_{}_{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory)?;
    for name in [
        "source.xsd",
        "target.xsd",
        "source-deep.xsd",
        "target-deep.xsd",
    ] {
        std::fs::copy(fixtures().join(name), directory.join(name))?;
    }
    std::fs::copy(fixtures().join(design), directory.join("mapping.mfd"))?;
    Ok(directory)
}

fn child<'a>(scope: &'a Scope, field: &str) -> Option<&'a Scope> {
    scope
        .children
        .iter()
        .find(|child| child.target_field == field)
}

fn segment(scope: &Scope, index: usize) -> Option<&Scope> {
    scope.concatenated()?.iter().nth(index)
}

fn bound_node(scope: &Scope, field: &str) -> Option<u32> {
    let mut bindings = scope
        .bindings
        .iter()
        .filter(|binding| binding.target_field == field);
    let node = bindings.next()?.node;
    bindings.next().is_none().then_some(node)
}

#[test]
fn identical_bindings_in_distinct_clone_occurrences_keep_distinct_values()
-> Result<(), Box<dyn Error>> {
    let cases: Vec<Case> = serde_json::from_str(include_str!(
        "fixtures/nested_clone_owners/occurrence_cases.json"
    ))?;
    assert_eq!(
        cases
            .iter()
            .map(|case| case.name.as_str())
            .collect::<Vec<_>>(),
        ["shared_constant_cells", "shared_constant_rows"]
    );
    for case in cases {
        let directory = package(&case.design)?;
        let mapping = directory.join("mapping.mfd");
        let imported = mfd::import(&mapping)?;
        assert!(
            imported.warnings.is_empty(),
            "{}: {:?}",
            case.name,
            imported.warnings
        );
        let validation = engine::validate(&imported.project);
        assert!(validation.is_empty(), "{}: {validation:?}", case.name);
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
            },
            "{}",
            case.name
        );
        assert_eq!(
            serde_json::to_value(&strict.imported.project)?,
            serde_json::to_value(&imported.project)?,
            "{}",
            case.name
        );
        let source = format_xml::from_str(&case.input_xml, &imported.project.source)?;
        let expected =
            format_json::from_str(&case.expected_json.to_string(), &imported.project.target)?;
        assert_eq!(
            engine::run(&imported.project, &source)?,
            expected,
            "{}",
            case.name
        );
        std::fs::remove_dir_all(directory)?;
    }
    Ok(())
}

#[test]
fn nested_sibling_clones_share_one_interned_node_as_separate_segments() -> Result<(), Box<dyn Error>>
{
    let directory = package("shared_constant_cells.mfd")?;
    let imported = mfd::import(&directory.join("mapping.mfd"))?;
    // Group (left) -> first fixed Row clone -> its two declared Cell clones.
    let cells = child(&imported.project.root, "Group")
        .and_then(|group| segment(group, 0))
        .and_then(|left| child(left, "Row"))
        .and_then(|rows| segment(rows, 1))
        .and_then(|row| child(row, "Cell"))
        .ok_or("left fixed Row must own its Cell clones")?;
    let first = segment(cells, 0).and_then(|cell| bound_node(cell, "Value"));
    let second = segment(cells, 1).and_then(|cell| bound_node(cell, "Value"));
    assert_eq!(cells.concatenated().map(|cells| cells.len()), Some(2));
    assert!(first.is_some());
    assert_eq!(first, second, "both occurrences bind one interned constant");
    std::fs::remove_dir_all(directory)?;
    Ok(())
}

#[test]
fn one_occurrence_aliasing_two_edges_keeps_ordered_refusals() -> Result<(), Box<dyn Error>> {
    let directory = package("aliased_cell_values.mfd")?;
    let imported = mfd::import(&directory.join("mapping.mfd"))?;
    // The deeper wrapper refuses first; its retained child then prevents the
    // enclosing Row wrapper from being distributed.
    assert_eq!(
        imported.warnings,
        [
            "nested cloned target `Group/Row/Cell` cannot prove complete placeholder binding preservation; its scope remains unsupported",
            "nested cloned target `Group/Row` has no complete, unique declared ancestor ownership; its scope remains unsupported",
        ]
    );
    assert!(!engine::validate(&imported.project).is_empty());
    assert!(matches!(
        mfd::import_with_profile(
            &directory.join("mapping.mfd"),
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        ),
        Err(mfd::MfdError::IncompatibleImport(_))
    ));
    std::fs::remove_dir_all(directory)?;
    Ok(())
}
