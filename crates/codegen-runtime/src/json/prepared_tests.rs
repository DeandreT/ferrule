use std::sync::Arc;

use ir::{
    Instance, IntegerRange, JsonMultipleOf, JsonMultipleOfConstraints, JsonPatternConstraints,
    NumericRange, ScalarType, SchemaNode, Value,
};

use super::{
    JsonBoundaryError, JsonOutputFormat, MAX_EMBEDDED_JSON_SCHEMA_BYTES, MAX_JSON_DOCUMENT_BYTES,
    PreparedJsonSchema, check_input_size, constraint_input_error, constraint_output_error,
    parse_json, parse_json_bytes, parse_json_bytes_with_utf8_cause, parse_json_with_format_cause,
    parse_schema, prepared, serialize_json, serialize_json_bytes, serialize_json_compact,
    serialize_json_compact_bytes, unique_items, validate_instance_document, validate_json_document,
    without_boundary_constraints,
};

/// The per-call composition before preparation, retained as an oracle: every
/// call decodes the descriptor and uses the unprepared format functions.
mod unprepared {
    use super::*;

    fn schema(descriptor: &str) -> Result<SchemaNode, JsonBoundaryError> {
        let schema = parse_schema(descriptor)?;
        if !schema.json_pattern_budget_is_valid() {
            return Err(JsonBoundaryError::InvalidEmbeddedSchema {
                message:
                    "schema-wide JSON pattern metadata, program, or fixed-value work budget is invalid"
                        .to_string(),
            });
        }
        Ok(schema)
    }

    pub fn parse(descriptor: &str, document: &str) -> Result<Instance, JsonBoundaryError> {
        check_input_size(document.len())?;
        let schema = schema(descriptor)?;
        if unique_items::schema_contains_assertion(&schema) {
            format_json::runtime_constraints::validate_raw_json_unique_items(&schema, document)
                .map_err(|error| JsonBoundaryError::InvalidInput {
                    message: error.to_string(),
                })?;
        }
        let formatter = without_boundary_constraints(schema.clone());
        let instance = format_json::from_str(document, &formatter).map_err(|error| {
            JsonBoundaryError::InvalidInput {
                message: error.to_string(),
            }
        })?;
        validate_instance_document(&schema, &instance).map_err(constraint_input_error)?;
        Ok(instance)
    }

    pub fn parse_bytes(descriptor: &str, document: &[u8]) -> Result<Instance, JsonBoundaryError> {
        check_input_size(document.len())?;
        let document =
            std::str::from_utf8(document).map_err(|error| JsonBoundaryError::InvalidInput {
                message: format!("document is not UTF-8: {error}"),
            })?;
        parse(descriptor, document)
    }

    pub fn parse_bytes_with_cause(
        descriptor: &str,
        document: &[u8],
    ) -> Result<Instance, JsonBoundaryError> {
        check_input_size(document.len())?;
        let document = std::str::from_utf8(document).map_err(JsonBoundaryError::InvalidUtf8)?;
        parse(descriptor, document)
    }

    pub fn serialize(
        descriptor: &str,
        instance: &Instance,
        format: JsonOutputFormat,
    ) -> Result<String, JsonBoundaryError> {
        let schema = schema(descriptor)?;
        let formatter = without_boundary_constraints(schema.clone());
        let value = format_json::to_value(&formatter, instance).map_err(|error| {
            JsonBoundaryError::InvalidOutput {
                message: error.to_string(),
            }
        })?;
        let mut document = match format {
            JsonOutputFormat::Pretty => serde_json::to_string_pretty(&value),
            JsonOutputFormat::Compact => serde_json::to_string(&value),
        }
        .map_err(|error| JsonBoundaryError::InvalidOutput {
            message: error.to_string(),
        })?;
        if matches!(format, JsonOutputFormat::Pretty) {
            document.push('\n');
        }
        if document.len() > MAX_JSON_DOCUMENT_BYTES {
            return Err(JsonBoundaryError::OutputTooLarge {
                bytes: document.len(),
                max: MAX_JSON_DOCUMENT_BYTES,
            });
        }
        unique_items::validate_document(&schema, &value).map_err(unique_items::output_error)?;
        validate_json_document(&schema, &value).map_err(constraint_output_error)?;
        Ok(document)
    }
}

