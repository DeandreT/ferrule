use ir::{
    FiniteF64, GroupAlternative, GroupAlternativeConstraint, GroupAlternativeConstraintValue,
    Instance, ItemCountRange, JsonAllowedValue, JsonAllowedValues, JsonContainsConstraint,
    JsonContainsConstraints, JsonContainsPredicate, JsonDependentSchemaConstraint,
    JsonDependentSchemaConstraints, JsonSchemaPredicate, NumberBound, NumberRange, NumericRange,
    ScalarType, SchemaNode, Value,
};

use super::{JsonBoundaryError, MAX_EMBEDDED_JSON_SCHEMA_BYTES, parse_json, serialize_json};

fn adjacent_values() -> (f64, f64) {
    // Independent literal bits: the corrected reader maps the two decimal
    // spellings to these values without reversing their order.
    let low = f64::from_bits(0x0031_fa18_2c40_c60d);
    let high = f64::from_bits(0x0031_fa18_2c40_c60e);
    assert_eq!(high.to_bits(), low.to_bits() + 1);
    (low, high)
}

// Exercise the public V2 decoder explicitly instead of depending on a parser
// loss to select V2 automatically. Each fixture lists its exact typed metadata
// positions; there is no recursive encoder or production admission change.
fn explicit_v2(schema: &SchemaNode, markers: &[(&str, u64)]) -> String {
    let mut payload = serde_json::to_value(schema).unwrap();
    for &(path, bits) in markers {
        let slot = payload.pointer_mut(path).expect("literal metadata path");
        assert_eq!(slot.as_f64().expect("finite metadata").to_bits(), bits);
        *slot = serde_json::Value::String(format!(
            "{}{bits:016x}",
            codegen_schema::FLOAT_BITS_MARKER_PREFIX,
        ));
    }
    let descriptor = format!(
        "{}{}",
        codegen_schema::V2_PREFIX,
        serde_json::to_string(&payload).unwrap(),
    );
    let decoded = codegen_schema::decode(&descriptor, usize::MAX).unwrap();
    assert_eq!(
        serde_json::to_string(&decoded).unwrap(),
        serde_json::to_string(schema).unwrap(),
        "the complete explicit V2 fixture preserves every schema value",
    );
    descriptor
}

fn minimum(name: &str, value: f64) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Float)
        .with_numeric_range(NumericRange::Number(
            NumberRange::new(
                Some(NumberBound::inclusive(FiniteF64::new(value).unwrap())),
                None,
            )
            .unwrap(),
        ))
        .unwrap()
}

fn allowed(name: &str, value: f64) -> SchemaNode {
    SchemaNode::scalar(name, ScalarType::Float)
        .with_json_allowed_values(
            JsonAllowedValues::new([
                JsonAllowedValue::Float(FiniteF64::new(0.0).unwrap()),
                JsonAllowedValue::Float(FiniteF64::new(value).unwrap()),
            ])
            .unwrap(),
        )
        .unwrap()
}

fn exact_range(name: &str, value: f64) -> SchemaNode {
    let finite = FiniteF64::new(value).unwrap();
    SchemaNode::scalar(name, ScalarType::Float)
        .with_numeric_range(NumericRange::Number(
            NumberRange::new(
                Some(NumberBound::inclusive(finite)),
                Some(NumberBound::inclusive(finite)),
            )
            .unwrap(),
        ))
        .unwrap()
}

fn check_case(
    schema: SchemaNode,
    accepted: &str,
    rejected: &str,
    accepted_instance: Instance,
    rejected_instance: Instance,
    markers: &[(&str, u64)],
) {
    for descriptor in [
        codegen_schema::encode(&schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES).unwrap(),
        explicit_v2(&schema, markers),
    ] {
        let decoded = codegen_schema::decode(&descriptor, MAX_EMBEDDED_JSON_SCHEMA_BYTES).unwrap();
        assert_eq!(
            serde_json::to_string(&decoded).unwrap(),
            serde_json::to_string(&schema).unwrap(),
            "all floating metadata bits, including signed zero, survive"
        );
        assert_eq!(
            parse_json(&descriptor, accepted).unwrap(),
            format_json::from_str(accepted, &schema).unwrap()
        );
        assert_eq!(
            serialize_json(&descriptor, &accepted_instance).unwrap(),
            format_json::to_string(&schema, &accepted_instance).unwrap()
        );
        assert!(format_json::from_str(rejected, &schema).is_err());
        assert!(matches!(
            parse_json(&descriptor, rejected),
            Err(JsonBoundaryError::InvalidInput { .. })
        ));
        assert!(format_json::to_string(&schema, &rejected_instance).is_err());
        assert!(matches!(
            serialize_json(&descriptor, &rejected_instance),
            Err(JsonBoundaryError::InvalidOutput { .. })
        ));
    }
}

