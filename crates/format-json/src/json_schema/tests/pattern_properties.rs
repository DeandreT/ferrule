use super::*;

fn unsupported_reason(schema: &str) -> String {
    match import_str_result(schema) {
        Err(JsonFormatError::UnsupportedSchemaObject { reason, .. })
        | Err(JsonFormatError::UnsupportedSchemaUnion { reason, .. }) => reason,
        Err(error) => panic!("expected an unsupported-schema diagnostic, got {error}"),
        Ok(_) => panic!("expected schema import to reject"),
    }
}

#[test]
fn homogeneous_closed_patterns_import_export_and_reimport() -> Result<(), Box<dyn std::error::Error>>
{
    let schema = import_str(
        r#"{
  "title":"Extensions",
  "type":"object",
  "additionalProperties":false,
  "properties":{
    "id":{"type":"integer"},
    "x-known":{"type":"string","minLength":1}
  },
  "patternProperties":{
    "^x-":{"type":"string","minLength":1},
    "^meta-":{"type":"string","minLength":1}
  },
  "propertyNames":{"pattern":"^(id|x-.*|meta-.*)$"}
}"#,
    );

    let selectors = schema
        .json_pattern_property_names()
        .ok_or("pattern selectors should be retained")?;
    assert_eq!(selectors.sources(), ["^x-", "^meta-"]);
    assert!(selectors.matches("x-runtime"));
    assert!(selectors.matches("meta-source"));
    assert!(!selectors.matches("other"));
    assert!(schema.json_property_names.is_some());
    let dynamic = schema
        .dynamic_fields()
        .ok_or("pattern value schema should be retained")?;
    assert_eq!(
        dynamic.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    );
    assert_eq!(
        dynamic.string_length_range,
        ir::StringLengthRange::new(1, None)
    );

    let rendered = export(&schema);
    let rendered_json: serde_json::Value = serde_json::from_str(&rendered)?;
    assert_eq!(
        rendered_json.get("additionalProperties"),
        Some(&serde_json::Value::Bool(false))
    );
    assert_eq!(
        rendered_json.pointer("/patternProperties/^x-/type"),
        Some(&serde_json::json!("string"))
    );
    assert_eq!(
        rendered_json.pointer("/patternProperties/^meta-/minLength"),
        Some(&serde_json::json!(1))
    );
    assert!(rendered_json.get("propertyNames").is_some());
    assert_eq!(import_str(&rendered), schema);
    Ok(())
}

#[test]
fn boolean_true_pattern_schema_is_canonical_arbitrary_json()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = import_str(
        r#"{
  "title":"Extensions",
  "type":"object",
  "additionalProperties":false,
  "patternProperties":{"^x-":true}
}"#,
    );
    assert!(
        schema
            .dynamic_fields()
            .is_some_and(|dynamic| dynamic.json_any)
    );

    let rendered = export(&schema);
    let rendered_json: serde_json::Value = serde_json::from_str(&rendered)?;
    assert_eq!(
        rendered_json.pointer("/patternProperties/^x-"),
        Some(&serde_json::json!({}))
    );
    assert_eq!(import_str(&rendered), schema);
    Ok(())
}

#[test]
fn empty_pattern_map_is_an_ordinary_closed_object() {
    let schema = import_str(
        r#"{
  "title":"Closed",
  "type":"object",
  "additionalProperties":false,
  "properties":{"id":{"type":"integer"}},
  "patternProperties":{}
}"#,
    );
    assert!(schema.dynamic_fields().is_none());
    assert!(schema.json_pattern_property_names().is_none());

    let rendered: serde_json::Value = serde_json::from_str(&export(&schema)).unwrap();
    assert!(rendered.get("patternProperties").is_none());
    assert_eq!(
        rendered.get("additionalProperties"),
        Some(&serde_json::Value::Bool(false))
    );
}

