use super::{ORACLE, declaration_rows, load, schema::Inventory, validate};
use serde_json::{Value, json};

fn candidate() -> Value {
    load("supporting-io-declarations.json")
}

fn owner<'a>(value: &'a mut Value, name: &str) -> &'a mut Value {
    value["owners"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|owner| owner["name"] == name)
        .unwrap()
}

fn member<'a>(value: &'a mut Value, name: &str, method: &str) -> &'a mut Value {
    owner(value, name)["members"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|member| member["name"] == method)
        .unwrap()
}

fn frozen_rejects(value: Value) {
    let error = validate(value).unwrap_err();
    assert!(error.contains("independent source oracle"), "{error}");
}

fn refresh_derived_counts(value: &mut Value) {
    let owners = value["owners"].as_array().unwrap();
    let mut counts = json!({"constructor":0,"method":0,"property":0,"field":0,"constant":0});
    let mut members = 0;
    let mut nested = 0;
    let mut constants = 0;
    for owner in owners {
        for member in owner["members"].as_array().unwrap() {
            let kind = member["kind"].as_str().unwrap();
            counts[kind] = json!(counts[kind].as_u64().unwrap() + 1);
            members += 1;
        }
        for ty in owner["nested_types"].as_array().unwrap() {
            nested += 1;
            constants += ty["constants"].as_array().unwrap().len();
        }
    }
    let owner_count = owners.len();
    let source_count = value["sources"].as_array().unwrap().len();
    let scope = &mut value["scope"];
    scope["owner_count"] = json!(owner_count);
    scope["source_count"] = json!(source_count);
    scope["direct_member_count"] = json!(members);
    scope["nested_type_count"] = json!(nested);
    scope["enum_constant_count"] = json!(constants);
    scope["member_counts"] = counts;
}

fn closed_cohort_rejects(value: Value, missing_rows: usize) {
    let inventory: Inventory = serde_json::from_value(value.clone()).unwrap();
    let expected: Vec<Value> = ORACLE
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let actual = declaration_rows(&inventory);
    assert_eq!(actual.len() + missing_rows, expected.len());
    assert_ne!(actual, expected);
    // Derived totals describe the smaller candidate accurately. The closed
    // source cohort must still reject it, before downstream signature checks.
    assert_eq!(validate(value).unwrap_err(), "finite scope");
}

#[test]
fn deleting_a_leaf_owner_and_its_source_cannot_shrink_the_closed_cohort() {
    let mut value = candidate();
    let owners = value["owners"].as_array_mut().unwrap();
    let removed = owners.pop().unwrap();
    assert_eq!(removed["name"], "com.altova.io.DocumentOutput");
    assert_eq!(removed["source_index"], 12);
    let source = value["sources"].as_array_mut().unwrap().pop().unwrap();
    assert_eq!(source["index"], 12);
    assert_eq!(source["path"], "com/altova/io/DocumentOutput.java");
    for owner in value["owners"].as_array().unwrap() {
        assert!(owner["source_index"].as_u64().unwrap() < 12);
        for relation in ["extends", "implements"] {
            assert!(
                owner[relation]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|base| base["resolved"] != removed["name"])
            );
        }
    }
    refresh_derived_counts(&mut value);
    assert_eq!(value["scope"]["owner_count"], 23);
    assert_eq!(value["scope"]["source_count"], 12);
    assert_eq!(value["scope"]["direct_member_count"], 97);
    assert_eq!(value["scope"]["member_counts"]["constructor"], 25);
    assert_eq!(value["scope"]["member_counts"]["method"], 34);
    // One source row, one owner row and its two direct member rows are absent.
    closed_cohort_rejects(value, 4);
}

#[test]
fn deleting_one_constructor_overload_cannot_redefine_the_frozen_surface() {
    let mut value = candidate();
    let members = owner(&mut value, "Altova.IO.FileOutput")["members"]
        .as_array_mut()
        .unwrap();
    let removed = members.remove(1);
    assert_eq!(removed["kind"], "constructor");
    assert_eq!(removed["name"], "FileOutput");
    assert_eq!(removed["parameters"].as_array().unwrap().len(), 2);
    for (index, member) in members.iter_mut().enumerate() {
        member["index"] = json!(index);
    }
    assert_eq!(members[0]["parameters"].as_array().unwrap().len(), 1);
    assert_eq!(members[1]["name"], "Fielname");
    refresh_derived_counts(&mut value);
    assert_eq!(value["scope"]["direct_member_count"], 98);
    assert_eq!(value["scope"]["member_counts"]["constructor"], 25);
    closed_cohort_rejects(value, 1);
}

