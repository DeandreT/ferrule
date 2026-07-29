use ir::{
    JsonMultipleOf, JsonMultipleOfConstraints, JsonPatternConstraints, ScalarType, SchemaNode,
};

use super::import_str;
use crate::JsonFormatError;

fn assert_invalid_pattern_export(schema: &SchemaNode) {
    assert!(matches!(
        super::super::export(schema),
        Err(JsonFormatError::InvalidPatternMetadata { .. })
    ));
}

#[test]
fn export_rejects_patterns_on_non_string_domains_and_mismatched_fixed_values() {
    let patterns = JsonPatternConstraints::new([["^A$"]]).unwrap_or_else(|error| panic!("{error}"));

    let mut integer = SchemaNode::scalar("Count", ScalarType::Int);
    integer.json_patterns = Some(patterns.clone());
    assert_invalid_pattern_export(&integer);

    let mut group = SchemaNode::group("Record", Vec::new());
    group.json_patterns = Some(patterns.clone());
    assert_invalid_pattern_export(&group);

    let mut fixed = SchemaNode::scalar_fixed("Code", ScalarType::String, "B");
    fixed.json_patterns = Some(patterns);
    assert_invalid_pattern_export(&fixed);
}

#[test]
fn export_rejects_schema_wide_pattern_program_overflow() {
    let children = (0..=ir::MAX_DISTINCT_JSON_PATTERNS)
        .map(|index| {
            let patterns = JsonPatternConstraints::new([[format!("^value-{index}$")]])
                .unwrap_or_else(|error| panic!("{error}"));
            let Some(child) = SchemaNode::scalar(format!("field-{index}"), ScalarType::String)
                .with_json_patterns(patterns)
            else {
                panic!("pattern metadata should fit its local string domain");
            };
            child
        })
        .collect();
    assert_invalid_pattern_export(&SchemaNode::group("Root", children));
}

#[test]
fn export_rejects_corrupted_multiple_of_domains_and_fixed_values() {
    let Some(divisor) = JsonMultipleOf::from_decimal_lexical("2") else {
        panic!("test divisor is representable");
    };
    let Ok(constraints) = JsonMultipleOfConstraints::new([[divisor]]) else {
        panic!("test constraints are valid");
    };

    let mut string = SchemaNode::scalar("Text", ScalarType::String);
    string.json_multiple_of = Some(constraints.clone());
    assert!(matches!(
        super::super::export(&string),
        Err(JsonFormatError::InvalidMultipleOfMetadata { .. })
    ));

    let mut fixed = SchemaNode::scalar_fixed("Count", ScalarType::Int, "3");
    fixed.json_multiple_of = Some(constraints.clone());
    assert!(matches!(
        super::super::export(&fixed),
        Err(JsonFormatError::InvalidMultipleOfMetadata { .. })
    ));

    let mut child = SchemaNode::scalar("Child", ScalarType::String);
    child.json_multiple_of = Some(constraints);
    assert!(matches!(
        super::super::export(&SchemaNode::group("Root", vec![child])),
        Err(JsonFormatError::InvalidMultipleOfMetadata { .. })
    ));
}

#[test]
fn export_rejects_exact_decimal_divisors_that_json_numbers_would_round() {
    let Some(divisor) = JsonMultipleOf::from_decimal_lexical("3e-324") else {
        panic!("exact decimal remains positive after finite JSON-number rounding");
    };
    let Ok(constraints) = JsonMultipleOfConstraints::new([[divisor]]) else {
        panic!("test constraints are valid");
    };
    let Some(schema) =
        SchemaNode::scalar("Value", ScalarType::Float).with_json_multiple_of(constraints)
    else {
        panic!("number scalar accepts the exact divisor");
    };
    assert!(matches!(
        super::super::export(&schema),
        Err(JsonFormatError::InvalidMultipleOfMetadata { ref reason })
            if reason.contains("without changing its exact value")
    ));
}

#[test]
fn all_of_typed_dynamic_fields_execute_and_roundtrip() {
    let schema = import_str(
        r#"{
  "title":"Root",
  "type":"object",
  "additionalProperties":{
    "allOf":[
      {"type":"string"},
      {"pattern":"^A$"}
    ]
  }
}"#,
    );
    assert!(crate::from_str(r#"{"valid":"A"}"#, &schema).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"invalid":"B"}"#, &schema),
        Err(JsonFormatError::PatternMismatch { .. })
    ));

    let rendered = super::export(&schema);
    let roundtrip = import_str(&rendered);
    assert_eq!(roundtrip, schema);
    assert!(matches!(
        crate::from_str(r#"{"invalid":"B"}"#, &roundtrip),
        Err(JsonFormatError::PatternMismatch { .. })
    ));
}
