//! Finite supplied-clock contract; no ambient clock or random-value oracle.
use std::path::{Path, PathBuf};

use mapping::{Node, RuntimeValue};

#[path = "offline_composition/hosts.rs"]
mod hosts;

#[path = "offline_composition/literal.rs"]
mod literal;

#[path = "../../engine/tests/grouped_runtime_composition/project.rs"]
mod native_counter;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/grouped-runtime-clock-composition")
        .join(name)
}

fn project(name: &str) -> mapping::Project {
    let imported = mfd::import_with_profile(
        &fixture(name),
        &mfd::ImportOptions::default(),
        mfd::ImportProfile::Executable,
    )
    .unwrap()
    .imported;
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(
        imported
            .project
            .graph
            .nodes
            .values()
            .filter(|node| matches!(
                node,
                Node::RuntimeValue {
                    value: RuntimeValue::CurrentDateTime
                }
            ))
            .count(),
        1
    );
    assert!(
        imported
            .project
            .root
            .children
            .iter()
            .any(|scope| scope.group_ending_with.is_some())
    );
    imported.project
}

#[test]
fn clock_and_native_counter_projects_use_ordinary_emission_without_profile_promotion() {
    for design in ["fanout.mfd", "group-only.mfd"] {
        hosts::ordinary_artifacts(&project(design));
    }
    hosts::ordinary_artifacts(&native_counter::project());
}

#[test]
fn supplied_clock_spans_root_boundary_groups_and_sibling_with_fresh_entry_calls() {
    for (design, catalog) in [
        ("fanout.mfd", "cases.json"),
        ("group-only.mfd", "group-only-cases.json"),
    ] {
        let project = project(design);
        let cases: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(fixture(catalog)).unwrap()).unwrap();
        // C1/C2/C1 on the same Project, then all empty/error controls twice.
        let order: Vec<_> = if design == "fanout.mfd" {
            vec![0, 1, 0, 2, 3, 4, 2, 3, 4]
        } else {
            vec![0, 1, 2, 0, 1, 2]
        };
        for index in order {
            let case = &cases[index];
            let source =
                format_json::from_str(&case["input"].to_string(), &project.source).unwrap();
            let mut context = engine::ExecutionContext::new(Path::new("clock.mfd"));
            if let Some(clock) = case["clock"].as_str() {
                context = context.with_current_datetime(clock);
            }
            let result = engine::run_with_context(&project, &source, &context);
            if case["missing_clock"] == true {
                assert_eq!(
                    result.unwrap_err(),
                    engine::EngineError::MissingRuntimeValue(RuntimeValue::CurrentDateTime),
                    "{design}: {case}"
                );
            } else {
                let actual = result.unwrap();
                assert_eq!(
                    actual,
                    literal::instance(&case["expected"]),
                    "{design}: {case}"
                );
                let json: serde_json::Value = serde_json::from_str(
                    &format_json::to_string(&project.target, &actual).unwrap(),
                )
                .unwrap();
                assert_eq!(json, case["expected"], "{design}: {case}");
            }
        }
    }
}

#[test]
#[ignore = "compiles ordinary generated Rust and C# Release hosts; needs resource clearance and .NET 10"]
fn generated_grouped_clock_release_hosts_match_complete_oracles() {
    for (design, catalog) in [
        ("fanout.mfd", "cases.json"),
        ("group-only.mfd", "group-only-cases.json"),
    ] {
        hosts::run(
            &project(design),
            &fixture(catalog),
            "clock",
            include_str!("offline_composition/clock.rs.txt"),
            include_str!("offline_composition/clock.cs.txt"),
        );
    }
}

#[test]
#[ignore = "compiles native Ferrule grouped-counter Rust and C# Release hosts; NOT MFD auto-number"]
fn generated_native_grouped_counter_release_hosts_match_complete_oracles() {
    hosts::run(
        &native_counter::project(),
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../engine/tests/grouped_runtime_composition/cases.json"),
        "native-counter",
        include_str!("offline_composition/counter.rs.txt"),
        include_str!("offline_composition/counter.cs.txt"),
    );
}
