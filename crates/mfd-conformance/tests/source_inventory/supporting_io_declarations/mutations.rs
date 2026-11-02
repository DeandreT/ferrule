use super::{load, validate};
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