fn encoded(schema: &SchemaNode) -> String {
    let Ok(descriptor) = codegen_schema::encode(schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES) else {
        panic!("test schema encodes");
    };
    descriptor
}

fn patterned(name: &str, pattern: &str) -> SchemaNode {
    let Some(schema) = JsonPatternConstraints::new([[pattern]])
        .ok()
        .and_then(|patterns| {
            SchemaNode::scalar(name, ScalarType::String).with_json_patterns(patterns)
        })
    else {
        panic!("test pattern is valid for its string field");
    };
    schema
}

fn order() -> SchemaNode {
    let Some(quantity) = IntegerRange::new(Some(1), Some(1_000)).and_then(|range| {
        SchemaNode::scalar("Quantity", ScalarType::Int)
            .with_numeric_range(NumericRange::Integer(range))
    }) else {
        panic!("test range is valid for its integer field");
    };
    let Some(price) = JsonMultipleOf::from_decimal_lexical("0.25")
        .and_then(|divisor| JsonMultipleOfConstraints::new([[divisor]]).ok())
        .and_then(|constraints| {
            SchemaNode::scalar("Price", ScalarType::Float).with_json_multiple_of(constraints)
        })
    else {
        panic!("test multipleOf is valid for its float field");
    };
    let mut tags = SchemaNode::scalar("Tag", ScalarType::String).repeating();
    tags.json_unique_items = true;
    SchemaNode::group(
        "Order",
        vec![
            patterned("Id", "^ORD-[0-9]{6}$"),
            SchemaNode::group(
                "Line",
                vec![patterned("Sku", "^[A-Z]{2}-[0-9]{4}$"), quantity, price],
            )
            .repeating(),
            tags,
        ],
    )
}

const BUDGETED_PATTERN: &str = "^(b{1000}|a*)$";

/// A pattern charged about a thousand work units per input scalar but matched
/// through few active states.
fn budgeted() -> SchemaNode {
    patterned("Value", BUDGETED_PATTERN).repeating()
}

/// `count` values that each use three fifths of the per-document pattern work
/// budget: one fits, two exceed it.
fn values(count: usize) -> String {
    let Ok(pattern) = json_pattern::PortableJsonPattern::compile(BUDGETED_PATTERN) else {
        panic!("budgeted pattern compiles");
    };
    let length =
        json_pattern::DEFAULT_MATCH_WORK_LIMIT as usize / pattern.instruction_count() * 3 / 5;
    serde_json::to_string(&vec!["a".repeat(length); count]).unwrap_or_default()
}

fn work_limited(result: &Result<Instance, JsonBoundaryError>) -> bool {
    matches!(result, Err(JsonBoundaryError::InvalidInput { message }) if message.contains("work limit"))
}

fn assert_parse_matches(descriptor: &str, document: &str) {
    let expected = unprepared::parse(descriptor, document);
    assert_eq!(parse_json(descriptor, document), expected, "{document:.80}");
    // The cause-retaining entry point differs only in how it carries a format
    // failure; its display text and every other variant are the same.
    let display =
        |result: Result<Instance, JsonBoundaryError>| result.map_err(|error| error.to_string());
    assert_eq!(
        display(parse_json_with_format_cause(descriptor, document)),
        display(unprepared::parse(descriptor, document))
    );
    if let Ok(prepared) = PreparedJsonSchema::prepare(descriptor) {
        assert_eq!(prepared.parse_json(document), expected);
        assert_eq!(
            prepared.parse_json_bytes(document.as_bytes()),
            unprepared::parse_bytes(descriptor, document.as_bytes())
        );
    }
    assert_eq!(
        parse_json_bytes(descriptor, document.as_bytes()),
        unprepared::parse_bytes(descriptor, document.as_bytes())
    );
}

