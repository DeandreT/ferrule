use ir::{
    FiniteF64, Instance, JsonMultipleOf, JsonMultipleOfConstraints, JsonPatternConstraints,
    NumberBound, NumberRange, NumericRange, ScalarType, ScalarTypeSet, SchemaNode, Value,
};

use super::{export, import_str, import_str_result};
use crate::JsonFormatError;

#[test]
fn direct_integer_and_number_constraints_execute_without_epsilon() {
    let integer = import_str(
        r#"{
  "title":"Count",
  "type":"integer",
  "multipleOf":2.5
}"#,
    );
    assert!(crate::from_str("-5", &integer).is_ok());
    assert!(matches!(
        crate::from_str("6", &integer),
        Err(JsonFormatError::MultipleOfMismatch { .. })
    ));
    assert!(crate::to_string(&integer, &Instance::Scalar(Value::Int(10))).is_ok());
    assert!(matches!(
        crate::to_string(&integer, &Instance::Scalar(Value::Int(11))),
        Err(JsonFormatError::MultipleOfMismatch { .. })
    ));

    let number = import_str(
        r#"{
  "title":"Amount",
  "type":"number",
  "multipleOf":0.1
}"#,
    );
    assert!(crate::from_str("0.3", &number).is_ok());
    assert!(matches!(
        crate::from_str("0.30000000000000004", &number),
        Err(JsonFormatError::MultipleOfMismatch { .. })
    ));
    let Some(exact) = FiniteF64::new(0.3) else {
        panic!("0.3 is finite");
    };
    assert!(crate::to_string(&number, &Instance::Scalar(Value::Float(exact.get()))).is_ok());
}

#[test]
fn scalar_unions_constrain_only_numeric_runtime_tags() {
    let schema = import_str(
        r#"{
  "title":"Value",
  "type":["string","integer","null"],
  "multipleOf":2
}"#,
    );
    assert!(schema.json_multiple_of.is_some());
    assert!(crate::from_str(r#""unaffected""#, &schema).is_ok());
    assert!(crate::from_str("null", &schema).is_ok());
    assert!(crate::from_str("4", &schema).is_ok());
    assert!(matches!(
        crate::from_str("3", &schema),
        Err(JsonFormatError::MultipleOfMismatch { .. })
    ));
    assert!(
        crate::to_string(
            &schema,
            &Instance::Scalar(Value::String("unaffected".into()))
        )
        .is_ok()
    );
}

#[test]
fn malformed_untyped_and_fixed_multiples_reject_without_widening() {
    for schema in [
        r#"{"title":"Zero","type":"number","multipleOf":0}"#,
        r#"{"title":"Negative","type":"integer","multipleOf":-2}"#,
        r#"{"title":"Text","type":"number","multipleOf":"2"}"#,
        r#"{"title":"Rounded","type":"number","multipleOf":184467440737095516150}"#,
        r#"{"title":"IntegralFloat","type":"number","multipleOf":1e20}"#,
        r#"{"title":"Ambiguous","multipleOf":2}"#,
        r#"{"title":"Fixed","type":"integer","const":3,"multipleOf":2}"#,
    ] {
        assert!(matches!(
            import_str_result(schema),
            Err(JsonFormatError::UnsupportedSchemaUnion { .. })
        ));
    }

    let ignored = import_str(r#"{"title":"Text","type":"string","multipleOf":2}"#);
    assert!(ignored.json_multiple_of.is_none());
}

#[test]
fn all_of_and_nullable_wrappers_intersect_exact_divisors() {
    let schema = import_str(
        r#"{
  "title":"Value",
  "anyOf":[
    {
      "allOf":[
        {"type":"integer","multipleOf":2},
        {"multipleOf":3}
      ]
    },
    {"type":"null"}
  ]
}"#,
    );
    let Some(constraints) = schema.json_multiple_of.as_ref() else {
        panic!("allOf divisors should survive nullable composition");
    };
    assert_eq!(constraints.any_of().len(), 1);
    assert_eq!(constraints.any_of()[0].len(), 2);
    assert!(crate::from_str("12", &schema).is_ok());
    assert!(crate::from_str("null", &schema).is_ok());
    assert!(matches!(
        crate::from_str("9", &schema),
        Err(JsonFormatError::MultipleOfMismatch { .. })
    ));
}

#[test]
fn any_of_unions_divisors_and_retains_heterogeneous_bypass() {
    let numeric = import_str(
        r#"{
  "title":"Number",
  "anyOf":[
    {"type":"integer","multipleOf":2},
    {"type":"integer","multipleOf":3}
  ]
}"#,
    );
    let Some(constraints) = numeric.json_multiple_of.as_ref() else {
        panic!("numeric divisor alternatives should survive");
    };
    assert_eq!(constraints.any_of().len(), 2);
    assert!(crate::from_str("4", &numeric).is_ok());
    assert!(crate::from_str("9", &numeric).is_ok());
    assert!(matches!(
        crate::from_str("5", &numeric),
        Err(JsonFormatError::MultipleOfMismatch { .. })
    ));

    let heterogeneous = import_str(
        r#"{
  "title":"Value",
  "anyOf":[
    {"type":"string"},
    {"type":"integer","multipleOf":2}
  ]
}"#,
    );
    assert!(heterogeneous.accepts_scalar_type(ScalarType::String));
    assert!(heterogeneous.accepts_scalar_type(ScalarType::Int));
    assert!(heterogeneous.json_multiple_of.is_some());
    assert!(crate::from_str(r#""text""#, &heterogeneous).is_ok());
    assert!(crate::from_str("6", &heterogeneous).is_ok());
    assert!(matches!(
        crate::from_str("7", &heterogeneous),
        Err(JsonFormatError::MultipleOfMismatch { .. })
    ));

    let unconstrained_numeric = import_str(
        r#"{
  "title":"Value",
  "anyOf":[
    {"type":"integer","multipleOf":2},
    {"type":"integer"}
  ]
}"#,
    );
    assert!(unconstrained_numeric.json_multiple_of.is_none());
    assert!(crate::from_str("7", &unconstrained_numeric).is_ok());
}

