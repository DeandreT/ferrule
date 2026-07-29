use ir::{Instance, NumericRange, ScalarType, SchemaKind, SchemaNode, Value};

use super::{export, import_str, import_str_result};
use crate::JsonFormatError;

#[test]
fn integer_ranges_normalize_modern_and_legacy_exclusive_bounds() -> Result<(), JsonFormatError> {
    let modern = import_str(
        r#"{
  "title":"Count",
  "type":"integer",
  "minimum":1.2,
  "exclusiveMaximum":5
}"#,
    );
    let Some(NumericRange::Integer(range)) = modern.numeric_range else {
        panic!("integer range should be retained");
    };
    assert_eq!(range.minimum(), Some(2));
    assert_eq!(range.maximum(), Some(4));
    assert!(crate::from_str("2", &modern).is_ok());
    assert!(crate::from_str("4", &modern).is_ok());
    assert!(matches!(
        crate::from_str("1", &modern),
        Err(JsonFormatError::RangeMismatch { .. })
    ));
    assert!(matches!(
        crate::to_string(&modern, &Instance::Scalar(Value::Int(5))),
        Err(JsonFormatError::RangeMismatch { .. })
    ));

    let legacy = import_str(
        r#"{
  "$schema":"http://json-schema.org/draft-04/schema#",
  "title":"Legacy",
  "type":"integer",
  "minimum":1,
  "exclusiveMinimum":true,
  "maximum":5,
  "exclusiveMaximum":false
}"#,
    );
    let Some(NumericRange::Integer(range)) = legacy.numeric_range else {
        panic!("legacy integer range should be retained");
    };
    assert_eq!(range.minimum(), Some(2));
    assert_eq!(range.maximum(), Some(5));

    let rendered: serde_json::Value = serde_json::from_str(&export(&legacy))?;
    assert_eq!(rendered.get("minimum"), Some(&serde_json::json!(2)));
    assert_eq!(rendered.get("maximum"), Some(&serde_json::json!(5)));
    assert!(rendered.get("exclusiveMinimum").is_none());
    assert!(rendered.get("exclusiveMaximum").is_none());
    assert_eq!(import_str(&export(&legacy)), legacy);
    Ok(())
}

#[test]
fn integer_ranges_preserve_large_i64_endpoints_and_reject_inexact_fractional_bounds() {
    let schema = import_str(
        r#"{
  "title":"Large",
  "type":"integer",
  "minimum":9007199254740993,
  "maximum":9223372036854775807
}"#,
    );
    let Some(NumericRange::Integer(range)) = schema.numeric_range else {
        panic!("large integer range should be retained");
    };
    assert_eq!(range.minimum(), Some(9_007_199_254_740_993));
    assert_eq!(range.maximum(), Some(i64::MAX));
    assert!(crate::from_str("9007199254740993", &schema).is_ok());
    assert!(crate::from_str("9007199254740992", &schema).is_err());

    for schema in [
        r#"{"title":"LargeInexact","type":"integer","minimum":9007199254740992.5}"#,
        r#"{"title":"UnitCliff","type":"integer","minimum":4503599627370496.5}"#,
        r#"{"title":"RoundedAbove","type":"integer","minimum":1.00000000000000001}"#,
        r#"{"title":"RoundedBelow","type":"integer","maximum":0.99999999999999999}"#,
        r#"{"title":"IntegralFloat","type":"integer","exclusiveMinimum":1.0}"#,
    ] {
        assert!(matches!(
            import_str_result(schema),
            Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
                if reason.contains("exactly normalizable")
        ));
    }
}

#[test]
fn number_ranges_preserve_finite_endpoints_exclusivity_and_nullable_null()
-> Result<(), JsonFormatError> {
    let schema = import_str(
        r#"{
  "title":"Ratio",
  "type":["number","null"],
  "exclusiveMinimum":0.25,
  "maximum":2.5
}"#,
    );
    let Some(NumericRange::Number(range)) = schema.numeric_range else {
        panic!("number range should be retained");
    };
    let Some(minimum) = range.minimum() else {
        panic!("minimum should be retained");
    };
    assert_eq!(minimum.value().get(), 0.25);
    assert!(minimum.is_exclusive());
    let Some(maximum) = range.maximum() else {
        panic!("maximum should be retained");
    };
    assert_eq!(maximum.value().get(), 2.5);
    assert!(!maximum.is_exclusive());
    assert!(schema.nullable);
    assert!(crate::from_str("null", &schema).is_ok());
    assert!(crate::from_str("0.25", &schema).is_err());
    assert!(crate::from_str("0.5", &schema).is_ok());
    assert!(crate::from_str("2.5", &schema).is_ok());

    let rendered: serde_json::Value = serde_json::from_str(&export(&schema))?;
    assert_eq!(
        rendered.get("exclusiveMinimum"),
        Some(&serde_json::json!(0.25))
    );
    assert_eq!(rendered.get("maximum"), Some(&serde_json::json!(2.5)));

    let composed = import_str(
        r#"{
  "title":"Composed",
  "oneOf":[
    {"type":"number","minimum":10},
    {"type":"null"}
  ]
}"#,
    );
    assert!(composed.nullable);
    assert!(composed.numeric_range.is_some());
    assert!(crate::from_str("null", &composed).is_ok());
    assert!(crate::from_str("9", &composed).is_err());
    Ok(())
}

