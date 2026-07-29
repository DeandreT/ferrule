use ir::{Instance, Value};

use super::*;

#[test]
fn ordinary_required_properties_distinguish_absence_from_explicit_null()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = import_str(
        r#"{
  "title":"Order",
  "type":"object",
  "additionalProperties":false,
  "required":["id","note"],
  "properties":{
    "id":{"type":"integer"},
    "note":{"type":["string","null"]},
    "optional":{"type":"string"}
  }
}"#,
    );
    assert_eq!(schema.required_fields(), ["id", "note"]);

    for missing in [r#"{"note":null}"#, r#"{"id":7}"#, "{}"] {
        assert!(matches!(
            crate::from_str(missing, &schema),
            Err(JsonFormatError::MissingRequiredProperty { .. })
        ));
    }
    let instance = crate::from_str(r#"{"id":7,"note":null}"#, &schema)?;
    assert_eq!(
        instance.field("note").and_then(Instance::as_scalar),
        Some(&Value::json_null())
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&crate::to_string(&schema, &instance)?)?,
        serde_json::json!({"id": 7, "note": null})
    );

    let missing_output = Instance::Group(vec![
        ("id".into(), Instance::Scalar(Value::Null)),
        (
            "note".into(),
            Instance::Scalar(Value::String("present".into())),
        ),
    ]);
    assert!(matches!(
        crate::to_string(&schema, &missing_output),
        Err(JsonFormatError::MissingRequiredProperty { ref property, .. })
            if property == "id"
    ));
    assert_eq!(import_str(&export(&schema)), schema);
    Ok(())
}

#[test]
fn required_runtime_named_properties_work_for_open_objects()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = import_str(
        r#"{
  "title":"Headers",
  "type":"object",
  "required":["x-correlation-id"],
  "additionalProperties":true
}"#,
    );
    assert_eq!(schema.required_fields(), ["x-correlation-id"]);
    assert!(schema.dynamic_fields().is_some());
    assert!(matches!(
        crate::from_str("{}", &schema),
        Err(JsonFormatError::MissingRequiredProperty { .. })
    ));
    let instance = crate::from_str(r#"{"x-correlation-id":"run-7","x-attempt":2}"#, &schema)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&crate::to_string(&schema, &instance)?)?,
        serde_json::json!({"x-correlation-id": "run-7", "x-attempt": 2})
    );
    assert_eq!(import_str(&export(&schema)), schema);
    Ok(())
}

#[test]
fn all_of_unions_required_properties_and_preserves_nested_array_items()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = import_str(
        r#"{
  "title":"Envelope",
  "type":"object",
  "required":["batch"],
  "properties":{
    "batch":{
      "type":"array",
      "items":{
        "type":"object",
        "additionalProperties":false,
        "required":["id"],
        "properties":{"id":{"type":"integer"},"label":{"type":"string"}}
      }
    }
  },
  "allOf":[
    {
      "type":"object",
      "required":["source"],
      "properties":{"source":{"type":"string"}}
    },
    {
      "type":"object",
      "required":["version"],
      "properties":{"version":{"type":"integer"}}
    }
  ]
}"#,
    );
    assert_eq!(schema.required_fields(), ["batch", "source", "version"]);
    assert_eq!(schema.child("batch").unwrap().required_fields(), ["id"]);
    assert!(
        crate::from_str(
            r#"{"batch":[{"id":1}],"source":"host","version":2}"#,
            &schema
        )
        .is_ok()
    );
    for input in [
        r#"{"batch":[{"id":1}],"source":"host"}"#,
        r#"{"batch":[{}],"source":"host","version":2}"#,
    ] {
        assert!(matches!(
            crate::from_str(input, &schema),
            Err(JsonFormatError::MissingRequiredProperty { .. })
        ));
    }
    assert_eq!(import_str(&export(&schema)), schema);
    Ok(())
}

#[test]
fn malformed_or_unrepresentable_required_declarations_reject_actionably() {
    for (schema, expected) in [
        (
            r#"{"title":"Bad","type":"object","properties":{},"required":"id"}"#,
            "must be an array",
        ),
        (
            r#"{"title":"Bad","type":"object","properties":{},"required":[""]}"#,
            "non-empty property names",
        ),
        (
            r#"{"title":"Bad","type":"object","properties":{"id":{"type":"integer"}},"required":["id","id"]}"#,
            "must be unique",
        ),
        (
            r#"{"title":"Bad","type":"object","additionalProperties":false,"properties":{},"required":["missing"]}"#,
            "must identify declared properties",
        ),
        (
            r#"{"title":"Bad","required":["id"]}"#,
            "without an object type or properties",
        ),
    ] {
        let error = import_str_result(schema).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}
