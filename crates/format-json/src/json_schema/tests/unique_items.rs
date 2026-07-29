use ir::{Instance, ScalarType, SchemaNode, Value};

use super::{export, import_str, import_str_result};
use crate::JsonFormatError;
use crate::json_schema::unique_items::{UniqueItemsValidationError, validate_unique_json_items};

#[test]
fn direct_unique_arrays_execute_and_roundtrip() -> Result<(), JsonFormatError> {
    let schema = import_str(
        r#"{
  "title":"Values",
  "type":"array",
  "uniqueItems":true,
  "items":{}
}"#,
    );
    assert!(schema.json_unique_items);
    assert!(crate::from_str(r#"[1,"1",true,null,{"a":1},[1,2]]"#, &schema).is_ok());
    assert!(matches!(
        crate::from_str(r#"[1,1.0]"#, &schema),
        Err(JsonFormatError::UniqueItemsMismatch {
            first_index: 1,
            duplicate_index: 2,
            ..
        })
    ));
    assert!(matches!(
        crate::from_str("\u{feff}[1,1.0]", &schema),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
    assert!(matches!(
        crate::from_str(r#"[{"a":1,"b":2},{"b":2.0,"a":1.0}]"#, &schema),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
    assert!(crate::from_str(r#"[[1,2],[2,1]]"#, &schema).is_ok());

    let rendered: serde_json::Value = serde_json::from_str(&export(&schema))?;
    assert_eq!(rendered.get("uniqueItems"), Some(&serde_json::json!(true)));
    assert!(
        rendered
            .get("items")
            .and_then(|items| items.get("uniqueItems"))
            .is_none()
    );
    assert_eq!(import_str(&export(&schema)), schema);
    Ok(())
}

#[test]
fn refs_all_of_nullable_and_array_any_of_preserve_exact_semantics() {
    let all_of = import_str(
        r#"{
  "title":"Rows",
  "allOf":[
    {"uniqueItems":true},
    {"type":"array","items":{"type":"string"}},
    {"uniqueItems":false}
  ]
}"#,
    );
    assert!(all_of.json_unique_items);

    let referenced = import_str(
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "title":"Rows",
  "$defs":{"Rows":{"type":"array","items":{"type":"integer"}}},
  "$ref":"#/$defs/Rows",
  "uniqueItems":true
}"##,
    );
    assert!(referenced.json_unique_items);

    let nullable = import_str(
        r#"{
  "title":"Rows",
  "anyOf":[
    {"type":"array","uniqueItems":true,"items":{"type":"string"}},
    {"type":"null"}
  ]
}"#,
    );
    assert!(nullable.container_nullable);
    assert!(nullable.json_unique_items);
    assert!(crate::from_str("null", &nullable).is_ok());
    assert!(matches!(
        crate::from_str(r#"["same","same"]"#, &nullable),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));

    for branches in [
        r#"
    {"type":"array","uniqueItems":true,"items":{"type":"string"}},
    {"type":"array","uniqueItems":false,"items":{"type":"string"}}"#,
        r#"
    {"type":"array","uniqueItems":false,"items":{"type":"string"}},
    {"type":"array","uniqueItems":true,"items":{"type":"string"}}"#,
    ] {
        let schema = import_str(&format!(
            r#"{{"title":"Rows","anyOf":[{branches}
  ]}}"#
        ));
        assert!(!schema.json_unique_items);
        assert!(crate::from_str(r#"["same","same"]"#, &schema).is_ok());
    }

    let all_unique = import_str(
        r#"{
  "title":"Rows",
  "anyOf":[
    {"type":"array","uniqueItems":true,"maxItems":2,"items":{"type":"string"}},
    {"type":"array","uniqueItems":true,"minItems":3,"items":{"type":"string"}}
  ]
}"#,
    );
    assert!(all_unique.json_unique_items);

    let outer_unique = import_str(
        r#"{
  "title":"Rows",
  "uniqueItems":true,
  "anyOf":[
    {"type":"array","uniqueItems":true,"items":{"type":"string"}},
    {"type":"array","uniqueItems":false,"items":{"type":"string"}}
  ]
}"#,
    );
    assert!(outer_unique.json_unique_items);
}

#[test]
fn malformed_incompatible_and_nested_unique_items_reject() {
    for (schema, expected) in [
        (
            r#"{"title":"Rows","type":"array","uniqueItems":"true","items":{}}"#,
            "must be a boolean",
        ),
        (
            r#"{"title":"Value","uniqueItems":true}"#,
            "without a concrete array",
        ),
        (
            r#"{
  "title":"Rows",
  "type":"array",
  "uniqueItems":true,
  "items":{"type":"array","items":{"type":"string"}}
}"#,
            "distinct wrapper levels",
        ),
        (
            r#"{
  "title":"Rows",
  "type":"array",
  "items":{"type":"array","uniqueItems":true,"items":{"type":"string"}}
}"#,
            "distinct wrapper levels",
        ),
    ] {
        assert!(
            matches!(
                import_str_result(schema),
                Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
                    if reason.contains(expected)
            ),
            "{schema}"
        );
    }
    assert!(import_str_result(r#"{"title":"Value","type":"string","uniqueItems":false}"#).is_ok());
    let scalar = import_str(r#"{"title":"Value","type":"string","uniqueItems":true}"#);
    assert!(!scalar.json_unique_items);
    let object = import_str(r#"{"title":"Value","type":"object","uniqueItems":true}"#);
    assert!(!object.json_unique_items);
    let scalar_all_of = import_str(
        r#"{
  "title":"Value",
  "allOf":[{"uniqueItems":true},{"type":"string"}]
}"#,
    );
    assert!(!scalar_all_of.json_unique_items);
}

#[test]
fn normalized_output_and_json_lines_enforce_uniqueness() {
    let schema = import_str(
        r#"{
  "title":"Values",
  "type":"array",
  "uniqueItems":true,
  "items":{"type":"number"}
}"#,
    );
    let duplicate = Instance::Repeated(vec![
        Instance::Scalar(Value::Int(1)),
        Instance::Scalar(Value::Float(1.0)),
    ]);
    assert!(matches!(
        crate::to_string(&schema, &duplicate),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
    assert!(matches!(
        crate::from_lines("1\n1.0\n", &schema),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
    assert!(matches!(
        crate::from_lines("9007199254740993\n9007199254740993.0\n", &schema),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
    assert!(crate::from_lines("9007199254740992.0\n9007199254740993.0\n", &schema).is_ok());
    assert!(matches!(
        crate::to_lines(&schema, &duplicate),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
    assert!(matches!(
        crate::from_lines("1\n2\n", &schema),
        Ok(Instance::Repeated(ref items)) if items.len() == 2
    ));
}

#[test]
fn canonical_validator_is_exact_and_bounded() -> Result<(), serde_json::Error> {
    let schema = import_str(
        r#"{
  "title":"Values",
  "type":"array",
  "uniqueItems":true,
  "items":{}
}"#,
    );
    let distinct_large: Vec<serde_json::Value> =
        serde_json::from_str("[9007199254740992,9007199254740993]")?;
    assert!(validate_unique_json_items(&distinct_large).is_ok());
    assert!(matches!(
        crate::json_schema::unique_items::validate_raw_json_unique_items(
            &schema,
            "[9007199254740993,9007199254740993.0]"
        ),
        Err(JsonFormatError::UniqueItemsMismatch {
            first_index: 1,
            duplicate_index: 2,
            ..
        })
    ));
    assert!(crate::from_str("[9007199254740992.0,9007199254740993.0]", &schema).is_ok());
    assert!(matches!(
        crate::json_schema::unique_items::validate_raw_json_unique_items(
            &schema,
            "[-0e999999999999999999999999999999999,0]"
        ),
        Err(JsonFormatError::UniqueItemsMismatch { .. })
    ));
    let equal_normalized: Vec<serde_json::Value> = serde_json::from_str("[1,1.0]")?;
    assert!(matches!(
        validate_unique_json_items(&equal_normalized),
        Err(UniqueItemsValidationError::Duplicate { .. })
    ));
    let too_many = vec![serde_json::Value::Null; ir::MAX_JSON_UNIQUE_ITEMS + 1];
    assert!(matches!(
        validate_unique_json_items(&too_many),
        Err(UniqueItemsValidationError::Limit {
            resource: "array items",
            max: ir::MAX_JSON_UNIQUE_ITEMS
        })
    ));
    Ok(())
}

#[test]
fn export_rejects_corrupted_nested_unique_items_metadata() {
    let mut invalid = SchemaNode::scalar("Value", ScalarType::String);
    invalid.json_unique_items = true;
    let root = SchemaNode::group("Root", vec![invalid]);
    assert!(matches!(
        super::super::export(&root),
        Err(JsonFormatError::InvalidUniqueItemsMetadata { .. })
    ));
}
