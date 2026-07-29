use super::{export, import_str, import_str_result};
use crate::JsonFormatError;
use ir::{Instance, Value};

const OBJECT_PROPERTIES: &str = r#"
  "type": "object",
  "additionalProperties": false,
  "properties": {
    "mode": { "type": "string" },
    "payload": { "type": "integer" }
  }
"#;

#[test]
fn presence_conditional_lowers_to_dependent_schema_and_roundtrips() {
    let schema = import_str(&format!(
        r#"{{
  {OBJECT_PROPERTIES},
  "if": {{ "type": "object", "required": ["mode"] }},
  "then": {{
    "type": "object",
    "required": ["payload"],
    "properties": {{
      "payload": {{ "type": "integer", "minimum": 2 }}
    }}
  }},
  "else": true
}}"#
    ));

    assert!(crate::from_str("{}", &schema).is_ok());
    assert!(crate::from_str(r#"{"payload":1}"#, &schema).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"mode":"strict"}"#, &schema),
        Err(JsonFormatError::DependentSchemaMismatch { .. })
    ));
    assert!(matches!(
        crate::from_str(r#"{"mode":"strict","payload":1}"#, &schema),
        Err(JsonFormatError::DependentSchemaMismatch { .. })
    ));
    assert!(crate::from_str(r#"{"mode":"strict","payload":2}"#, &schema).is_ok());

    let rendered = export(&schema);
    let rendered_json: serde_json::Value = serde_json::from_str(&rendered).unwrap();
    assert!(rendered_json.get("if").is_none());
    assert!(
        rendered_json.pointer("/dependentSchemas/mode").is_some(),
        "{rendered}"
    );
    assert_eq!(import_str(&rendered), schema);
}

#[test]
fn required_only_then_uses_canonical_property_dependency() {
    let schema = import_str(&format!(
        r#"{{
  {OBJECT_PROPERTIES},
  "if": {{ "required": ["mode"] }},
  "then": {{ "required": ["payload"] }}
}}"#
    ));
    assert!(matches!(
        crate::from_str(r#"{"mode":"strict"}"#, &schema),
        Err(JsonFormatError::MissingDependentProperty { .. })
    ));
    let rendered: serde_json::Value = serde_json::from_str(&export(&schema)).unwrap();
    assert_eq!(
        rendered.pointer("/dependentRequired/mode"),
        Some(&serde_json::json!(["payload"]))
    );
}

#[test]
fn false_then_forbids_the_trigger_property() {
    let schema = import_str(&format!(
        r#"{{
  {OBJECT_PROPERTIES},
  "if": {{ "required": ["mode"] }},
  "then": false
}}"#
    ));
    assert!(crate::from_str("{}", &schema).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"mode":"strict"}"#, &schema),
        Err(JsonFormatError::DependentSchemaMismatch { .. })
    ));
}

#[test]
fn explicitly_typed_nullable_condition_preserves_null_and_applies_then()
-> Result<(), Box<dyn std::error::Error>> {
    for else_declaration in ["", r#","else":true"#] {
        let schema = import_str(&format!(
            r#"{{
  "type":["object","null"],
  "additionalProperties":false,
  "properties":{{
    "mode":{{"type":"string"}},
    "payload":{{"type":"integer"}}
  }},
  "if":{{"type":"object","required":["mode"]}},
  "then":{{"required":["payload"]}}
  {else_declaration}
}}"#
        ));
        assert!(schema.container_nullable);
        assert!(crate::from_str("null", &schema).is_ok());
        assert!(crate::from_str("{}", &schema).is_ok());
        assert!(crate::from_str(r#"{"payload":1}"#, &schema).is_ok());
        assert!(matches!(
            crate::from_str(r#"{"mode":"strict"}"#, &schema),
            Err(JsonFormatError::MissingDependentProperty { .. })
        ));
        let instance = crate::from_str(r#"{"mode":"strict","payload":2}"#, &schema)?;
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&crate::to_string(&schema, &instance)?)?,
            serde_json::json!({"mode":"strict","payload":2})
        );
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&crate::to_string(
                &schema,
                &Instance::Scalar(Value::json_null()),
            )?)?,
            serde_json::Value::Null
        );

        let rendered = export(&schema);
        let rendered_json: serde_json::Value = serde_json::from_str(&rendered)?;
        assert!(rendered_json.get("if").is_none());
        assert_eq!(
            rendered_json.pointer("/anyOf/0/dependentRequired/mode"),
            Some(&serde_json::json!(["payload"]))
        );
        assert_eq!(import_str(&rendered), schema);
    }
    Ok(())
}

