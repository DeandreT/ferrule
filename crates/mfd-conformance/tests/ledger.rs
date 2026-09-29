use std::path::Path;

use mfd_conformance::{BUNDLED_LEDGER, Error, parse};
use serde_json::{Value, json};

fn bundled() -> Value {
    serde_json::from_str(BUNDLED_LEDGER).expect("bundled ledger is JSON")
}

fn rejected(mut edit: impl FnMut(&mut Value), expected: &str) {
    let mut document = bundled();
    edit(&mut document);
    let error = parse(&document.to_string()).expect_err("invalid ledger must reject");
    assert!(error.to_string().contains(expected), "{error}");
}

#[test]
fn bundled_inventory_is_nonempty_versioned_and_locally_grounded() -> Result<(), Error> {
    let ledger = parse(BUNDLED_LEDGER)?;
    assert_eq!(ledger.target.release, "2026r2");
    assert_eq!(ledger.target.edition, "Enterprise");
    assert!(!ledger.inventory_complete);
    assert!(ledger.capabilities.len() > 1);
    ledger.validate_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
}

#[test]
fn all_inventory_id_namespaces_reject_duplicates() {
    for collection in ["profiles", "evidence", "capabilities"] {
        rejected(
            |document| {
                let values = document[collection].as_array_mut().expect("array");
                values.push(values[0].clone());
            },
            "duplicate id",
        );
    }
}

#[test]
fn empty_inventory_and_missing_profile_are_not_valid_baselines() {
    rejected(
        |document| document["capabilities"] = json!([]),
        "capabilities must not be empty",
    );
    rejected(
        |document| document["profiles"] = json!([]),
        "profiles must include",
    );
    rejected(
        |document| document["capabilities"][0]["profiles"] = json!([]),
        "must belong to a profile",
    );
}

#[test]
fn missing_fields_unknown_fields_and_invalid_states_reject() {
    for field in [
        "import",
        "interpreter",
        "native_export",
        "ferrule_roundtrip",
        "gui",
        "debug",
        "rust",
        "csharp",
        "vendor_backends",
    ] {
        rejected(
            |document| {
                document["capabilities"][0]["support"]
                    .as_object_mut()
                    .expect("object")
                    .remove(field);
            },
            "missing field",
        );
    }
    rejected(
        |document| {
            document["capabilities"][0]["support"]["vendor_backends"]
                .as_object_mut()
                .expect("object")
                .remove("java");
        },
        "missing field",
    );
    rejected(
        |document| document["schema_version"] = json!(2),
        "schema_version must be 1",
    );
    rejected(
        |document| document["invented"] = json!(true),
        "unknown field",
    );
    rejected(
        |document| {
            document["capabilities"][0]["support"]["import"]["status"] = json!("complete-ish")
        },
        "unknown variant",
    );
    rejected(
        |document| document["capabilities"][0]["support"]["import"]["detail"] = json!(" "),
        "detail must not be empty",
    );
    rejected(
        |document| document["target"]["release"] = json!(""),
        "target.release must not be empty",
    );
    rejected(
        |document| document["capabilities"][0]["id"] = json!("Invalid ID"),
        "invalid id",
    );
}

#[test]
fn every_reference_namespace_is_checked() {
    rejected(
        |document| document["capabilities"][0]["profiles"] = json!(["unknown"]),
        "profiles references unknown id",
    );
    rejected(
        |document| document["capabilities"][0]["depends_on"] = json!(["unknown"]),
        "depends_on references unknown id",
    );
    rejected(
        |document| {
            document["capabilities"][0]["support"]["import"]["evidence"] = json!(["unknown"])
        },
        "evidence references unknown id",
    );
    rejected(
        |document| {
            document["capabilities"][0]["support"]["import"]["evidence"] =
                json!(["roadmap", "roadmap"])
        },
        "repeats reference",
    );
    rejected(
        |document| document["evidence"] = json!([]),
        "evidence references unknown id",
    );
}

#[test]
fn dependency_cycles_reject() {
    rejected(
        |document| {
            document["capabilities"][0]["depends_on"] = json!(["interop.mfd-designs"]);
        },
        "dependency cycle",
    );
    rejected(
        |document| {
            document["capabilities"][0]["depends_on"] = json!(["interop.vendor-roundtrip"]);
            document["capabilities"][1]["depends_on"] = json!(["interop.mfd-designs"]);
        },
        "dependency cycle",
    );
}

#[test]
fn supported_claims_require_evidence_and_vendor_claims_require_vendor_execution() {
    rejected(
        |document| {
            document["capabilities"][0]["support"]["import"]["evidence"] = json!([]);
        },
        "requires evidence",
    );
    rejected(
        |document| {
            document["capabilities"][0]["support"]["native_export"] = json!({
                "status": "supported", "detail": "Self-roundtrip only", "evidence": ["survey-structural"]
            });
        },
        "requires vendor_execution evidence",
    );
    rejected(
        |document| {
            document["capabilities"][0]["support"]["vendor_backends"]["java"] = json!({
                "status": "partial", "detail": "Emitter source only", "evidence": ["vendor-codegen"]
            });
        },
        "requires vendor_execution evidence",
    );
}

#[test]
fn evidence_paths_must_be_portable_and_exist() -> Result<(), Error> {
    for location in [
        "../ROADMAP.md",
        "/etc/passwd",
        "docs/../ROADMAP.md",
        "C:\\ROADMAP.md",
        "docs//mfd-interop.md",
    ] {
        rejected(
            |document| {
                document["evidence"][5]["location"] = json!(location);
            },
            "invalid location",
        );
    }
    for location in [
        "http://www.altova.com/mapforce",
        "https://.",
        "https://foo..bar/path",
    ] {
        rejected(
            |document| document["evidence"][0]["location"] = json!(location),
            "invalid location",
        );
    }
    let mut document = bundled();
    document["evidence"][5]["location"] = json!("conformance/missing-evidence.json");
    let ledger = parse(&document.to_string())?;
    let error = ledger
        .validate_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .expect_err("missing file must reject");
    assert!(error.to_string().contains("missing file"));
    Ok(())
}

#[test]
fn native_status_is_independent_of_ferrule_self_roundtrip() -> Result<(), Error> {
    let ledger = parse(BUNDLED_LEDGER)?;
    let capability = ledger
        .capabilities
        .iter()
        .find(|entry| entry.id == "interop.mfd-designs")
        .expect("MFD capability");
    assert_eq!(
        capability.support.native_export.status,
        mfd_conformance::Status::Unverified
    );
    assert_eq!(
        capability.support.ferrule_roundtrip.status,
        mfd_conformance::Status::Partial
    );
    let mut encoded = serde_json::to_value(&ledger).expect("serializes");
    encoded["capabilities"][0]["support"]["ferrule_roundtrip"]["status"] = json!("supported");
    let updated = parse(&encoded.to_string())?;
    assert_eq!(
        updated.capabilities[0].support.native_export.status,
        mfd_conformance::Status::Unverified
    );
    Ok(())
}

#[test]
fn command_rejects_invalid_input_with_nonzero_exit_status() {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mfd-conformance"))
        .arg("Cargo.toml")
        .output()
        .expect("validator binary runs");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}