#[test]
fn embedded_schema_preserves_adjacent_float_range_and_allowed_value_boundaries() {
    let (low, high) = adjacent_values();
    let physical: SchemaNode = serde_json::from_str(
        r#"{"name":"Amount","numeric_range":{"kind":"number","bounds":{"minimum":{"value":1.0000000000000001e-307}}},"kind":{"kind":"scalar","ty":"float"}}"#,
    )
    .unwrap();
    assert!(
        explicit_v2(
            &physical,
            &[("/numeric_range/bounds/minimum/value", high.to_bits())]
        )
        .starts_with(codegen_schema::V2_PREFIX)
    );
    for (schema, markers) in [
        (
            physical,
            vec![("/numeric_range/bounds/minimum/value", high.to_bits())],
        ),
        (
            exact_range("Amount", high),
            vec![
                ("/numeric_range/bounds/minimum/value", high.to_bits()),
                ("/numeric_range/bounds/maximum/value", high.to_bits()),
            ],
        ),
        (
            allowed("Amount", high),
            vec![("/json_allowed_values/1/value", high.to_bits())],
        ),
    ] {
        check_case(
            schema,
            "1.0000000000000001e-307",
            "1e-307",
            Instance::Scalar(Value::Float(high)),
            Instance::Scalar(Value::Float(low)),
            &markers,
        );
    }
    for (schema, markers) in [
        (
            exact_range("Amount", low),
            vec![
                ("/numeric_range/bounds/minimum/value", low.to_bits()),
                ("/numeric_range/bounds/maximum/value", low.to_bits()),
            ],
        ),
        (
            allowed("Amount", low),
            vec![("/json_allowed_values/1/value", low.to_bits())],
        ),
    ] {
        check_case(
            schema,
            "1e-307",
            "1.0000000000000001e-307",
            Instance::Scalar(Value::Float(low)),
            Instance::Scalar(Value::Float(high)),
            &markers,
        );
    }
}

#[test]
fn embedded_schema_preserves_float_alternatives_and_nested_predicates() {
    let (low, high) = adjacent_values();
    let group = |value| {
        Instance::Group((vec![("Amount".into(), Instance::Scalar(Value::Float(value)))]).into())
    };
    for (value, other, accepted, rejected) in [
        (high, low, "1.0000000000000001e-307", "1e-307"),
        (low, high, "1e-307", "1.0000000000000001e-307"),
    ] {
        let alternatives = SchemaNode::group(
            "Root",
            vec![SchemaNode::scalar("Amount", ScalarType::Float)],
        )
        .with_alternatives(vec![GroupAlternative {
            name: "Selected".into(),
            members: vec!["Amount".into()],
            required: vec!["Amount".into()],
            constraints: vec![GroupAlternativeConstraint {
                member: "Amount".into(),
                value: GroupAlternativeConstraintValue::Float(FiniteF64::new(value).unwrap()),
            }],
        }])
        .unwrap();
        check_case(
            alternatives,
            &format!(r#"{{"Amount":{accepted}}}"#),
            &format!(r#"{{"Amount":{rejected}}}"#),
            group(value),
            group(other),
            &[(
                "/kind/alternatives/0/constraints/0/value/value",
                value.to_bits(),
            )],
        );

        let contains = SchemaNode::scalar("Values", ScalarType::Float)
            .repeating()
            .with_json_contains(
                JsonContainsConstraints::new([JsonContainsConstraint::new(
                    JsonContainsPredicate::schema(allowed("candidate", value)),
                    ItemCountRange::new(1, None).unwrap(),
                )])
                .unwrap(),
            )
            .unwrap();
        check_case(
            contains,
            &format!("[{accepted}]"),
            &format!("[{rejected}]"),
            Instance::Repeated(vec![Instance::Scalar(Value::Float(value))]),
            Instance::Repeated(vec![Instance::Scalar(Value::Float(other))]),
            &[(
                "/json_contains/0/predicate/schema/json_allowed_values/1/value",
                value.to_bits(),
            )],
        );

        let predicate = SchemaNode::group("predicate", vec![exact_range("Amount", value)])
            .with_dynamic_fields(
                SchemaNode::scalar("*", ScalarType::String)
                    .json_any()
                    .unwrap(),
            )
            .unwrap();
        let dependent = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Trigger", ScalarType::Bool),
                SchemaNode::scalar("Amount", ScalarType::Float),
            ],
        )
        .with_json_dependent_schemas(
            JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
                "Trigger",
                JsonSchemaPredicate::schema(predicate),
            )])
            .unwrap(),
        )
        .unwrap();
        let triggered = |value| {
            Instance::Group(
                (vec![
                    ("Trigger".into(), Instance::Scalar(Value::Bool(true))),
                    ("Amount".into(), Instance::Scalar(Value::Float(value))),
                ])
                .into(),
            )
        };
        check_case(
            dependent,
            &format!(r#"{{"Trigger":true,"Amount":{accepted}}}"#),
            &format!(r#"{{"Trigger":true,"Amount":{rejected}}}"#),
            triggered(value),
            triggered(other),
            &[
                (
                    "/json_dependent_schemas/0/predicate/schema/kind/children/0/numeric_range/bounds/minimum/value",
                    value.to_bits(),
                ),
                (
                    "/json_dependent_schemas/0/predicate/schema/kind/children/0/numeric_range/bounds/maximum/value",
                    value.to_bits(),
                ),
            ],
        );
    }
}

