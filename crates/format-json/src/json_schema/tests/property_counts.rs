use ir::{Instance, ItemCountRange, PropertyCountRange, SchemaKind, Value};

use super::{export, import_str, import_str_result};
use crate::JsonFormatError;

#[test]
fn ordinary_open_nullable_and_referenced_object_counts_roundtrip() -> Result<(), JsonFormatError> {
    let ordinary = import_str(
        r#"{
  "title":"Record",
  "type":"object",
  "minProperties":1,
  "maxProperties":2,
  "additionalProperties":false,
  "properties":{
    "label":{"type":["string","null"]},
    "count":{"type":"integer"}
  }
}"#,
    );
    let Some(range) = ordinary.property_count_range else {
        panic!("ordinary property-count range should be retained");
    };
    assert_eq!(range.minimum(), 1);
    assert_eq!(range.maximum(), Some(2));
    assert!(crate::from_str(r#"{"label":null}"#, &ordinary).is_ok());
    assert!(matches!(
        crate::from_str("{}", &ordinary),
        Err(JsonFormatError::PropertyCountMismatch { .. })
    ));
    assert!(matches!(
        crate::from_str(r#"{"label":"x","count":1,"extra":true}"#, &ordinary),
        Err(JsonFormatError::PropertyCountMismatch { .. })
    ));
    let rendered: serde_json::Value = serde_json::from_str(&export(&ordinary))?;
    assert_eq!(rendered.get("minProperties"), Some(&serde_json::json!(1)));
    assert_eq!(rendered.get("maxProperties"), Some(&serde_json::json!(2)));
    assert_eq!(import_str(&export(&ordinary)), ordinary);

    let nullable = import_str(
        r#"{
  "title":"Nullable",
  "type":["object","null"],
  "minProperties":1,
  "properties":{"label":{"type":"string"}},
  "additionalProperties":false
}"#,
    );
    assert!(nullable.container_nullable);
    assert!(crate::from_str("null", &nullable).is_ok());
    assert!(crate::from_str(r#"{"label":"x"}"#, &nullable).is_ok());
    assert!(matches!(
        crate::from_str("{}", &nullable),
        Err(JsonFormatError::PropertyCountMismatch { .. })
    ));

    let referenced = import_str(
        r##"{
  "$defs":{
    "Record":{
      "type":"object",
      "minProperties":1,
      "properties":{"label":{"type":"string"}},
      "additionalProperties":true
    }
  },
  "$ref":"#/$defs/Record",
  "maxProperties":2
}"##,
    );
    let Some(range) = referenced.property_count_range else {
        panic!("modern ref sibling property count should intersect");
    };
    assert_eq!(range.minimum(), 1);
    assert_eq!(range.maximum(), Some(2));
    Ok(())
}

#[test]
fn property_count_input_precedes_shape_checks_and_output_uses_emitted_properties() {
    let schema = import_str(
        r#"{
  "type":"object",
  "minProperties":1,
  "maxProperties":1,
  "properties":{"value":{"type":"string"}},
  "additionalProperties":false
}"#,
    );
    assert!(matches!(
        crate::from_str(r#"{"unknown":1,"other":2}"#, &schema),
        Err(JsonFormatError::PropertyCountMismatch { .. })
    ));
    assert!(matches!(
        crate::from_str(r#"{"unknown":1}"#, &schema),
        Err(JsonFormatError::UndeclaredProperty { .. })
    ));
    let duplicate = crate::from_str(r#"{"value":"first","value":"second"}"#, &schema);
    assert!(matches!(
        duplicate,
        Ok(Instance::Group(ref fields))
            if fields == &vec![(
                "value".into(),
                Instance::Scalar(Value::String("second".into()))
            )]
    ));

    let omitted = Instance::Group(vec![("value".into(), Instance::Scalar(Value::Null))]);
    assert!(matches!(
        crate::to_string(&schema, &omitted),
        Err(JsonFormatError::PropertyCountMismatch { .. })
    ));

    let nullable = import_str(
        r#"{
  "type":"object",
  "minProperties":1,
  "maxProperties":1,
  "properties":{"value":{"type":["string","null"]}},
  "additionalProperties":false
}"#,
    );
    let explicit_null =
        Instance::Group(vec![("value".into(), Instance::Scalar(Value::json_null()))]);
    assert!(matches!(
        crate::to_string(&nullable, &explicit_null),
        Ok(ref rendered) if rendered == "{\n  \"value\": null\n}\n"
    ));

    let dynamic = import_str(
        r#"{
  "type":"object",
  "minProperties":2,
  "maxProperties":2,
  "additionalProperties":{"type":"string"}
}"#,
    );
    let instance = crate::from_str(r#"{"first":"a","second":"b"}"#, &dynamic);
    let Ok(instance) = instance else {
        panic!("two dynamic fields should satisfy the object count");
    };
    assert!(crate::to_string(&dynamic, &instance).is_ok());
}

#[test]
fn all_of_intersects_property_counts_and_rejects_infeasible_objects() {
    let schema = import_str(
        r#"{
  "title":"Composed",
  "allOf":[
    {"minProperties":1},
    {
      "type":"object",
      "maxProperties":4,
      "properties":{"a":{"type":"string"},"b":{"type":"string"}},
      "additionalProperties":true
    },
    {"minProperties":2,"maxProperties":3}
  ]
}"#,
    );
    let Some(range) = schema.property_count_range else {
        panic!("allOf property-count intersection should be retained");
    };
    assert_eq!(range.minimum(), 2);
    assert_eq!(range.maximum(), Some(3));
    assert!(crate::from_str(r#"{"a":"x","other":1}"#, &schema).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"a":"x"}"#, &schema),
        Err(JsonFormatError::PropertyCountMismatch { .. })
    ));

    for invalid in [
        r#"{
  "allOf":[
    {"type":"object","minProperties":3,"additionalProperties":true},
    {"maxProperties":2}
  ]
}"#,
        r#"{
  "type":"object",
  "minProperties":2,
  "properties":{"only":{"type":"string"}},
  "additionalProperties":false
}"#,
        r#"{
  "type":"object",
  "maxProperties":1,
  "required":["a","b"],
  "properties":{"a":{"type":"string"},"b":{"type":"string"}},
  "additionalProperties":false
}"#,
    ] {
        assert!(matches!(
            import_str_result(invalid),
            Err(JsonFormatError::UnsupportedSchemaUnion { .. })
        ));
    }
}

