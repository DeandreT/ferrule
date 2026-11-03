//! Optional-array omission, not a new outward same-name collection rule.
use std::path::{Path, PathBuf};

#[path = "offline_composition/hosts.rs"]
mod hosts;

#[path = "offline_composition/literal.rs"]
mod literal;

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/sparse-nested-child")
        .join(name)
}

fn project() -> mapping::Project {
    let imported = mfd::import(&fixture("sparse.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());
    imported.project
}

#[test]
fn absent_repetitions_keep_complete_typed_json_and_compact_xml_oracles() {
    let project = project();
    hosts::ordinary_artifacts(&project);
    let cases: Vec<serde_json::Value> = serde_json::from_str(include_str!(
        "fixtures/sparse-nested-child/absence-composition-cases.json"
    ))
    .unwrap();
    assert_eq!(cases.len(), 6);
    for case in cases.iter().chain(cases.iter().rev()) {
        let source =
            format_json::from_str(case["input"].as_str().unwrap(), &project.source).unwrap();
        let actual = engine::run(&project, &source).unwrap();
        assert_eq!(actual, literal::instance(&case["expected"]), "{case}");
        let json: serde_json::Value =
            serde_json::from_str(&format_json::to_string(&project.target, &actual).unwrap())
                .unwrap();
        assert_eq!(json, case["expected"], "{case}");
        let xml = format_xml::to_string_with_options(
            &project.target,
            &actual,
            &format_xml::XmlWriteOptions {
                declaration: false,
                indent: false,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            xml.as_bytes(),
            case["expected_xml"].as_str().unwrap().as_bytes(),
            "{case}"
        );
    }
}

#[test]
#[ignore = "compiles ordinary generated Rust and C# Release hosts; needs resource clearance and .NET 10"]
fn generated_sparse_absence_release_hosts_match_complete_oracles() {
    hosts::run(
        &project(),
        &fixture("absence-composition-cases.json"),
        "sparse",
        include_str!("offline_composition/sparse.rs.txt"),
        include_str!("offline_composition/sparse.cs.txt"),
    );
}
