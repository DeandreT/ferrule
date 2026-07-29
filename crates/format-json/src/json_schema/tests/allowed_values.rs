use ir::{Instance, JsonAllowedValue, ScalarType, SchemaKind, Value};

use super::{JsonFormatError, import_str_result};

#[test]
fn direct_heterogeneous_enum_is_exact_executable_and_roundtrips()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = import_str_result(
        r#"{
  "title":"Choice",
  "enum":["red","blue",1,1.5,true,null]
}"#,
    )?;
    let Some(values) = &schema.json_allowed_values else {
        return Err("multi-value enum must retain allowed-value metadata".into());
    };
    assert!(values.contains(&JsonAllowedValue::String("red".to_string())));
    assert!(values.contains(&JsonAllowedValue::Int(1)));
    assert!(values.contains_json_null());
    assert!(schema.nullable);
    let SchemaKind::ScalarUnion { types } = schema.kind else {
        return Err("heterogeneous enum must retain a scalar union".into());
    };
    for ty in [ScalarType::String, ScalarType::Float, ScalarType::Bool] {
        assert!(types.contains(ty));
    }
    for input in [r#""red""#, r#""blue""#, "1", "1.0", "1.5", "true", "null"] {
        let instance = crate::from_str(input, &schema)?;
        crate::to_string(&schema, &instance)?;
    }
    assert!(matches!(
        crate::from_str("2", &schema),
        Err(JsonFormatError::AllowedValueMismatch { .. })
    ));
    assert!(matches!(
        crate::from_str("false", &schema),
        Err(JsonFormatError::AllowedValueMismatch { .. })
    ));

    let rendered = super::super::export(&schema)?;
    let roundtrip = import_str_result(&rendered)?;
    assert_eq!(roundtrip, schema);
    Ok(())
}

#[test]
fn declared_domains_intersect_enum_values_and_normalized_output()
-> Result<(), Box<dyn std::error::Error>> {
    let narrowed = import_str_result(r#"{"title":"Code","type":"string","enum":["one",2,"two"]}"#)?;
    let Some(values) = &narrowed.json_allowed_values else {
        return Err("two retained string values require an allowed set".into());
    };
    assert_eq!(
        values.values(),
        [
            JsonAllowedValue::String("one".to_string()),
            JsonAllowedValue::String("two".to_string()),
        ]
    );

    let numeric_text = import_str_result(r#"{"title":"Digit","type":"string","enum":["1","2"]}"#)?;
    assert_eq!(
        crate::to_string(&numeric_text, &Instance::Scalar(Value::Int(1)))?,
        "\"1\"\n"
    );
    assert!(matches!(
        crate::to_string(&numeric_text, &Instance::Scalar(Value::Int(3))),
        Err(JsonFormatError::AllowedValueMismatch { .. })
    ));
    assert!(matches!(
        import_str_result(r#"{"title":"Impossible","type":"boolean","enum":[1,2]}"#),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("declared scalar type")
    ));
    Ok(())
}

#[test]
fn all_of_intersects_and_finite_any_of_and_one_of_compose_exactly()
-> Result<(), Box<dyn std::error::Error>> {
    let all_of = import_str_result(
        r#"{
  "title":"All",
  "allOf":[
    {"type":"string","enum":["a","b","c"]},
    {"enum":["b","c","d"]}
  ]
}"#,
    )?;
    assert_allowed_strings(&all_of, &["b", "c"])?;

    let any_of = import_str_result(
        r#"{
  "title":"Any",
  "anyOf":[
    {"const":"a"},
    {"enum":["b","c"]},
    {"const":null}
  ]
}"#,
    )?;
    assert!(any_of.nullable);
    for input in [r#""a""#, r#""b""#, r#""c""#, "null"] {
        assert!(crate::from_str(input, &any_of).is_ok(), "{input}");
    }

    let one_of = import_str_result(
        r#"{
  "title":"One",
  "oneOf":[
    {"enum":["a","b"]},
    {"enum":["b","c"]}
  ]
}"#,
    )?;
    assert_allowed_strings(&one_of, &["a", "c"])?;
    assert!(crate::from_str(r#""b""#, &one_of).is_err());

    let constrained = import_str_result(
        r#"{
  "title":"Constrained",
  "anyOf":[
    {"type":"integer","enum":[1,2],"minimum":2},
    {"type":"integer","enum":[3,4],"maximum":3}
  ]
}"#,
    )?;
    let Some(values) = &constrained.json_allowed_values else {
        return Err("finite constrained union must retain its exact values".into());
    };
    assert_eq!(
        values.values(),
        [JsonAllowedValue::Int(2), JsonAllowedValue::Int(3)]
    );
    Ok(())
}

#[test]
fn refs_arrays_and_outer_constraints_preserve_allowed_sets()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = import_str_result(
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "title":"Root",
  "type":"object",
  "properties":{
    "code":{"$ref":"#/$defs/code","enum":["b","c"]},
    "items":{"type":"array","items":{"enum":[1,2,3]}}
  },
  "$defs":{"code":{"type":"string","enum":["a","b"]}}
}"##,
    )?;
    let Some(code) = schema.child("code") else {
        return Err("code field must import".into());
    };
    assert_eq!(code.fixed.as_deref(), Some("b"));
    let Some(items) = schema.child("items") else {
        return Err("items field must import".into());
    };
    assert!(items.repeating);
    assert!(items.json_allowed_values.is_some());
    assert!(crate::from_str(r#"{"code":"b","items":[1,2,3]}"#, &schema).is_ok());
    assert!(crate::from_str(r#"{"code":"a","items":[1]}"#, &schema).is_err());
    assert!(crate::from_str(r#"{"code":"b","items":[4]}"#, &schema).is_err());

    let outer = import_str_result(
        r#"{
  "title":"Outer",
  "anyOf":[{"const":"a"},{"const":"b"}],
  "enum":["b","c"]
}"#,
    )?;
    assert_eq!(outer.fixed.as_deref(), Some("b"));
    Ok(())
}

#[test]
fn enum_bounds_and_unsupported_members_reject_before_widening() {
    let values = (0..=ir::MAX_JSON_ALLOWED_VALUES)
        .map(|value| value.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let excessive = format!(r#"{{"title":"Many","enum":[{values}]}}"#);
    assert!(matches!(
        import_str_result(&excessive),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("bounded scalar allowed-value limit")
    ));
    for schema in [
        r#"{"title":"Object","enum":[{"x":1},"ok"]}"#,
        r#"{"title":"Array","enum":[[1,2],"ok"]}"#,
    ] {
        assert!(matches!(
            import_str_result(schema),
            Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
                if reason.contains("object and array")
        ));
    }
}

fn assert_allowed_strings(
    schema: &ir::SchemaNode,
    expected: &[&str],
) -> Result<(), Box<dyn std::error::Error>> {
    let Some(values) = &schema.json_allowed_values else {
        return Err("expected allowed-value metadata".into());
    };
    let actual = values
        .values()
        .iter()
        .filter_map(|value| match value {
            JsonAllowedValue::String(value) => Some(value.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
    Ok(())
}
