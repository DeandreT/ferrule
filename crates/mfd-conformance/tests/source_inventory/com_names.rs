use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use super::{load, source_only, unique_ids};

fn names(values: &Value) -> BTreeSet<&str> {
    let array = values.as_array().unwrap();
    let set: BTreeSet<_> = array.iter().map(|value| value.as_str().unwrap()).collect();
    assert_eq!(set.len(), array.len());
    set
}

fn check_row(row: &Value, documented: &BTreeSet<&str>) {
    let only_catalog = names(&row["catalog_only"]);
    let only_library = names(&row["type_library_only"]);
    assert!(only_catalog.is_subset(documented));
    assert!(only_library.is_disjoint(documented));
    let matched = documented.len() - only_catalog.len();
    let library_names = matched + only_library.len();
    assert_eq!(row["catalog_name_count"], documented.len());
    assert_eq!(row["matched_name_count"], matched);
    assert_eq!(row["type_library_name_count"], library_names);
    assert!(row["record_count"].as_u64().unwrap() >= library_names as u64);
}

fn sum(rows: &[Value], key: &str) -> u64 {
    rows.iter().map(|row| row[key].as_u64().unwrap()).sum()
}

fn check_alignment(alignment: &Value) {
    let catalog = load("com-api.json");
    let artifacts = load("artifact-observations.json");
    assert_eq!(alignment["inventory_complete"], false);
    assert_eq!(alignment["catalog_source"]["file"], "com-api.json");
    assert_eq!(
        alignment["catalog_source"]["release_binding"],
        "unestablished"
    );
    assert_eq!(catalog["release_binding"], "unestablished");
    let source = &alignment["type_library_source"];
    assert_eq!(source["observation"], "artifact-observations.json");
    let artifact = artifacts["matched_members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|member| member["name"] == source["member"])
        .unwrap();
    assert_eq!(source["member"], "MapForce.tlb");
    assert_eq!(source["sha256"], artifact["sha256"]);
    for field in ["library", "version", "platform"] {
        let original = if field == "library" { "name" } else { field };
        assert_eq!(source[field], artifacts["type_library"][original]);
    }
    for hash in [
        &source["dump_sha256"],
        &alignment["catalog_source"]["sha256"],
    ] {
        let text = hash.as_str().unwrap();
        assert_eq!(text.len(), 64);
        assert!(text.bytes().all(|byte| byte.is_ascii_hexdigit()));
    }

    let members = catalog["members"].as_array().unwrap();
    let interfaces = alignment["interfaces"].as_array().unwrap();
    unique_ids(interfaces, "catalog_name");
    let mut type_indexes = BTreeSet::new();
    let owners: BTreeSet<_> = catalog["interfaces"]
        .as_array()
        .unwrap()
        .iter()
        .map(|owner| owner["name"].as_str().unwrap())
        .collect();
    assert_eq!(interfaces.len(), owners.len());
    for row in interfaces {
        let owner = row["catalog_name"].as_str().unwrap();
        assert!(owners.contains(owner));
        assert_eq!(row["type_library_name"], format!("I{owner}"));
        assert!(type_indexes.insert(row["type_info_index"].as_u64().unwrap()));
        let documented = members
            .iter()
            .filter(|member| {
                member["member_identity"]["owner"] == owner
                    && member["member_identity"]["kind"] != "event"
            })
            .map(|member| member["member_identity"]["name"].as_str().unwrap())
            .collect();
        check_row(row, &documented);
    }
    let enums: BTreeMap<_, _> = catalog["enumerations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["name"].as_str().unwrap(), row))
        .collect();
    let compared = alignment["enumerations"].as_array().unwrap();
    unique_ids(compared, "catalog_name");
    assert_eq!(compared.len(), enums.len() - 1);
    for row in compared {
        let name = row["catalog_name"].as_str().unwrap();
        assert_ne!(name, "ENUMViewMode");
        assert_eq!(row["type_library_name"], name);
        assert!(type_indexes.insert(row["type_info_index"].as_u64().unwrap()));
        let documented = enums[name]["members"]
            .as_array()
            .unwrap()
            .iter()
            .map(|member| member["name"].as_str().unwrap())
            .collect();
        check_row(row, &documented);
        assert_eq!(row["record_count"], row["type_library_name_count"]);
    }
    let counts = &alignment["counts"];
    assert_eq!(
        counts,
        &json!({
            "interfaces": 22, "interface_function_records": 317,
            "catalog_non_event_names": 253, "type_library_interface_names": 266,
            "matched_interface_names": 252, "catalog_only_interface_names": 1,
            "type_library_only_interface_names": 14, "enum_name_tables_compared": 19,
            "enum_names_compared": 127, "type_library_only_enum_names": 5,
            "catalog_only_enum_names": 0, "excluded_catalog_events": 8
        })
    );
    for (count, field) in [
        ("interface_function_records", "record_count"),
        ("catalog_non_event_names", "catalog_name_count"),
        ("type_library_interface_names", "type_library_name_count"),
        ("matched_interface_names", "matched_name_count"),
    ] {
        assert_eq!(counts[count], sum(interfaces, field));
    }
    assert_eq!(counts["enum_names_compared"], sum(compared, "record_count"));
    assert_eq!(
        members
            .iter()
            .filter(|m| m["member_identity"]["kind"] == "event")
            .count(),
        8
    );
    assert_eq!(
        alignment["unresolved_dump_tables"],
        json!([
            {"type_library_name":"ENUMViewMode","type_info_index":47,"function_records":0,"variable_records":3},
            {"type_library_name":"_IDocumentEvents","type_info_index":52,"function_records":2,"variable_records":0},
            {"type_library_name":"_IProjectEvents","type_info_index":55,"function_records":1,"variable_records":0},
            {"type_library_name":"_IProjectItemEvents","type_info_index":57,"function_records":2,"variable_records":0}
        ])
    );
    assert!(alignment["remaining"].as_array().unwrap().len() >= 5);
    source_only(alignment);
}

#[test]
fn com_name_alignment_preserves_discrepancies_and_source_only_boundary() {
    check_alignment(&load("com-name-alignment.json"));
}

#[test]
fn com_name_alignment_rejects_coverage_and_evidence_promotions() {
    let original = load("com-name-alignment.json");
    for pointer in ["/inventory_complete", "/runtime_proof"] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = json!(true);
        assert!(std::panic::catch_unwind(|| check_alignment(&changed)).is_err());
    }
    for (pointer, value) in [
        ("/catalog_source/release_binding", json!("2026r2")),
        ("/interfaces/15/catalog_only", json!([])),
        (
            "/interfaces/15/type_library_only",
            json!(["HighlightMyConnectionsRecursively"]),
        ),
        ("/unresolved_dump_tables", json!([])),
        (
            "/execution_evidence",
            json!(["name matching is not execution"]),
        ),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(std::panic::catch_unwind(|| check_alignment(&changed)).is_err());
    }
    let mut missing = original;
    missing["interfaces"].as_array_mut().unwrap().pop();
    assert!(std::panic::catch_unwind(|| check_alignment(&missing)).is_err());
}