#[test]
fn object_alternatives_lift_only_one_common_property_count_range() {
    let schema = import_str(
        r#"{
  "oneOf":[
    {
      "title":"a",
      "type":"object",
      "minProperties":1,
      "maxProperties":1,
      "required":["a"],
      "properties":{"a":{"type":"string"}},
      "additionalProperties":false
    },
    {
      "title":"b",
      "type":"object",
      "minProperties":1,
      "maxProperties":1,
      "required":["b"],
      "properties":{"b":{"type":"string"}},
      "additionalProperties":false
    }
  ]
}"#,
    );
    let Some(range) = schema.property_count_range else {
        panic!("identical alternative property counts should lift");
    };
    assert_eq!(range.minimum(), 1);
    assert_eq!(range.maximum(), Some(1));
    assert!(crate::from_str(r#"{"a":"x"}"#, &schema).is_ok());
    assert_eq!(import_str(&export(&schema)), schema);

    assert!(matches!(
        import_str_result(
            r#"{
  "anyOf":[
    {
      "title":"a",
      "type":"object",
      "minProperties":1,
      "required":["a"],
      "properties":{"a":{"type":"string"}},
      "additionalProperties":false
    },
    {
      "title":"b",
      "type":"object",
      "minProperties":2,
      "required":["b"],
      "properties":{"b":{"type":"string"},"other":{"type":"string"}},
      "additionalProperties":false
    }
  ]
}"#
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("differing property-count")
    ));
}

#[test]
fn malformed_ambiguous_and_dialect_specific_property_counts_are_exact() {
    for schema in [
        r#"{"type":"object","minProperties":-1}"#,
        r#"{"type":"object","maxProperties":1.0}"#,
        r#"{"type":"object","minProperties":"1"}"#,
        r#"{"type":"object","minProperties":2,"maxProperties":1}"#,
        r#"{"minProperties":1}"#,
        r#"{"additionalProperties":{"minProperties":1},"type":"object"}"#,
        r#"{"type":"object","maxProperties":18446744073709551616}"#,
    ] {
        assert!(import_str_result(schema).is_err(), "{schema}");
    }
    let scalar = import_str(r#"{"type":"string","minProperties":2}"#);
    assert!(matches!(scalar.kind, SchemaKind::Scalar { .. }));
    assert!(scalar.property_count_range.is_none());
    let no_op = import_str(r#"{"minProperties":0}"#);
    assert!(no_op.property_count_range.is_none());

    let modern = import_str(
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$defs":{
    "Record":{
      "type":"object",
      "minProperties":1,
      "properties":{"value":{"type":"string"}},
      "additionalProperties":true
    }
  },
  "$ref":"#/$defs/Record",
  "maxProperties":2
}"##,
    );
    assert_eq!(
        modern
            .property_count_range
            .and_then(PropertyCountRange::maximum),
        Some(2)
    );
    let legacy = import_str(
        r##"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "definitions":{
    "Record":{
      "type":"object",
      "minProperties":1,
      "properties":{"value":{"type":"string"}},
      "additionalProperties":true
    }
  },
  "$ref":"#/definitions/Record",
  "maxProperties":"ignored"
}"##,
    );
    assert_eq!(
        legacy.property_count_range.map(PropertyCountRange::minimum),
        Some(1)
    );
}

