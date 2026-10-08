use std::collections::BTreeSet;
use std::path::PathBuf;

use mfd_conformance::{
    BUNDLED_LEDGER, DIMENSIONS, EvidenceKind, GateFailureReason, Selection, Status, gate, parse,
    summarize,
};
use serde_json::{Value, json};

const PROPOSAL: &str = include_str!("../../../conformance/json-object-schemas.proposal.json");
const NATIVE: &str = "mfd-2026r2-enterprise";
const EXTENSIONS: &str = "ferrule-native-extensions";
const CLOSED: &str = "json.objects.closed-fields.native";
const REQUIRED: &str = "json.objects.declared-required.native";
const OPERATIONS: [&str; 8] = [
    "closed-fields",
    "typed-extra-fields",
    "arbitrary-extra-fields",
    "declared-required",
    "runtime-required",
    "nullable-object",
    "exclusive-alternatives",
    "inclusive-alternatives",
];

fn selection(profile: &str, capability: &str, dimension: &str) -> Selection {
    Selection {
        profiles: vec![profile.into()],
        capabilities: vec![capability.into()],
        dimensions: vec![dimension.into()],
    }
}

fn document() -> Value {
    serde_json::from_str(PROPOSAL).expect("proposal is JSON")
}

fn assessment<'a>(document: &'a mut Value, id: &str, dimension: &str) -> &'a mut Value {
    let capability = document["capabilities"]
        .as_array_mut()
        .expect("capability array")
        .iter_mut()
        .find(|entry| entry["id"] == id)
        .expect("capability ID");
    let support = &mut capability["support"];
    if let Some(backend) = dimension.strip_prefix("vendor_backends.") {
        &mut support["vendor_backends"][backend]
    } else {
        &mut support[dimension]
    }
}

#[test]
fn proposal_has_independent_explicit_object_cells_without_promoting_history() {
    let ledger = parse(PROPOSAL).expect("proposal validates");
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    ledger
        .validate_files(&root)
        .expect("repository source leads exist");
    assert!(!ledger.inventory_complete);
    let expected: BTreeSet<_> = OPERATIONS
        .iter()
        .flat_map(|operation| {
            ["native", "extensions"].map(|profile| format!("json.objects.{operation}.{profile}"))
        })
        .collect();
    let actual: BTreeSet<_> = ledger
        .capabilities
        .iter()
        .map(|entry| entry.id.clone())
        .collect();
    assert_eq!(actual, expected);
    assert_eq!(ledger.capabilities.len(), 16);
    assert_eq!(
        summarize(&ledger, &Selection::default())
            .unwrap()
            .selected_cells,
        224
    );
    assert_eq!(ledger.target.release, "2026r2");
    assert_eq!(ledger.target.edition, "Enterprise");
    assert!(
        ledger
            .evidence
            .iter()
            .all(|entry| entry.kind == EvidenceKind::Repository)
    );
    let mut unverified = 0;
    let mut unassessed = 0;
    for capability in &ledger.capabilities {
        let expected_profile = if capability.id.ends_with(".native") {
            NATIVE
        } else {
            EXTENSIONS
        };
        assert_eq!(capability.profiles, vec![expected_profile.to_owned()]);
        assert_eq!(capability.support.assessments().len(), DIMENSIONS.len());
        for (dimension, cell) in capability.support.assessments() {
            let expected_status = if expected_profile == NATIVE
                && matches!(
                    dimension,
                    "import"
                        | "interpreter"
                        | "native_export"
                        | "ferrule_roundtrip"
                        | "rust"
                        | "csharp"
                ) {
                Status::Unverified
            } else {
                Status::Unassessed
            };
            assert_eq!(
                cell.status, expected_status,
                "{} {dimension}",
                capability.id
            );
            if cell.status == Status::Unverified {
                unverified += 1;
            }
            if cell.status == Status::Unassessed {
                unassessed += 1;
            }
            assert!(matches!(
                cell.status,
                Status::Unverified | Status::Unassessed
            ));
            assert!(
                cell.detail.starts_with("Applicable candidate:")
                    || cell.detail.starts_with("Applicability")
            );
            assert!(cell.detail.contains("Missing") || cell.detail.contains("unknown"));
            assert!(
                cell.evidence
                    .iter()
                    .all(|id| !id.starts_with("historical-"))
            );
        }
    }
    assert_eq!((unverified, unassessed), (48, 176));
    let canonical = parse(BUNDLED_LEDGER).expect("canonical ledger validates independently");
    let broad = canonical
        .capabilities
        .iter()
        .find(|entry| entry.id == "formats.json")
        .unwrap();
    assert!(broad.title.contains("complete JSON Schema composition"));
    assert!(ledger.capabilities.iter().all(|entry| entry.id != broad.id));
    assert!(!canonical.inventory_complete);
}

#[test]
fn source_leads_do_not_pass_a_focused_behavior_gate() {
    let ledger = parse(PROPOSAL).unwrap();
    for (id, dimension) in [
        (REQUIRED, "interpreter"),
        (CLOSED, "rust"),
        (CLOSED, "native_export"),
        (CLOSED, "vendor_backends.csharp"),
    ] {
        let report = gate(&ledger, &selection(NATIVE, id, dimension)).unwrap();
        assert_eq!(report.summary.selected_cells, 1);
        assert_eq!(report.in_scope_cells, 1);
        assert!(!report.passed);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(
            report.failures[0].reason,
            GateFailureReason::StatusNotSupported
        );
    }
}