#[test]
fn nullable_condition_with_false_then_still_accepts_null() {
    let schema = import_str(
        r#"{
  "type":["object","null"],
  "additionalProperties":false,
  "properties":{"mode":{"type":"string"}},
  "if":{"type":"object","required":["mode"]},
  "then":false
}"#,
    );
    assert!(schema.container_nullable);
    assert!(crate::from_str("null", &schema).is_ok());
    assert!(crate::from_str("{}", &schema).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"mode":"strict"}"#, &schema),
        Err(JsonFormatError::DependentSchemaMismatch { .. })
    ));
}

#[test]
fn false_else_requires_the_trigger_when_then_is_absent_or_true() {
    for then_declaration in ["", r#","then":true"#] {
        let schema = import_str(&format!(
            r#"{{
  {OBJECT_PROPERTIES},
  "if":{{"required":["mode"]}}
  {then_declaration},
  "else":false
}}"#
        ));
        assert_eq!(schema.required_fields(), ["mode"]);
        assert!(!schema.container_nullable);
        assert!(matches!(
            crate::from_str("{}", &schema),
            Err(JsonFormatError::MissingRequiredProperty { ref property, .. })
                if property == "mode"
        ));
        assert!(crate::from_str(r#"{"mode":"strict"}"#, &schema).is_ok());
        assert!(crate::from_str("null", &schema).is_err());
        assert!(schema.json_dependent_schemas.is_none());
        assert!(schema.json_property_dependencies.is_none());

        let rendered = export(&schema);
        let rendered_json: serde_json::Value = serde_json::from_str(&rendered).unwrap();
        assert_eq!(
            rendered_json.get("required"),
            Some(&serde_json::json!(["mode"]))
        );
        assert!(rendered_json.get("if").is_none());
        assert_eq!(import_str(&rendered), schema);
    }
}

#[test]
fn false_else_narrows_nullable_object_and_retains_then() -> Result<(), Box<dyn std::error::Error>> {
    let schema = import_str(
        r#"{
  "type":["null","object"],
  "additionalProperties":false,
  "properties":{
    "mode":{"type":"string"},
    "payload":{"type":"integer"}
  },
  "if":{"type":"object","required":["mode"]},
  "then":{"required":["payload"]},
  "else":false
}"#,
    );
    assert!(!schema.container_nullable);
    assert_eq!(schema.required_fields(), ["mode"]);
    assert!(matches!(
        crate::from_str("null", &schema),
        Err(JsonFormatError::Shape { .. })
    ));
    assert!(matches!(
        crate::from_str("{}", &schema),
        Err(JsonFormatError::MissingRequiredProperty { ref property, .. })
            if property == "mode"
    ));
    assert!(matches!(
        crate::from_str(r#"{"mode":"strict"}"#, &schema),
        Err(JsonFormatError::MissingDependentProperty { ref property, .. })
            if property == "payload"
    ));
    let instance = crate::from_str(r#"{"mode":"strict","payload":4}"#, &schema)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&crate::to_string(&schema, &instance)?)?,
        serde_json::json!({"mode":"strict","payload":4})
    );
    assert!(matches!(
        crate::to_string(&schema, &Instance::Group(Vec::new())),
        Err(JsonFormatError::MissingRequiredProperty { ref property, .. })
            if property == "mode"
    ));

    let rendered = export(&schema);
    let rendered_json: serde_json::Value = serde_json::from_str(&rendered)?;
    assert_eq!(
        rendered_json.get("type"),
        Some(&serde_json::json!("object"))
    );
    assert_eq!(
        rendered_json.get("required"),
        Some(&serde_json::json!(["mode"]))
    );
    assert_eq!(
        rendered_json.pointer("/dependentRequired/mode"),
        Some(&serde_json::json!(["payload"]))
    );
    assert_eq!(import_str(&rendered), schema);
    Ok(())
}

#[test]
fn false_then_and_false_else_retain_an_exact_empty_object_domain() {
    let schema = import_str(&format!(
        r#"{{
  {OBJECT_PROPERTIES},
  "if":{{"required":["mode"]}},
  "then":false,
  "else":false
}}"#
    ));
    assert_eq!(schema.required_fields(), ["mode"]);
    assert!(matches!(
        crate::from_str("{}", &schema),
        Err(JsonFormatError::MissingRequiredProperty { .. })
    ));
    assert!(matches!(
        crate::from_str(r#"{"mode":"strict"}"#, &schema),
        Err(JsonFormatError::DependentSchemaMismatch { .. })
    ));
    assert_eq!(import_str(&export(&schema)), schema);
}