fn assert_bytes_match(descriptor: &str, document: &[u8]) {
    let expected = unprepared::parse_bytes(descriptor, document);
    let with_cause = unprepared::parse_bytes_with_cause(descriptor, document);
    assert_eq!(parse_json_bytes(descriptor, document), expected);
    assert_eq!(
        parse_json_bytes_with_utf8_cause(descriptor, document),
        with_cause
    );
    if let Ok(prepared) = PreparedJsonSchema::prepare(descriptor) {
        assert_eq!(prepared.parse_json_bytes(document), expected);
        assert_eq!(
            prepared.parse_json_bytes_with_utf8_cause(document),
            with_cause
        );
    }
}

fn assert_serialize_matches(descriptor: &str, instance: &Instance) {
    let pretty = unprepared::serialize(descriptor, instance, JsonOutputFormat::Pretty);
    let compact = unprepared::serialize(descriptor, instance, JsonOutputFormat::Compact);
    assert_eq!(serialize_json(descriptor, instance), pretty);
    assert_eq!(
        serialize_json_bytes(descriptor, instance),
        unprepared::serialize(descriptor, instance, JsonOutputFormat::Pretty)
            .map(String::into_bytes)
    );
    assert_eq!(serialize_json_compact(descriptor, instance), compact);
    assert_eq!(
        serialize_json_compact_bytes(descriptor, instance),
        unprepared::serialize(descriptor, instance, JsonOutputFormat::Compact)
            .map(String::into_bytes)
    );
    if let Ok(prepared) = PreparedJsonSchema::prepare(descriptor) {
        assert_eq!(prepared.serialize_json(instance), pretty);
        assert_eq!(prepared.serialize_json_compact(instance), compact);
    }
}

fn line(sku: &str, quantity: i64, price: f64) -> Instance {
    Instance::Group(
        vec![
            ("Sku".into(), Instance::Scalar(Value::String(sku.into()))),
            ("Quantity".into(), Instance::Scalar(Value::Int(quantity))),
            ("Price".into(), Instance::Scalar(Value::Float(price))),
        ]
        .into(),
    )
}

fn order_instance(id: &str, lines: Vec<Instance>, tags: &[&str]) -> Instance {
    Instance::Group(
        vec![
            ("Id".into(), Instance::Scalar(Value::String(id.into()))),
            ("Line".into(), Instance::Repeated(lines)),
            (
                "Tag".into(),
                Instance::Repeated(
                    tags.iter()
                        .map(|tag| Instance::Scalar(Value::String((*tag).into())))
                        .collect(),
                ),
            ),
        ]
        .into(),
    )
}

#[test]
fn prepared_schema_state_is_immutable_and_shareable() {
    fn shareable<T: Send + Sync>() {}
    shareable::<PreparedJsonSchema>();
    let descriptor = encoded(&order());
    let (Ok(first), Ok(second)) = (prepared::shared(&descriptor), prepared::shared(&descriptor))
    else {
        panic!("order schema prepares");
    };
    assert!(Arc::ptr_eq(&first, &second));
    // Equal text in a different allocation shares the same preparation.
    let copy = descriptor.clone();
    assert!(prepared::shared(&copy).is_ok_and(|third| Arc::ptr_eq(&first, &third)));
    let threads = std::thread::scope(|scope| {
        let handles = (0..4)
            .map(|_| scope.spawn(|| prepared::shared(&descriptor)))
            .collect::<Vec<_>>();
        handles
            .into_iter()
            .map(|handle| handle.join().ok().and_then(Result::ok))
            .collect::<Vec<_>>()
    });
    assert!(threads.iter().all(|shared| {
        shared
            .as_ref()
            .is_some_and(|shared| Arc::ptr_eq(&first, shared))
    }));
}

#[test]
fn rejected_descriptors_are_not_retained() {
    for invalid in [
        "FERRULE-EMBEDDED-SCHEMA/4\n{}".to_string(),
        "x".repeat(MAX_EMBEDDED_JSON_SCHEMA_BYTES + 1),
        "{".to_string(),
    ] {
        let first = prepared::shared(&invalid).err();
        assert!(first.is_some());
        assert_eq!(prepared::shared(&invalid).err(), first);
        assert_eq!(PreparedJsonSchema::prepare(&invalid).err(), first);
    }
}

