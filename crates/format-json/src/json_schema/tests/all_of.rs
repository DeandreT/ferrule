use ir::{Instance, Value};

use super::*;

#[test]
fn object_all_of_flattens_refs_nested_composition_and_equal_properties() {
    let schema = import_str(
        r##"{
  "title": "Profile",
  "type": "object",
  "properties": {
    "id": { "type": "integer" }
  },
  "allOf": [
    {},
    true,
    { "$ref": "#/$defs/address" },
    {
      "type": "object",
      "properties": {
        "active": { "type": "boolean" }
      }
    },
    {
      "allOf": [
        {
          "type": "object",
          "properties": {
            "id": { "type": "integer" }
          }
        },
        {
          "type": "object",
          "properties": {
            "score": { "type": "number" }
          }
        }
      ]
    }
  ],
  "$defs": {
    "address": {
      "type": "object",
      "properties": {
        "street": { "type": "string" }
      }
    }
  }
}"##,
    );
    let SchemaKind::Group {
        children,
        alternatives,
        dynamic,
        ..
    } = &schema.kind
    else {
        panic!("object allOf should flatten to a group");
    };
    assert_eq!(
        children
            .iter()
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>(),
        ["id", "street", "active", "score"]
    );
    assert!(alternatives.is_empty());
    assert!(dynamic.is_none());

    let input = r#"{"id":7,"street":"Main","active":true,"score":9.5}"#;
    let instance = crate::from_str(input, &schema).unwrap();
    assert_eq!(
        instance.field("id").and_then(Instance::as_scalar),
        Some(&Value::Int(7))
    );
    let rendered = crate::to_string(&schema, &instance).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&rendered).unwrap(),
        serde_json::from_str::<serde_json::Value>(input).unwrap()
    );

    let flattened = export(&schema);
    assert!(!flattened.contains("\"allOf\""));
    assert_eq!(import_str(&flattened), schema);
}

#[test]
fn object_all_of_preserves_compatible_dynamic_fields_and_closed_intersections() {
    let open = import_str(
        r#"{
  "title": "Open",
  "allOf": [
    {
      "type": "object",
      "additionalProperties": { "type": "integer" }
    },
    {
      "type": "object",
      "additionalProperties": { "type": "integer" }
    }
  ]
}"#,
    );
    assert!(matches!(
        open.dynamic_fields().map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));

    let closed = import_str(
        r#"{
  "title": "Closed",
  "allOf": [
    {
      "type": "object",
      "additionalProperties": { "type": "integer" }
    },
    {
      "type": "object",
      "additionalProperties": false,
      "properties": { "known": { "type": "string" } }
    }
  ]
}"#,
    );
    assert!(closed.dynamic_fields().is_none());
    assert!(closed.child("known").is_some());
}

#[test]
fn object_all_of_rejects_non_objects_and_incompatible_intersections() {
    for (label, body, expected) in [
        (
            "empty",
            r#""allOf": []"#,
            "allOf must contain at least one object schema",
        ),
        (
            "scalar",
            r#""allOf": [{"type":"object"},{"type":"string"}]"#,
            "allOf branches must resolve to object schemas",
        ),
        (
            "property",
            r#""allOf": [
              {"type":"object","properties":{"value":{"type":"string"}}},
              {"type":"object","properties":{"value":{"type":"integer"}}}
            ]"#,
            "allOf property `value` has incompatible schemas",
        ),
        (
            "dynamic",
            r#""allOf": [
              {"type":"object","additionalProperties":{"type":"string"}},
              {"type":"object","additionalProperties":{"type":"integer"}}
            ]"#,
            "incompatible additionalProperties schemas",
        ),
        (
            "alternative",
            r#""allOf": [
              {
                "oneOf": [
                  {"title":"left","type":"object","additionalProperties":false,"properties":{"left":{"type":"string"}}},
                  {"title":"right","type":"object","additionalProperties":false,"properties":{"right":{"type":"string"}}}
                ]
              },
              {"type":"object"}
            ]"#,
            "allOf branches cannot contain object alternatives",
        ),
    ] {
        let error = import_str_result(&format!(r#"{{"title":"{label}",{body}}}"#)).unwrap_err();
        assert!(
            matches!(
                error,
                JsonFormatError::UnsupportedSchemaUnion { ref reason, .. }
                    if reason.contains(expected)
            ),
            "{label}: {error}"
        );
    }
}
