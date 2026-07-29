use ir::{Instance, ItemCountRange, SchemaKind, Value};

use super::{export, import_str, import_str_result};
use crate::JsonFormatError;

#[test]
fn ordinary_nullable_and_referenced_array_counts_roundtrip() -> Result<(), JsonFormatError> {
    let ordinary = import_str(
        r#"{
  "title":"Values",
  "type":"array",
  "minItems":2,
  "maxItems":4,
  "items":{"type":"integer"}
}"#,
    );
    let Some(range) = ordinary.item_count_range else {
        panic!("ordinary item-count range should be retained");
    };
    assert_eq!(range.minimum(), 2);
    assert_eq!(range.maximum(), Some(4));
    assert!(crate::from_str("[1,2]", &ordinary).is_ok());
    assert!(crate::from_str("[1]", &ordinary).is_err());
    assert!(crate::from_str("[1,2,3,4,5]", &ordinary).is_err());
    let rendered: serde_json::Value = serde_json::from_str(&export(&ordinary))?;
    assert_eq!(rendered.get("minItems"), Some(&serde_json::json!(2)));
    assert_eq!(rendered.get("maxItems"), Some(&serde_json::json!(4)));
    assert!(
        rendered
            .get("items")
            .and_then(|items| items.get("minItems"))
            .is_none()
    );
    assert_eq!(import_str(&export(&ordinary)), ordinary);

    let nullable = import_str(
        r#"{
  "title":"Nullable",
  "anyOf":[
    {"type":"array","minItems":2,"items":{"type":"string"}},
    {"type":"null"}
  ],
  "maxItems":3
}"#,
    );
    assert!(nullable.container_nullable);
    let Some(range) = nullable.item_count_range else {
        panic!("nullable item-count intersection should be retained");
    };
    assert_eq!(range.minimum(), 2);
    assert_eq!(range.maximum(), Some(3));
    assert!(crate::from_str("null", &nullable).is_ok());
    assert!(crate::from_str(r#"["a","b"]"#, &nullable).is_ok());
    assert!(crate::from_str(r#"["a"]"#, &nullable).is_err());

    let referenced = import_str(
        r##"{
  "title":"Referenced",
  "$defs":{
    "Rows":{"type":"array","minItems":1,"items":{"type":"boolean"}}
  },
  "$ref":"#/$defs/Rows",
  "maxItems":2
}"##,
    );
    let Some(range) = referenced.item_count_range else {
        panic!("ref sibling item-count range should intersect");
    };
    assert_eq!(range.minimum(), 1);
    assert_eq!(range.maximum(), Some(2));
    Ok(())
}

#[test]
fn all_of_delays_and_intersects_array_count_constraints() {
    let schema = import_str(
        r#"{
  "title":"Composed",
  "allOf":[
    {"minItems":2},
    {"type":"array","maxItems":8,"items":{"type":"string"}},
    {"minItems":4,"maxItems":6}
  ]
}"#,
    );
    let Some(range) = schema.item_count_range else {
        panic!("allOf item-count intersection should be retained");
    };
    assert_eq!(range.minimum(), 4);
    assert_eq!(range.maximum(), Some(6));
    assert!(crate::from_str(r#"["a","b","c","d"]"#, &schema).is_ok());
    assert!(crate::from_str(r#"["a","b","c"]"#, &schema).is_err());
    let reversed = import_str(
        r#"{
  "title":"Composed",
  "allOf":[
    {"minItems":4,"maxItems":6},
    {"type":"array","maxItems":8,"items":{"type":"string"}},
    {"minItems":2}
  ]
}"#,
    );
    assert_eq!(reversed, schema);

    assert!(matches!(
        import_str_result(
            r#"{
  "title":"Empty",
  "allOf":[
    {"type":"array","minItems":3,"items":{"type":"string"}},
    {"maxItems":2}
  ]
}"#
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("empty intersection")
    ));

    let scalar = import_str(r#"{"title":"Scalar","allOf":[{"type":"string"},{"minItems":2}]}"#);
    assert!(matches!(scalar.kind, SchemaKind::Scalar { .. }));
    assert!(scalar.item_count_range.is_none());
}

#[test]
fn malformed_ambiguous_and_nested_count_constraints_reject() {
    for (schema, expected) in [
        (
            r#"{"title":"x","type":"array","minItems":-1,"items":{}}"#,
            "exact non-negative integer",
        ),
        (
            r#"{"title":"x","type":"array","maxItems":1.0,"items":{}}"#,
            "exact non-negative integer",
        ),
        (
            r#"{"title":"x","type":"array","minItems":"1","items":{}}"#,
            "exact non-negative integer",
        ),
        (
            r#"{"title":"x","type":"array","minItems":3,"maxItems":2,"items":{}}"#,
            "must not exceed",
        ),
        (
            r#"{"title":"x","minItems":1}"#,
            "without a concrete array type",
        ),
        (
            r#"{
  "title":"x",
  "type":"array",
  "minItems":1,
  "items":{"type":"array","maxItems":2,"items":{"type":"string"}}
}"#,
            "distinct wrapper levels",
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

    let scalar = import_str(r#"{"title":"Scalar","type":"string","minItems":2}"#);
    assert!(scalar.item_count_range.is_none());
    let unconstrained = import_str(r#"{"title":"Anything","minItems":0}"#);
    assert!(unconstrained.item_count_range.is_none());

    let unresolved_noop = import_str(r##"{"title":"Noop","$ref":"#/missing","minItems":0}"##);
    assert!(unresolved_noop.item_count_range.is_none());
    assert!(matches!(
        import_str_result(r##"{"title":"Broken","$ref":"#/missing","minItems":"0"}"##),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("exact non-negative integer")
    ));
    assert!(matches!(
        import_str_result(r##"{"title":"Constrained","$ref":"#/missing","minItems":1}"##),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("unresolved or cyclic")
    ));

    let maximum = import_str(&format!(
        r#"{{"title":"Maximum","type":"array","maxItems":{},"items":{{"type":"string"}}}}"#,
        u64::MAX
    ));
    assert_eq!(
        maximum.item_count_range.and_then(ItemCountRange::maximum),
        Some(u64::MAX)
    );
    let rendered = export(&maximum);
    assert!(rendered.contains(&u64::MAX.to_string()));
    assert!(matches!(
        import_str_result(
            r#"{"title":"Overflow","type":"array","maxItems":18446744073709551616,"items":{}}"#
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("exact non-negative integer")
    ));
}

#[test]
fn array_any_of_requires_exact_item_and_count_domain_containment() {
    let unconstrained = import_str(
        r#"{
  "title":"Contiguous",
  "anyOf":[
    {"type":"array","maxItems":2,"items":{"type":"string"}},
    {"type":"array","minItems":3,"items":{"type":"string"}}
  ]
}"#,
    );
    assert!(unconstrained.item_count_range.is_none());

    let contained = import_str(
        r#"{
  "title":"Contained",
  "anyOf":[
    {
      "type":"array",
      "maxItems":10,
      "items":{"type":["string","integer"]}
    },
    {
      "type":"array",
      "minItems":2,
      "maxItems":3,
      "items":{"type":"string"}
    }
  ]
}"#,
    );
    let Some(range) = contained.item_count_range else {
        panic!("broader array count domain should be retained");
    };
    assert_eq!(range.minimum(), 0);
    assert_eq!(range.maximum(), Some(10));

    for schema in [
        r#"{
  "title":"Arbitrary",
  "anyOf":[
    {"type":"array","items":{}},
    {"type":"array","items":{"type":"string"}}
  ]
}"#,
        r#"{
  "title":"Arbitrary",
  "anyOf":[
    {"type":"array","items":{"type":"string"}},
    {"type":"array","items":{}}
  ]
}"#,
    ] {
        let arbitrary = import_str(schema);
        assert!(arbitrary.json_any);
        assert!(crate::from_str(r#"[1,true,{"nested":[]},null]"#, &arbitrary).is_ok());
    }

    for schema in [
        r#"{
  "title":"Bridged",
  "anyOf":[
    {"type":"array","maxItems":0,"items":{"type":"string"}},
    {"type":"array","minItems":2,"maxItems":2,"items":{"type":"string"}},
    {"type":"array","minItems":1,"maxItems":1,"items":{"type":"string"}}
  ]
}"#,
        r#"{
  "title":"Bridged",
  "anyOf":[
    {"type":"array","minItems":1,"maxItems":1,"items":{"type":"string"}},
    {"type":"array","maxItems":0,"items":{"type":"string"}},
    {"type":"array","minItems":2,"maxItems":2,"items":{"type":"string"}}
  ]
}"#,
    ] {
        let bridged = import_str(schema);
        let Some(range) = bridged.item_count_range else {
            panic!("the three contiguous singleton intervals should merge");
        };
        assert_eq!(range.minimum(), 0);
        assert_eq!(range.maximum(), Some(2));
    }

    let broad_last = import_str(
        r#"{
  "title":"BroadLast",
  "anyOf":[
    {"type":"array","items":{"type":"string"}},
    {"type":"array","items":{"type":"integer"}},
    {"type":"array","items":{"type":["string","integer"]}}
  ]
}"#,
    );
    assert!(crate::from_str(r#"["a",1]"#, &broad_last).is_ok());

    for schema in [
        r#"{
  "title":"Disjoint",
  "anyOf":[
    {"type":"array","maxItems":1,"items":{"type":"string"}},
    {"type":"array","minItems":3,"maxItems":4,"items":{"type":"string"}}
  ]
}"#,
        r#"{
  "title":"ConstrainedItems",
  "anyOf":[
    {"type":"array","items":{"type":"integer","minimum":5}},
    {"type":"array","items":{"type":"integer","maximum":0}}
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
fn count_validation_precedes_item_validation() {
    let schema = import_str(
        r#"{
  "title":"Values",
  "type":"array",
  "minItems":2,
  "items":{"type":"integer"}
}"#,
    );
    assert!(matches!(
        crate::from_str("[true]", &schema),
        Err(JsonFormatError::ItemCountMismatch { .. })
    ));
    assert!(matches!(
        crate::to_string(
            &schema,
            &Instance::Repeated(vec![Instance::Scalar(Value::Bool(true))])
        ),
        Err(JsonFormatError::ItemCountMismatch { .. })
    ));
}

#[test]
fn optional_positive_minimum_preserves_absence_but_rejects_present_empty() {
    let optional = import_str(
        r#"{
  "title":"Root",
  "type":"object",
  "properties":{
    "Rows":{"type":"array","minItems":1,"items":{"type":"integer"}}
  }
}"#,
    );
    let absent = crate::from_str("{}", &optional);
    let Ok(absent) = absent else {
        panic!("absent optional array should parse");
    };
    assert!(matches!(
        crate::to_string(&optional, &absent),
        Ok(ref rendered) if rendered == "{}\n"
    ));
    assert!(matches!(
        crate::from_str(r#"{"Rows":[]}"#, &optional),
        Err(JsonFormatError::ItemCountMismatch { .. })
    ));

    let required = import_str(
        r#"{
  "title":"Root",
  "type":"object",
  "properties":{
    "Rows":{"type":"array","minItems":1,"items":{"type":"integer"}}
  },
  "required":["Rows"]
}"#,
    );
    let output = Instance::Group(vec![("Rows".into(), Instance::Repeated(Vec::new()))]);
    assert!(matches!(
        crate::to_string(&required, &output),
        Err(JsonFormatError::MissingRequiredProperty { .. })
    ));
}

#[test]
fn json_lines_enforce_the_logical_root_collection_count() {
    let Some(range) = ItemCountRange::new(2, Some(3)) else {
        panic!("test count range is valid");
    };
    let Some(schema) = ir::SchemaNode::scalar("Row", ir::ScalarType::Int)
        .repeating()
        .with_item_count_range(range)
    else {
        panic!("test schema is valid");
    };
    assert!(matches!(
        crate::from_lines("1\n", &schema),
        Err(JsonFormatError::ItemCountMismatch { .. })
    ));
    assert!(matches!(
        crate::to_lines(
            &schema,
            &Instance::Repeated(vec![Instance::Scalar(Value::Int(1))])
        ),
        Err(JsonFormatError::ItemCountMismatch { .. })
    ));
    assert!(matches!(
        crate::to_lines(&schema, &Instance::Scalar(Value::Int(1))),
        Err(JsonFormatError::ItemCountMismatch { .. })
    ));
    assert!(matches!(
        crate::from_lines("1\n2\n", &schema),
        Ok(Instance::Repeated(ref items))
            if items
                == &vec![
                    Instance::Scalar(Value::Int(1)),
                    Instance::Scalar(Value::Int(2))
                ]
    ));
    assert!(matches!(
        crate::to_lines(
            &schema,
            &Instance::Repeated(vec![
                Instance::Scalar(Value::Int(1)),
                Instance::Scalar(Value::Int(2))
            ])
        ),
        Ok(ref rendered) if rendered == "1\n2\n"
    ));
    assert!(matches!(
        crate::from_str("[1]", &schema),
        Err(JsonFormatError::ItemCountMismatch { .. })
    ));

    let Some(maximum) = ItemCountRange::new(0, Some(1)) else {
        panic!("test count range is valid");
    };
    let Some(single) = ir::SchemaNode::scalar("Row", ir::ScalarType::Int)
        .repeating()
        .with_item_count_range(maximum)
    else {
        panic!("test schema is valid");
    };
    assert!(matches!(
        crate::to_lines(&single, &Instance::Scalar(Value::Int(1))),
        Ok(ref rendered) if rendered == "1\n"
    ));

    let mut nullable = single;
    nullable.container_nullable = true;
    assert!(matches!(
        crate::from_lines("1\n", &nullable),
        Err(JsonFormatError::NullableJsonLinesContainer { .. })
    ));
    assert!(matches!(
        crate::to_lines(
            &nullable,
            &Instance::Repeated(vec![Instance::Scalar(Value::Int(1))])
        ),
        Err(JsonFormatError::NullableJsonLinesContainer { .. })
    ));
}