#[test]
fn prepared_parsing_matches_unprepared_results_and_error_precedence() {
    let order = encoded(&order());
    for document in [
        r#"{"Id":"ORD-000001","Line":[{"Sku":"AB-0001","Quantity":2,"Price":1.25}],"Tag":["a","b"]}"#,
        "\u{feff}{\"Id\":\"ORD-000001\"}",
        r#"[{"Id":"ORD-000001"},{"Id":"ORD-000002"}]"#,
        r#"{"Id":"ORD-1"}"#,
        r#"{"Id":"ORD-000001","Line":[{"Sku":"ab-0001"}]}"#,
        r#"{"Id":"ORD-000001","Line":[{"Quantity":0}]}"#,
        r#"{"Id":"ORD-000001","Line":[{"Price":1.3}]}"#,
        r#"{"Id":"ORD-000001","Tag":["a","a"]}"#,
        r#"{"Id":"ORD-000001","Tag":["a","a"],"Line":[{"Sku":"bad"}]}"#,
        r#"{"Id":"ORD-000001","Unknown":true}"#,
        r#"{"Id":"ORD-000001","#,
        "",
    ] {
        assert_parse_matches(&order, document);
    }

    let budgeted = encoded(&budgeted());
    for count in [0, 1, 2, 3] {
        assert_parse_matches(&budgeted, &values(count));
    }
    assert!(!work_limited(&parse_json(&budgeted, &values(1))));
    assert!(work_limited(&parse_json(&budgeted, &values(2))));

    // Descriptor errors follow the input size check and precede every
    // document error; format-level schema errors follow JSON syntax errors.
    let mut missing_anchor = SchemaNode::group("Node", vec![SchemaNode::group("Child", vec![])]);
    if let ir::SchemaKind::Group { children, .. } = &mut missing_anchor.kind {
        children[0].recursive_ref = Some("Missing".into());
    }
    let missing_anchor = serde_json::to_string(&missing_anchor).unwrap_or_default();
    for descriptor in [
        "FERRULE-EMBEDDED-SCHEMA/4\n{}",
        "{",
        missing_anchor.as_str(),
    ] {
        for document in ["{}", "{", r#"{"Child":{}}"#] {
            assert_parse_matches(descriptor, document);
        }
        assert_bytes_match(descriptor, &[0xff]);
        assert_bytes_match(descriptor, &vec![b' '; MAX_JSON_DOCUMENT_BYTES + 1]);
    }

    // The inclusive byte limit, with decoding failures after it.
    let string = encoded(&SchemaNode::scalar("Value", ScalarType::String));
    let mut document = vec![b'a'; MAX_JSON_DOCUMENT_BYTES + 1];
    document[0] = 0xff;
    assert_bytes_match(&string, &document);
    document.truncate(MAX_JSON_DOCUMENT_BYTES);
    assert_bytes_match(&string, &document);
    document.fill(b'a');
    document[0] = b'"';
    document[MAX_JSON_DOCUMENT_BYTES - 1] = b'"';
    let Ok(text) = String::from_utf8(document) else {
        panic!("test document is ASCII");
    };
    assert_parse_matches(&string, &text);
    assert!(parse_json(&string, &text).is_ok());
}