#[test]
fn embedded_schema_retains_v1_behavior_and_typed_v2_failures() {
    let stable = minimum("Amount", -0.0);
    let descriptor = codegen_schema::encode(&stable, MAX_EMBEDDED_JSON_SCHEMA_BYTES).unwrap();
    assert_eq!(descriptor, serde_json::to_string(&stable).unwrap());
    let decoded = super::parse_schema(&descriptor).unwrap();
    assert_eq!(
        serde_json::to_string(&stable).unwrap(),
        serde_json::to_string(&decoded).unwrap()
    );
    assert_eq!(
        parse_json(&descriptor, "0").unwrap(),
        Instance::Scalar(Value::Float(0.0))
    );

    let (_, high) = adjacent_values();
    let schema = SchemaNode::group("Root", vec![minimum("Amount", high), minimum("Zero", -0.0)]);
    let descriptor = explicit_v2(
        &schema,
        &[
            (
                "/kind/children/0/numeric_range/bounds/minimum/value",
                high.to_bits(),
            ),
            (
                "/kind/children/1/numeric_range/bounds/minimum/value",
                0x8000000000000000,
            ),
        ],
    );
    assert!(descriptor.starts_with(codegen_schema::V2_PREFIX));
    let decoded = super::parse_schema(&descriptor).unwrap();
    let Some(NumericRange::Number(range)) = decoded.child("Zero").unwrap().numeric_range else {
        unreachable!()
    };
    assert_eq!(
        range.minimum().unwrap().value().get().to_bits(),
        (-0.0_f64).to_bits()
    );
    let descriptor = explicit_v2(
        &minimum("Amount", high),
        &[("/numeric_range/bounds/minimum/value", high.to_bits())],
    );
    let decoded = super::parse_schema(&descriptor).unwrap();
    assert_eq!(
        serde_json::to_string(&decoded).unwrap(),
        serde_json::to_string(&minimum("Amount", high)).unwrap()
    );
    let marker = format!(
        "{}{bits:016x}",
        codegen_schema::FLOAT_BITS_MARKER_PREFIX,
        bits = high.to_bits()
    );
    for invalid in [
        "FERRULE-EMBEDDED-SCHEMA/3\n{}".into(),
        descriptor.replace(&marker, "FERRULE-F64-BITS:7ff0000000000000"),
    ] {
        assert!(matches!(
            parse_json(&invalid, "0"),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { .. })
        ));
        assert!(matches!(
            serialize_json(&invalid, &Instance::Scalar(Value::Float(0.0))),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { .. })
        ));
    }
}

#[test]
fn embedded_schema_prefix_participates_in_public_json_byte_cap() {
    let (_, high) = adjacent_values();
    let mut padding = SchemaNode::scalar("Padding", ScalarType::String);
    padding.fixed = Some(String::new());
    let mut schema = SchemaNode::group("Root", vec![minimum("Amount", high), padding]);
    let markers = [(
        "/kind/children/0/numeric_range/bounds/minimum/value",
        high.to_bits(),
    )];
    let overhead = explicit_v2(&schema, &markers).len();
    let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
        unreachable!()
    };
    children[1].fixed = Some("x".repeat(MAX_EMBEDDED_JSON_SCHEMA_BYTES - overhead));
    let descriptor = explicit_v2(&schema, &markers);
    assert!(descriptor.starts_with(codegen_schema::V2_PREFIX));
    assert_eq!(descriptor.len(), MAX_EMBEDDED_JSON_SCHEMA_BYTES);
    assert!(super::parse_schema(&descriptor).is_ok());
    let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
        unreachable!()
    };
    children[1].fixed.as_mut().unwrap().push('x');
    let descriptor = explicit_v2(&schema, &markers);
    assert_eq!(
        super::parse_schema(&descriptor),
        Err(JsonBoundaryError::EmbeddedSchemaTooLarge {
            bytes: MAX_EMBEDDED_JSON_SCHEMA_BYTES + 1,
            max: MAX_EMBEDDED_JSON_SCHEMA_BYTES,
        })
    );
}
