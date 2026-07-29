use ir::{
    Instance, JsonDependentSchemaConstraint, JsonDependentSchemaConstraints,
    JsonPatternPropertyNames, JsonSchemaPredicate, ScalarType, SchemaNode, Value,
};

use super::{JsonBoundaryError, parse_json, serialize_json};

fn schema(
    sources: impl IntoIterator<Item = &'static str>,
) -> Result<String, Box<dyn std::error::Error>> {
    let mut dynamic = SchemaNode::scalar("*", ScalarType::Int);
    dynamic.nullable = true;
    let selectors = JsonPatternPropertyNames::new(sources)?;
    let schema = SchemaNode::group(
        "Object",
        vec![SchemaNode::scalar("fixed", ScalarType::String)],
    )
    .with_dynamic_fields(dynamic)
    .and_then(|schema| schema.with_json_pattern_property_names(selectors))
    .ok_or("patternProperties test schema is valid")?;
    Ok(serde_json::to_string(&schema)?)
}

#[test]
fn boundaries_select_dynamic_names_after_fixed_children() -> Result<(), Box<dyn std::error::Error>>
{
    let schema = schema(["^x-", "meta"])?;
    let valid = r#"{"fixed":"declared","x-one":1,"x-meta":2,"meta-two":null}"#;
    let parsed = parse_json(&schema, valid)?;
    assert_eq!(
        serialize_json(&schema, &parsed)?.replace([' ', '\n'], ""),
        valid
    );

    assert!(matches!(
        parse_json(&schema, r#"{"other":1}"#),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("other") && message.contains("patternProperties"),
    ));
    assert!(matches!(
        serialize_json(
            &schema,
            &Instance::Group(vec![(
                "other".into(),
                Instance::Scalar(Value::Int(1)),
            )]),
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("other") && message.contains("patternProperties"),
    ));
    assert_eq!(
        serialize_json(
            &schema,
            &Instance::Group(vec![("other".into(), Instance::Scalar(Value::Null),)]),
        )?,
        "{}\n"
    );
    Ok(())
}

#[test]
fn selector_matching_uses_the_document_pattern_budget() -> Result<(), Box<dyn std::error::Error>> {
    let schema = schema(["^(a?){8000}$"])?;
    let key = "a".repeat(8_000);
    assert!(matches!(
        parse_json(&schema, &format!("{{{key:?}:1}}")),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("work limit"),
    ));
    Ok(())
}

#[test]
fn selectors_in_dependent_predicates_share_the_document_pattern_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let source = "^(a?){5000}$";
    let selectors = || JsonPatternPropertyNames::new([source]);
    let dynamic = || SchemaNode::scalar("*", ScalarType::Int);
    let trigger = || SchemaNode::scalar("Trigger", ScalarType::Bool);

    let predicate = SchemaNode::group("dependent object", vec![trigger()])
        .with_dynamic_fields(dynamic())
        .and_then(|schema| schema.with_json_pattern_property_names(selectors().ok()?))
        .ok_or("dependent predicate patternProperties schema is valid")?;
    let dependent = JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
        "Trigger",
        JsonSchemaPredicate::schema(predicate),
    )])
    .ok_or("dependent schema constraint is valid")?;
    let root = SchemaNode::group("Object", vec![trigger()])
        .with_dynamic_fields(dynamic())
        .and_then(|schema| schema.with_json_pattern_property_names(selectors().ok()?))
        .and_then(|schema| schema.with_json_dependent_schemas(dependent))
        .ok_or("root patternProperties schema is valid")?;
    let encoded = serde_json::to_string(&root)?;
    let property = "a".repeat(5_000);
    let document = serde_json::to_string(&serde_json::json!({
        "Trigger": true,
        (property.clone()): 1,
    }))?;

    assert!(matches!(
        parse_json(&encoded, &document),
        Err(JsonBoundaryError::InvalidInput { ref message })
            if message.contains("work limit"),
    ));
    assert!(matches!(
        serialize_json(
            &encoded,
            &Instance::Group(vec![
                ("Trigger".into(), Instance::Scalar(Value::Bool(true))),
                (property, Instance::Scalar(Value::Int(1))),
            ]),
        ),
        Err(JsonBoundaryError::InvalidOutput { ref message })
            if message.contains("work limit"),
    ));
    Ok(())
}
