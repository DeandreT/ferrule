use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use mfd_conformance::{
    BUNDLED_LEDGER, DIMENSIONS, GateFailureReason, Selection, Status, gate, parse, summarize,
};
use serde_json::{Value, json};

fn selection(capabilities: &[&str], dimensions: &[&str]) -> Selection {
    Selection {
        profiles: vec!["mfd-2026r2-enterprise".into()],
        capabilities: capabilities.iter().map(|item| (*item).into()).collect(),
        dimensions: dimensions.iter().map(|item| (*item).into()).collect(),
    }
}

fn bundled_value() -> Value {
    serde_json::from_str(BUNDLED_LEDGER).expect("bundled ledger is JSON")
}

fn modify_capability(
    document: &mut Value,
    id: &str,
    dimension: &str,
    status: &str,
    evidence: Value,
) {
    let capability = document["capabilities"]
        .as_array_mut()
        .expect("capabilities")
        .iter_mut()
        .find(|item| item["id"] == id)
        .expect("capability");
    capability["support"][dimension]["status"] = json!(status);
    capability["support"][dimension]["evidence"] = evidence;
}

#[test]
fn summary_counts_each_profile_and_dimension_in_stable_order() {
    let ledger = parse(BUNDLED_LEDGER).expect("ledger parses");
    let forward = selection(
        &["interop.vendor-roundtrip", "interop.mfd-designs"],
        &["ferrule_roundtrip", "native_export"],
    );
    let reverse = selection(
        &["interop.mfd-designs", "interop.vendor-roundtrip"],
        &["native_export", "ferrule_roundtrip"],
    );
    let summary = summarize(&ledger, &forward).expect("summarizes");
    assert_eq!(summary.selected_cells, 4);
    assert_eq!(summary.by_profile_dimension.len(), 2);
    assert_eq!(summary.by_profile_dimension[0].dimension, "native_export");
    assert_eq!(
        summary.by_profile_dimension[0].statuses[&Status::Unverified],
        2
    );
    assert_eq!(
        summary.by_profile_dimension[1].dimension,
        "ferrule_roundtrip"
    );
    assert_eq!(
        summary.by_profile_dimension[1].statuses[&Status::Partial],
        2
    );
    assert_eq!(
        serde_json::to_string(&summary).expect("JSON"),
        serde_json::to_string(&summarize(&ledger, &reverse).expect("summarizes")).expect("JSON")
    );
}

#[test]
fn full_profile_gate_fails_until_inventory_is_complete() {
    let ledger = parse(BUNDLED_LEDGER).expect("ledger parses");
    let report = gate(
        &ledger,
        &Selection {
            profiles: vec!["mfd-2026r2-enterprise".into()],
            ..Selection::default()
        },
    )
    .expect("gate evaluates");
    assert!(!report.passed);
    assert!(report.summary.selection.full_profiles);
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.reason == GateFailureReason::IncompleteInventory)
    );

    let explicitly_everything = Selection {
        profiles: vec!["mfd-2026r2-enterprise".into()],
        capabilities: ledger
            .capabilities
            .iter()
            .filter(|capability| {
                capability
                    .profiles
                    .contains(&"mfd-2026r2-enterprise".into())
            })
            .map(|capability| capability.id.clone())
            .collect(),
        dimensions: DIMENSIONS
            .iter()
            .map(|dimension| (*dimension).into())
            .collect(),
    };
    let report = gate(&ledger, &explicitly_everything).expect("gate evaluates");
    assert!(report.summary.selection.full_profiles);
    assert!(
        report
            .failures
            .iter()
            .any(|failure| failure.reason == GateFailureReason::IncompleteInventory)
    );
}

#[test]
fn filtered_development_gate_can_pass_but_partial_blocked_and_unknown_cannot() {
    let scope = selection(&["interop.mfd-designs"], &["import"]);
    let mut document = bundled_value();
    modify_capability(
        &mut document,
        "interop.mfd-designs",
        "import",
        "supported",
        json!(["interop-docs"]),
    );
    let ledger = parse(&document.to_string()).expect("synthetic supported ledger parses");
    let report = gate(&ledger, &scope).expect("gate evaluates");
    assert!(report.passed);
    assert_eq!(report.in_scope_cells, 1);
    assert!(!report.summary.selection.full_profiles);

    for status in [
        "unassessed",
        "unverified",
        "unsupported",
        "partial",
        "blocked",
    ] {
        modify_capability(
            &mut document,
            "interop.mfd-designs",
            "import",
            status,
            json!(["interop-docs"]),
        );
        let ledger = parse(&document.to_string()).expect("ledger parses");
        let report = gate(&ledger, &scope).expect("gate evaluates");
        assert!(!report.passed, "{status}");
        assert_eq!(report.failures.len(), 1, "{status}");
        assert_eq!(
            report.failures[0].reason,
            GateFailureReason::StatusNotSupported,
            "{status}"
        );
    }
}

