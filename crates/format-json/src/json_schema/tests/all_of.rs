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
            "score": { "type": "number" },
            "limit": { "type": "number" }
          }
        },
        {
          "type": "object",
          "properties": {
            "limit": { "type": "integer" }
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
        ["id", "street", "active", "score", "limit"]
    );
    assert!(matches!(
        schema.child("limit").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));
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

    let narrowed = import_str(
        r#"{
  "title": "Narrowed",
  "allOf": [
    {
      "type": "object",
      "additionalProperties": { "type": "number" }
    },
    {
      "type": "object",
      "additionalProperties": { "type": "integer" }
    }
  ]
}"#,
    );
    assert!(matches!(
        narrowed.dynamic_fields().map(|node| &node.kind),
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
fn scalar_all_of_intersects_domains_and_nullability() {
    let schema = import_str(
        r#"{
  "title": "Values",
  "type": "object",
  "properties": {
    "code": {
      "allOf": [
        { "type": ["string", "integer"] },
        { "type": ["string", "number"] }
      ]
    },
    "count": {
      "allOf": [
        { "type": "number" },
        { "type": "integer" }
      ]
    },
    "label": {
      "allOf": [
        { "type": ["string", "null"] },
        { "type": "string" }
      ]
    },
    "amount": {
      "allOf": [
        { "type": "number" },
        { "type": "number" }
      ]
    }
  }
}"#,
    );

    let code = schema.child("code").unwrap();
    let SchemaKind::ScalarUnion { types } = code.kind else {
        panic!("code should retain the string/integer intersection");
    };
    assert!(types.contains(ScalarType::String));
    assert!(types.contains(ScalarType::Int));
    assert!(!types.contains(ScalarType::Float));
    assert!(!code.nullable);
    assert!(matches!(
        schema.child("count").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));
    assert!(matches!(
        schema
            .child("label")
            .map(|node| (&node.kind, node.nullable)),
        Some((
            SchemaKind::Scalar {
                ty: ScalarType::String
            },
            false
        ))
    ));
    assert!(matches!(
        schema.child("amount").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Float
        })
    ));

    let input = r#"{"code":7,"count":3,"label":"ready","amount":4}"#;
    let instance = crate::from_str(input, &schema).unwrap();
    assert_eq!(
        instance.field("code").and_then(Instance::as_scalar),
        Some(&Value::Int(7))
    );
    assert_eq!(
        instance.field("amount").and_then(Instance::as_scalar),
        Some(&Value::Float(4.0))
    );
    let exported = export(&schema);
    assert!(!exported.contains("\"allOf\""));
    assert_eq!(import_str(&exported), schema);
}

#[test]
fn unconstrained_ref_is_an_order_independent_all_of_identity() {
    let schema = import_str(
        r##"{
  "title":"Identity",
  "type":"object",
  "properties":{
    "patternFirst":{
      "allOf":[
        {"$ref":"#/$defs/Any"},
        {"type":"string","pattern":"^A$"}
      ]
    },
    "patternLast":{
      "allOf":[
        {"type":"string","pattern":"^A$"},
        {"$ref":"#/$defs/Any"}
      ]
    },
    "integerFirst":{
      "allOf":[{"$ref":"#/$defs/Any"},{"type":"integer"}]
    },
    "integerLast":{
      "allOf":[{"type":"integer"},{"$ref":"#/$defs/Any"}]
    },
    "objectFirst":{
      "allOf":[
        {"$ref":"#/$defs/Any"},
        {"type":"object","properties":{"value":{"type":"boolean"}}}
      ]
    },
    "objectLast":{
      "allOf":[
        {"type":"object","properties":{"value":{"type":"boolean"}}},
        {"$ref":"#/$defs/Any"}
      ]
    }
  },
  "$defs":{"Any":{}}
}"##,
    );

    for name in ["patternFirst", "patternLast"] {
        let child = schema
            .child(name)
            .unwrap_or_else(|| panic!("{name} is present"));
        assert!(child.json_patterns.is_some(), "{name}");
    }
    for name in ["integerFirst", "integerLast"] {
        assert!(matches!(
            schema.child(name).map(|child| &child.kind),
            Some(SchemaKind::Scalar {
                ty: ScalarType::Int
            })
        ));
    }
    for name in ["objectFirst", "objectLast"] {
        assert!(
            schema
                .child(name)
                .and_then(|child| child.child("value"))
                .is_some()
        );
    }

    assert!(
        crate::from_str(
            r#"{
  "patternFirst":"A",
  "patternLast":"A",
  "integerFirst":1,
  "integerLast":2,
  "objectFirst":{"value":true},
  "objectLast":{"value":false}
}"#,
            &schema
        )
        .is_ok()
    );
    assert!(matches!(
        crate::from_str(r#"{"patternFirst":"B"}"#, &schema),
        Err(crate::JsonFormatError::PatternMismatch { .. })
    ));
    assert_eq!(import_str(&export(&schema)), schema);
}

