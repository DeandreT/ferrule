use ir::{
    Instance, JsonDependentSchemaConstraint, JsonDependentSchemaConstraints,
    JsonPatternConstraints, JsonSchemaPredicate, ScalarType, SchemaNode, Value,
};

use super::{
    JsonBoundaryError, parse_json, parse_json_bytes, serialize_json, serialize_json_bytes,
};

fn arbitrary_json(name: &str) -> Result<SchemaNode, &'static str> {
    SchemaNode::scalar(name, ScalarType::String)
        .json_any()
        .ok_or("arbitrary JSON schema is valid")
}

fn predicate(required: &[&str], children: Vec<SchemaNode>) -> Result<SchemaNode, &'static str> {
    SchemaNode::group("dependent object", children)
        .with_dynamic_fields(arbitrary_json("*")?)
        .ok_or("dependent predicate accepts other object properties")?
        .with_required_fields(required.iter().map(|name| (*name).to_string()).collect())
        .ok_or("dependent predicate required fields are declared")
}

fn dependent(
    name: &str,
    trigger: &str,
    predicate: SchemaNode,
    children: Vec<SchemaNode>,
) -> Result<SchemaNode, &'static str> {
    let constraints = JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
        trigger,
        JsonSchemaPredicate::schema(predicate),
    )])
    .ok_or("dependent schema constraint is effective and bounded")?;
    SchemaNode::group(name, children)
        .with_json_dependent_schemas(constraints)
        .ok_or("dependent schema metadata belongs to the object")
}

fn exact_string(name: &str, value: &str) -> Result<SchemaNode, &'static str> {
    SchemaNode::scalar(name, ScalarType::String)
        .with_fixed(value)
        .ok_or("fixed string belongs to the string domain")
}

fn root_schema() -> Result<SchemaNode, &'static str> {
    dependent(
        "Root",
        "Trigger",
        predicate(&["Guard"], vec![exact_string("Guard", "accepted")?])?,
        vec![
            SchemaNode::scalar("Trigger", ScalarType::String)
                .nullable()
                .ok_or("trigger is nullable")?,
            SchemaNode::scalar("Guard", ScalarType::String),
        ],
    )
}