#[test]
fn reordering_complete_constructor_parameters_fails_the_signature_oracle() {
    let mut value = candidate();
    let constructor = &mut owner(&mut value, "Altova.IO.FileOutput")["members"][1];
    assert_eq!(constructor["kind"], "constructor");
    let parameters = constructor["parameters"].as_array_mut().unwrap();
    let original = parameters.clone();
    assert_eq!(original.len(), 2);
    parameters.swap(0, 1);
    assert_eq!(parameters[0], original[1]);
    assert_eq!(parameters[1], original[0]);
    // Names, types and all other metadata travel together. Reindex the swapped
    // records so rejection tests declaration order, not malformed ordinals.
    for (index, parameter) in parameters.iter_mut().enumerate() {
        parameter["index"] = json!(index);
    }
    assert_eq!(parameters[0]["name"], "fm");
    assert_eq!(parameters[0]["type"]["resolved"], "System.IO.FileMode");
    assert_eq!(parameters[1]["name"], "f");
    assert_eq!(parameters[1]["type"]["resolved"], "System.String");
    frozen_rejects(value);
}

#[test]
fn coherent_registered_owner_and_base_changes_fail_frozen_source_facts() {
    let mut value = candidate();
    let a = owner(&mut value, "Altova.IO.FileInput");
    a["extends"][0] = json!({"spelling":"Input","resolved":"Altova.IO.Input"});
    frozen_rejects(value);

    let mut value = candidate();
    let a = owner(&mut value, "Altova.IO.FileInput");
    a["name"] = json!("Altova.IO.FileOutput");
    a["research_id"] = json!("enterprise2026r2.supporting.Altova.IO.FileOutput");
    let b = value["owners"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .rev()
        .find(|o| o["name"] == "Altova.IO.FileOutput")
        .unwrap();
    b["name"] = json!("Altova.IO.FileInput");
    b["research_id"] = json!("enterprise2026r2.supporting.Altova.IO.FileInput");
    frozen_rejects(value);
}

#[test]
fn coherent_type_parameter_and_throws_edits_fail_independent_oracle() {
    let mut value = candidate();
    member(&mut value, "Altova.IO.ReaderInput", "ReaderInput")["parameters"][0]["type"] =
        json!({"spelling":"Stream","resolved":"System.IO.Stream"});
    frozen_rejects(value);

    let mut value = candidate();
    member(&mut value, "Altova.IO.StreamInput", "StreamInput")["parameters"][0]["name"] =
        json!("stream");
    frozen_rejects(value);

    let mut value = candidate();
    member(&mut value, "com.altova.io.StreamOutput", "getStream")["throws"][0] =
        json!({"spelling":"java.io.IOException","resolved":"java.io.IOException"});
    frozen_rejects(value);

    let mut value = candidate();
    member(&mut value, "com.altova.io.StringOutput", "getString")["type"] =
        json!({"spelling":"String","resolved":"java.lang.String"});
    frozen_rejects(value);
}

#[test]
fn accessor_visibility_and_anchors_cannot_be_rewritten() {
    let mut value = candidate();
    member(&mut value, "Altova.IO.StreamOutput", "Stream")["accessors"][0]["declaration_lines"] =
        json!([159, 159]);
    frozen_rejects(value);
    let mut value = candidate();
    member(&mut value, "Altova.IO.FileInput", "Filename")["accessors"][0]["visibility"] =
        json!("private");
    assert!(
        validate(value)
            .unwrap_err()
            .contains("accessor declaration")
    );
}

#[test]
fn nested_enum_ownership_and_declared_constants_are_frozen() {
    let mut value = candidate();
    let a = owner(&mut value, "Altova.IO.Input")["nested_types"].take();
    let b = owner(&mut value, "Altova.IO.Output")["nested_types"].take();
    owner(&mut value, "Altova.IO.Input")["nested_types"] = b;
    owner(&mut value, "Altova.IO.Output")["nested_types"] = a;
    frozen_rejects(value);
    let mut value = candidate();
    member(&mut value, "com.altova.io.Input", "IO_STREAM")["constant_value"] = json!("2");
    frozen_rejects(value);
}

#[test]
fn source_hashes_imports_and_bounded_declaration_coordinates_are_frozen() {
    let mut value = candidate();
    value["sources"][0]["sha256"] = json!("a".repeat(64));
    value["sources"][0]["wrapper_sha256"] = json!("a".repeat(64));
    frozen_rejects(value);
    let mut value = candidate();
    value["sources"][5]["imports"][0]["name"] = json!("java.io.OutputStream");
    let o = owner(&mut value, "com.altova.io.StreamInput");
    o["members"][0]["parameters"][0]["type"] =
        json!({"spelling":"OutputStream","resolved":"java.io.OutputStream"});
    o["members"][1]["type"] = json!({"spelling":"OutputStream","resolved":"java.io.OutputStream"});
    frozen_rejects(value);
    let mut value = candidate();
    owner(&mut value, "Altova.IO.Input")["source_type_index"] = json!(1);
    frozen_rejects(value);
}

#[test]
fn inherited_members_cannot_be_promoted_or_overloads_merged() {
    let mut value = candidate();
    let mut moved = owner(&mut value, "Altova.IO.Input")["members"]
        .as_array_mut()
        .unwrap()
        .remove(5);
    for (index, m) in owner(&mut value, "Altova.IO.Input")["members"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        m["index"] = json!(index);
    }
    moved["index"] = json!(2);
    moved["declaration_lines"] = json!([66, 66]);
    owner(&mut value, "Altova.IO.FileInput")["members"]
        .as_array_mut()
        .unwrap()
        .push(moved);
    frozen_rejects(value);

    let mut value = candidate();
    let o = owner(&mut value, "Altova.IO.StreamOutput");
    let p = o["members"][0]["parameters"].clone();
    o["members"][1]["parameters"] = p;
    assert!(validate(value).unwrap_err().contains("duplicate overload"));
}

#[test]
fn literal_spelling_and_explicit_modifier_order_are_not_editorialized() {
    let mut value = candidate();
    member(&mut value, "Altova.IO.FileOutput", "Fielname")["name"] = json!("Filename");
    frozen_rejects(value);
    let mut value = candidate();
    member(&mut value, "com.altova.io.StreamInput", "createInput")["modifiers"] =
        json!(["public", "static"]);
    frozen_rejects(value);
}

#[test]
fn schema_rejects_bodies_unknown_fields_wrong_types_and_missing_explicit_nulls() {
    for field in [
        "body",
        "property_body",
        "initializer",
        "documentation",
        "implementation",
    ] {
        let mut value = candidate();
        member(&mut value, "Altova.IO.FileOutput", "Fielname")[field] =
            json!("forbidden implementation placeholder");
        assert!(validate(value).unwrap_err().contains("unknown field"));
    }
    let mut value = candidate();
    value["sources"][0]["bytes"] = json!("6989");
    assert!(validate(value).is_err());
    let mut value = candidate();
    member(&mut value, "Altova.IO.Input", "Input")["parameters"][0]
        .as_object_mut()
        .unwrap()
        .remove("default");
    assert!(validate(value).unwrap_err().contains("missing field"));
    let mut value = candidate();
    member(&mut value, "Altova.IO.Output", "type")
        .as_object_mut()
        .unwrap()
        .remove("constant_value");
    assert!(validate(value).unwrap_err().contains("missing field"));
    let mut value = candidate();
    value["execution_evidence"] = json!([{"runtime_proof":true}]);
    assert!(validate(value).is_err());
}

#[test]
fn assessments_and_source_operation_boundaries_cannot_be_promoted() {
    for (field, replacement) in [
        ("inventory_complete", json!(true)),
        ("runtime_proof", json!(true)),
        ("status", json!("supported")),
        ("applicability", json!("applicable")),
        ("execution_evidence", json!(["invented execution"])),
        ("execution_receipts", json!(["invented receipt"])),
    ] {
        let mut value = candidate();
        value[field] = replacement.clone();
        assert!(validate(value).is_err());
        if field != "inventory_complete" {
            let mut value = candidate();
            value["assessment_boundary"][field] = replacement;
            assert!(validate(value).is_err());
        }
    }
    for field in [
        "vendor_generation",
        "vendor_compilation",
        "vendor_execution",
    ] {
        let mut value = candidate();
        value["provenance"][field] = json!(true);
        assert!(validate(value).is_err());
    }
    let mut value = candidate();
    owner(&mut value, "Altova.IO.Input")["assessment_ref"] = json!("supported");
    assert!(validate(value).is_err());
    let mut value = candidate();
    value["scope"]["unqualified"]
        .as_array_mut()
        .unwrap()
        .remove(0);
    assert!(validate(value).is_err());
}

#[test]
fn condition_and_manual_language_claims_remain_separate_from_compiled_availability() {
    let mut value = candidate();
    value["sources"][0]["conditions"] = json!(["invented build condition"]);
    assert!(validate(value).is_err());
    let mut value = candidate();
    value["manual_comparison"]["manual_signature"] =
        json!("com.altova.io.StreamOutput(java.io.OutputStream stream)");
    assert!(
        validate(value)
            .unwrap_err()
            .contains("separate manual/generated declarations")
    );
}
