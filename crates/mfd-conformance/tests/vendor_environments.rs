use std::process::Command;

use mfd_conformance::{
    BUNDLED_LEDGER, DIMENSIONS, Error, GateFailureReason, SCHEMA_VERSION, Selection, Status, gate,
    parse, summarize,
};
use serde_json::{Value, json};

const PROPOSAL: &str = include_str!("../../../conformance/json-object-schemas.proposal.json");
// Archival preimages are intentionally version 1, not current-schema ledgers.
const LEGACY_CANONICAL: &str = include_str!("fixtures/mfd-2026r2-before-backend-environments.json");
const LEGACY_PROPOSAL: &str =
    include_str!("fixtures/json-objects-before-backend-environments.json");
const NEW_BACKENDS: [&str; 2] = ["xslt3", "built_in"];
const EXPECTED_DIMENSIONS: [&str; 16] = [
    "import",
    "interpreter",
    "native_export",
    "ferrule_roundtrip",
    "gui",
    "debug",
    "rust",
    "csharp",
    "vendor_backends.xslt1",
    "vendor_backends.xslt2",
    "vendor_backends.xslt3",
    "vendor_backends.xquery1",
    "vendor_backends.cpp",
    "vendor_backends.java",
    "vendor_backends.csharp",
    "vendor_backends.built_in",
];

fn document(input: &str) -> Value {
    serde_json::from_str(input).expect("ledger JSON")
}

fn support<'a>(document: &'a mut Value, id: &str) -> &'a mut Value {
    &mut document["capabilities"]
        .as_array_mut()
        .expect("capabilities")
        .iter_mut()
        .find(|capability| capability["id"] == id)
        .expect("capability")["support"]
}

fn selection(profile: &str, id: &str, backend: &str) -> Selection {
    Selection {
        profiles: vec![profile.into()],
        capabilities: vec![id.into()],
        dimensions: vec![format!("vendor_backends.{backend}")],
    }
}