#[test]
fn any_of_rejects_correlated_or_disjoint_numeric_axes() {
    let correlated = r#"{
  "title":"Correlated",
  "anyOf":[
    {"type":"integer","minimum":0,"multipleOf":2},
    {"type":"integer","minimum":10,"multipleOf":3}
  ]
}"#;
    assert!(matches!(
        import_str_result(correlated),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("correlates")
    ));

    let disjoint = r#"{
  "title":"Disjoint",
  "anyOf":[
    {"type":"integer","maximum":0,"multipleOf":2},
    {"type":"integer","minimum":10,"multipleOf":2}
  ]
}"#;
    assert!(matches!(
        import_str_result(disjoint),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("disjoint")
    ));

    let overlapping_one_of = r#"{
  "title":"Overlap",
  "oneOf":[
    {"type":"integer","multipleOf":2},
    {"type":"integer","multipleOf":3}
  ]
}"#;
    assert!(matches!(
        import_str_result(overlapping_one_of),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("overlap")
    ));

    let type_correlated = r#"{
  "title":"TypeCorrelated",
  "anyOf":[
    {"type":"integer"},
    {"type":"number","multipleOf":2}
  ]
}"#;
    assert!(matches!(
        import_str_result(type_correlated),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("numeric types")
    ));
}

