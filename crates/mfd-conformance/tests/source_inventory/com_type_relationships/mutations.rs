//! Coherent edits must fail even when every redundant identity is rewritten.

use serde_json::{Value, json};

use super::{FILE, load, rejects, validate};

fn walk(value: &Value, root: &Value, path: &str) -> usize {
    let mut count = 0;
    match value {
        Value::Object(fields) => {
            let mut changed = root.clone();
            changed
                .pointer_mut(path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .insert("unsupported_qualification".into(), json!("ABI supported"));
            rejects(&changed, path);
            count += 1;
            for (key, child) in fields {
                count += walk(child, root, &format!("{path}/{key}"));
            }
        }
        Value::Array(items) => {
            let mut changed = root.clone();
            changed
                .pointer_mut(path)
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(Value::Null);
            rejects(&changed, path);
            count += 1;
            for (i, child) in items.iter().enumerate() {
                count += walk(child, root, &format!("{path}/{i}"));
            }
        }
        scalar => {
            let replacement = match scalar {
                Value::Null => json!("invented"),
                Value::Bool(b) => json!(!b),
                Value::Number(n) => json!(n.as_i64().unwrap() + 1),
                Value::String(s) => json!(format!("{s}_changed")),
                _ => unreachable!(),
            };
            let mut changed = root.clone();
            *changed.pointer_mut(path).unwrap() = replacement;
            rejects(&changed, path);
            count += 1;
        }
    }
    count
}

#[test]
fn type_relationships_reject_every_scalar_array_extension_and_unknown_claim() {
    let v = load(FILE);
    assert!(walk(&v, &v, "") > 3200);
}

fn set_target(v: &mut Value, record: usize, target: usize) {
    let t = &v["types"][target];
    let identity = json!([t[0], t[1], t[5]]);
    v["references"][record][3] = json!(format!("{:08x}", target * 100));
    v["references"][record][4] = identity;
}

#[test]
fn independent_oracle_rejects_coherent_guid_target_and_list_rewrites() {
    let original = load(FILE);
    // All redundant GUID occurrences agree, including the target identity.
    let mut guid = original.clone();
    let invented = json!("eedbce9c-9bf7-435d-836e-caeefc3258b7");
    guid["types"][0][5] = invented.clone();
    guid["guids"][6][1] = invented.clone();
    guid["references"][6][4][2] = invented;
    validate::structure(&guid);
    rejects(&guid, "coherent changed GUID");

    // Same-kind targets plus their owning coclass names are coherently swapped.
    let mut target = original.clone();
    let name = target["types"][63][1].clone();
    target["types"][63][1] = target["types"][64][1].clone();
    target["types"][64][1] = name;
    set_target(&mut target, 21, 30);
    set_target(&mut target, 22, 28);
    validate::structure(&target);
    rejects(&target, "coherent same-kind target swap");

    // Swap two complete one-record list payloads, rewriting the owner heads.
    // Ownership, target GUIDs, order, extents and reachability remain consistent.
    let mut relink = original;
    for field in 1..8 {
        let word = relink["references"][8][field].clone();
        relink["references"][8][field] = relink["references"][9][field].clone();
        relink["references"][9][field] = word;
    }
    relink["types"][50][9] = json!("00000090");
    relink["types"][51][9] = json!("00000080");
    validate::structure(&relink);
    rejects(&relink, "coherent relinked list");
}

#[test]
fn type_relationships_reject_missing_empty_source_and_broken_lists() {
    let original = load(FILE);
    let cases: &[fn(&mut Value)] = &[
        |v| {
            // Remove an empty source and adjust its owner and summary too.
            v["references"].as_array_mut().unwrap().remove(1);
            v["types"][42][8] = json!(1);
            v["references"][0][7] = json!("ffffffff");
            v["counts"]["reference_records"] = json!(31);
            v["counts"]["default_sources"] = json!(8);
        },
        |v| v["references"][1][5] = json!(1), // lost FSOURCE
        |v| v["references"][0][5] = json!(3), // ordinary interface is not source
        |v| v["references"][1][7] = json!("00000000"), // cycle
        |v| v["types"][44][9] = json!("00000000"), // shared list head
        |v| v["references"][0][7] = json!("00000200"), // outside segment
        |v| v["references"][0][7] = json!("00000011"), // misaligned
        |v| {
            v["types"][42][8] = json!(1); // dangling second record
            v["references"][0][7] = json!("ffffffff");
        },
        |v| v["references"][31][3] = json!("00001d4c"), // absent local target
        |v| v["references"][31][4][0] = json!(0),       // dummy exception lost
        |v| v["references"][0][6] = json!("00000000"),  // custom-data terminator
        |v| v["references"][0][2] = json!(1),           // owner ordinal
        |v| v["references"].as_array_mut().unwrap().swap(0, 1),
        |v| v["references"][1] = v["references"][0].clone(),
        |v| {
            v["types"].as_array_mut().unwrap().remove(41);
        },
        |v| {
            v["types"][2].as_array_mut().unwrap().remove(5);
        }, // explicit nullable GUID required
        |v| v["types"][41][9] = json!("00000001"), // invented event base
        |v| v["types"][41][8] = json!(0),          // do not normalize the unresolved header
    ];
    for (i, case) in cases.iter().enumerate() {
        let mut changed = original.clone();
        case(&mut changed);
        rejects(&changed, &format!("list/header mutation {i}"));
    }
}

#[test]
fn structural_controls_reject_reference_drift_without_using_the_oracle() {
    let original = load(FILE);
    let cases: &[fn(&mut Value)] = &[
        |v| v["references"][1][7] = json!("00000000"),  // cycle
        |v| v["types"][44][9] = json!("00000000"),      // shared list
        |v| v["references"][0][7] = json!("00000200"),  // out of bounds
        |v| v["references"][0][7] = json!("00000011"),  // misaligned
        |v| v["references"][0][7] = json!("ffffffff"),  // early termination
        |v| v["references"][31][3] = json!("00001d4c"), // unresolved target
        |v| v["references"][1][5] = json!(1),           // missing source flag
        |v| v["references"][0][1] = json!(44),          // wrong owner
        |v| v["references"][0][2] = json!(1),           // wrong ordinal
        |v| v["references"][0][6] = json!("00000000"),  // custom data
        |v| v["references"][0][8] = json!([10252, 15148]), // invalid dump anchor
        |v| v["references"][0][4][2] = json!("eedbce9c-9bf7-435d-836e-caeefc3258b5"),
        |v| {
            v["types"][42][8] = json!(1);
            v["references"][0][7] = json!("ffffffff"); // unreachable empty source
        },
    ];
    for (i, case) in cases.iter().enumerate() {
        let mut changed = original.clone();
        case(&mut changed);
        assert!(
            std::panic::catch_unwind(|| validate::structure(&changed)).is_err(),
            "accepted structural mutation {i}"
        );
    }
}

#[test]
fn type_relationships_reject_wrong_table_anchors_and_qualification_promotions() {
    let original = load(FILE);
    for (pointer, value) in [
        ("/references/0/8/0", json!(10252)), // displaced printed address
        ("/references/6/8/1", json!(15148)), // late invalid RefRecord rendering
        ("/references/15/8/1", json!(15239)),
        ("/sources/reference_table/0", json!(10252)),
        ("/types/41/13/4", json!(15148)),
        ("/guids/0/4", json!("new_type_capability")),
        ("/library/platform", json!("Windows ABI qualified")),
        ("/import/dependency_availability", json!("installed")),
        ("/assessment/inventory_complete", json!(true)),
        ("/assessment/runtime_proof", json!(true)),
        ("/assessment/status", json!("supported")),
        ("/assessment/applicability", json!("all_backends")),
        ("/assessment/execution_evidence", json!(["GUID match"])),
        ("/boundaries", json!([])),
    ] {
        let mut changed = original.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        rejects(&changed, pointer);
    }
}

#[test]
fn joined_existing_inventory_cannot_silently_change_type_identity_or_counts() {
    let v = load(FILE);
    let coverage = load("com-callable-coverage.json");
    let enums = load("com-enum-values.json");
    validate::joins(&v, &coverage, &enums);
    for (pointer, value) in [
        ("/declarations/41/name", json!("IApplication")),
        ("/declarations/41/function_records", json!(1)),
        ("/declarations/2/variable_records", json!(4)),
        ("/counts/zero_callable_types", json!(28)),
        ("/counts/adopted_records", json!(324)),
    ] {
        let mut changed = coverage.clone();
        *changed.pointer_mut(pointer).unwrap() = value;
        assert!(std::panic::catch_unwind(|| validate::joins(&v, &changed, &enums)).is_err());
    }
    let mut changed = enums;
    changed["enumerations"][0]["name"] = json!("InventedEnum");
    assert!(std::panic::catch_unwind(|| validate::joins(&v, &coverage, &changed)).is_err());
}