#[test]
fn not_applicable_requires_proof_and_cannot_create_a_vacuous_pass() {
    let mut document = bundled_value();
    modify_capability(
        &mut document,
        "interop.vendor-roundtrip",
        "import",
        "supported",
        json!(["interop-docs"]),
    );
    let scope = selection(&["interop.vendor-roundtrip"], &["import", "interpreter"]);
    let ledger = parse(&document.to_string()).expect("ledger parses");
    let report = gate(&ledger, &scope).expect("gate evaluates");
    assert!(!report.passed);
    assert_eq!(
        report.summary.by_profile_dimension[1].unevidenced_not_applicable,
        1
    );
    assert_eq!(
        report.failures[0].reason,
        GateFailureReason::UnevidencedNotApplicable
    );

    modify_capability(
        &mut document,
        "interop.vendor-roundtrip",
        "interpreter",
        "not_applicable",
        json!(["vendor-manual"]),
    );
    let ledger = parse(&document.to_string()).expect("ledger parses");
    let report = gate(&ledger, &scope).expect("gate evaluates");
    assert!(report.passed);
    assert_eq!(report.in_scope_cells, 1);
    assert_eq!(report.excluded_not_applicable_cells, 1);

    let only_na = selection(&["interop.vendor-roundtrip"], &["interpreter"]);
    let report = gate(&ledger, &only_na).expect("gate evaluates");
    assert!(!report.passed);
    assert_eq!(report.excluded_not_applicable_cells, 1);
    assert_eq!(report.failures[0].reason, GateFailureReason::NoInScopeCells);
}

#[test]
fn native_export_and_vendor_backends_do_not_accept_ferrule_roundtrip_evidence() {
    let mut document = bundled_value();
    modify_capability(
        &mut document,
        "interop.mfd-designs",
        "ferrule_roundtrip",
        "supported",
        json!(["survey-structural"]),
    );
    let ledger = parse(&document.to_string()).expect("ledger parses");
    let report = gate(
        &ledger,
        &selection(&["interop.mfd-designs"], &["native_export"]),
    )
    .expect("gate evaluates");
    assert!(!report.passed);
    assert_eq!(report.failures[0].status, Some(Status::Unverified));

    modify_capability(
        &mut document,
        "interop.mfd-designs",
        "native_export",
        "supported",
        json!(["survey-structural"]),
    );
    let error = parse(&document.to_string()).expect_err("self-roundtrip cannot prove vendor run");
    assert!(
        error
            .to_string()
            .contains("requires vendor_execution evidence")
    );

    let capability = document["capabilities"]
        .as_array_mut()
        .expect("capabilities")
        .iter_mut()
        .find(|item| item["id"] == "interop.mfd-designs")
        .expect("capability");
    capability["support"]["vendor_backends"]["java"] = json!({
        "status": "supported",
        "evidence": ["survey-structural"],
        "detail": "Synthetic parser rejection case"
    });
    let error = parse(&document.to_string()).expect_err("vendor backend requires vendor run");
    assert!(
        error
            .to_string()
            .contains("vendor_backends.java requires vendor_execution")
    );
}