#[test]
fn false_else_merges_existing_required_fields_without_duplicates() {
    let appended = import_str(
        r#"{
  "type":"object",
  "additionalProperties":false,
  "required":["payload"],
  "properties":{
    "mode":{"type":"string"},
    "payload":{"type":"integer"}
  },
  "if":{"required":["mode"]},
  "else":false
}"#,
    );
    assert_eq!(appended.required_fields(), ["payload", "mode"]);

    let deduplicated = import_str(
        r#"{
  "type":"object",
  "additionalProperties":false,
  "required":["payload","mode"],
  "properties":{
    "mode":{"type":"string"},
    "payload":{"type":"integer"}
  },
  "if":{"required":["mode"]},
  "else":false
}"#,
    );
    assert_eq!(deduplicated.required_fields(), ["payload", "mode"]);
    for input in ["{}", r#"{"mode":"strict"}"#, r#"{"payload":1}"#] {
        assert!(matches!(
            crate::from_str(input, &deduplicated),
            Err(JsonFormatError::MissingRequiredProperty { .. })
        ));
    }
    assert!(crate::from_str(r#"{"mode":"strict","payload":1}"#, &deduplicated).is_ok());
}

#[test]
fn false_else_rejects_impossible_or_conflicting_trigger_requirements() {
    for (schema, expected) in [
        (
            r#"{
              "type":"object",
              "additionalProperties":false,
              "properties":{"payload":{"type":"integer"}},
              "if":{"required":["mode"]},
              "else":false
            }"#,
            "does not declare",
        ),
        (
            r#"{
              "type":"object",
              "additionalProperties":false,
              "maxProperties":0,
              "properties":{"mode":{"type":"string"}},
              "if":{"required":["mode"]},
              "else":false
            }"#,
            "conflicts",
        ),
        (
            r#"{
              "type":"object",
              "oneOf":[
                {
                  "title":"mode",
                  "type":"object",
                  "additionalProperties":false,
                  "properties":{"mode":{"type":"string"}},
                  "required":["mode"]
                },
                {
                  "title":"payload",
                  "type":"object",
                  "additionalProperties":false,
                  "properties":{"payload":{"type":"integer"}},
                  "required":["payload"]
                }
              ],
              "if":{"required":["mode"]},
              "else":false
            }"#,
            "alternatives",
        ),
    ] {
        let result = import_str_result(schema);
        assert!(
            matches!(
                result,
                Err(JsonFormatError::UnsupportedSchemaObject { ref reason, .. })
                    if reason.contains(expected)
            ),
            "{result:?}"
        );
    }
}

#[test]
fn draft4_and_draft6_ignore_conditional_keywords() {
    for dialect in [
        "http://json-schema.org/draft-04/schema#",
        "http://json-schema.org/draft-06/schema#",
    ] {
        let schema = import_str(&format!(
            r#"{{
  "$schema": "{dialect}",
  {OBJECT_PROPERTIES},
  "if": {{ "properties": {{ "mode": {{ "const": "strict" }} }} }},
  "then": false,
  "else": false
}}"#
        ));
        assert!(crate::from_str(r#"{"mode":"strict"}"#, &schema).is_ok());
        assert!(schema.json_dependent_schemas.is_none());
        assert!(schema.json_property_dependencies.is_none());
    }
}

#[test]
fn supported_dialects_apply_presence_conditionals() {
    for dialect in [
        None,
        Some("http://json-schema.org/draft-07/schema#"),
        Some("https://json-schema.org/draft/2019-09/schema"),
        Some("https://json-schema.org/draft/2020-12/schema"),
    ] {
        let declaration = dialect.map_or(String::new(), |dialect| {
            format!(r#""$schema": "{dialect}","#)
        });
        let schema = import_str(&format!(
            r#"{{
  {declaration}
  {OBJECT_PROPERTIES},
  "if": {{ "required": ["mode"] }},
  "then": false
}}"#
        ));
        assert!(matches!(
            crate::from_str(r#"{"mode":"strict"}"#, &schema),
            Err(JsonFormatError::DependentSchemaMismatch { .. })
        ));
    }
}