#[test]
fn array_all_of_intersects_scalar_and_object_item_shapes() {
    let scalar = import_str(
        r#"{
  "title": "ScalarRows",
  "allOf": [
    {
      "type": ["array", "null"],
      "items": { "type": ["string", "integer"] }
    },
    {
      "type": "array",
      "items": { "type": ["string", "number"] }
    }
  ]
}"#,
    );
    assert!(scalar.repeating);
    assert!(!scalar.container_nullable);
    let SchemaKind::ScalarUnion { types } = scalar.kind else {
        panic!("array items should retain the string/integer intersection");
    };
    assert!(types.contains(ScalarType::String));
    assert!(types.contains(ScalarType::Int));
    assert!(!types.contains(ScalarType::Float));

    let objects = import_str(
        r#"{
  "title": "ObjectRows",
  "allOf": [
    {
      "type": "array",
      "items": {
        "type": "object",
        "properties": { "left": { "type": "string" } }
      }
    },
    {
      "type": "array",
      "items": {
        "type": "object",
        "properties": { "right": { "type": "integer" } }
      }
    }
  ]
}"#,
    );
    assert!(objects.repeating);
    assert!(objects.child("left").is_some());
    assert!(objects.child("right").is_some());

    let input = r#"[{"left":"a","right":1},{"left":"b","right":2}]"#;
    let instance = crate::from_str(input, &objects).unwrap();
    let rendered = crate::to_string(&objects, &instance).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&rendered).unwrap(),
        serde_json::from_str::<serde_json::Value>(input).unwrap()
    );
    assert_eq!(import_str(&export(&objects)), objects);
}

#[test]
fn all_of_rejects_incompatible_intersections() {
    for (label, body, expected) in [
        (
            "empty",
            r#""allOf": []"#,
            "allOf must contain at least one schema",
        ),
        (
            "scalar",
            r#""allOf": [{"type":"object"},{"type":"string"}]"#,
            "incompatible scalar and object shapes",
        ),
        (
            "disjoint-scalars",
            r#""allOf": [{"type":"integer"},{"type":"string"}]"#,
            "no value type in common",
        ),
        (
            "array-and-scalar",
            r#""allOf": [{"type":"array","items":{"type":"string"}},{"type":"string"}]"#,
            "incompatible array and non-array shapes",
        ),
        (
            "false",
            r#""allOf": [{"type":"object"},false]"#,
            "always-invalid false schema",
        ),
        (
            "property",
            r#""allOf": [
              {"type":"object","properties":{"value":{"type":"string"}}},
              {"type":"object","properties":{"value":{"type":"integer"}}}
            ]"#,
            "no value type in common",
        ),
        (
            "dynamic",
            r#""allOf": [
              {"type":"object","additionalProperties":{"type":"string"}},
              {"type":"object","additionalProperties":{"type":"integer"}}
            ]"#,
            "no value type in common",
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
