//! Generated JSON boundaries share the native tuple-frame codec, and their
//! own embedded-constraint walks reach every tuple position.

use ir::{
    Instance, ItemCountRange, JsonAllowedValue, JsonAllowedValues, JsonMultipleOf,
    JsonMultipleOfConstraints, JsonTupleLayout, JsonTupleTail, ScalarType, SchemaNode, Value,
};

use super::{JsonBoundaryError, MAX_EMBEDDED_JSON_SCHEMA_BYTES, parse_json, serialize_json};

/// `[code, {codes}, ...even integers]`: an allowed-value member, an object
/// member with a unique array, and a `multipleOf` tail.
fn frame() -> SchemaNode {
    let allowed = JsonAllowedValues::new([
        JsonAllowedValue::String("a".into()),
        JsonAllowedValue::String("b".into()),
    ])
    .expect("allowed values");
    let code = SchemaNode::scalar("item1", ScalarType::String)
        .with_json_allowed_values(allowed)
        .expect("allowed values match the string member");
    let mut codes = SchemaNode::scalar("codes", ScalarType::Int).repeating();
    codes.json_unique_items = true;
    let even = JsonMultipleOfConstraints::new([[
        JsonMultipleOf::from_decimal_lexical("2").expect("divisor")
    ]])
    .expect("multipleOf constraints");
    let rest = SchemaNode::scalar("rest", ScalarType::Int)
        .with_json_multiple_of(even)
        .expect("multipleOf matches the integer tail")
        .repeating();
    SchemaNode::group(
        "Row",
        vec![code, SchemaNode::group("item2", vec![codes]), rest],
    )
    .with_json_tuple(JsonTupleLayout {
        length: ItemCountRange::new(0, Some(4)),
        tail: JsonTupleTail::Open,
    })
    .expect("canonical frame")
}

fn descriptor(schema: &SchemaNode) -> String {
    codegen_schema::encode(schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES).expect("descriptor encodes")
}

fn row(code: &str, codes: &[i64], rest: &[i64]) -> Instance {
    let scalars = |values: &[i64]| {
        Instance::Repeated(
            values
                .iter()
                .map(|value| Instance::Scalar(Value::Int(*value)))
                .collect(),
        )
    };
    Instance::Group(
        vec![
            ("item1".into(), Instance::Scalar(Value::String(code.into()))),
            (
                "item2".into(),
                Instance::Group(vec![("codes".into(), scalars(codes))].into()),
            ),
            ("rest".into(), scalars(rest)),
        ]
        .into(),
    )
}

fn invalid_input(result: Result<Instance, JsonBoundaryError>) -> String {
    match result {
        Err(JsonBoundaryError::InvalidInput { message }) => message,
        other => panic!("expected invalid input, got {other:?}"),
    }
}

fn invalid_output(result: Result<String, JsonBoundaryError>) -> String {
    match result {
        Err(JsonBoundaryError::InvalidOutput { message }) => message,
        other => panic!("expected invalid output, got {other:?}"),
    }
}

#[test]
fn generated_boundaries_read_and_write_tuple_frames_natively() {
    let schema = frame();
    let descriptor = descriptor(&schema);
    let document = r#"["a",{"codes":[1,2]},4,6]"#;
    let expected = row("a", &[1, 2], &[4, 6]);
    assert_eq!(parse_json(&descriptor, document).expect("parse"), expected);
    assert_eq!(
        serialize_json(&descriptor, &expected).expect("serialize"),
        format_json::to_string(&schema, &expected).expect("native")
    );
    let message = invalid_input(parse_json(&descriptor, r#"["a",{},4,6,8]"#));
    assert!(
        message.contains("requires a tuple of at most 4 items"),
        "{message}"
    );
}

#[test]
fn positions_distinguished_only_by_embedded_constraints_stay_a_frame() {
    let allowed = JsonAllowedValues::new([
        JsonAllowedValue::String("a".into()),
        JsonAllowedValue::String("b".into()),
    ])
    .expect("allowed values");
    let schema = SchemaNode::group(
        "Pair",
        vec![
            SchemaNode::scalar("item1", ScalarType::String)
                .with_json_allowed_values(allowed)
                .expect("allowed values match the string member"),
            SchemaNode::scalar("item2", ScalarType::String),
        ],
    )
    .with_json_tuple(JsonTupleLayout {
        length: ItemCountRange::new(0, Some(2)),
        tail: JsonTupleTail::Closed,
    })
    .expect("canonical frame");
    let descriptor = descriptor(&schema);
    let pair = Instance::Group(
        vec![
            ("item1".into(), Instance::Scalar(Value::String("a".into()))),
            ("item2".into(), Instance::Scalar(Value::String("z".into()))),
        ]
        .into(),
    );
    assert_eq!(
        parse_json(&descriptor, r#"["a","z"]"#).expect("parse"),
        pair
    );
    assert!(serialize_json(&descriptor, &pair).is_ok());
    let message = invalid_input(parse_json(&descriptor, r#"["z","a"]"#));
    assert!(message.contains("`item1`"), "{message}");
}

#[test]
fn embedded_constraints_reach_every_tuple_position() {
    let descriptor = descriptor(&frame());
    for (document, expected) in [
        (
            r#"["c"]"#,
            "`item1` is not one of its JSON Schema allowed values",
        ),
        (r#"["a",{},3]"#, "`rest` is not an exact multiple"),
        (
            r#"["a",{"codes":[1,1]}]"#,
            "`codes` requires unique array items",
        ),
    ] {
        let message = invalid_input(parse_json(&descriptor, document));
        assert!(message.contains(expected), "{document}: {message}");
    }
    for (instance, expected) in [
        (
            row("c", &[], &[]),
            "`item1` is not one of its JSON Schema allowed values",
        ),
        (row("a", &[], &[3]), "`rest` is not an exact multiple"),
        (
            row("a", &[1, 1], &[]),
            "`codes` requires unique JSON array items",
        ),
    ] {
        let message = invalid_output(serialize_json(&descriptor, &instance));
        assert!(message.contains(expected), "{instance:?}: {message}");
    }
}
