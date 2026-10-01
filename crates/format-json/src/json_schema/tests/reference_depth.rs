use ir::{ScalarType, SchemaKind};
use serde_json::{Map, Value, json};

use super::import_str_result;
use crate::{JsonFormatError, json_schema};

fn definitions(count: usize, terminal: Value) -> Value {
    let mut definitions = Map::new();
    for index in 0..count {
        let value = if index + 1 == count {
            terminal.clone()
        } else {
            json!({"$ref": format!("#/$defs/N{}", index + 1)})
        };
        definitions.insert(format!("N{index}"), value);
    }
    Value::Object(definitions)
}

fn root_chain(count: usize, terminal: Value) -> String {
    json!({"$ref":"#/$defs/N0", "$defs":definitions(count, terminal)}).to_string()
}

fn assert_reference_limit(result: Result<ir::SchemaNode, JsonFormatError>) {
    assert!(
        matches!(
            result,
            Err(JsonFormatError::SchemaResourceLimit {
                kind: "reference depth",
                limit: 64
            })
        ),
        "{result:?}"
    );
}

fn import_small_stack(text: String) -> Result<ir::SchemaNode, JsonFormatError> {
    std::thread::Builder::new()
        .stack_size(2 * 1024 * 1024)
        .spawn(move || json_schema::import_str(&text))
        .unwrap()
        .join()
        .unwrap()
}

fn union_chain(keyword: &str, count: usize, terminal: Value, other: Value) -> String {
    let mut root = json!({
        "title":"Union",
        "$defs": definitions(count, terminal),
    });
    root[keyword] = json!([{"$ref":"#/$defs/N0"}, other]);
    root.to_string()
}

#[test]
fn flat_local_chain_accepts_64_hops_without_recursive_stack_growth() {
    let text = root_chain(64, json!({"title":"End","type":"string","minLength":2}));
    let imported = import_small_stack(text).unwrap();
    assert_eq!(imported.name, "End");
    assert_eq!(
        imported.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    );
    assert_eq!(imported.string_length_range.unwrap().minimum(), 2);
}

#[test]
fn union_classifiers_bound_null_scalar_and_array_reference_paths() {
    let cases = [
        ("oneOf", json!({"type":"string"}), json!({"type":"null"})),
        ("anyOf", json!({"type":"string"}), json!({"type":"null"})),
        (
            "oneOf",
            json!({"type":"string","const":"A"}),
            json!({"type":"string","const":"B"}),
        ),
        (
            "anyOf",
            json!({"type":"array","items":{"type":"integer"},"minItems":1}),
            json!({"type":"array","items":{"type":"integer"},"minItems":1,"maxItems":2}),
        ),
    ];
    for (keyword, terminal, other) in cases {
        let accepted =
            import_small_stack(union_chain(keyword, 64, terminal.clone(), other.clone()));
        assert!(accepted.is_ok(), "{keyword}: {accepted:?}");
        assert_reference_limit(import_small_stack(union_chain(
            keyword, 65, terminal, other,
        )));
    }
}

#[test]
fn over_limit_ref_cannot_be_ignored_as_a_structured_union_branch() {
    let text = union_chain(
        "anyOf",
        65,
        json!({"type":"object","properties":{"Value":{"type":"string"}}}),
        json!({"const":"A"}),
    );
    assert_reference_limit(import_small_stack(text));
}

#[test]
fn flat_local_chain_rejects_65_hops_in_title_and_parser_paths() {
    let title_chain = root_chain(65, json!({"title":"End","type":"string"}));
    assert_reference_limit(json_schema::import_str(&title_chain));

    let mut parser_chain: Value = serde_json::from_str(&title_chain).unwrap();
    parser_chain["title"] = json!("Root");
    assert_reference_limit(json_schema::import_str(&parser_chain.to_string()));
    assert_reference_limit(import_str_result(&parser_chain.to_string()));
}

#[test]
fn nested_composition_and_contains_predicate_share_the_local_chain_limit() {
    let defs = definitions(64, json!({"type":"string","const":"ok"}));
    let nested = json!({
        "title":"Envelope",
        "type":"object",
        "allOf":[
            {"type":"object","properties":{"Value":{"$ref":"#/$defs/N0"}}},
            {"type":"object","properties":{"Items":{
                "type":"array", "items":{"type":"string"},
                "contains":{"$ref":"#/$defs/N0"}
            }}}
        ],
        "$defs":defs
    });
    let imported = json_schema::import_str(&nested.to_string()).unwrap();
    assert_eq!(
        imported.child("Value").unwrap().fixed.as_deref(),
        Some("ok")
    );
    assert!(imported.child("Items").unwrap().json_contains.is_some());

    let mut too_deep = nested;
    too_deep["$defs"] = definitions(65, json!({"type":"string","const":"ok"}));
    assert_reference_limit(json_schema::import_str(&too_deep.to_string()));
}

#[test]
fn cyclic_legacy_fallback_and_error_cleanup_remain_stable() {
    let cyclic = root_chain(64, json!({"$ref":"#/$defs/N0"}));
    let imported = json_schema::import_str(&cyclic).unwrap();
    assert_eq!(imported.name, "root");
    assert_eq!(
        imported.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    );

    let invalid: Value = serde_json::from_str(&root_chain(
        3,
        json!({"type":"string", "minLength":"invalid"}),
    ))
    .unwrap();
    let mut active_refs = vec!["#/outer".to_string()];
    assert!(json_schema::parse("Value", &invalid, &invalid, &mut active_refs).is_err());
    assert_eq!(active_refs, ["#/outer"]);
}