#[test]
fn nullable_object_patterns_bypass_null_and_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    for types in [r#"["object","null"]"#, r#"["null","object"]"#] {
        let schema = import_str(&format!(
            r#"{{
  "title":"NullableExtensions",
  "type":{types},
  "additionalProperties":false,
  "patternProperties":{{"^x-":{{"type":"integer"}}}}
}}"#
        ));
        assert!(schema.container_nullable);
        let null = crate::from_str("null", &schema)?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&crate::to_string(&schema, &null)?)?,
            serde_json::Value::Null
        );

        let rendered = export(&schema);
        let rendered_json: serde_json::Value = serde_json::from_str(&rendered)?;
        assert_eq!(
            rendered_json.pointer("/anyOf/0/additionalProperties"),
            Some(&serde_json::Value::Bool(false))
        );
        assert_eq!(
            rendered_json.pointer("/anyOf/0/patternProperties/^x-/type"),
            Some(&serde_json::json!("integer"))
        );
        assert_eq!(import_str(&rendered), schema);
    }
    Ok(())
}

#[test]
fn singleton_object_type_array_normalizes_and_roundtrips() -> Result<(), Box<dyn std::error::Error>>
{
    let schema = import_str(
        r#"{
  "title":"Extensions",
  "type":["object"],
  "additionalProperties":false,
  "patternProperties":{"^x-":{"type":"integer"}}
}"#,
    );
    assert!(!schema.container_nullable);
    assert!(schema.json_pattern_property_names().is_some());

    let rendered = export(&schema);
    let rendered_json: serde_json::Value = serde_json::from_str(&rendered)?;
    assert_eq!(
        rendered_json.get("type"),
        Some(&serde_json::json!("object"))
    );
    assert_eq!(import_str(&rendered), schema);
    Ok(())
}

