use ir::{
    Instance, JsonFormatAnnotations, JsonPatternConstraints, JsonPropertyNameConstraints,
    JsonPropertyNameSet, ScalarType, SchemaNode, StringLengthRange, Value,
};

use super::{JsonBoundaryError, parse_json, serialize_json};

fn open_object(constraints: JsonPropertyNameConstraints) -> Result<SchemaNode, &'static str> {
    let mut schema = SchemaNode::group("Object", Vec::new())
        .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::Int))
        .ok_or("test object accepts dynamic integer fields")?;
    schema.json_property_names = Some(constraints);
    schema
        .metadata_is_valid()
        .then_some(schema)
        .ok_or("test property-name metadata is valid")
}

fn constrained_names() -> Result<JsonPropertyNameConstraints, &'static str> {
    let allowed =
        JsonPropertyNameSet::new([String::new(), "valid".to_string(), "omitted".to_string()])
            .map_err(|_| "test name set is bounded")?;
    let length =
        StringLengthRange::new(0, Some(7)).ok_or("test name length range is constrained")?;
    let patterns = JsonPatternConstraints::new([["^$|^[a-z]+$"]])
        .map_err(|_| "test property-name pattern is portable")?;
    let formats = JsonFormatAnnotations::new(["member-name".to_string()])
        .map_err(|_| "test format annotation is bounded")?;
    JsonPropertyNameConstraints::schema(Some(allowed), Some(length), Some(patterns), formats)
        .ok_or("test property-name schema is constrained")
}

fn names_excluding_pattern(source: &str) -> Result<JsonPropertyNameConstraints, &'static str> {
    let excluded_patterns = JsonPatternConstraints::new([[source]])
        .map_err(|_| "test excluded property-name pattern is portable")?;
    JsonPropertyNameConstraints::schema_with_exclusions(
        None,
        None,
        None,
        None,
        Some(excluded_patterns),
        JsonFormatAnnotations::default(),
    )
    .ok_or("test excluded property-name pattern is constrained")
}

#[test]
fn boundaries_validate_actual_input_and_normalized_output_property_names()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = serde_json::to_string(&open_object(constrained_names()?)?)?;
    assert!(parse_json(&schema, r#"{"":1,"valid":2}"#).is_ok());
    assert!(matches!(
        parse_json(&schema, r#"{"bad-key":1}"#),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("bad-key") && message.contains("property name"),
    ));

    assert_eq!(
        serialize_json(
            &schema,
            &Instance::Group(vec![
                ("".into(), Instance::Scalar(Value::Int(1))),
                ("valid".into(), Instance::Scalar(Value::Int(2))),
                ("omitted".into(), Instance::Scalar(Value::Null)),
            ]),
        )
        .as_deref(),
        Ok("{\n  \"\": 1,\n  \"valid\": 2\n}\n"),
    );
    assert_eq!(
        serialize_json(
            &schema,
            &Instance::Group(vec![("bad-key".into(), Instance::Scalar(Value::Null))]),
        )
        .as_deref(),
        Ok("{}\n"),
    );
    assert!(matches!(
        serialize_json(
            &schema,
            &Instance::Group(vec![(
                "bad-key".into(),
                Instance::Scalar(Value::Int(1)),
            )]),
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("bad-key") && message.contains("property name"),
    ));
    Ok(())
}

#[test]
fn exact_false_accepts_only_empty_objects() -> Result<(), Box<dyn std::error::Error>> {
    let schema = serde_json::to_string(&open_object(JsonPropertyNameConstraints::never())?)?;
    assert!(parse_json(&schema, "{}").is_ok());
    assert!(matches!(
        parse_json(&schema, r#"{"":1}"#),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("property name"),
    ));
    assert_eq!(
        serialize_json(&schema, &Instance::Group(Vec::new())).as_deref(),
        Ok("{}\n"),
    );
    Ok(())
}

#[test]
fn boundaries_validate_excluded_property_name_patterns() -> Result<(), Box<dyn std::error::Error>> {
    let schema = serde_json::to_string(&open_object(names_excluding_pattern("^private$")?)?)?;
    assert!(parse_json(&schema, r#"{"public":1}"#).is_ok());
    assert!(matches!(
        parse_json(&schema, r#"{"private":1}"#),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("private") && message.contains("property name"),
    ));
    assert_eq!(
        serialize_json(
            &schema,
            &Instance::Group(vec![("public".into(), Instance::Scalar(Value::Int(1)),)]),
        )
        .as_deref(),
        Ok("{\n  \"public\": 1\n}\n"),
    );
    assert!(matches!(
        serialize_json(
            &schema,
            &Instance::Group(vec![(
                "private".into(),
                Instance::Scalar(Value::Int(1)),
            )]),
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("private") && message.contains("property name"),
    ));
    Ok(())
}

#[test]
fn excluded_property_name_patterns_share_a_bounded_document_work_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let constraints = names_excluding_pattern("^(a?){8000}$")?;
    let schema = serde_json::to_string(&open_object(constraints)?)?;
    let key = "a".repeat(8_000);
    let document = format!("{{{key:?}:1}}");
    assert!(matches!(
        parse_json(&schema, &document),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("work limit"),
    ));
    Ok(())
}
