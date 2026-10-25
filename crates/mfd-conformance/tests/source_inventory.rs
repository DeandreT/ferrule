//! Source inventories describe documentation, never execution qualification.

use std::collections::BTreeSet;
use std::path::Path;

use serde_json::Value;

fn load(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../conformance/mfd-2026r2-source-inventory")
        .join(name);
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

fn source_only(value: &Value) {
    match value {
        Value::Object(fields) => {
            for (name, value) in fields {
                match name.as_str() {
                    "runtime_proof" => assert_eq!(value, &Value::Bool(false)),
                    "status" => assert_eq!(value, "unassessed"),
                    "applicability" => assert_eq!(value, "unknown"),
                    "execution_evidence" | "execution_receipts" => {
                        assert!(value.as_array().unwrap().is_empty());
                    }
                    "text_lines" | "physical_pdf_pages" => {
                        let range = value.as_array().unwrap();
                        assert!(!range.is_empty());
                        let numbers: Vec<_> = range.iter().map(|n| n.as_u64().unwrap()).collect();
                        assert!(numbers.iter().all(|n| *n > 0));
                        assert!(numbers.windows(2).all(|pair| pair[0] <= pair[1]));
                        if name == "physical_pdf_pages" {
                            assert!(numbers.iter().all(|n| *n <= 1751));
                        }
                    }
                    _ => {}
                }
                source_only(value);
            }
        }
        Value::Array(values) => values.iter().for_each(source_only),
        _ => {}
    }
}

fn unique_ids(values: &[Value], field: &str) {
    let mut seen = BTreeSet::new();
    for value in values {
        let id = value[field].as_str().unwrap();
        assert!(seen.insert(id), "duplicate source identity: {id}");
    }
}

#[test]
fn manual_reviews_preserve_finite_coverage_and_source_only_assessments() {
    let functions = load("functions.json");
    let constructors = load("constructors.json");
    let nonleaf = load("nonleaf.json");
    let functions_units = functions["source_units"].as_array().unwrap();
    let constructor_units = constructors["source_units"]["xs_constructors"]
        .as_array()
        .unwrap();
    let parent_units = nonleaf["source_units"].as_array().unwrap();
    assert_eq!(functions_units.len(), 319);
    assert_eq!(constructor_units.len(), 43);
    assert_eq!(nonleaf["coverage"].as_array().unwrap().len(), 191);
    assert!(
        nonleaf["unreviewed_parent_pages"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        nonleaf["unresolved_parent_bindings"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    unique_ids(functions_units, "research_id");
    unique_ids(constructor_units, "research_id");
    unique_ids(parent_units, "research_id");
    for inventory in [&functions, &constructors, &nonleaf] {
        source_only(inventory);
    }
}

#[test]
fn provisional_com_catalog_does_not_claim_pinned_release_or_execution() {
    let com = load("com-api.json");
    assert_eq!(com["release_binding"], "unestablished");
    assert_eq!(com["interfaces"].as_array().unwrap().len(), 22);
    let members = com["members"].as_array().unwrap();
    assert_eq!(members.len(), 261);
    assert_eq!(com["enumerations"].as_array().unwrap().len(), 20);
    unique_ids(members, "id");
    for source in com["source_documents"].as_array().unwrap() {
        assert_eq!(source["release_binding"], "unestablished");
        let hash = source["raw_sha256"].as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }
    source_only(&com);
}

#[test]
fn external_references_keep_historical_and_runtime_boundaries_explicit() {
    let references = load("external-references.json");
    let rows = references["references"].as_array().unwrap();
    assert_eq!(rows.len(), 2);
    unique_ids(rows, "id");
    for row in rows {
        assert!(
            row["requested_url"]
                .as_str()
                .unwrap()
                .starts_with("https://")
        );
        assert!(
            row["resolved_url"]
                .as_str()
                .unwrap()
                .starts_with("https://")
        );
        let hash = row["raw_sha256"].as_str().unwrap();
        assert_eq!(hash.len(), 64);
        assert!(hash.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert!(!row["boundary"].as_str().unwrap().is_empty());
    }
    source_only(&references);
}

#[test]
fn supporting_declarations_preserve_source_and_installer_boundaries() {
    let supporting = load("supporting.json");
    assert_eq!(supporting["inventory_complete"], false);
    let io = supporting["io_classes"].as_array().unwrap();
    let exceptions = supporting["vendor_exception_classes"].as_array().unwrap();
    assert_eq!(io.len(), 24);
    assert_eq!(exceptions.len(), 16);
    unique_ids(io, "research_id");
    unique_ids(exceptions, "research_id");
    assert_eq!(
        supporting["shared_cpp_exception_methods"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(
        supporting["platform_exception_references"]
            .as_array()
            .unwrap()
            .len(),
        9
    );
    let requests = supporting["installer_artifact_requests"]["bundles"]
        .as_array()
        .unwrap();
    assert_eq!(requests.len(), 6);
    for request in requests {
        assert_eq!(request["resolution"], "not_obtained");
    }
    source_only(&supporting);
}