#[test]
fn equal_divisors_allow_contiguous_range_union_and_signed_zero() {
    let contiguous = import_str(
        r#"{
  "title":"Window",
  "anyOf":[
    {"type":"integer","minimum":0,"maximum":4,"multipleOf":2},
    {"type":"integer","minimum":5,"maximum":10,"multipleOf":2}
  ]
}"#,
    );
    let Some(NumericRange::Integer(range)) = contiguous.numeric_range else {
        panic!("contiguous integer ranges should merge");
    };
    assert_eq!(range.minimum(), Some(0));
    assert_eq!(range.maximum(), Some(10));

    let Some(negative_zero) = FiniteF64::new(-0.0) else {
        panic!("negative zero is finite");
    };
    let Some(positive_zero) = FiniteF64::new(0.0) else {
        panic!("positive zero is finite");
    };
    let Some(inclusive) = NumberRange::new(None, Some(NumberBound::inclusive(negative_zero)))
    else {
        panic!("inclusive signed-zero range is valid");
    };
    let Some(exclusive) = NumberRange::new(None, Some(NumberBound::exclusive(positive_zero)))
    else {
        panic!("exclusive signed-zero range is valid");
    };
    for ranges in [[inclusive, exclusive], [exclusive, inclusive]] {
        let Ok(merged) = super::super::ranges::union(
            "SignedZero",
            ranges.into_iter().map(NumericRange::Number).map(Some),
        ) else {
            panic!("signed-zero range union is representable");
        };
        let Some(NumericRange::Number(range)) = merged else {
            panic!("signed-zero ranges should merge");
        };
        assert!(range.contains(0.0));
        assert!(range.contains(-0.0));
    }
}

#[test]
fn modern_ref_siblings_apply_while_legacy_siblings_ignore_multiple_of() {
    let modern = import_str(
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "title":"Modern",
  "$ref":"#/$defs/value",
  "multipleOf":3,
  "$defs":{"value":{"type":"integer","multipleOf":2}}
}"##,
    );
    assert!(crate::from_str("12", &modern).is_ok());
    assert!(matches!(
        crate::from_str("9", &modern),
        Err(JsonFormatError::MultipleOfMismatch { .. })
    ));

    let legacy = import_str(
        r##"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "title":"Legacy",
  "$ref":"#/definitions/value",
  "multipleOf":3,
  "definitions":{"value":{"type":"integer","multipleOf":2}}
}"##,
    );
    assert!(crate::from_str("4", &legacy).is_ok());
}

#[test]
fn canonical_export_roundtrips_divisor_dnf_and_exact_decimal_lexicals() {
    let schema = import_str(
        r#"{
  "title":"Value",
  "anyOf":[
    {"type":"number","multipleOf":0.1},
    {"type":"number","multipleOf":2.5}
  ]
}"#,
    );
    let rendered = export(&schema);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&rendered) else {
        panic!("exported schema is JSON");
    };
    assert!(value.get("allOf").is_some());
    assert_eq!(import_str(&rendered), schema);

    let Some(smallest) = JsonMultipleOf::from_decimal_lexical("5e-324") else {
        panic!("smallest subnormal divisor is representable");
    };
    assert_eq!(
        smallest.to_decimal_lexical().parse::<f64>().ok(),
        Some(f64::from_bits(1))
    );
}

#[test]
fn export_combines_string_pattern_and_numeric_divisor_dnf_without_correlation() {
    let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
        panic!("test scalar union is heterogeneous");
    };
    let Ok(patterns) = JsonPatternConstraints::new([["^A$"], ["^B$"]]) else {
        panic!("test pattern DNF is valid");
    };
    let Some(two) = JsonMultipleOf::from_decimal_lexical("2") else {
        panic!("two is representable");
    };
    let Some(three) = JsonMultipleOf::from_decimal_lexical("3") else {
        panic!("three is representable");
    };
    let Ok(multiples) = JsonMultipleOfConstraints::new([[two], [three]]) else {
        panic!("test multipleOf DNF is valid");
    };
    let Some(schema) = SchemaNode::scalar_union("Value", types)
        .with_json_patterns(patterns)
        .and_then(|schema| schema.with_json_multiple_of(multiples))
    else {
        panic!("disjoint scalar domains retain independent constraints");
    };
    let rendered = export(&schema);
    assert_eq!(import_str(&rendered), schema);
    assert!(crate::from_str(r#""A""#, &schema).is_ok());
    assert!(crate::from_str("4", &schema).is_ok());
    assert!(matches!(
        crate::from_str("5", &schema),
        Err(JsonFormatError::MultipleOfMismatch { .. })
    ));
}