#[test]
fn supported_dialects_require_false_else_triggers() {
    for dialect in [
        None,
        Some("http://json-schema.org/draft-07/schema#"),
        Some("https://json-schema.org/draft/2019-09/schema"),
        Some("https://json-schema.org/draft/2020-12/schema"),
    ] {
        let declaration = dialect.map_or(String::new(), |dialect| {
            format!(r#""$schema": "{dialect}","#)
        });
        let schema = import_str(&format!(
            r#"{{
  {declaration}
  {OBJECT_PROPERTIES},
  "if":{{"required":["mode"]}},
  "then":true,
  "else":false
}}"#
        ));
        assert_eq!(schema.required_fields(), ["mode"]);
        assert!(schema.json_dependent_schemas.is_none());
        assert!(schema.json_property_dependencies.is_none());
        assert!(matches!(
            crate::from_str("{}", &schema),
            Err(JsonFormatError::MissingRequiredProperty { ref property, .. })
                if property == "mode"
        ));
        assert!(crate::from_str(r#"{"mode":"strict"}"#, &schema).is_ok());
    }
}

#[test]
fn typed_all_of_branch_applies_without_cross_branch_pairing() {
    let applied = import_str(&format!(
        r#"{{
  "allOf": [
    {{ {OBJECT_PROPERTIES} }},
    {{
      "type": "object",
      "if": {{ "required": ["mode"] }},
      "then": false
    }}
  ]
}}"#
    ));
    assert!(matches!(
        crate::from_str(r#"{"mode":"strict"}"#, &applied),
        Err(JsonFormatError::DependentSchemaMismatch { .. })
    ));

    let unpaired = import_str(&format!(
        r#"{{
  "allOf": [
    {{ {OBJECT_PROPERTIES} }},
    {{ "if": {{ "required": ["mode"] }} }},
    {{ "then": false }}
  ]
}}"#
    ));
    assert!(crate::from_str(r#"{"mode":"strict"}"#, &unpaired).is_ok());
}

#[test]
fn typed_all_of_branch_can_require_a_false_else_trigger() {
    let schema = import_str(&format!(
        r#"{{
  "allOf":[
    {{{OBJECT_PROPERTIES}}},
    {{
      "type":"object",
      "if":{{"required":["mode"]}},
      "else":false
    }}
  ]
}}"#
    ));
    assert_eq!(schema.required_fields(), ["mode"]);
    assert!(matches!(
        crate::from_str("{}", &schema),
        Err(JsonFormatError::MissingRequiredProperty { .. })
    ));
    assert!(crate::from_str(r#"{"mode":"strict"}"#, &schema).is_ok());
}

#[test]
fn value_sensitive_and_multi_property_conditions_reject() {
    for condition in [
        r#"{"required":["left","right"]}"#,
        r#"{"properties":{"mode":{"const":"strict"}}}"#,
        r#"{"type":"array","required":["mode"]}"#,
        "true",
    ] {
        let error = import_str_result(&format!(
            r#"{{
  {OBJECT_PROPERTIES},
  "if": {condition},
  "then": false
}}"#
        ));
        assert!(matches!(
            error,
            Err(JsonFormatError::UnsupportedSchemaObject { ref reason, .. })
                if reason.contains("required")
                    || reason.contains("presence")
                    || reason.contains("type")
        ));
    }
}

#[test]
fn nontrivial_else_and_non_schema_then_reject() {
    for (then_schema, else_schema, expected) in [
        ("false", r#"{"required":["payload"]}"#, "`else`"),
        ("17", "true", "`then`"),
    ] {
        let error = import_str_result(&format!(
            r#"{{
  {OBJECT_PROPERTIES},
  "if": {{ "required": ["mode"] }},
  "then": {then_schema},
  "else": {else_schema}
}}"#
        ));
        assert!(matches!(
            error,
            Err(JsonFormatError::UnsupportedSchemaObject { ref reason, .. })
                if reason.contains(expected)
        ));
    }
}

#[test]
fn nullable_and_non_object_conditionals_reject() {
    for schema in [
        r#"{
          "type": ["object", "null"],
          "properties": { "mode": { "type": "string" } },
          "if": { "required": ["mode"] },
          "then": false
        }"#,
        r#"{
          "type": "string",
          "if": { "required": ["mode"] },
          "then": false
        }"#,
        r#"{
          "type": "array",
          "items": { "type": "string" },
          "if": { "required": ["mode"] },
          "then": false
        }"#,
        r#"{
          "properties": { "mode": { "type": "string" } },
          "if": { "required": ["mode"] },
          "then": false
        }"#,
        r#"{
          "oneOf": [
            {
              "type": "object",
              "properties": { "mode": { "type": "string" } }
            },
            { "type": "null" }
          ],
          "if": { "required": ["mode"] },
          "then": false
        }"#,
    ] {
        let result = import_str_result(schema);
        assert!(
            matches!(
                &result,
                Err(JsonFormatError::UnsupportedSchemaObject { reason, .. })
                if reason.contains("explicit `type")
                    || reason.contains("nullable object conditional")
                    || reason.contains("concrete object")
            ) || matches!(
                &result,
                Err(JsonFormatError::UnsupportedSchemaUnion { reason, .. })
                    if reason.contains("cannot preserve `if`")
            ),
            "{result:?}"
        );
    }
}