#[test]
fn prepared_serialization_matches_unprepared_results_and_output_limits() {
    let order = encoded(&order());
    for instance in [
        order_instance("ORD-000001", vec![line("AB-0001", 2, 1.25)], &["a", "b"]),
        order_instance("ORD-1", vec![], &[]),
        order_instance("ORD-000001", vec![line("ab", 2, 1.25)], &[]),
        order_instance("ORD-000001", vec![line("AB-0001", 0, 1.25)], &[]),
        order_instance("ORD-000001", vec![line("AB-0001", 1, 1.3)], &[]),
        order_instance("ORD-000001", vec![], &["a", "a"]),
        Instance::Repeated(vec![
            order_instance("ORD-000001", vec![], &[]),
            order_instance("ORD-000002", vec![], &[]),
        ]),
        Instance::Scalar(Value::Int(7)),
    ] {
        assert_serialize_matches(&order, &instance);
    }
    for descriptor in ["FERRULE-EMBEDDED-SCHEMA/4\n{}", "{"] {
        assert_serialize_matches(descriptor, &Instance::Scalar(Value::Null));
    }

    // Exactly M pretty bytes, then M + 1. Compact limits are covered by the
    // public compact tests, which now share the same prepared path.
    let string = encoded(&SchemaNode::scalar("Value", ScalarType::String));
    let Ok(prepared) = PreparedJsonSchema::prepare(&string) else {
        panic!("string schema prepares");
    };
    for (length, accepted) in [
        (MAX_JSON_DOCUMENT_BYTES - 3, true),
        (MAX_JSON_DOCUMENT_BYTES - 2, false),
    ] {
        let instance = Instance::Scalar(Value::String("a".repeat(length)));
        let expected = unprepared::serialize(&string, &instance, JsonOutputFormat::Pretty);
        assert_eq!(expected.is_ok(), accepted);
        assert_eq!(prepared.serialize_json(&instance), expected);
        if !accepted {
            assert_eq!(serialize_json(&string, &instance), expected);
        }
    }
}

/// Concurrent calls with different inputs reach different limits. Each call
/// keeps its own pattern work budget and byte accounting, so a call that
/// exhausts its budget never consumes another call's.
#[test]
fn concurrent_calls_keep_isolated_accounting() {
    let budgeted = encoded(&budgeted());
    let order = encoded(&order());
    let within = values(1);
    let exhausted = values(2);
    let oversized = vec![b' '; MAX_JSON_DOCUMENT_BYTES + 1];
    let accepted = r#"{"Id":"ORD-000001","Line":[{"Sku":"AB-0001","Quantity":2,"Price":1.25}]}"#;
    let Ok(expected_within) = unprepared::parse(&budgeted, &within) else {
        panic!("one value fits the work budget");
    };
    let Ok(expected_order) = unprepared::parse(&order, accepted) else {
        panic!("order document is valid");
    };
    let expected_exhausted = unprepared::parse(&budgeted, &exhausted);
    assert!(work_limited(&expected_exhausted));
    let oversized_error = JsonBoundaryError::InputTooLarge {
        bytes: MAX_JSON_DOCUMENT_BYTES + 1,
        max: MAX_JSON_DOCUMENT_BYTES,
    };
    let output = order_instance("ORD-000001", vec![line("AB-0001", 2, 1.25)], &[]);
    let expected_output = unprepared::serialize(&order, &output, JsonOutputFormat::Pretty);
    let invalid_output = order_instance("ORD-1", vec![], &[]);
    let expected_invalid_output =
        unprepared::serialize(&order, &invalid_output, JsonOutputFormat::Pretty);
    assert!(matches!(
        expected_invalid_output,
        Err(JsonBoundaryError::InvalidOutput { .. })
    ));

    std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 0..2 {
            handles.push(scope.spawn(|| {
                (0..12).all(|_| parse_json(&budgeted, &within).as_ref() == Ok(&expected_within))
            }));
            handles.push(scope.spawn(|| {
                (0..12).all(|_| parse_json(&budgeted, &exhausted) == expected_exhausted)
            }));
            handles.push(scope.spawn(|| {
                (0..12).all(|_| {
                    parse_json_bytes(&budgeted, &oversized).as_ref() == Err(&oversized_error)
                        && parse_json(&order, accepted).as_ref() == Ok(&expected_order)
                })
            }));
            handles.push(scope.spawn(|| {
                (0..12).all(|_| {
                    serialize_json(&order, &output) == expected_output
                        && serialize_json(&order, &invalid_output) == expected_invalid_output
                })
            }));
        }
        for handle in handles {
            assert!(handle.join().unwrap_or(false));
        }
    });
}
