use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};

use super::{export, import_str, import_str_result};
use crate::JsonFormatError;

#[test]
fn scalar_constants_import_validate_export_and_roundtrip() -> Result<(), JsonFormatError> {
    let schema = import_str(
        r#"{
  "title":"Constants",
  "type":"object",
  "additionalProperties":false,
  "properties":{
    "text":{"type":"string","const":"ready"},
    "count":{"type":"integer","enum":[7]},
    "ratio":{"type":"number","const":1.25},
    "enabled":{"const":true},
    "items":{"type":"array","items":{"type":"string","const":"x"}}
  }
}"#,
    );
    let SchemaKind::Group { children, .. } = &schema.kind else {
        panic!("constants should import as object children");
    };
    assert_eq!(
        children
            .iter()
            .map(|child| (child.name.as_str(), child.fixed.as_deref()))
            .collect::<Vec<_>>(),
        [
            ("text", Some("ready")),
            ("count", Some("7")),
            ("ratio", Some("1.25")),
            ("enabled", Some("true")),
            ("items", Some("x")),
        ]
    );

    let input = r#"{"text":"ready","count":7,"ratio":1.25,"enabled":true,"items":["x","x"]}"#;
    let instance = crate::from_str(input, &schema)?;
    let rendered: serde_json::Value = serde_json::from_str(&crate::to_string(&schema, &instance)?)?;
    assert_eq!(rendered, serde_json::from_str::<serde_json::Value>(input)?);
    assert!(matches!(
        crate::from_str(r#"{"text":"wrong"}"#, &schema),
        Err(JsonFormatError::ConstantMismatch { ref name, .. }) if name == "text"
    ));
    assert!(matches!(
        crate::from_str(r#"{"items":["x","wrong"]}"#, &schema),
        Err(JsonFormatError::ConstantMismatch { ref name, .. }) if name == "items"
    ));
    assert!(crate::from_str("{}", &schema).is_ok());

    let exported = export(&schema);
    let exported_value: serde_json::Value = serde_json::from_str(&exported)?;
    assert_eq!(
        exported_value.pointer("/properties/count/const"),
        Some(&serde_json::json!(7))
    );
    assert!(exported_value.pointer("/properties/count/enum").is_none());
    assert_eq!(import_str(&exported), schema);
    Ok(())
}

#[test]
fn scalar_constant_output_rejects_mapped_mismatches_and_explicit_null() {
    let mut fixed = SchemaNode::scalar("status", ScalarType::String);
    fixed.fixed = Some("ready".into());
    assert!(matches!(
        crate::to_string(
            &fixed,
            &Instance::Scalar(Value::String("wrong".into()))
        ),
        Err(JsonFormatError::ConstantMismatch { ref name, .. }) if name == "status"
    ));

    fixed.nullable = true;
    assert!(matches!(
        crate::from_str("null", &fixed),
        Err(JsonFormatError::ConstantMismatch { ref name, .. }) if name == "status"
    ));
    assert!(matches!(
        crate::to_string(&fixed, &Instance::Scalar(Value::json_null())),
        Err(JsonFormatError::ConstantMismatch { ref name, .. }) if name == "status"
    ));
}

#[test]
fn const_intersects_enum_and_all_of_without_widening() {
    let selected =
        import_str(r#"{"title":"Selected","type":"string","const":"b","enum":["a","b","c"]}"#);
    assert_eq!(selected.fixed.as_deref(), Some("b"));
    assert!(crate::from_str(r#""b""#, &selected).is_ok());
    assert!(crate::from_str(r#""a""#, &selected).is_err());

    let narrowed = import_str(
        r#"{
  "title":"Narrowed",
  "allOf":[
    {"type":"number"},
    {"type":["integer","number"],"const":7},
    {"enum":[7]}
  ]
}"#,
    );
    assert_eq!(
        narrowed.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    );
    assert_eq!(narrowed.fixed.as_deref(), Some("7"));
    assert!(crate::from_str("7", &narrowed).is_ok());
    assert!(crate::from_str("7.5", &narrowed).is_err());

    let same_union = import_str(
        r#"{
  "title":"SameUnion",
  "allOf":[
    {"type":["string","integer"]},
    {"type":["string","integer"]}
  ]
}"#,
    );
    let SchemaKind::ScalarUnion { types } = same_union.kind else {
        panic!("an identical allOf scalar union must retain its tagged domain");
    };
    assert!(types.contains(ScalarType::String));
    assert!(types.contains(ScalarType::Int));

    let conflict =
        import_str_result(r#"{"title":"Conflict","allOf":[{"const":"a"},{"const":"b"}]}"#);
    assert!(matches!(
        conflict,
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("no value in common")
    ));
}

#[test]
fn unsupported_constant_domains_and_enumerations_reject_exactly() {
    for (schema, expected) in [
        (r#"{"title":"x","const":null}"#, "null-only"),
        (r#"{"title":"x","const":{"x":1}}"#, "object and array"),
        (r#"{"title":"x","const":[1]}"#, "object and array"),
        (
            r#"{"title":"x","type":"integer","const":"1"}"#,
            "declared scalar type",
        ),
        (r#"{"title":"x","enum":[]}"#, "no possible values"),
        (r#"{"title":"x","enum":"x"}"#, "must be an array"),
        (
            r#"{"title":"x","const":"c","enum":["a","b"]}"#,
            "no value in common",
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
}
