use super::*;

fn candidate() -> (Value, Value) {
    (
        load("installer-java-api-docs.json"),
        load("installer-java-api-docs-sources.json"),
    )
}

fn reject(value: Value, manifest: Value) {
    let error = validate(value, manifest).expect_err("mutation must be rejected");
    assert!(error.len() < 200, "rejection diagnostic must stay bounded");
}

fn member<'a>(value: &'a mut Value, suffix: &str, fragment: &str) -> &'a mut Value {
    value["declarations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r[0] == "M" && r[1].as_str().unwrap().ends_with(suffix) && r[5] == fragment)
        .unwrap()
}

fn rebuild_signature(row: &mut Value) {
    let mut prefix: Vec<_> = row[10]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m.as_str().unwrap().to_owned())
        .collect();
    for field in [11, 12] {
        if let Some(s) = row[field].as_str() {
            prefix.push(s.to_owned());
        }
    }
    prefix.push(row[4].as_str().unwrap().to_owned());
    let parameters: Vec<_> = row[13]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| match p[2].as_str() {
            Some(name) => format!("{} {name}", p[1].as_str().unwrap()),
            None => p[1].as_str().unwrap().to_owned(),
        })
        .collect();
    let mut signature = format!("{}({})", prefix.join(" "), parameters.join(","));
    let throws: Vec<_> = row[14]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t.as_str().unwrap())
        .collect();
    if !throws.is_empty() {
        signature.push_str(&format!("throws {}", throws.join(",")));
    }
    row[9] = json!(signature);
}

#[test]
fn strict_profile_rejects_unknown_fields_missing_nulls_and_qualification_promotions() {
    for pointer in [
        "",
        "/source",
        "/scope",
        "/oracle",
        "/columns",
        "/comparisons",
        "/comparisons/online_count_discrepancy",
    ] {
        let (mut value, manifest) = candidate();
        value
            .pointer_mut(pointer)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unreviewed".into(), json!(true));
        reject(value, manifest);
    }
    for (pointer, replacement) in [
        ("/runtime_proof", json!(true)),
        ("/inventory_complete", json!(true)),
        ("/status", json!("supported")),
        ("/applicability", json!("java")),
        ("/execution_evidence", json!(["declarations"])),
        ("/execution_receipts", json!(["source-presence"])),
        ("/assessment_inheritance", json!("runtime qualified")),
        ("/source/consumer_java_requirement", json!("14.0.1")),
        ("/source/generator_metadata_role", json!("minimum_runtime")),
        ("/source/javadoc_generator", json!("17")),
        ("/source/generated_at", json!("2026-05-21")),
        ("/source/vendor_generation", json!(true)),
        ("/source/vendor_compilation", json!(true)),
        ("/source/vendor_execution", json!(true)),
        ("/source/com_activation", json!(true)),
        ("/source/html_or_javascript_rendering", json!(true)),
        ("/source/trust", json!("Authenticode verified")),
        ("/source/release_binding", json!("implemented_Java_ABI")),
        ("/scope/source_implementation_observed", json!(true)),
        ("/scope/sdk_complete", json!(true)),
        ("/scope/unqualified", json!([])),
        ("/scope/missing_claims", json!([])),
        ("/scope/exclusions", json!([])),
        ("/comparisons/relationship", json!("ABI_equivalent")),
        ("/comparisons/online_release_binding", json!("2026r2")),
        ("/comparisons/new_enum_capabilities", json!(130)),
        ("/comparisons/aliases_established", json!(true)),
        ("/comparisons/javadoc_enum_numbers", json!("TLB_values")),
        (
            "/comparisons/online_count_discrepancy/resolution",
            json!("resolved"),
        ),
        (
            "/comparisons/online_count_discrepancy/javadoc_literal",
            json!("18"),
        ),
    ] {
        let (mut value, manifest) = candidate();
        *value.pointer_mut(pointer).unwrap() = replacement;
        reject(value, manifest);
    }
    let (mut value, manifest) = candidate();
    value["source"]
        .as_object_mut()
        .unwrap()
        .remove("consumer_java_requirement");
    reject(value, manifest);
}

#[test]
fn coherent_coverage_removal_cannot_shrink_the_frozen_cohort() {
    let (mut value, mut manifest) = candidate();
    let owner = "com.altova.automation.libs.GUID";
    let page = "com/altova/automation/libs/GUID.html";
    value["declarations"]
        .as_array_mut()
        .unwrap()
        .retain(|r| r[1] != owner);
    value["indices"]
        .as_array_mut()
        .unwrap()
        .retain(|r| r[2] != page);
    value["claims"]
        .as_array_mut()
        .unwrap()
        .retain(|r| r[0] != owner);
    manifest["members"]
        .as_array_mut()
        .unwrap()
        .retain(|r| r[0] != page);
    reject(value, manifest);

    let (mut value, manifest) = candidate();
    let owner = "com.altova.automation.libs.COMIDispatchObject";
    let fragment = "getProperty(int,com.altova.automation.libs.JVariant[])";
    value["declarations"]
        .as_array_mut()
        .unwrap()
        .retain(|r| !(r[1] == owner && r[5] == fragment));
    let mut index = 0;
    for row in value["declarations"].as_array_mut().unwrap() {
        if row[0] == "M" && row[1] == owner {
            row[2] = json!(index);
            index += 1;
        }
    }
    value["claims"]
        .as_array_mut()
        .unwrap()
        .retain(|r| !(r[0] == owner && r[1] == fragment));
    reject(value, manifest);
}

