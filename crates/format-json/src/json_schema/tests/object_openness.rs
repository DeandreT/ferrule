use ir::{Instance, SchemaKind, Value};

use super::*;

#[test]
fn omitted_additional_properties_is_open_and_roundtrips_arbitrary_values()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = import_str(
        r#"{
  "title":"Envelope",
  "type":"object",
  "required":["known"],
  "properties":{
    "known":{"type":"integer"},
    "nested":{
      "type":"object",
      "properties":{"label":{"type":"string"}}
    }
  }
}"#,
    );
    assert!(
        schema
            .dynamic_fields()
            .is_some_and(|dynamic| dynamic.json_any)
    );
    assert!(
        schema
            .child("nested")
            .and_then(SchemaNode::dynamic_fields)
            .is_some_and(|dynamic| dynamic.json_any)
    );

    let input = r#"{
  "known":7,
  "extra":{"array":[1,true,null]},
  "nested":{"label":"x","extension":{"value":2}}
}"#;
    let instance = crate::from_str(input, &schema)?;
    assert_eq!(
        instance.field("known").and_then(Instance::as_scalar),
        Some(&Value::Int(7))
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&crate::to_string(&schema, &instance)?)?,
        serde_json::from_str::<serde_json::Value>(input)?
    );

    let exported: serde_json::Value = serde_json::from_str(&export(&schema))?;
    assert_eq!(exported["additionalProperties"], serde_json::json!({}));
    assert_eq!(
        exported["properties"]["nested"]["additionalProperties"],
        serde_json::json!({})
    );
    assert_eq!(import_str(&export(&schema)), schema);
    Ok(())
}

#[test]
fn explicit_closed_objects_reject_undeclared_input_at_every_native_boundary()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = import_str(
        r#"{
  "title":"Rows",
  "type":"array",
  "items":{
    "type":"object",
    "additionalProperties":false,
    "properties":{
      "id":{"type":"integer"},
      "nested":{
        "type":["object","null"],
        "additionalProperties":false,
        "properties":{"label":{"type":"string"}}
      }
    }
  }
}"#,
    );
    assert!(matches!(
        crate::from_str(r#"[{"id":1,"extra":true}]"#, &schema),
        Err(JsonFormatError::UndeclaredProperty {
            ref object,
            ref property
        }) if object == "Rows" && property == "extra"
    ));
    assert!(matches!(
        crate::from_str(r#"[{"id":1,"nested":{"label":"x","extra":true}}]"#, &schema),
        Err(JsonFormatError::UndeclaredProperty {
            ref property, ..
        }) if property == "extra"
    ));
    assert!(crate::from_str(r#"[{"id":1,"nested":null}]"#, &schema).is_ok());

    assert!(matches!(
        crate::from_lines(r#"{"id":1}
{"id":2,"extra":true}
"#, &schema),
        Err(JsonFormatError::UndeclaredProperty {
            ref object,
            ref property
        }) if object == "Rows" && property == "extra"
    ));
    Ok(())
}

#[test]
fn object_validation_keywords_reject_instead_of_widening() {
    for (schema, keyword) in [
        (
            r#"{"title":"Root","type":"object","patternProperties":{"^x-":{"type":"string"}}}"#,
            "patternProperties",
        ),
        (
            r#"{"title":"Root","type":"object","unevaluatedProperties":false}"#,
            "unevaluatedProperties",
        ),
        (
            r#"{
  "title":"Root",
  "type":"object",
  "properties":{
    "nested":{
      "type":"object",
      "patternProperties":{"^x-":{"type":"string"}}
    }
  }
}"#,
            "patternProperties",
        ),
        (
            r#"{
  "title":"Root",
  "allOf":[
    {"type":"object"},
    {"type":"object","unevaluatedProperties":false}
  ]
}"#,
            "unevaluatedProperties",
        ),
    ] {
        let error = import_str_result(schema).unwrap_err();
        assert!(
            matches!(
                error,
                JsonFormatError::UnsupportedSchemaObject { ref reason, .. }
                    if reason.contains(keyword)
            ),
            "{error}"
        );
    }
}

#[test]
fn all_of_intersects_declared_and_additional_property_domains_exactly() {
    let closed = import_str(
        r#"{
  "title":"Closed",
  "allOf":[
    {
      "type":"object",
      "additionalProperties":false,
      "properties":{
        "kept":{"type":"string"},
        "conflict":{"type":"string"}
      }
    },
    {
      "type":"object",
      "properties":{
        "kept":{"type":"string"},
        "conflict":{"type":"integer"},
        "forbidden":{"type":"boolean"}
      }
    }
  ]
}"#,
    );
    assert!(closed.dynamic_fields().is_none());
    assert!(closed.child("kept").is_some());
    assert!(closed.child("conflict").is_none());
    assert!(closed.child("forbidden").is_none());
    assert!(crate::from_str(r#"{"kept":"yes"}"#, &closed).is_ok());
    for property in ["conflict", "forbidden"] {
        let input = format!(r#"{{"{property}":true}}"#);
        assert!(matches!(
            crate::from_str(&input, &closed),
            Err(JsonFormatError::UndeclaredProperty {
                property: ref rejected,
                ..
            }) if rejected == property
        ));
    }

    let typed_dynamic_intersection = import_str(
        r#"{
  "title":"NoDynamicValues",
  "allOf":[
    {"type":"object","additionalProperties":{"type":"string"}},
    {"type":"object","additionalProperties":{"type":"integer"}}
  ]
}"#,
    );
    assert!(typed_dynamic_intersection.dynamic_fields().is_none());
    assert!(crate::from_str("{}", &typed_dynamic_intersection).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"x":"value"}"#, &typed_dynamic_intersection),
        Err(JsonFormatError::UndeclaredProperty { .. })
    ));
}

#[test]
fn all_of_rejects_required_forbidden_and_open_named_conflicts() {
    for schema in [
        r#"{
  "title":"RequiredForbidden",
  "allOf":[
    {"type":"object","additionalProperties":false},
    {
      "type":"object",
      "required":["value"],
      "properties":{"value":{"type":"string"}}
    }
  ]
}"#,
        r#"{
  "title":"RequiredEmpty",
  "allOf":[
    {
      "type":"object",
      "additionalProperties":false,
      "required":["value"],
      "properties":{"value":{"type":"string"}}
    },
    {
      "type":"object",
      "additionalProperties":false,
      "properties":{"value":{"type":"integer"}}
    }
  ]
}"#,
        r#"{
  "title":"OpenNamedConflict",
  "allOf":[
    {
      "type":"object",
      "properties":{"value":{"type":"string"}}
    },
    {
      "type":"object",
      "properties":{"value":{"type":"integer"}}
    }
  ]
}"#,
        r#"{
  "title":"RequiredDynamicConflict",
  "allOf":[
    {
      "type":"object",
      "required":["value"],
      "additionalProperties":{"type":"string"}
    },
    {
      "type":"object",
      "additionalProperties":{"type":"integer"}
    }
  ]
}"#,
    ] {
        assert!(matches!(
            import_str_result(schema),
            Err(JsonFormatError::UnsupportedSchemaUnion { .. })
        ));
    }
}