#[test]
fn dependent_predicates_support_recursively_bounded_nested_rules()
-> Result<(), Box<dyn std::error::Error>> {
    let nested_predicate = dependent(
        "Embedded",
        "X",
        predicate(&["Y"], vec![exact_string("Y", "nested")?])?,
        vec![
            SchemaNode::scalar("X", ScalarType::Int),
            SchemaNode::scalar("Y", ScalarType::String),
        ],
    )?;
    let schema = dependent(
        "Root",
        "Trigger",
        predicate(&["Embedded"], vec![nested_predicate])?,
        vec![
            SchemaNode::scalar("Trigger", ScalarType::Bool),
            SchemaNode::group(
                "Embedded",
                vec![
                    SchemaNode::scalar("X", ScalarType::Int),
                    SchemaNode::scalar("Y", ScalarType::String),
                ],
            ),
        ],
    )?;
    let encoded = serde_json::to_string(&schema)?;
    assert!(
        parse_json(
            &encoded,
            r#"{"Trigger":true,"Embedded":{"X":1,"Y":"nested"}}"#
        )
        .is_ok()
    );
    assert!(
        parse_json(&encoded, r#"{"Embedded":{"X":1,"Y":"wrong"}}"#).is_ok(),
        "nested rules inside an inactive outer predicate do not apply"
    );
    assert!(matches!(
        parse_json(&encoded, r#"{"Trigger":true,"Embedded":{"X":1,"Y":"wrong"}}"#),
        Err(JsonBoundaryError::InvalidInput { ref message })
            if message.contains("Root") && message.contains("Trigger"),
    ));
    Ok(())
}

#[test]
fn boundaries_validate_present_triggers_and_normalized_output()
-> Result<(), Box<dyn std::error::Error>> {
    let schema = serde_json::to_string(&root_schema()?)?;

    assert!(parse_json(&schema, "{}").is_ok());
    for document in [
        r#"{"Trigger":null,"Guard":"accepted"}"#,
        r#"{"Trigger":"value","Guard":"accepted"}"#,
    ] {
        assert!(parse_json(&schema, document).is_ok());
        assert!(parse_json_bytes(&schema, document.as_bytes()).is_ok());
    }
    for document in [
        r#"{"Trigger":null}"#,
        r#"{"Trigger":"value","Guard":"wrong"}"#,
    ] {
        assert!(matches!(
            parse_json(&schema, document),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("Root") && message.contains("Trigger"),
        ));
    }

    let valid = Instance::Group(vec![
        (
            "Trigger".into(),
            Instance::Scalar(Value::String("value".into())),
        ),
        (
            "Guard".into(),
            Instance::Scalar(Value::String("accepted".into())),
        ),
    ]);
    assert_eq!(
        serialize_json_bytes(&schema, &valid)?,
        b"{\n  \"Trigger\": \"value\",\n  \"Guard\": \"accepted\"\n}\n",
    );
    let omitted = Instance::Group(vec![
        (
            "Trigger".into(),
            Instance::Scalar(Value::String("value".into())),
        ),
        ("Guard".into(), Instance::Scalar(Value::Null)),
    ]);
    assert!(matches!(
        serialize_json(&schema, &omitted),
        Err(JsonBoundaryError::InvalidOutput { ref message })
            if message.contains("Root") && message.contains("Trigger"),
    ));
    Ok(())
}

#[test]
fn boundaries_apply_nested_repeated_and_nullable_dependent_schemas()
-> Result<(), Box<dyn std::error::Error>> {
    let object = |name: &str| {
        dependent(
            name,
            "A",
            predicate(&["B"], vec![SchemaNode::scalar("B", ScalarType::Int)])?,
            vec![
                SchemaNode::scalar("A", ScalarType::Int),
                SchemaNode::scalar("B", ScalarType::Int),
            ],
        )
    };
    let mut maybe = object("Maybe")?;
    maybe.container_nullable = true;
    let schema = SchemaNode::group(
        "Root",
        vec![object("Nested")?, object("Rows")?.repeating(), maybe],
    );
    let encoded = serde_json::to_string(&schema)?;
    assert!(
        parse_json(
            &encoded,
            r#"{"Nested":{"A":1,"B":2},"Rows":[{"A":1,"B":2},{}],"Maybe":null}"#,
        )
        .is_ok()
    );
    for document in [
        r#"{"Nested":{"A":1},"Rows":[],"Maybe":null}"#,
        r#"{"Nested":{},"Rows":[{"A":1}],"Maybe":null}"#,
        r#"{"Nested":{},"Rows":[],"Maybe":{"A":1}}"#,
    ] {
        assert!(matches!(
            parse_json(&encoded, document),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("dependent schema"),
        ));
    }
    Ok(())
}

#[test]
fn dependent_predicates_share_pattern_work_and_reject_noncanonical_metadata()
-> Result<(), Box<dyn std::error::Error>> {
    let patterns = JsonPatternConstraints::new([["^(a?){5000}$"]])
        .map_err(|_| "test predicate pattern remains structurally bounded")?;
    let ordinary = SchemaNode::scalar("Ordinary", ScalarType::String)
        .with_json_patterns(patterns.clone())
        .ok_or("ordinary pattern belongs to a string")?;
    let guard = SchemaNode::scalar("Guard", ScalarType::String)
        .with_json_patterns(patterns)
        .ok_or("dependent pattern belongs to a string")?;
    let schema = dependent(
        "Root",
        "Trigger",
        predicate(&["Guard"], vec![guard])?,
        vec![
            ordinary,
            SchemaNode::scalar("Trigger", ScalarType::Bool),
            SchemaNode::scalar("Guard", ScalarType::String),
        ],
    )?;
    let encoded = serde_json::to_string(&schema)?;
    let value = "a".repeat(5_000);
    let document = serde_json::to_string(&serde_json::json!({
        "Ordinary": value,
        "Trigger": true,
        "Guard": value,
    }))?;
    assert!(matches!(
        parse_json(&encoded, &document),
        Err(JsonBoundaryError::InvalidInput { ref message })
            if message.contains("work limit"),
    ));

    let mut invalid = serde_json::to_value(schema)?;
    let constraints = invalid
        .get_mut("json_dependent_schemas")
        .and_then(serde_json::Value::as_array_mut)
        .ok_or("serialized dependent metadata is an array")?;
    let duplicate = constraints
        .first()
        .cloned()
        .ok_or("serialized dependent metadata is nonempty")?;
    constraints.push(duplicate);
    assert!(matches!(
        parse_json(&serde_json::to_string(&invalid)?, "{}"),
        Err(JsonBoundaryError::InvalidEmbeddedSchema { ref message })
            if message.contains("dependent schema"),
    ));
    Ok(())
}
