use super::{load, oracle, schema::Inventory, validate};
use serde_json::{Value, json};

fn candidate() -> Value {
    load("mapping-entry-declarations.json")
}

fn frozen_rejects(value: Value) {
    let error = validate(value).unwrap_err();
    assert!(error.contains("independent source oracle"), "{error}");
}

fn refresh_counts(value: &mut Value) {
    let owners = value["owners"].as_array().unwrap();
    let members: Vec<_> = owners
        .iter()
        .flat_map(|o| o["members"].as_array().unwrap())
        .collect();
    let (owner_count, member_count) = (owners.len(), members.len());
    let parameter_count: usize = members
        .iter()
        .map(|m| m["parameters"].as_array().unwrap().len())
        .sum();
    let accessor_count: usize = members
        .iter()
        .map(|m| m["accessors"].as_array().unwrap().len())
        .sum();
    let throws_count = members
        .iter()
        .filter(|m| !m["throws"].as_array().unwrap().is_empty())
        .count();
    value["scope"]["owner_count"] = json!(owner_count);
    value["scope"]["direct_member_count"] = json!(member_count);
    value["scope"]["parameter_count"] = json!(parameter_count);
    value["scope"]["accessor_count"] = json!(accessor_count);
    value["scope"]["throws_clause_count"] = json!(throws_count);
    value["scope"]["source_count"] = json!(value["sources"].as_array().unwrap().len());
    value["scope"]["profile_count"] = json!(value["profiles"].as_array().unwrap().len());
}

fn remove_member_refs(value: &mut Value, prefix: &str) {
    for contract in value["contract_enrichments"].as_array_mut().unwrap() {
        contract["member_refs"]
            .as_array_mut()
            .unwrap()
            .retain(|r| !r.as_str().unwrap().starts_with(prefix));
    }
}

#[test]
fn coherent_owner_and_source_removal_cannot_shrink_the_frozen_cohort() {
    let mut v = candidate();
    let removed = v["owners"].as_array_mut().unwrap().pop().unwrap();
    assert_eq!(removed["language"], "Java");
    v["sources"]
        .as_array_mut()
        .unwrap()
        .retain(|s| s["language"] == "C#");
    for (index, source) in v["sources"].as_array_mut().unwrap().iter_mut().enumerate() {
        source["index"] = json!(index);
    }
    let o = &mut v["owners"][0];
    o["anchors"][1]["source_index"] = json!(1);
    for m in o["members"].as_array_mut().unwrap() {
        m["anchors"][1]["source_index"] = json!(1);
        for a in m["accessors"].as_array_mut().unwrap() {
            a["anchors"][1]["source_index"] = json!(1);
        }
    }
    remove_member_refs(&mut v, "#/owners/1/");
    v["io_base_joins"]
        .as_array_mut()
        .unwrap()
        .retain(|j| !j["owner"].as_str().unwrap().starts_with("com."));
    refresh_counts(&mut v);
    assert_eq!(v["scope"]["direct_member_count"], 3);
    let typed: Inventory = serde_json::from_value(v.clone()).unwrap();
    assert_eq!(oracle::rows(&typed).len(), 8); // 2 profiles, 2 sources, 1 owner, 3 members.
    frozen_rejects(v);
}

#[test]
fn coherent_overload_removal_updates_counts_and_contract_references_but_still_fails() {
    let mut v = candidate();
    let removed = v["owners"][0]["members"]
        .as_array_mut()
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(removed["name"], "Run");
    assert_eq!(
        removed["parameters"][0]["type"]["spelling"],
        "Altova.IO.Input"
    );
    remove_member_refs(&mut v, "#/owners/0/members/2");
    refresh_counts(&mut v);
    assert_eq!(v["scope"]["direct_member_count"], 6);
    assert_eq!(v["scope"]["parameter_count"], 7);
    frozen_rejects(v);
}

#[test]
fn complete_parameter_reorder_and_reindex_cannot_redefine_either_overload() {
    for (oi, mi) in [(0, 1), (0, 2), (1, 2), (1, 3)] {
        let mut v = candidate();
        let p = v["owners"][oi]["members"][mi]["parameters"]
            .as_array_mut()
            .unwrap();
        p.swap(0, 1);
        for (index, parameter) in p.iter_mut().enumerate() {
            parameter["index"] = json!(index);
        }
        frozen_rejects(v);
    }
}