#[test]
fn vendor_not_applicable_needs_vendor_evidence() {
    let mut document = bundled_value();
    modify_capability(
        &mut document,
        "authoring.blank-project",
        "native_export",
        "not_applicable",
        json!(["roadmap"]),
    );
    let scope = selection(&["authoring.blank-project"], &["native_export"]);
    let ledger = parse(&document.to_string()).expect("ledger parses");
    let report = gate(&ledger, &scope).expect("gate evaluates");
    assert_eq!(
        report.summary.by_profile_dimension[0].unevidenced_not_applicable,
        1
    );
    assert_eq!(
        report.failures[0].reason,
        GateFailureReason::UnevidencedNotApplicable
    );

    modify_capability(
        &mut document,
        "authoring.blank-project",
        "native_export",
        "not_applicable",
        json!(["vendor-manual"]),
    );
    let ledger = parse(&document.to_string()).expect("ledger parses");
    let report = gate(&ledger, &scope).expect("gate evaluates");
    assert_eq!(
        report.summary.by_profile_dimension[0].evidenced_not_applicable,
        1
    );
    assert_eq!(report.excluded_not_applicable_cells, 1);
    assert_eq!(report.failures[0].reason, GateFailureReason::NoInScopeCells);
}

#[test]
fn ferrule_extension_non_applicability_can_use_ferrule_evidence() {
    let mut document = bundled_value();
    modify_capability(
        &mut document,
        "extensions.recursive-construction",
        "native_export",
        "not_applicable",
        json!(["recursive-export"]),
    );
    let ledger = parse(&document.to_string()).expect("ledger parses");
    let scope = Selection {
        profiles: vec!["ferrule-native-extensions".into()],
        capabilities: vec!["extensions.recursive-construction".into()],
        dimensions: vec!["native_export".into()],
    };
    let report = gate(&ledger, &scope).expect("gate evaluates");
    assert_eq!(report.excluded_not_applicable_cells, 1);
    assert_eq!(report.failures[0].reason, GateFailureReason::NoInScopeCells);
}

#[test]
fn unknown_or_empty_filters_fail_instead_of_passing() {
    let ledger = parse(BUNDLED_LEDGER).expect("ledger parses");
    for scope in [
        selection(&["missing"], &["import"]),
        selection(&["interop.mfd-designs"], &["missing"]),
        Selection {
            profiles: vec!["ferrule-native-extensions".into()],
            capabilities: vec!["interop.mfd-designs".into()],
            dimensions: vec!["import".into()],
        },
        selection(&["interop.mfd-designs", "interop.mfd-designs"], &["import"]),
    ] {
        assert!(gate(&ledger, &scope).is_err());
    }
}

#[test]
fn cli_reports_json_and_uses_exit_status_for_gate_result() {
    let binary = env!("CARGO_BIN_EXE_mfd-conformance");
    let filtered = [
        "--profile",
        "mfd-2026r2-enterprise",
        "--capability",
        "interop.mfd-designs",
        "--dimension",
        "import",
        "--format",
        "json",
    ];
    let summary = Command::new(binary)
        .arg("summary")
        .args(filtered)
        .output()
        .expect("summary runs");
    assert!(summary.status.success());
    let summary_json: Value = serde_json::from_slice(&summary.stdout).expect("summary JSON");
    assert_eq!(summary_json["selected_cells"], 1);
    assert_eq!(
        summary_json["by_profile_dimension"][0]["statuses"]["partial"],
        1
    );

    let failed = Command::new(binary)
        .arg("gate")
        .args(filtered)
        .output()
        .expect("gate runs");
    assert_eq!(failed.status.code(), Some(1));
    let failed_json: Value = serde_json::from_slice(&failed.stdout).expect("gate JSON");
    assert_eq!(failed_json["passed"], false);
    assert_eq!(failed_json["failures"][0]["reason"], "status_not_supported");

    let mut document = bundled_value();
    modify_capability(
        &mut document,
        "interop.mfd-designs",
        "import",
        "supported",
        json!(["interop-docs"]),
    );
    let path = temporary_ledger_path();
    std::fs::write(&path, document.to_string()).expect("write temporary ledger");
    let passed = Command::new(binary)
        .arg(&path)
        .arg("gate")
        .args(filtered)
        .output()
        .expect("gate runs");
    std::fs::remove_file(path).expect("remove temporary ledger");
    assert!(
        passed.status.success(),
        "{}",
        String::from_utf8_lossy(&passed.stderr)
    );
    let passed_json: Value = serde_json::from_slice(&passed.stdout).expect("gate JSON");
    assert_eq!(passed_json["passed"], true);
}

fn temporary_ledger_path() -> PathBuf {
    let target = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target");
    std::fs::create_dir_all(&target).expect("target directory");
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("clock")
        .as_nanos();
    target.join(format!(
        "mfd-conformance-{}-{time}.json",
        std::process::id()
    ))
}