#[test]
fn empty_unknown_and_duplicate_intersections_cannot_pass() {
    let ledger = parse(PROPOSAL).unwrap();
    for scope in [
        selection(EXTENSIONS, REQUIRED, "interpreter"),
        selection(NATIVE, "json.objects.missing.native", "interpreter"),
        selection(NATIVE, REQUIRED, "missing"),
        Selection {
            profiles: vec![NATIVE.into()],
            capabilities: vec![REQUIRED.into(), REQUIRED.into()],
            dimensions: vec!["interpreter".into()],
        },
    ] {
        assert!(gate(&ledger, &scope).is_err());
    }
    // Empty axes mean all values, not a zero-cell success.
    let report = gate(&ledger, &Selection::default()).unwrap();
    assert_eq!(report.summary.selected_cells, 224);
    assert!(!report.passed);
    assert!(
        report
            .failures
            .iter()
            .any(|failure| { failure.reason == GateFailureReason::IncompleteInventory })
    );
}

#[test]
fn missing_proof_and_missing_repository_file_refuse() {
    let mut document = document();
    *assessment(&mut document, REQUIRED, "interpreter") = json!({
        "status": "supported", "evidence": [],
        "detail": "Synthetic missing-evidence rejection; not behavioral proof."
    });
    let error = parse(&document.to_string()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("requires evidence for supported/partial")
    );

    let mut ledger = parse(PROPOSAL).unwrap();
    // A directory is guaranteed not to be an evidence file in this repository.
    ledger.evidence[0].location = "conformance".into();
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let error = ledger.validate_files(&root).unwrap_err();
    assert!(error.to_string().contains("references a missing file"));
}

#[test]
fn historical_import_and_emission_cannot_be_promoted_to_vendor_execution() {
    for dimension in [
        "native_export",
        "vendor_backends.xslt1",
        "vendor_backends.csharp",
    ] {
        for evidence in ["historical-import-roundtrip", "historical-emission"] {
            let mut document = document();
            *assessment(&mut document, CLOSED, dimension) = json!({
                "status": "supported", "evidence": [evidence],
                "detail": "Synthetic prohibited proof promotion."
            });
            let error = parse(&document.to_string()).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("requires vendor_execution evidence")
            );
        }
    }
}

#[test]
fn synthetic_positive_and_exclusion_controls_keep_profiles_independent() {
    let mut document = document();
    *assessment(&mut document, REQUIRED, "interpreter") = json!({
        "status": "supported", "evidence": ["inventory-guards"],
        "detail": "Synthetic validator positive control only; not JSON execution proof."
    });
    let ledger = parse(&document.to_string()).unwrap();
    let report = gate(&ledger, &selection(NATIVE, REQUIRED, "interpreter")).unwrap();
    assert!(report.passed);
    assert_eq!(report.in_scope_cells, 1);
    let independent = gate(
        &ledger,
        &selection(
            EXTENSIONS,
            "json.objects.declared-required.extensions",
            "interpreter",
        ),
    )
    .unwrap();
    assert!(!independent.passed);
    assert_eq!(independent.failures[0].status, Some(Status::Unassessed));

    *assessment(
        &mut document,
        "json.objects.closed-fields.extensions",
        "native_export",
    ) = json!({
        "status": "not_applicable", "evidence": ["profile-contract"],
        "detail": "Synthetic evidence-only exclusion control; not an actual operation exclusion."
    });
    let ledger = parse(&document.to_string()).unwrap();
    let excluded = gate(
        &ledger,
        &selection(
            EXTENSIONS,
            "json.objects.closed-fields.extensions",
            "native_export",
        ),
    )
    .unwrap();
    assert!(!excluded.passed);
    assert_eq!(excluded.in_scope_cells, 0);
    assert_eq!(excluded.excluded_not_applicable_cells, 1);
    assert_eq!(
        excluded.failures[0].reason,
        GateFailureReason::NoInScopeCells
    );
}

#[test]
fn focused_summary_is_stable_and_full_profile_remains_incomplete() {
    let ledger = parse(PROPOSAL).unwrap();
    let first = Selection {
        profiles: vec![NATIVE.into()],
        capabilities: vec![CLOSED.into(), REQUIRED.into()],
        dimensions: vec!["interpreter".into(), "import".into()],
    };
    let reverse = Selection {
        profiles: first.profiles.clone(),
        capabilities: first.capabilities.iter().rev().cloned().collect(),
        dimensions: first.dimensions.iter().rev().cloned().collect(),
    };
    let forward = summarize(&ledger, &first).unwrap();
    assert_eq!(forward.selected_cells, 4);
    assert_eq!(
        serde_json::to_string(&forward).unwrap(),
        serde_json::to_string(&summarize(&ledger, &reverse).unwrap()).unwrap(),
    );
    for profile in [NATIVE, EXTENSIONS] {
        let report = gate(
            &ledger,
            &Selection {
                profiles: vec![profile.into()],
                ..Selection::default()
            },
        )
        .unwrap();
        assert!(report.summary.selection.full_profiles);
        assert_eq!(report.summary.selected_cells, 112);
        assert!(!report.passed);
        assert!(
            report
                .failures
                .iter()
                .any(|failure| { failure.reason == GateFailureReason::IncompleteInventory })
        );
    }
}