#[test]
fn structured_and_repeating_pattern_values_execute_and_roundtrip()
-> Result<(), Box<dyn std::error::Error>> {
    let structured = import_str(
        r#"{
  "title":"StructuredExtensions",
  "type":"object",
  "additionalProperties":false,
  "patternProperties":{
    "^record-":{
      "type":"object",
      "properties":{"name":{"type":"string"}},
      "required":["name"],
      "additionalProperties":false
    }
  }
}"#,
    );
    let structured_value = r#"{"record-first":{"name":"Ada"}}"#;
    let structured_instance = crate::from_str(structured_value, &structured)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&crate::to_string(
            &structured,
            &structured_instance
        )?)?,
        serde_json::from_str::<serde_json::Value>(structured_value)?
    );
    assert!(crate::from_str(r#"{"record-first":{}}"#, &structured).is_err());

    let arrays = import_str(
        r#"{
  "title":"ArrayExtensions",
  "type":"object",
  "additionalProperties":false,
  "properties":{
    "list-known":{
      "type":"array",
      "minItems":1,
      "maxItems":2,
      "items":{"type":"integer"}
    }
  },
  "patternProperties":{
    "^list-":{
      "type":"array",
      "minItems":1,
      "maxItems":2,
      "items":{"type":"integer"}
    },
    "^batch-":{
      "type":"array",
      "minItems":1,
      "maxItems":2,
      "items":{"type":"integer"}
    }
  }
}"#,
    );
    let dynamic = arrays
        .dynamic_fields()
        .ok_or("array pattern value schema should be retained")?;
    assert!(dynamic.repeating);
    assert_eq!(
        dynamic.item_count_range,
        ir::ItemCountRange::new(1, Some(2))
    );
    let array_value = r#"{"list-known":[1,2],"batch-extra":[3]}"#;
    let array_instance = crate::from_str(array_value, &arrays)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&crate::to_string(&arrays, &array_instance)?)?,
        serde_json::from_str::<serde_json::Value>(array_value)?
    );
    assert!(crate::from_str(r#"{"list-empty":[]}"#, &arrays).is_err());
    assert!(crate::from_str(r#"{"list-invalid":["one"]}"#, &arrays).is_err());

    for schema in [&structured, &arrays] {
        let rendered = export(schema);
        assert_eq!(import_str(&rendered), *schema);
    }
    let rendered: serde_json::Value = serde_json::from_str(&export(&arrays))?;
    assert_eq!(
        rendered.pointer("/patternProperties/^list-/type"),
        Some(&serde_json::json!("array"))
    );
    assert_eq!(
        rendered.pointer("/patternProperties/^batch-/items/type"),
        Some(&serde_json::json!("integer"))
    );
    Ok(())
}

#[test]
fn dependency_trigger_normalization_shares_one_import_work_budget() {
    let trigger = "a".repeat(5_000);
    let schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "BoundedDependencies",
        "type": "object",
        "additionalProperties": false,
        "properties": {
            "fixed": {"type": "string"}
        },
        "patternProperties": {
            "^(a?){5000}$": {"type": "string"}
        },
        "dependentRequired": {
            trigger.clone(): ["fixed"]
        },
        "dependentSchemas": {
            trigger: {
                "properties": {
                    "fixed": {"const": "ok"}
                }
            }
        }
    });
    let reason = unsupported_reason(&schema.to_string());
    assert!(
        reason.contains("schema-wide import work budget"),
        "{reason}"
    );
}

#[test]
fn impossible_dependency_triggers_are_pruned_from_closed_pattern_objects()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = import_str(
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "title":"DependentExtensions",
  "type":"object",
  "additionalProperties":false,
  "properties":{"fixed":{"type":"string"}},
  "patternProperties":{"^x-":{"type":"string"}},
  "dependentRequired":{
    "never":["also-never"],
    "fixed":["x-required"],
    "x-trigger":["fixed"]
  },
  "dependentSchemas":{
    "not-selected":false,
    "x-required-only":{"required":["fixed"]},
    "x-predicate":{"properties":{"fixed":{"const":"ok"}}}
  }
}"#,
    );

    let dependencies = schema
        .json_property_dependencies
        .as_ref()
        .ok_or("possible dependencies should be retained")?;
    assert!(dependencies.requirements("never").is_none());
    assert_eq!(
        dependencies.requirements("fixed"),
        Some(&["x-required".to_string()][..])
    );
    assert_eq!(
        dependencies.requirements("x-trigger"),
        Some(&["fixed".to_string()][..])
    );
    assert_eq!(
        dependencies.requirements("x-required-only"),
        Some(&["fixed".to_string()][..])
    );
    let predicates = schema
        .json_dependent_schemas
        .as_ref()
        .ok_or("possible dependent predicate should be retained")?;
    assert_eq!(predicates.as_slice().len(), 1);
    assert_eq!(predicates.as_slice()[0].trigger(), "x-predicate");

    assert!(crate::from_str(r#"{"fixed":"ok","x-required":"yes"}"#, &schema).is_ok());
    assert!(crate::from_str(r#"{"fixed":"ok"}"#, &schema).is_err());
    assert!(crate::from_str(r#"{"x-trigger":"yes"}"#, &schema).is_err());
    assert!(
        crate::from_str(
            r#"{"fixed":"wrong","x-required":"yes","x-predicate":"yes"}"#,
            &schema
        )
        .is_err()
    );

    let rendered = export(&schema);
    let rendered_json: serde_json::Value = serde_json::from_str(&rendered)?;
    assert!(rendered_json.pointer("/dependentRequired/never").is_none());
    assert!(
        rendered_json
            .pointer("/dependentSchemas/not-selected")
            .is_none()
    );
    assert_eq!(import_str(&rendered), schema);
    Ok(())
}

#[test]
fn distinct_or_invalid_pattern_value_schemas_reject() {
    for (patterns, expected) in [
        (
            r#""^x-":{"type":"string"},"^meta-":{"type":"integer"}"#,
            "identical",
        ),
        (r#""^x-":false"#, "false"),
        (r#""^x-":17"#, "boolean or object"),
    ] {
        let reason = unsupported_reason(&format!(
            r#"{{
  "type":"object",
  "additionalProperties":false,
  "patternProperties":{{{patterns}}}
}}"#
        ));
        assert!(reason.contains(expected), "{reason}");
    }
}

#[test]
fn pattern_object_must_be_explicitly_closed() {
    for (additional, expected) in [
        ("", "additionalProperties: false"),
        (
            r#","additionalProperties":true"#,
            "additionalProperties: false",
        ),
        (
            r#","additionalProperties":{"type":"string"}"#,
            "additionalProperties: false",
        ),
    ] {
        let reason = unsupported_reason(&format!(
            r#"{{
  "type":"object"
  {additional},
  "patternProperties":{{"^x-":{{"type":"string"}}}}
}}"#
        ));
        assert!(reason.contains(expected), "{reason}");
    }

    let reason = unsupported_reason(
        r#"{
  "additionalProperties":false,
  "patternProperties":{"^x-":{"type":"string"}}
}"#,
    );
    assert!(reason.contains("explicit object"), "{reason}");
}

#[test]
fn matching_declared_property_must_have_the_common_schema() {
    let reason = unsupported_reason(
        r#"{
  "type":"object",
  "additionalProperties":false,
  "properties":{"x-known":{"type":"integer"}},
  "patternProperties":{"^x-":{"type":"string"}}
}"#,
    );
    assert!(
        reason.contains("selectors conflict"),
        "unexpected diagnostic: {reason}"
    );

    let schema = import_str(
        r#"{
  "type":"object",
  "additionalProperties":false,
  "properties":{
    "x-known":{"type":"string"},
    "ordinary":{"type":"integer"}
  },
  "patternProperties":{"^x-":{"type":"string"}}
}"#,
    );
    assert!(schema.child("ordinary").is_some());
    assert!(schema.json_pattern_property_names().is_some());
}