#[test]
fn repeating_objects_and_json_lines_enforce_row_and_property_counts_independently() {
    let schema = import_str(
        r#"{
  "type":"array",
  "minItems":2,
  "maxItems":2,
  "items":{
    "type":"object",
    "minProperties":1,
    "maxProperties":1,
    "properties":{"value":{"type":"integer"}},
    "additionalProperties":false
  }
}"#,
    );
    assert!(schema.repeating);
    assert_eq!(
        schema.item_count_range.map(ItemCountRange::minimum),
        Some(2)
    );
    assert_eq!(
        schema.property_count_range.map(PropertyCountRange::minimum),
        Some(1)
    );
    assert!(
        crate::from_lines(
            r#"{"value":1}
{"value":2}
"#,
            &schema
        )
        .is_ok()
    );
    assert!(matches!(
        crate::from_lines(r#"{"value":1}"#, &schema),
        Err(JsonFormatError::ItemCountMismatch { .. })
    ));
    assert!(matches!(
        crate::from_lines(
            r#"{"value":1}
{}
"#,
            &schema
        ),
        Err(JsonFormatError::PropertyCountMismatch { .. })
    ));
}

#[test]
fn export_rejects_corrupted_property_count_metadata() {
    let Some(range) = PropertyCountRange::new(1, None) else {
        panic!("test property-count range is valid");
    };
    let mut scalar = ir::SchemaNode::scalar("value", ir::ScalarType::String);
    scalar.property_count_range = Some(range);
    assert!(matches!(
        super::super::export(&scalar),
        Err(JsonFormatError::InvalidPropertyCountMetadata { .. })
    ));

    let mut nested =
        ir::SchemaNode::group("root", vec![ir::SchemaNode::group("record", Vec::new())]);
    let SchemaKind::Group { children, .. } = &mut nested.kind else {
        panic!("test root is a group");
    };
    children[0].property_count_range = Some(range);
    assert!(matches!(
        super::super::export(&nested),
        Err(JsonFormatError::InvalidPropertyCountMetadata { .. })
    ));
}
