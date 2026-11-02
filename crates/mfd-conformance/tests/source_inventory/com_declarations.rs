//! Shared source-only declaration checks; no vendor artifacts or activation.

#[path = "com_declarations/cohorts.rs"]
mod cohorts;
#[path = "com_declarations/coverage.rs"]
mod coverage;
#[path = "com_declarations/oracle.rs"]
mod oracle;
#[path = "com_declarations/remaining.rs"]
mod remaining;
#[path = "com_declarations/schema.rs"]
mod schema;
#[path = "com_declarations/validate.rs"]
mod validate;

use serde_json::{Value, json};

use super::load;

#[test]
fn core_declarations_match_independent_dump_oracle_and_catalog() {
    let core = checked_core(&load("com-core-declarations.json"));
    let coverage = coverage::check(&load("com-callable-coverage.json"));
    coverage::declaration_extents(&core, &coverage);
    assert_eq!(core.records.len(), 114);
    assert_eq!(
        core.records
            .iter()
            .map(|r| r.parameters.len())
            .sum::<usize>(),
        119
    );
    assert_eq!(
        core.records
            .iter()
            .filter(|r| r.catalog_id.is_none())
            .count(),
        8
    );
    for (kind, count) in [
        (schema::Invocation::PropertyGet, 58),
        (schema::Invocation::PropertyPut, 26),
        (schema::Invocation::Function, 30),
    ] {
        assert_eq!(
            core.records
                .iter()
                .filter(|r| r.invocation_kind == kind)
                .count(),
            count
        );
    }
    let flagged: Vec<_> = core
        .records
        .iter()
        .filter(|r| r.function_flags != 0)
        .map(|r| (r.owner.as_str(), r.name.as_str(), r.function_flags))
        .collect();
    assert_eq!(
        flagged,
        [
            ("IDocument", "Test1", 64),
            ("IDocument", "Test2", 64),
            ("IDocument", "Test3", 64)
        ]
    );
    let quit = core
        .records
        .iter()
        .find(|r| r.owner == "IApplication" && r.name == "Quit")
        .unwrap();
    let online = validate::catalog()["members"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["id"] == quit.catalog_id.as_ref().unwrap().as_str())
        .unwrap();
    assert_eq!(
        online["contract"]["return_contract_state"],
        "not_stated_do_not_infer_void"
    );
    assert_eq!(quit.return_type.leaf.tuple(), "HRESULT");
    // These are generic raw declarations, even when the manual names a class.
    for r in core
        .records
        .iter()
        .filter(|r| r.name == "Application" || r.name == "Parent")
    {
        assert_eq!(r.parameters[0].datatype.leaf.tuple(), "DISPATCH");
        assert_eq!(r.parameters[0].datatype.pointer_depth, 1);
    }
    for r in core
        .records
        .iter()
        .filter(|r| r.invocation_kind == schema::Invocation::PropertyPut)
    {
        assert!(r.parameters[0].name.is_none());
    }
    assert_eq!(
        coverage
            .records
            .iter()
            .filter(|r| r.function_dump_lines.is_none())
            .count(),
        5
    );
    let component = coverage
        .records
        .iter()
        .find(|r| r.owner == "IComponent" && r.name == "GenerateOutput")
        .unwrap();
    assert_eq!(component.cohort, "graph"); // pbError alone is not a complete signature
}

fn rejects(check: fn(&Value), value: &Value, label: &str) {
    assert!(
        std::panic::catch_unwind(|| check(value)).is_err(),
        "accepted mutation: {label}"
    );
}

fn core(value: &Value) {
    checked_core(value);
}

fn declared(value: &Value) {
    let cohort = cohorts::by_id(value["cohort"].as_str().unwrap());
    validate::declarations(value, cohort);
}

fn checked_core(value: &Value) -> schema::Declarations {
    validate::declarations(value, cohorts::by_id("core"))
}
fn manifest(value: &Value) {
    coverage::check(value);
}

// Mutate every scalar and add an unsupported field to every object. The typed
// schema rejects unknown qualification claims even inside leaves/anchors.
fn mutation_walk(value: &Value, root: &Value, path: &str, check: fn(&Value)) -> usize {
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
            rejects(check, &changed, path);
            count += 1;
            for (key, child) in fields {
                count += mutation_walk(child, root, &format!("{path}/{key}"), check);
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
            rejects(check, &changed, path);
            count += 1;
            for (i, child) in items.iter().enumerate() {
                count += mutation_walk(child, root, &format!("{path}/{i}"), check);
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
            rejects(check, &changed, path);
            count += 1;
        }
    }
    count
}

#[test]
fn shared_declaration_schema_rejects_mutated_leaves_and_extra_claims() {
    let value = load("com-core-declarations.json");
    assert!(mutation_walk(&value, &value, "", core) > 5000);
}

#[test]
fn finite_coverage_rejects_mutated_leaves_and_extra_claims() {
    let value = load("com-callable-coverage.json");
    assert!(mutation_walk(&value, &value, "", manifest) > 4000);
}

fn coherent_cases(file: &str, check: fn(&Value), cases: &[fn(&mut Value)]) {
    let original = load(file);
    for (i, case) in cases.iter().enumerate() {
        let mut value = original.clone();
        case(&mut value);
        rejects(check, &value, &format!("coherent case {i}"));
    }
}