#[test]
fn setter_visibility_accessor_tokens_and_java_throws_are_source_facts() {
    let mut v = candidate();
    let setter = &mut v["owners"][0]["members"][0]["accessors"][1];
    setter["visibility"] = json!("private");
    setter["explicit_visibility"] = json!("private");
    frozen_rejects(v);

    let mut v = candidate();
    v["owners"][0]["members"][0]["accessors"]
        .as_array_mut()
        .unwrap()
        .swap(0, 1);
    frozen_rejects(v);

    let mut v = candidate();
    v["owners"][1]["members"][2]["throws"][0]["spelling"] = json!("RuntimeException");
    frozen_rejects(v);

    let mut v = candidate();
    v["owners"][1]["members"][3]["throws"] = json!([]);
    refresh_counts(&mut v);
    assert_eq!(v["scope"]["throws_clause_count"], 1);
    frozen_rejects(v);
}

#[test]
fn profile_hash_header_closure_namespace_imports_and_coordinates_are_frozen() {
    for (pointer, replacement) in [
        ("/sources/2/sha256", json!("a".repeat(64))),
        ("/sources/2/bytes", json!(11359)),
        ("/sources/3/lf_lines", json!(385)),
        ("/sources/2/imports/9/name", json!("OtherSchema")),
        ("/owners/0/anchors/1/declaration_lines", json!([31, 31])),
        ("/owners/1/anchors/0/closing_line", json!(382)),
        (
            "/owners/1/members/2/anchors/1/declaration_lines",
            json!([334, 334]),
        ),
        (
            "/owners/0/extends/0",
            json!({"spelling":"Other.TraceProvider","resolved":"Other.TraceProvider","binding":"explicit_qualified"}),
        ),
    ] {
        let mut v = candidate();
        *v.pointer_mut(pointer).unwrap() = replacement;
        frozen_rejects(v);
    }
    let mut v = candidate();
    v["sources"][0]["namespace"] = json!("OtherMapping");
    v["sources"][2]["namespace"] = json!("OtherMapping");
    v["owners"][0]["name"] = json!("OtherMapping.MappingMapToProductValuePairs");
    frozen_rejects(v);
    let mut v = candidate();
    v["sources"][2]["sha256"] = v["sources"][0]["sha256"].clone();
    v["sources"][2]["bytes"] = v["sources"][0]["bytes"].clone();
    frozen_rejects(v);
}

#[test]
fn literal_case_types_and_lookup_unknowns_cannot_be_editorialized() {
    for (pointer, replacement) in [
        ("/owners/0/members/1/name", json!("run")),
        ("/owners/1/members/2/name", json!("Run")),
        ("/owners/1/members/0/parameters/0/name", json!("close")),
        (
            "/owners/0/members/0/type",
            json!({"spelling":"Boolean","resolved":null,"binding":"unresolved_unqualified"}),
        ),
        (
            "/owners/1/members/2/throws/0",
            json!({"spelling":"java.lang.Exception","resolved":"java.lang.Exception","binding":"explicit_qualified"}),
        ),
        (
            "/owners/0/members/1/parameters/0/type",
            json!({"spelling":"string","resolved":"System.String","binding":"language_keyword"}),
        ),
    ] {
        let mut v = candidate();
        *v.pointer_mut(pointer).unwrap() = replacement;
        frozen_rejects(v);
    }
    let mut v = candidate();
    let p = v["owners"][0]["members"][1]["parameters"].clone();
    v["owners"][0]["members"][2]["parameters"] = p;
    assert_eq!(validate(v).unwrap_err(), "duplicate overload");
}

#[test]
fn inherited_and_nested_implementation_members_cannot_be_promoted() {
    let mut v = candidate();
    v["owners"][1]["members"][1]["name"] = json!("getTraceTarget");
    v["owners"][1]["members"][1]["type"] = json!({"spelling":"com.altova.TraceTarget","resolved":"com.altova.TraceTarget","binding":"explicit_qualified"});
    frozen_rejects(v);
    let mut v = candidate();
    let mut m = v["owners"][1]["members"][1].clone();
    m["index"] = json!(4);
    m["name"] = json!("moveNext");
    m["throws"] =
        json!([{"spelling":"Exception","resolved":null,"binding":"unresolved_unqualified"}]);
    for a in m["anchors"].as_array_mut().unwrap() {
        a["declaration_lines"] = json!([380, 380]);
    }
    v["owners"][1]["members"].as_array_mut().unwrap().push(m);
    refresh_counts(&mut v);
    frozen_rejects(v);
    let mut v = candidate();
    v["owners"][0]["explicit_constructors"] = json!(["MappingMapToProductValuePairs()"]);
    v["scope"]["explicit_constructor_count"] = json!(1);
    assert_eq!(validate(v).unwrap_err(), "direct public owner boundary");
}

