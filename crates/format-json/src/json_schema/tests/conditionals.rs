use super::{export, import_str, import_str_result};
use crate::JsonFormatError;

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
        ("false", "false", "`else`"),
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
                if reason.contains("explicit non-null")
            ) || matches!(
                &result,
                Err(JsonFormatError::UnsupportedSchemaUnion { reason, .. })
                    if reason.contains("cannot preserve `if`")
            ),
            "{result:?}"
        );
    }
}