// The v2 migration itself preserved the historical cells. This later, reviewed
// XML qualification changes exactly one cell and adds its execution evidence.
// Check both completely before comparing the remaining archival document.
fn check_later_xml_qualification(current: &mut Value, previous: &Value) {
    assert_eq!(
        support(current, "formats.xml")["native_export"],
        json!({
            "status": "partial",
            "evidence": ["survey-vendor-harness", "xml-schema-reference"],
            "detail": "A finite unqualified Envelope/integer/string physical-target export was opened, executed, saved and reopened in 2026r2 under Wine; explicit AddSchemaLocation=0 preserves the no-hint output. The original control uses xs:long; Ferrule exports xs:integer, so bounded-long schema preservation is not qualified. General XML export, namespace/derivation profiles, other backends and publication parity remain unqualified."
        })
    );
    let historical = previous["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|capability| capability["id"] == "formats.xml")
        .unwrap();
    support(current, "formats.xml")["native_export"] =
        historical["support"]["native_export"].clone();
    let evidence = current["evidence"].as_array_mut().unwrap();
    let index = evidence
        .iter()
        .position(|entry| entry["id"] == "xml-schema-reference")
        .unwrap();
    assert_eq!(
        evidence.remove(index),
        json!({
            "id": "xml-schema-reference",
            "kind": "vendor_execution",
            "location": "docs/conformance/xml-schema-reference.md",
            "description": "Finite 2026r2 Wine observation of physical XML schema-reference controls, complete outputs and an actual Ferrule export save/reopen."
        })
    );
}

#[test]
fn schema_v2_preserves_every_legacy_row_and_assessment() {
    for (input, legacy) in [
        (BUNDLED_LEDGER, LEGACY_CANONICAL),
        (PROPOSAL, LEGACY_PROPOSAL),
    ] {
        let mut current = document(input);
        let previous = document(legacy);
        assert_eq!(current["schema_version"], 2);
        assert_eq!(previous["schema_version"], 1);
        assert_eq!(current["inventory_complete"], false);
        current["schema_version"] = json!(1);
        if legacy == LEGACY_PROPOSAL {
            assert_eq!(
                current["description"],
                previous["description"]
                    .as_str()
                    .expect("description")
                    .replace(
                        "All 224 dimension cells remain",
                        "Every dimension cell remains"
                    )
            );
            current["description"] = previous["description"].clone();
        }
        if legacy == LEGACY_CANONICAL {
            check_later_xml_qualification(&mut current, &previous);
            // Inventory additions do not change the archived capability prefix.
            // Other tests validate every current row and all required dimensions.
            let historical_count = previous["capabilities"]
                .as_array()
                .expect("historical capabilities")
                .len();
            current["capabilities"]
                .as_array_mut()
                .expect("current capabilities")
                .truncate(historical_count);
        }
        for capability in current["capabilities"]
            .as_array_mut()
            .expect("capabilities")
        {
            let backends = capability["support"]["vendor_backends"]
                .as_object_mut()
                .expect("vendor backends");
            for backend in NEW_BACKENDS {
                let cell = backends.remove(backend).expect("mandatory new assessment");
                assert_eq!(cell["status"], "unassessed");
                assert_eq!(cell["evidence"], json!([]));
                let detail = cell["detail"].as_str().expect("detail");
                for required in ["unknown", "2026r2", "operation/backend", "vendor execution"] {
                    assert!(detail.contains(required), "{backend}: {detail}");
                }
            }
        }
        // Compare the entire historical document, not only counts or statuses.
        // This also protects row order, scope, profile, dependencies and evidence.
        assert_eq!(current, previous);
    }
}

#[test]
fn version_and_field_migration_are_explicit_not_defaulted() {
    assert_eq!(SCHEMA_VERSION, 2);
    for legacy in [LEGACY_CANONICAL, LEGACY_PROPOSAL] {
        let error = parse(legacy).expect_err("legacy fields are not silently synthesized");
        assert!(matches!(&error, Error::Json(_)));
        assert!(error.to_string().contains("missing field `xslt3`"));
        let mut only_version = document(legacy);
        only_version["schema_version"] = json!(2);
        assert!(parse(&only_version.to_string()).is_err());
    }
    for input in [BUNDLED_LEDGER, PROPOSAL] {
        let mut legacy_version = document(input);
        legacy_version["schema_version"] = json!(1);
        let error = parse(&legacy_version.to_string()).expect_err("populated version 1 rejects");
        assert!(matches!(&error, Error::Invalid(_)));
        assert!(error.to_string().contains("schema_version must be 2"));
        for backend in NEW_BACKENDS {
            let mut incomplete = document(input);
            incomplete["capabilities"][0]["support"]["vendor_backends"]
                .as_object_mut()
                .unwrap()
                .remove(backend);
            let error = parse(&incomplete.to_string()).expect_err("new fields are mandatory");
            assert!(matches!(&error, Error::Json(_)));
            assert!(
                error
                    .to_string()
                    .contains(&format!("missing field `{backend}`"))
            );
        }
    }
}

#[test]
fn every_assessment_and_summary_contains_all_sixteen_dimensions() {
    assert_eq!(DIMENSIONS, EXPECTED_DIMENSIONS);
    for input in [BUNDLED_LEDGER, PROPOSAL] {
        let ledger = parse(input).expect("version 2 ledger");
        assert_eq!(serde_json::to_value(&ledger).unwrap(), document(input));
        for capability in &ledger.capabilities {
            assert_eq!(
                capability
                    .support
                    .assessments()
                    .map(|(dimension, _)| dimension),
                EXPECTED_DIMENSIONS
            );
        }
        let summary = summarize(&ledger, &Selection::default()).unwrap();
        assert_eq!(summary.schema_version, 2);
        assert_eq!(summary.selected_cells, ledger.capabilities.len() * 16);
        assert_eq!(
            summary.by_profile_dimension.len(),
            ledger.profiles.len() * 16
        );
        for row in &summary.by_profile_dimension {
            if ["vendor_backends.xslt3", "vendor_backends.built_in"]
                .contains(&row.dimension.as_str())
            {
                assert_eq!(row.statuses.len(), 1);
                assert_eq!(row.statuses[&Status::Unassessed], row.capabilities);
                assert_eq!(row.evidenced_not_applicable, 0);
                assert_eq!(row.unevidenced_not_applicable, 0);
            }
        }
    }
}

#[test]
fn ferrule_interpreter_support_does_not_qualify_either_vendor_environment() {
    for (profile, id) in [
        ("mfd-2026r2-enterprise", "interop.mfd-designs"),
        (
            "ferrule-native-extensions",
            "extensions.recursive-construction",
        ),
    ] {
        let mut document = document(BUNDLED_LEDGER);
        support(&mut document, id)["interpreter"] = json!({
            "status": "supported", "evidence": ["interop-docs"],
            "detail": "Synthetic interpreter gate control only; not behavioral qualification."
        });
        let ledger = parse(&document.to_string()).unwrap();
        let interpreter = Selection {
            profiles: vec![profile.into()],
            capabilities: vec![id.into()],
            dimensions: vec!["interpreter".into()],
        };
        assert!(gate(&ledger, &interpreter).unwrap().passed);
        for backend in NEW_BACKENDS {
            let report = gate(&ledger, &selection(profile, id, backend)).unwrap();
            assert!(!report.passed);
            assert_eq!(report.summary.selected_cells, 1);
            assert_eq!(report.in_scope_cells, 1);
            assert_eq!(report.excluded_not_applicable_cells, 0);
            assert_eq!(report.failures.len(), 1);
            assert_eq!(report.failures[0].status, Some(Status::Unassessed));
            assert_eq!(
                report.failures[0].reason,
                GateFailureReason::StatusNotSupported
            );
            assert_eq!(
                report.failures[0].dimension.as_deref(),
                Some(format!("vendor_backends.{backend}").as_str())
            );
        }
    }
}

#[test]
fn each_new_environment_requires_its_own_linked_vendor_execution_evidence() {
    // All records and claimed support below are in-memory validator controls.
    // No vendor is executed and no execution receipt is written or adopted.
    for (profile, id) in [
        ("mfd-2026r2-enterprise", "interop.mfd-designs"),
        (
            "ferrule-native-extensions",
            "extensions.recursive-construction",
        ),
    ] {
        for backend in NEW_BACKENDS {
            for status in ["supported", "partial"] {
                let mut document = document(BUNDLED_LEDGER);
                for evidence in ["interop-docs", "vendor-codegen"] {
                    support(&mut document, id)["vendor_backends"][backend] = json!({
                        "status": status, "evidence": [evidence],
                        "detail": "Synthetic prohibited evidence promotion control."
                    });
                    let error = parse(&document.to_string()).expect_err("not vendor execution");
                    assert!(error.to_string().contains(&format!(
                        "vendor_backends.{backend} requires vendor_execution evidence"
                    )));
                }
                let receipt_id = format!("synthetic-{backend}");
                document["evidence"].as_array_mut().unwrap().push(json!({
                    "id": receipt_id,
                    "kind": "vendor_execution",
                    "location": "crates/mfd-conformance/tests/vendor_environments.rs",
                    "description": "In-memory parser/gate control; not an actual vendor run."
                }));
                support(&mut document, id)["vendor_backends"][backend]["evidence"] =
                    json!([receipt_id]);
                let ledger = parse(&document.to_string()).expect("synthetic vendor reference");
                let report = gate(&ledger, &selection(profile, id, backend)).unwrap();
                assert_eq!(report.passed, status == "supported");
                assert_eq!(report.in_scope_cells, 1);
                let other = if backend == "xslt3" {
                    "built_in"
                } else {
                    "xslt3"
                };
                let independent = gate(&ledger, &selection(profile, id, other)).unwrap();
                assert!(!independent.passed);
                assert_eq!(independent.failures[0].status, Some(Status::Unassessed));

                support(&mut document, id)["vendor_backends"][other] = json!({
                    "status": "supported", "evidence": ["interop-docs"],
                    "detail": "Another cell's linked vendor reference is not inherited."
                });
                let error = parse(&document.to_string()).expect_err("no global proof inheritance");
                assert!(error.to_string().contains(&format!(
                    "vendor_backends.{other} requires vendor_execution evidence"
                )));
            }
        }
    }
}

#[test]
fn new_environments_preserve_profile_specific_exclusion_rules() {
    for backend in NEW_BACKENDS {
        for (profile, id, repository_exclusion) in [
            ("mfd-2026r2-enterprise", "interop.mfd-designs", false),
            (
                "ferrule-native-extensions",
                "extensions.recursive-construction",
                true,
            ),
        ] {
            for evidence in ["interop-docs", "vendor-manual"] {
                let mut document = document(BUNDLED_LEDGER);
                support(&mut document, id)["vendor_backends"][backend] = json!({
                    "status": "not_applicable", "evidence": [evidence],
                    "detail": "Synthetic evidence classification control, not a real exclusion."
                });
                let ledger = parse(&document.to_string()).unwrap();
                let report = gate(&ledger, &selection(profile, id, backend)).unwrap();
                let excluded = repository_exclusion || evidence == "vendor-manual";
                assert!(!report.passed);
                assert_eq!(report.in_scope_cells, 0);
                assert_eq!(report.excluded_not_applicable_cells, usize::from(excluded));
                assert_eq!(
                    report.summary.by_profile_dimension[0].evidenced_not_applicable,
                    usize::from(excluded)
                );
                assert_eq!(
                    report.summary.by_profile_dimension[0].unevidenced_not_applicable,
                    usize::from(!excluded)
                );
                assert!(
                    report
                        .failures
                        .iter()
                        .any(|failure| { failure.reason == GateFailureReason::NoInScopeCells })
                );
                assert_eq!(
                    report.failures.iter().any(|failure| {
                        failure.reason == GateFailureReason::UnevidencedNotApplicable
                    }),
                    !excluded
                );
            }
        }
    }
}

#[test]
fn full_profile_selection_includes_new_environments_and_stays_incomplete() {
    let ledger = parse(BUNDLED_LEDGER).unwrap();
    let mut scope = Selection {
        profiles: vec!["mfd-2026r2-enterprise".into()],
        dimensions: EXPECTED_DIMENSIONS
            .into_iter()
            .filter(|dimension| {
                !["vendor_backends.xslt3", "vendor_backends.built_in"].contains(dimension)
            })
            .map(String::from)
            .collect(),
        ..Selection::default()
    };
    assert_eq!(scope.dimensions.len(), 14);
    assert!(
        !gate(&ledger, &scope)
            .unwrap()
            .summary
            .selection
            .full_profiles
    );
    scope.dimensions = EXPECTED_DIMENSIONS.into_iter().map(String::from).collect();
    let report = gate(&ledger, &scope).unwrap();
    assert!(report.summary.selection.full_profiles);
    assert!(!report.passed);
    assert!(
        report
            .failures
            .iter()
            .any(|failure| { failure.reason == GateFailureReason::IncompleteInventory })
    );
}

#[test]
fn cli_selects_and_reports_each_new_unassessed_environment() {
    for backend in NEW_BACKENDS {
        let dimension = format!("vendor_backends.{backend}");
        for command in ["summary", "gate"] {
            let output = Command::new(env!("CARGO_BIN_EXE_mfd-conformance"))
                .args([
                    command,
                    "--profile",
                    "mfd-2026r2-enterprise",
                    "--capability",
                    "interop.mfd-designs",
                    "--dimension",
                    &dimension,
                    "--format",
                    "json",
                ])
                .output()
                .expect("validator command");
            assert_eq!(
                output.status.code(),
                Some(if command == "summary" { 0 } else { 1 })
            );
            let value: Value = serde_json::from_slice(&output.stdout).expect("JSON report");
            let summary = if command == "summary" {
                &value
            } else {
                &value["summary"]
            };
            assert_eq!(summary["schema_version"], 2);
            assert_eq!(summary["selected_cells"], 1);
            assert_eq!(summary["by_profile_dimension"][0]["dimension"], dimension);
            assert_eq!(
                summary["by_profile_dimension"][0]["statuses"]["unassessed"],
                1
            );
            if command == "gate" {
                assert_eq!(value["passed"], false);
                assert_eq!(value["failures"][0]["reason"], "status_not_supported");
                assert_eq!(value["failures"][0]["status"], "unassessed");
            }
        }
    }
}
