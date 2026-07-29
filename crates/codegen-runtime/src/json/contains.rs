use ir::{
    FiniteF64, Instance, ItemCountRange, JsonContainsConstraint, JsonContainsConstraints,
    JsonContainsPredicate, JsonPatternConstraints, NumberBound, NumberRange, NumericRange,
    ScalarType, SchemaNode, Value,
};

use super::{JsonBoundaryError, parse_json, serialize_json};

fn exact_string(name: &str, value: &str) -> Result<SchemaNode, &'static str> {
    SchemaNode::scalar(name, ScalarType::String)
        .with_fixed(value)
        .ok_or("test fixed value matches the string domain")
}

fn contains(
    predicate: SchemaNode,
    minimum: u64,
    maximum: Option<u64>,
) -> Result<JsonContainsConstraints, &'static str> {
    let range = ItemCountRange::new(minimum, maximum).ok_or("test contains interval is ordered")?;
    JsonContainsConstraints::new([JsonContainsConstraint::new(
        JsonContainsPredicate::schema(predicate),
        range,
    )])
    .ok_or("test contains constraint is effective and bounded")
}

fn exact_strings(
    name: &str,
    value: &str,
    minimum: u64,
    maximum: Option<u64>,
) -> Result<SchemaNode, &'static str> {
    SchemaNode::scalar(name, ScalarType::String)
        .repeating()
        .with_json_contains(contains(
            exact_string("contains item", value)?,
            minimum,
            maximum,
        )?)
        .ok_or("contains metadata belongs to an array")
}

#[test]
fn boundaries_count_exact_raw_input_and_normalized_output_matches()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = serde_json::to_string(&exact_strings("Codes", "keep", 2, Some(2))?)?;
    assert!(parse_json(&schema, r#"["keep","other","keep"]"#).is_ok());
    for document in [r#"["keep","other"]"#, r#"["keep","keep","keep"]"#] {
        assert!(matches!(
            parse_json(&schema, document),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("Codes") && message.contains("matching"),
        ));
    }
    assert_eq!(
        serialize_json(
            &schema,
            &Instance::Repeated(vec![
                Instance::Scalar(Value::String("keep".into())),
                Instance::Scalar(Value::String("other".into())),
                Instance::Scalar(Value::String("keep".into())),
            ]),
        )?,
        "[\n  \"keep\",\n  \"other\",\n  \"keep\"\n]\n",
    );
    assert!(matches!(
        serialize_json(
            &schema,
            &Instance::Repeated(vec![Instance::Scalar(Value::String("keep".into()))]),
        ),
        Err(JsonBoundaryError::InvalidOutput { ref message })
            if message.contains("Codes") && message.contains("matching"),
    ));

    let finite_two = FiniteF64::new(2.0).ok_or("test number is finite")?;
    let range = NumberRange::new(
        Some(NumberBound::inclusive(finite_two)),
        Some(NumberBound::inclusive(finite_two)),
    )
    .ok_or("test numeric interval is valid")?;
    let predicate = SchemaNode::scalar("contains item", ScalarType::Float)
        .with_numeric_range(NumericRange::Number(range))
        .ok_or("numeric range matches float predicate")?;
    let numeric = SchemaNode::scalar("Amounts", ScalarType::Float)
        .repeating()
        .with_json_contains(contains(predicate, 1, Some(1))?)
        .ok_or("contains metadata belongs to an array")?;
    assert_eq!(
        serialize_json(
            &serde_json::to_string(&numeric)?,
            &Instance::Repeated(vec![
                Instance::Scalar(Value::String("2".into())),
                Instance::Scalar(Value::String("3".into())),
            ]),
        )?,
        "[\n  2.0,\n  3.0\n]\n",
    );
    Ok(())
}

#[test]
fn boundaries_apply_nested_repeated_and_nullable_contains() -> Result<(), Box<dyn std::error::Error>>
{
    let mut nullable = exact_strings("Maybe", "ok", 1, None)?;
    nullable.container_nullable = true;
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::group("Nested", vec![exact_strings("Codes", "ok", 1, None)?]),
            SchemaNode::group("Rows", vec![exact_strings("Codes", "ok", 1, None)?]).repeating(),
            nullable,
        ],
    );
    let encoded = serde_json::to_string(&schema)?;
    assert!(
        parse_json(
            &encoded,
            r#"{
                "Nested":{"Codes":["no","ok"]},
                "Rows":[{"Codes":["ok"]},{"Codes":["no","ok"]}],
                "Maybe":null
            }"#,
        )
        .is_ok()
    );
    assert!(matches!(
        parse_json(
            &encoded,
            r#"{
                "Nested":{"Codes":["ok"]},
                "Rows":[{"Codes":["no"]}],
                "Maybe":null
            }"#,
        ),
        Err(JsonBoundaryError::InvalidInput { ref message })
            if message.contains("Codes") && message.contains("matching"),
    ));
    assert!(matches!(
        parse_json(
            &encoded,
            r#"{
                "Nested":{"Codes":["ok"]},
                "Rows":[],
                "Maybe":["no"]
            }"#,
        ),
        Err(JsonBoundaryError::InvalidInput { ref message })
            if message.contains("Maybe") && message.contains("matching"),
    ));
    Ok(())
}

#[test]
fn contains_metadata_and_pattern_work_fail_as_typed_boundary_errors()
-> Result<(), Box<dyn std::error::Error>> {
    let patterns = JsonPatternConstraints::new([["^(a?){5000}$"]])
        .map_err(|_| "test predicate pattern remains structurally bounded")?;
    let ordinary = SchemaNode::scalar("Ordinary", ScalarType::String)
        .with_json_patterns(patterns.clone())
        .ok_or("pattern matches a string predicate")?;
    let predicate = SchemaNode::scalar("contains item", ScalarType::String)
        .with_json_patterns(patterns)
        .ok_or("pattern matches a string predicate")?;
    let values = SchemaNode::scalar("Values", ScalarType::String)
        .repeating()
        .with_json_contains(contains(predicate, 1, None)?)
        .ok_or("contains metadata belongs to an array")?;
    let schema = SchemaNode::group("Root", vec![ordinary, values.clone()]);
    let encoded = serde_json::to_string(&schema)?;
    let value = "a".repeat(5_000);
    let document = serde_json::to_string(&serde_json::json!({
        "Ordinary": value,
        "Values": [value],
    }))?;
    assert!(matches!(
        parse_json(&encoded, &document),
        Err(JsonBoundaryError::InvalidInput { ref message })
            if message.contains("work limit"),
    ));

    let invalid =
        serde_json::to_string(&values)?.replace("\"repeating\":true", "\"repeating\":false");
    assert!(matches!(
        parse_json(&invalid, r#"["a"]"#),
        Err(JsonBoundaryError::InvalidEmbeddedSchema { ref message })
            if message.contains("contains"),
    ));
    Ok(())
}
