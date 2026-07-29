use ir::{ScalarType, ScalarTypeSet, SchemaKind, Value};

use super::*;

#[test]
fn flat_nullable_object_composition_executes_and_roundtrips() {
    let schema = import_str(
        r#"{
  "title":"Event",
  "oneOf":[
    {"type":"null"},
    {
      "title":"created",
      "type":"object",
      "additionalProperties":false,
      "required":["kind","id"],
      "properties":{
        "kind":{"type":"string","const":"created"},
        "id":{"type":"integer"}
      }
    },
    {
      "title":"deleted",
      "type":"object",
      "additionalProperties":false,
      "required":["kind","reason"],
      "properties":{
        "kind":{"type":"string","const":"deleted"},
        "reason":{"type":"string"}
      }
    }
  ]
}"#,
    );
    assert!(schema.container_nullable);
    assert!(matches!(schema.kind, SchemaKind::Group { .. }));
    assert_eq!(schema.alternatives().len(), 2);

    for input in [
        "null",
        r#"{"kind":"created","id":7}"#,
        r#"{"kind":"deleted","reason":"duplicate"}"#,
    ] {
        let instance = crate::from_str(input, &schema).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &crate::to_string(&schema, &instance).unwrap()
            )
            .unwrap(),
            serde_json::from_str::<serde_json::Value>(input).unwrap()
        );
    }
    assert!(matches!(
        crate::from_str(r#"{"kind":"other"}"#, &schema),
        Err(JsonFormatError::NoMatchingAlternative { .. })
    ));
    assert_eq!(import_str(&export(&schema)), schema);
}

#[test]
fn flat_nullable_array_any_of_uses_the_exact_superset_item_domain() {
    let schema = import_str(
        r#"{
  "title":"Identifiers",
  "anyOf":[
    {"type":"array","items":{"type":"string"}},
    {"type":"null"},
    {"type":"array","items":{"type":["string","integer"]}}
  ]
}"#,
    );
    assert!(schema.repeating);
    assert!(schema.container_nullable);
    let Some(expected) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
        panic!("test union must contain two scalar types");
    };
    assert_eq!(schema.kind, SchemaKind::ScalarUnion { types: expected });

    let null = crate::from_str("null", &schema).unwrap();
    assert_eq!(null.as_scalar(), Some(&Value::json_null()));
    let values = crate::from_str(r#"["external",17]"#, &schema).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&crate::to_string(&schema, &values).unwrap())
            .unwrap(),
        serde_json::json!(["external", 17])
    );
    assert!(matches!(
        crate::from_str("[true]", &schema),
        Err(JsonFormatError::Shape { .. })
    ));
    assert_eq!(import_str(&export(&schema)), schema);
}

#[test]
fn nested_scalar_union_and_repeated_null_branches_canonicalize_exactly() {
    let nested = import_str(
        r#"{
  "title":"Identifier",
  "anyOf":[
    {"type":"null"},
    {"anyOf":[{"type":"string"},{"type":"integer"}]}
  ]
}"#,
    );
    assert!(nested.nullable);
    let Some(expected) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
        panic!("test union must contain two scalar types");
    };
    assert_eq!(nested.kind, SchemaKind::ScalarUnion { types: expected });
    for input in ["null", r#""external""#, "17"] {
        assert!(crate::from_str(input, &nested).is_ok(), "{input}");
    }
    assert_eq!(import_str(&export(&nested)), nested);

    let repeated_null = import_str(
        r#"{
  "title":"MaybeText",
  "anyOf":[
    {"type":"null"},
    {"type":"string"},
    {"type":"null"}
  ]
}"#,
    );
    assert!(repeated_null.nullable);
    assert!(crate::from_str("null", &repeated_null).is_ok());
    assert!(crate::from_str(r#""text""#, &repeated_null).is_ok());
    assert_eq!(import_str(&export(&repeated_null)), repeated_null);
}

#[test]
fn exclusive_nested_null_overlap_is_removed_without_widening() {
    let schema = import_str(
        r#"{
  "title":"NonNullText",
  "oneOf":[
    {"type":"null"},
    {"anyOf":[{"type":"string"},{"type":"null"}]}
  ]
}"#,
    );
    assert!(!schema.nullable);
    assert!(crate::from_str(r#""text""#, &schema).is_ok());
    assert!(matches!(
        crate::from_str("null", &schema),
        Err(JsonFormatError::Shape { .. })
    ));
    assert_eq!(import_str(&export(&schema)), schema);
}

#[test]
fn exclusive_null_with_an_unconstrained_branch_rejects_actionably() {
    let error = import_str_result(
        r#"{
  "title":"AnyNonNull",
  "oneOf":[
    {},
    {"type":"null"}
  ]
}"#,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("overlaps an unconstrained branch on null")
    );
}