#[test]
fn malformed_selectors_and_unsafe_compositions_reject() {
    let malformed = unsupported_reason(
        r#"{
  "type":"object",
  "additionalProperties":false,
  "patternProperties":{"(?=x)":{"type":"string"}}
}"#,
    );
    assert!(malformed.contains("invalid") || malformed.contains("unsupported"));

    let all_of = unsupported_reason(
        r#"{
  "allOf":[
    {
      "type":"object",
      "additionalProperties":false,
      "patternProperties":{"^x-":{"type":"string"}}
    },
    {"type":"object","additionalProperties":false}
  ]
}"#,
    );
    assert!(all_of.contains("allOf"), "{all_of}");

    let alternative = unsupported_reason(
        r#"{
  "oneOf":[
    {
      "type":"object",
      "additionalProperties":false,
      "properties":{"kind":{"type":"string","const":"pattern"}},
      "patternProperties":{"^x-":{"type":"string"}}
    },
    {
      "type":"object",
      "additionalProperties":false,
      "properties":{"kind":{"type":"string","const":"plain"}}
    }
  ]
}"#,
    );
    assert!(alternative.contains("oneOf"), "{alternative}");
}

#[test]
fn referenced_common_schema_is_inlined_canonically() -> Result<(), Box<dyn std::error::Error>> {
    let schema = import_str(
        r##"{
  "title":"ReferencedExtensions",
  "type":"object",
  "additionalProperties":false,
  "patternProperties":{
    "^x-":{"$ref":"#/$defs/value"},
    "^meta-":{"$ref":"#/$defs/value"}
  },
  "$defs":{"value":{"type":"boolean"}}
}"##,
    );
    let rendered = export(&schema);
    let rendered_json: serde_json::Value = serde_json::from_str(&rendered)?;
    assert_eq!(
        rendered_json.pointer("/patternProperties/^x-/type"),
        Some(&serde_json::json!("boolean"))
    );
    assert_eq!(import_str(&rendered), schema);

    let sibling = unsupported_reason(
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$ref":"#/$defs/base",
  "patternProperties":{"^x-":{"type":"boolean"}},
  "$defs":{
    "base":{"type":"object","additionalProperties":false}
  }
}"##,
    );
    assert!(
        sibling.contains("patternProperties") && sibling.contains("$ref"),
        "{sibling}"
    );
    Ok(())
}
