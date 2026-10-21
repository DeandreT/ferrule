//! Identical interned bindings in distinct declared clone occurrences keep
//! distinct provenance, while one occurrence whose sibling entries share one
//! expression keeps a binding per entry and refuses like distinct feeds.

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
fn one_occurrence_aliasing_two_edges_refuses_like_distinct_feeds() -> Result<(), Box<dyn Error>> {
    let aliased = package("aliased_cell_values.mfd")?;
    // The control feeds the second sibling Value entry from its own constant.
    let distinct = package("aliased_cell_values.mfd")?;
    let design = std::fs::read_to_string(distinct.join("mapping.mfd"))?;
    let control = design
        .replace(
            "<edge from=\"181\" to=\"307\"/>",
            "<edge from=\"188\" to=\"307\"/>",
        )
        .replace(
            "</children>",
            "<component name=\"constant\" library=\"core\" kind=\"2\"><targets><datapoint key=\"188\"/></targets><data><constant value=\"left-one-cell-alt\" datatype=\"string\"/></data></component></children>",
        );
    assert_ne!(control, design);
    std::fs::write(distinct.join("mapping.mfd"), control)?;

    let mut outcomes = Vec::new();
    for directory in [&aliased, &distinct] {
        let mapping = directory.join("mapping.mfd");
        let imported = mfd::import(&mapping)?;
        // Both sibling entries survive as bindings, so clone distribution is
        // proved and only the singular duplicate-binding refusal remains.
        assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        let validation = engine::validate(&imported.project);
        let Err(mfd::MfdError::IncompatibleImport(report)) = mfd::import_with_profile(
            &mapping,
            &mfd::ImportOptions::default(),
            mfd::ImportProfile::Executable,
        ) else {
            return Err("executable import did not refuse".into());
        };
        outcomes.push((validation, report));
    }
    let (aliased_validation, aliased_report) = &outcomes[0];
    assert_eq!(
        aliased_validation
            .iter()
            .map(|issue| (issue.location.as_str(), issue.message.as_str()))
            .collect::<Vec<_>>(),
        [(
            "scope `Group/<segment 1>/Row/<segment 2>/Cell/<segment 1>`",
            "target field `Value` is bound more than once"
        )]
    );
    assert_eq!(outcomes[1].0, *aliased_validation);
    assert_eq!(outcomes[1].1, *aliased_report);
    std::fs::remove_dir_all(aliased)?;
    std::fs::remove_dir_all(distinct)?;
    Ok(())
}
