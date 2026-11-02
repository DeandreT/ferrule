use std::panic::{AssertUnwindSafe, catch_unwind};

use super::*;

fn rejected(value: &Value) {
    assert!(catch_unwind(AssertUnwindSafe(|| validate(value))).is_err());
}

fn coherent(edit: impl FnOnce(&mut Value)) {
    let mut value = load(FILE);
    edit(&mut value);
    structure(&value);
    assert!(catch_unwind(AssertUnwindSafe(|| frozen(&value))).is_err());
}

fn object_paths(value: &Value, prefix: String, out: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            out.push(prefix.clone());
            for (key, child) in fields {
                object_paths(child, format!("{prefix}/{key}"), out);
            }
        }
        Value::Array(children) => {
            for (i, child) in children.iter().enumerate() {
                object_paths(child, format!("{prefix}/{i}"), out);
            }
        }
        _ => {}
    }
}

#[test]
fn unknown_and_missing_fields_are_rejected_at_every_object_depth() {
    let original = load(FILE);
    let mut paths = Vec::new();
    object_paths(&original, String::new(), &mut paths);
    for path in paths {
        let mut value = original.clone();
        value
            .pointer_mut(&path)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert("unsupported".into(), json!(true));
        rejected(&value);
        for key in original.pointer(&path).unwrap().as_object().unwrap().keys() {
            let mut value = original.clone();
            value
                .pointer_mut(&path)
                .unwrap()
                .as_object_mut()
                .unwrap()
                .remove(key);
            rejected(&value);
        }
    }
}

fn scalar_paths(value: &Value, prefix: String, out: &mut Vec<String>) {
    match value {
        Value::Object(fields) => {
            for (key, child) in fields {
                scalar_paths(child, format!("{prefix}/{key}"), out);
            }
        }
        Value::Array(children) if !children.is_empty() => {
            for (i, child) in children.iter().enumerate() {
                scalar_paths(child, format!("{prefix}/{i}"), out);
            }
        }
        _ => out.push(prefix),
    }
}

#[test]
fn every_scalar_null_and_empty_category_is_frozen() {
    let original = load(FILE);
    let mut paths = Vec::new();
    scalar_paths(&original, String::new(), &mut paths);
    for path in paths {
        let mut value = original.clone();
        let target = value.pointer_mut(&path).unwrap();
        *target = match target {
            Value::Bool(b) => json!(!*b),
            Value::Number(_) => json!(-1),
            Value::Array(_) => json!([null]),
            _ => json!("__mutated__"),
        };
        assert!(
            catch_unwind(AssertUnwindSafe(|| validate(&value))).is_err(),
            "unchecked leaf: {path}"
        );
    }
}

#[test]
fn coherently_missing_owner_or_overload_fails_the_source_oracle() {
    coherent(|v| {
        v["owners"].as_array_mut().unwrap().remove(2);
        for (field, n) in [
            ("owner_count", 15),
            ("direct_public_callables", 29),
            ("new_declarations", 27),
            ("constructors", 23),
            ("parameters", 41),
            ("default_arguments", 8),
        ] {
            v["scope"][field] = json!(n);
        }
        v["scope"]["language_owner_counts"]["C++"] = json!(9);
    });
    coherent(|v| {
        v["owners"][4]["members"].as_array_mut().unwrap().remove(1);
        for (field, n) in [
            ("direct_public_callables", 29),
            ("new_declarations", 27),
            ("constructors", 23),
            ("parameters", 41),
            ("default_arguments", 8),
        ] {
            v["scope"][field] = json!(n);
        }
    });
}

#[test]
fn coherent_overload_and_parameter_reordering_fails_the_source_oracle() {
    coherent(|v| {
        let m = v["owners"][4]["members"].as_array_mut().unwrap();
        m.swap(0, 1);
        m[0]["ordinal"] = json!(0);
        m[1]["ordinal"] = json!(1);
    });
    coherent(|v| {
        let m = &mut v["owners"][6]["members"][0];
        m["parameters"].as_array_mut().unwrap().swap(0, 1);
        m["parameters"][0]["index"] = json!(0);
        m["parameters"][1]["index"] = json!(1);
        m["declaration"] = json!(
            "OutOfRangeException(const string_type& targetTypeName, const string_type& sourceTypeName, const string_type& value)"
        );
    });
}

#[test]
fn coherent_default_and_qualifier_changes_fail_the_source_oracle() {
    coherent(|v| {
        let m = &mut v["owners"][0]["members"][0];
        m["parameters"][1]["default_literal"] = json!("nullptr");
        m["declaration"] =
            json!("Error(const string_type& message, const std::exception* inner = nullptr)");
    });
    coherent(|v| {
        let m = &mut v["owners"][0]["members"][1];
        m["qualifiers"] = json!([]);
        m["declaration"] = json!("~Error()");
    });
    coherent(|v| {
        let m = &mut v["owners"][1]["members"][2];
        m["qualifiers"] = json!([]);
        m["declaration"] = json!("string_type message()");
    });
}