#[test]
fn const_and_all_of_ranges_intersect_without_widening() {
    let fixed =
        import_str(r#"{"title":"Fixed","type":"integer","const":3,"minimum":1,"maximum":3}"#);
    assert_eq!(fixed.fixed.as_deref(), Some("3"));
    assert!(fixed.numeric_range.is_some());
    assert!(crate::from_str("3", &fixed).is_ok());

    assert!(matches!(
        import_str_result(
            r#"{"title":"Outside","type":"integer","const":0,"minimum":1,"maximum":3}"#
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("outside")
    ));

    let intersected = import_str(
        r#"{
  "title":"Intersected",
  "allOf":[
    {"type":"integer","minimum":0,"maximum":20},
    {"type":"integer","exclusiveMinimum":4,"maximum":10},
    {"type":"integer","minimum":7}
  ]
}"#,
    );
    let Some(NumericRange::Integer(range)) = intersected.numeric_range else {
        panic!("allOf range should be retained");
    };
    assert_eq!(range.minimum(), Some(7));
    assert_eq!(range.maximum(), Some(10));
    assert!(crate::from_str("7", &intersected).is_ok());
    assert!(crate::from_str("6", &intersected).is_err());

    assert!(matches!(
        import_str_result(
            r#"{
  "title":"Empty",
  "allOf":[
    {"type":"number","minimum":1},
    {"type":"number","exclusiveMaximum":1}
  ]
}"#
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("empty intersection")
    ));
}

#[test]
fn malformed_unrepresentable_and_union_ranges_reject_actionably() {
    for (schema, expected) in [
        (
            r#"{"title":"x","type":"integer","minimum":"1"}"#,
            "`minimum` must be a number",
        ),
        (
            r#"{"title":"x","type":"number","exclusiveMaximum":null}"#,
            "number or legacy boolean",
        ),
        (
            r#"{"title":"x","type":"integer","exclusiveMinimum":true}"#,
            "requires `minimum`",
        ),
        (
            r#"{"title":"x","minimum":1}"#,
            "without a concrete numeric scalar type",
        ),
        (
            r#"{"title":"x","type":["integer","number"],"minimum":1}"#,
            "general scalar unions",
        ),
        (
            r#"{"title":"x","type":"integer","minimum":5,"exclusiveMaximum":5}"#,
            "no signed 64-bit integer values",
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

#[test]
fn manual_range_metadata_validates_output_coercion_and_nullable_values() {
    let Some(range) = ir::IntegerRange::new(Some(5), Some(8)) else {
        panic!("test range is valid");
    };
    let Some(mut schema) = SchemaNode::scalar("Count", ScalarType::Int)
        .with_numeric_range(NumericRange::Integer(range))
    else {
        panic!("integer range matches integer schema");
    };
    assert!(matches!(
        crate::to_string(&schema, &Instance::Scalar(Value::String("7".into()))),
        Ok(ref text) if text == "7\n"
    ));
    assert!(matches!(
        crate::to_string(&schema, &Instance::Scalar(Value::String("9".into()))),
        Err(JsonFormatError::RangeMismatch { .. })
    ));
    schema.nullable = true;
    assert!(crate::from_str("null", &schema).is_ok());
    assert_eq!(
        schema.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    );
}

#[test]
fn range_only_all_of_branches_wait_for_a_concrete_scalar_domain() {
    let schema = import_str(
        r#"{
  "title":"Delayed",
  "allOf":[
    {"minimum":1.2,"maximum":9.8},
    {"type":"integer"}
  ]
}"#,
    );
    let Some(NumericRange::Integer(range)) = schema.numeric_range else {
        panic!("delayed range should be normalized as an integer interval");
    };
    assert_eq!(range.minimum(), Some(2));
    assert_eq!(range.maximum(), Some(9));

    let fixed = import_str(r#"{"title":"FixedString","allOf":[{"minimum":1},{"const":"x"}]}"#);
    assert_eq!(fixed.fixed.as_deref(), Some("x"));
    assert!(fixed.numeric_range.is_none());

    let object = import_str(
        r#"{
  "title":"Object",
  "properties":{"value":{"type":"string"}},
  "minimum":1
}"#,
    );
    assert!(matches!(object.kind, SchemaKind::Group { .. }));
    assert!(crate::from_str(r#"{"value":"ok"}"#, &object).is_ok());
}

#[test]
fn nullable_wrapper_and_branch_ranges_intersect() {
    let schema = import_str(
        r#"{
  "title":"Nullable",
  "oneOf":[
    {"type":"number","minimum":2},
    {"type":"null"}
  ],
  "exclusiveMaximum":5
}"#,
    );
    let Some(NumericRange::Number(range)) = schema.numeric_range else {
        panic!("nullable wrapper range should be retained");
    };
    assert_eq!(range.minimum().map(|bound| bound.value().get()), Some(2.0));
    assert_eq!(range.maximum().map(|bound| bound.value().get()), Some(5.0));
    assert!(range.maximum().is_some_and(|bound| bound.is_exclusive()));
    assert!(crate::from_str("null", &schema).is_ok());
    assert!(crate::from_str("2", &schema).is_ok());
    assert!(crate::from_str("5", &schema).is_err());

    assert!(matches!(
        import_str_result(
            r#"{
  "title":"EmptyNullable",
  "anyOf":[{"type":"integer","minimum":5},{"type":"null"}],
  "maximum":4
}"#
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("empty intersection")
    ));
}

#[test]
fn array_level_ranges_do_not_constrain_array_items() {
    let schema = import_str(
        r#"{
  "title":"Values",
  "type":["array","null"],
  "minimum":5,
  "items":{"type":"integer","maximum":3}
}"#,
    );
    assert!(schema.repeating);
    assert!(schema.container_nullable);
    let Some(NumericRange::Integer(range)) = schema.numeric_range else {
        panic!("item range should be retained");
    };
    assert_eq!(range.minimum(), None);
    assert_eq!(range.maximum(), Some(3));
    assert!(crate::from_str("[1,3]", &schema).is_ok());
    assert!(crate::from_str("[4]", &schema).is_err());
    assert!(crate::from_str("null", &schema).is_ok());

    let string = import_str(
        r#"{
  "title":"Text",
  "type":"string",
  "minimum":18446744073709551616
}"#,
    );
    assert!(crate::from_str(r#""value""#, &string).is_ok());
}

#[test]
fn nullable_structured_wrappers_ignore_vacuous_numeric_ranges() {
    let array = import_str(
        r#"{
  "title":"NullableArray",
  "anyOf":[
    {"type":"array","items":{"type":"integer"}},
    {"type":"null"}
  ],
  "minimum":5
}"#,
    );
    assert!(array.repeating);
    assert!(crate::from_str("[1]", &array).is_ok());
    assert!(crate::from_str("null", &array).is_ok());

    let object = import_str(
        r#"{
  "title":"NullableObject",
  "oneOf":[
    {"type":"object","properties":{"value":{"type":"integer"}}},
    {"type":"null"}
  ],
  "maximum":9
}"#,
    );
    assert!(matches!(object.kind, SchemaKind::Group { .. }));
    assert!(crate::from_str(r#"{"value":10}"#, &object).is_ok());
    assert!(crate::from_str("null", &object).is_ok());

    let multi_object = import_str(
        r#"{
  "title":"NullableAlternatives",
  "anyOf":[
    {
      "type":"object",
      "properties":{"kind":{"const":"a"},"left":{"type":"string"}},
      "required":["kind","left"],
      "additionalProperties":false
    },
    {
      "type":"object",
      "properties":{"kind":{"const":"b"},"right":{"type":"integer"}},
      "required":["kind","right"],
      "additionalProperties":false
    },
    {"type":"null"}
  ],
  "minimum":1
}"#,
    );
    assert!(matches!(multi_object.kind, SchemaKind::Group { .. }));
    assert!(crate::from_str(r#"{"kind":"a","left":"ok"}"#, &multi_object).is_ok());
    assert!(crate::from_str("null", &multi_object).is_ok());
}

#[test]
fn mixed_dialect_bounds_are_permissively_intersected() {
    let schema = import_str(
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "title":"MixedDialect",
  "type":"integer",
  "minimum":1,
  "exclusiveMinimum":2,
  "maximum":8,
  "exclusiveMaximum":false
}"#,
    );
    let Some(NumericRange::Integer(range)) = schema.numeric_range else {
        panic!("mixed-dialect range should be retained");
    };
    assert_eq!(range.minimum(), Some(3));
    assert_eq!(range.maximum(), Some(8));

    let no_op = import_str(r#"{"title":"NoOp","type":"number","exclusiveMinimum":false}"#);
    assert!(no_op.numeric_range.is_none());
}

#[test]
fn mixed_number_integer_intersections_reject_precision_cliffs() {
    let exact = import_str(
        r#"{
  "title":"ExactIntegral",
  "allOf":[
    {"type":"number","exclusiveMinimum":9007199254740992},
    {"type":"integer"}
  ]
}"#,
    );
    let Some(NumericRange::Integer(range)) = exact.numeric_range else {
        panic!("exact integral number bound should survive integer intersection");
    };
    assert_eq!(range.minimum(), Some(9_007_199_254_740_993));

    for schema in [
        r#"{
  "title":"RoundedIntegral",
  "allOf":[
    {"type":"number","minimum":1.00000000000000001},
    {"type":"integer"}
  ]
}"#,
        r#"{
  "title":"FractionalCliff",
  "allOf":[
    {"type":"number","maximum":9007199254740991.5},
    {"type":"integer"}
  ]
}"#,
    ] {
        assert!(matches!(
            import_str_result(schema),
            Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
                if reason.contains("ambiguous")
        ));
    }
}