#[test]
fn typed_additional_properties_remain_typed_after_open_intersection() {
    let schema = import_str(
        r#"{
  "title":"Metrics",
  "allOf":[
    {
      "type":"object",
      "properties":{"source":{"type":"integer"}}
    },
    {
      "type":"object",
      "additionalProperties":{"type":"integer"}
    }
  ]
}"#,
    );
    assert!(matches!(
        schema.dynamic_fields().map(|dynamic| &dynamic.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));
    assert!(matches!(
        schema.child("source").map(|source| &source.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));
    assert!(crate::from_str(r#"{"source":1,"count":2}"#, &schema).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"count":"two"}"#, &schema),
        Err(JsonFormatError::Shape { .. })
    ));
}

#[test]
fn all_of_object_openness_is_independent_of_branch_order() {
    let branches = [
        r#"{"type":"object","properties":{"value":{"type":"string"}}}"#,
        r#"{"type":"object","properties":{"value":{"type":"integer"}}}"#,
        r#"{"type":"object","additionalProperties":false}"#,
    ];
    let permutations = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let mut expected = None;
    for order in permutations {
        let schema = import_str(&format!(
            r#"{{"title":"Root","allOf":[{},{},{}]}}"#,
            branches[order[0]], branches[order[1]], branches[order[2]]
        ));
        assert!(schema.dynamic_fields().is_none());
        assert!(schema.child("value").is_none());
        assert!(crate::from_str("{}", &schema).is_ok());
        assert!(matches!(
            crate::from_str(r#"{"value":"x"}"#, &schema),
            Err(JsonFormatError::UndeclaredProperty { .. })
        ));
        if let Some(expected) = &expected {
            assert_eq!(&schema, expected);
        } else {
            expected = Some(schema);
        }
    }
}

#[test]
fn all_of_dynamic_closure_is_independent_of_branch_order() {
    let branches = [
        r#"{"type":"object","properties":{"value":{"type":"string"}}}"#,
        r#"{"type":"object","properties":{"value":{"type":"integer"}}}"#,
        r#"{"type":"object","additionalProperties":{"type":"string"}}"#,
        r#"{"type":"object","additionalProperties":{"type":"integer"}}"#,
    ];
    for order in [[0, 1, 2, 3], [0, 2, 1, 3], [3, 1, 2, 0], [2, 3, 0, 1]] {
        let schema = import_str(&format!(
            r#"{{"title":"Root","allOf":[{},{},{},{}]}}"#,
            branches[order[0]], branches[order[1]], branches[order[2]], branches[order[3]]
        ));
        assert!(schema.dynamic_fields().is_none());
        assert!(schema.child("value").is_none());
        assert!(crate::from_str("{}", &schema).is_ok());
        assert!(matches!(
            crate::from_str(r#"{"value":1}"#, &schema),
            Err(JsonFormatError::UndeclaredProperty { .. })
        ));
    }
}