#[test]
fn coherent_parameters_types_modifiers_throws_and_literals_are_pinned() {
    let (mut value, manifest) = candidate();
    let row = member(
        &mut value,
        ".Application",
        "openURL(java.lang.String,java.lang.String,java.lang.String)",
    );
    row[13].as_array_mut().unwrap().swap(0, 1);
    for (i, p) in row[13].as_array_mut().unwrap().iter_mut().enumerate() {
        p[0] = json!(i);
    }
    rebuild_signature(row);
    reject(value, manifest);

    for (field, replacement) in [
        (10, json!(["protected"])),
        (11, json!("<T>")),
        (12, json!("java.lang.Object")),
        (14, json!([])),
        (15, json!([])),
    ] {
        let (mut value, manifest) = candidate();
        let row = member(&mut value, ".Application", "openDocument(java.lang.String)");
        row[field] = replacement;
        rebuild_signature(row);
        reject(value, manifest);
    }
    let (mut value, manifest) = candidate();
    let row = member(&mut value, ".Application", "openDocument(java.lang.String)");
    row[13][0][1] = json!("java.lang.Object");
    row[13][0][2] = Value::Null;
    rebuild_signature(row);
    reject(value, manifest);

    let (mut value, manifest) = candidate();
    let row = member(&mut value, ".JVariant", "VT_BYREF");
    row[16] = json!("16385");
    reject(value, manifest);

    let (mut value, manifest) = candidate();
    let row = member(&mut value, ".ENUMProjectType", "eProjectTypeCount");
    row[16] = json!("18");
    row[17] = json!("documented_literal");
    row[18] = json!([91, 96]);
    reject(value, manifest);
}

#[test]
fn documentation_exclusions_prose_references_and_claim_limits_are_not_declarations() {
    let (mut value, manifest) = candidate();
    let row = value["declarations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r[0] == "X")
        .unwrap();
    row[0] = json!("M");
    reject(value, manifest);

    for (owner, fragment, name) in [
        (
            ".COMIUnknownObject",
            "equals(java.lang.Object)",
            "COMIUnknownObject",
        ),
        (".JVariant", "setByRefFlag()", "SetByRefFlag"),
        (
            ".Options",
            "getCompatibilityMode()",
            "getCompatibilityModeCurrent",
        ),
        (".Document", "generateCHashCode()", "generateCSharpCode"),
        (
            ".MapForceView",
            "getHighlightMyConnectionsRecursivly()",
            "getHighlightMyConnectionsRecursively",
        ),
    ] {
        let (mut value, manifest) = candidate();
        let row = member(&mut value, owner, fragment);
        row[4] = json!(name);
        row[5] = json!(format!("{name}()"));
        row[9] = json!(format!("public void {name}()"));
        reject(value, manifest);
    }

    for facet in [
        "internal_use",
        "removed_accessor_note",
        "native_creation_restriction",
        "wrapper_restriction",
        "registration_requirement",
        "native_dependency",
        "indexing",
        "prose_only_constructor",
        "prose_name_discrepancy",
        "historical_example",
        "release_recommendation",
    ] {
        let (mut value, manifest) = candidate();
        let claim = value["claims"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r[2] == facet)
            .unwrap();
        claim[3] = json!("Independently qualified runtime guarantee with available dependencies.");
        reject(value, manifest);
    }
}

#[test]
fn all_source_anchors_hashes_indexes_and_tuple_shapes_remain_exact() {
    for kind in ["T", "M", "X"] {
        let (mut value, manifest) = candidate();
        let row = value["declarations"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r[0] == kind)
            .unwrap();
        let anchor_field = if kind == "T" { 6 } else { 8 };
        let start = row[anchor_field][0].as_u64().unwrap();
        let end = row[anchor_field][1].as_u64().unwrap();
        row[anchor_field] = json!([start + 1, end + 1]);
        reject(value, manifest);
    }
    for index in [0, 1, 2, 3, 4] {
        let (value, mut manifest) = candidate();
        let row = manifest["members"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|r| r[4] == "documented_type")
            .unwrap();
        row[index] = match index {
            0 => json!("../Application.html"),
            1 => json!(row[1].as_u64().unwrap() + 1),
            2 => json!("0".repeat(64)),
            3 => json!(row[3].as_u64().unwrap() + 1),
            _ => json!("excluded_navigation_or_assets"),
        };
        reject(value, manifest);
    }
    let (mut value, manifest) = candidate();
    value["indices"][0][2] = value["indices"][1][2].clone();
    reject(value, manifest);

    let (mut value, manifest) = candidate();
    value["declarations"][1]
        .as_array_mut()
        .unwrap()
        .push(json!("unknown"));
    reject(value, manifest);

    let (value, mut manifest) = candidate();
    manifest["unreviewed"] = json!(true);
    reject(value, manifest);
}