#[test]
fn coherent_declaration_mutations_cannot_rewrite_the_source_contract() {
    coherent_cases(
        "com-core-declarations.json",
        core,
        &[
            |v| {
                v["records"].as_array_mut().unwrap().remove(0);
            },
            |v| {
                v["records"][1] = v["records"][0].clone();
            },
            |v| {
                v["records"].as_array_mut().unwrap().swap(3, 4);
            },
            |v| {
                v["records"][0]["owner"] = json!("IOptions");
                v["records"][0]["type_info_index"] = json!(1);
            },
            |v| {
                v["records"][4]["invocation_kind"] = json!("property_get");
                v["records"][4]["invocation_kind_value"] = json!(2);
            },
            |v| {
                let r = v["records"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|r| r["name"] == "Test1")
                    .unwrap();
                r["function_flags"] = json!(0);
            },
            |v| {
                v["records"][4]["parameters"][0]["name"] = json!("value");
                v["records"][4]["parameters"][0]["name_offset"] = json!(64);
            },
            |v| {
                let p = &mut v["records"][5]["parameters"][0];
                p["type"]["pointer_depth"] = json!(1);
                p["type"]["descriptor_chain"]
                    .as_array_mut()
                    .unwrap()
                    .remove(0);
            },
            |v| {
                let p = &mut v["records"][5]["parameters"][0];
                p["type"]["leaf"]["name"] = json!("IDocument");
                p["type"]["leaf"]["type_info_index"] = json!(7);
            },
            |v| {
                v["records"][8]["parameters"]
                    .as_array_mut()
                    .unwrap()
                    .swap(0, 1);
            },
            |v| {
                v["records"][0]["parameters"][0]["parameter_flags"] = json!(1);
                v["records"][0]["parameters"][0]["direction"] = json!(["in"]);
            },
            |v| {
                v["records"][0]["parameters"][0]["default_value"] = json!(false);
            },
            |v| {
                v["records"][13]["catalog_id"] = json!("provisional.com.Application.OpenDocument");
            },
            |v| {
                v["records"][0]["dispatch_member_id"] = json!(-4);
                v["records"][0]["raw_member_id"] = json!("fffffffc");
            },
            |v| {
                v["descriptors"][0]["target_word"] = json!("00000000");
            },
            |v| {
                v["records"][13]
                    .as_object_mut()
                    .unwrap()
                    .remove("catalog_id");
            },
            |v| {
                v["records"][4]["parameters"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("name");
            },
            |v| {
                // Coherently redirect the complete Options interface descriptor to
                // IDocument, including its declared leaf identity.
                let d = v["descriptors"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|d| d["descriptor_offset"] == 104)
                    .unwrap();
                d["target_word"] = json!("000002bc");
                let leaf = &mut v["records"][5]["parameters"][0]["type"]["leaf"];
                leaf["name"] = json!("IDocument");
                leaf["type_info_index"] = json!(7);
            },
            |v| {
                // Both enum accessors receive fully valid but different enum types.
                let records = v["records"].as_array_mut().unwrap();
                for kind in ["property_get", "property_put"] {
                    let replacement = records
                        .iter()
                        .find(|r| {
                            r["owner"] == "IOptions"
                                && r["name"] == "CPPSettings_LibraryType"
                                && r["invocation_kind"] == kind
                        })
                        .unwrap()["parameters"][0]["type"]
                        .clone();
                    let r = records
                        .iter_mut()
                        .find(|r| {
                            r["owner"] == "IOptions"
                                && r["name"] == "CPPSettings_DOMType"
                                && r["invocation_kind"] == kind
                        })
                        .unwrap();
                    r["parameters"][0]["type"] = replacement;
                }
            },
            |v| {
                for (position, index, id) in [(0, 1, 1), (1, 0, 0)] {
                    v["records"][position]["record_index"] = json!(index);
                    v["records"][position]["dispatch_member_id"] = json!(id);
                    v["records"][position]["raw_member_id"] = json!(format!("{id:08x}"));
                }
            },
        ],
    );
}

#[test]
fn coherent_coverage_mutations_cannot_promote_partial_or_missing_records() {
    coherent_cases(
        "com-callable-coverage.json",
        manifest,
        &[
            |v| {
                v["records"].as_array_mut().unwrap().remove(0);
                v["counts"]["adopted_records"] = json!(324);
                v["cohorts"][3]["record_count"] = json!(113);
            },
            |v| {
                v["records"][1] = v["records"][0].clone();
            },
            |v| {
                v["records"].as_array_mut().unwrap().swap(0, 1);
            },
            |v| {
                let r = v["records"]
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .find(|r| r["owner"] == "IComponent" && r["name"] == "GenerateOutput")
                    .unwrap();
                r["cohort"] = json!("generation");
                v["cohorts"][0]["record_count"] = json!(5);
                v["cohorts"][4]["record_count"] = json!(78);
            },
            |v| {
                v["cohorts"][7]["state"] = json!("remaining");
            },
            |v| {
                let r = v["records"].as_array_mut().unwrap().last_mut().unwrap();
                r["function_dump_lines"] = json!([15000, 15043]);
            },
            |v| {
                v["records"][0]["cohort"] = json!("new_unbounded_remainder");
            },
            |v| {
                v["cohorts"][7].as_object_mut().unwrap().remove("fragment");
            },
            |v| {
                v["records"]
                    .as_array_mut()
                    .unwrap()
                    .last_mut()
                    .unwrap()
                    .as_object_mut()
                    .unwrap()
                    .remove("function_dump_lines");
            },
        ],
    );
}