#[test]
fn coherent_owner_spelling_or_base_rewrites_are_not_aliases() {
    coherent(|v| {
        let o = &mut v["owners"][2];
        o["source_name"] = json!("altova::InvalidArgumentsException");
        o["exact_name_match"] = json!(true);
        o["relationship"] = json!("exact_name_match");
        o["class_fact"]["declaration"] =
            json!("class InvalidArgumentsException : public Exception");
        o["members"][0]["name"] = json!("InvalidArgumentsException");
        o["members"][0]["declaration"] = json!(
            "InvalidArgumentsException(const string_type& message, const std::exception* inner = 0)"
        );
    });
    coherent(|v| {
        let o = &mut v["owners"][12];
        o["class_fact"]["base_literal"] = json!("System.Exception");
        o["class_fact"]["declaration"] =
            json!("public class DataSourceUnavailableException : System.Exception");
        o["members"][1]["parameters"][1]["type_literal"] = json!("System.Exception");
        o["members"][1]["declaration"] =
            json!("public DataSourceUnavailableException(string message, System.Exception inner)");
    });
    coherent(|v| {
        let o = &mut v["owners"][15];
        o["class_fact"]["base_literal"] = json!("Exception");
        o["class_fact"]["declaration"] =
            json!("public class TargetInstanceUnavailableException extends Exception");
    });
}

#[test]
fn unicode_alternatives_attributes_imports_and_source_anchors_are_not_optional() {
    coherent(|v| {
        v["aliases"][0]["alternatives"]
            .as_array_mut()
            .unwrap()
            .remove(0);
    });
    coherent(|v| {
        v["owners"][10]["class_fact"]["attributes"] = json!([]);
    });
    coherent(|v| {
        v["sources"][2]["imports"].as_array_mut().unwrap().remove(1);
    });
    coherent(|v| {
        v["sources"][0]["sha256"] = json!("0".repeat(64));
        v["owners"][0]["members"][0]["source_lines"] = json!([33, 33]);
    });
    coherent(|v| {
        v["owners"][0]["members"][3]["observation_ref"] =
            json!("installed.java.source-instance-unavailable");
    });
}

#[test]
fn existing_declarations_are_referenced_not_duplicated() {
    let mut v = load(FILE);
    v["owners"][0]["members"][3]["return_type_literal"] = json!("const std::exception*");
    rejected(&v);
    let mut v = load(FILE);
    v["owners"][14]["class_fact"]["base_literal"] = json!("RuntimeException");
    rejected(&v);
    let mut artifacts = load("artifact-observations.json");
    artifacts["installed_declarations"][0]["return_type"] = json!("std::exception");
    assert!(
        catch_unwind(|| joins(
            &load(FILE),
            &load("supporting.json"),
            &artifacts,
            &load("schema-wrappers.json")
        ))
        .is_err()
    );
    let mut artifacts = load("artifact-observations.json");
    artifacts["installed_declarations"][1]["declared_base"] = json!("Exception");
    assert!(
        catch_unwind(|| joins(
            &load(FILE),
            &load("supporting.json"),
            &artifacts,
            &load("schema-wrappers.json")
        ))
        .is_err()
    );
}

#[test]
fn source_only_facts_cannot_become_binding_or_behavior_guarantees() {
    for field in [
        "binding",
        "abi",
        "ownership",
        "lifetime",
        "returned_message",
        "inner_ownership",
        "runtime_errors",
        "defaults",
        "dependencies",
        "language_versions",
        "authenticode_trust",
    ] {
        let mut v = load(FILE);
        v["qualification"][field] = json!("qualified");
        rejected(&v);
    }
    for (path, value) in [
        ("/inventory_complete", json!(true)),
        ("/status", json!("supported")),
        ("/applicability", json!("all")),
        ("/runtime_proof", json!(true)),
        ("/execution_evidence", json!(["source inventory"])),
        ("/execution_receipts", json!(["source inventory"])),
        (
            "/qualification/rule",
            json!("The declared throw() guarantees no runtime errors."),
        ),
        (
            "/aliases/0/scope",
            json!("The compiled string_type is std::wstring."),
        ),
        (
            "/owners/14/members/0/java_throws",
            json!(["RuntimeException"]),
        ),
        ("/owners/0/fields", json!(["injected_field"])),
        ("/scope/excluded_other_public_owners", json!([])),
    ] {
        let mut v = load(FILE);
        *v.pointer_mut(path).unwrap() = value;
        rejected(&v);
    }
}