#[test]
fn schema_rejects_implementation_fields_wrong_types_and_missing_nulls() {
    for target in [
        "",
        "/owners/0",
        "/owners/0/members/0",
        "/owners/0/members/0/accessors/0",
        "/owners/1/members/0/parameters/0",
    ] {
        for field in [
            "body",
            "initializer",
            "private_initializer",
            "default_close",
            "comments",
            "example",
            "schema_members",
            "implementation",
        ] {
            let mut v = candidate();
            v.pointer_mut(target).unwrap()[field] = json!("forbidden");
            assert!(validate(v).unwrap_err().contains("unknown field"));
        }
    }
    for (target, field) in [
        ("/owners/1/members/0/parameters/0", "default"),
        ("/owners/0/members/0/accessors/1", "explicit_visibility"),
        ("/owners/1/members/2/throws/0", "resolved"),
    ] {
        let mut v = candidate();
        v.pointer_mut(target)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .remove(field);
        assert!(validate(v).unwrap_err().contains("missing field"));
    }
    let mut v = candidate();
    v["sources"][0]["bytes"] = json!("11254");
    assert!(validate(v).is_err());
    let mut v = candidate();
    v["owners"][1]["members"][0]["parameters"][0]["default"] = json!("false");
    assert_eq!(validate(v).unwrap_err(), "ordered parameter/default");
}

#[test]
fn source_profile_version_scope_and_manual_claims_cannot_promote_qualification() {
    for (pointer, replacement) in [
        ("/scope/api_shape", json!("all_generated_mapping_APIs")),
        ("/profiles/0/api_scope", json!("universal_generated_api")),
        ("/profiles/1/output_count", json!(2)),
        ("/profiles/1/generator_version", json!("28.3.0.0")),
        ("/provenance/generator_version", json!("28.3.0.0")),
        (
            "/provenance/source_binding",
            json!("whole_product_release_qualified"),
        ),
        ("/provenance/signature_trust", json!("verified")),
        ("/provenance/profile_byte_identity", json!(true)),
        (
            "/provenance/profile_integration_signatures_equal",
            json!(false),
        ),
        ("/scope/inherited_members", json!("included")),
        ("/scope/implementation_copied", json!(true)),
        (
            "/contract_enrichments/1/relationship",
            json!("default_true_and_close_on_error_verified"),
        ),
        (
            "/io_base_joins/0/relationship",
            json!("dependency_available"),
        ),
        (
            "/contract_enrichments/2/identity",
            json!("invented-research-id"),
        ),
        (
            "/contract_enrichments/0/pointer",
            json!("/source_usage_contracts/1"),
        ),
        ("/io_base_joins/0/pointer", json!("/owners/1")),
        ("/owners/0/assessment_ref", json!("supported")),
        ("/sources/2/conditions", json!(["invented build condition"])),
    ] {
        let mut v = candidate();
        *v.pointer_mut(pointer).unwrap() = replacement;
        assert!(validate(v).is_err(), "{pointer}");
    }
    for (field, replacement) in [
        ("inventory_complete", json!(true)),
        ("runtime_proof", json!(true)),
        ("status", json!("supported")),
        ("applicability", json!("applicable")),
        ("execution_evidence", json!(["invented execution"])),
        ("execution_receipts", json!(["invented receipt"])),
    ] {
        let mut v = candidate();
        v[field] = replacement.clone();
        assert!(validate(v).is_err());
        if field != "inventory_complete" {
            let mut v = candidate();
            v["assessment_boundary"][field] = replacement;
            assert!(validate(v).is_err());
        }
    }
    for field in [
        "vendor_generation",
        "vendor_compilation",
        "vendor_execution",
    ] {
        let mut v = candidate();
        v["provenance"][field] = json!(true);
        assert!(validate(v).is_err());
    }
    let mut v = candidate();
    v["scope"]["unqualified"].as_array_mut().unwrap().remove(0);
    assert!(validate(v).is_err());
}
