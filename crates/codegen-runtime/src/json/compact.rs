use ir::{
    Instance, IntegerRange, JsonAllowedValue, JsonAllowedValues, JsonMultipleOf,
    JsonMultipleOfConstraints, JsonPatternConstraints, NumericRange, ScalarType, SchemaNode, Value,
};

use super::{
    JsonBoundaryError, MAX_EMBEDDED_JSON_SCHEMA_BYTES, MAX_JSON_DOCUMENT_BYTES, serialize_json,
    serialize_json_bytes, serialize_json_compact, serialize_json_compact_bytes,
};

fn encoded(schema: &SchemaNode) -> String {
    codegen_schema::encode(schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES).expect("bounded test schema")
}

fn row(value: &str) -> Instance {
    Instance::Group(
        vec![(
            "Value".into(),
            Instance::Scalar(Value::String(value.into())),
        )]
        .into(),
    )
}

fn assert_compact(schema: &str, instance: &Instance, expected: &str) {
    assert_eq!(
        serialize_json_compact(schema, instance).as_deref(),
        Ok(expected)
    );
    assert_eq!(
        serialize_json_compact_bytes(schema, instance).as_deref(),
        Ok(expected.as_bytes())
    );
}

fn assert_output_error(schema: &str, instance: &Instance, message: &str) {
    for result in [
        serialize_json(schema, instance).map(drop),
        serialize_json_bytes(schema, instance).map(drop),
        serialize_json_compact(schema, instance).map(drop),
        serialize_json_compact_bytes(schema, instance).map(drop),
    ] {
        assert_eq!(
            result,
            Err(JsonBoundaryError::InvalidOutput {
                message: message.into()
            })
        );
    }
}

#[test]
fn compact_documents_match_frozen_18_and_31_byte_fixtures() {
    let schema = SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    let descriptor = encoded(&schema);
    for (value, expected) in [
        ("P.root", r#"{"Value":"P.root"}"#),
        ("N.root", r#"{"Value":"N.root"}"#),
    ] {
        assert_eq!(expected.len(), 18);
        assert_compact(&descriptor, &row(value), expected);
    }
    let rows = Instance::Repeated(vec![row("N1"), row("N2")]);
    let expected = r#"[{"Value":"N1"},{"Value":"N2"}]"#;
    assert_eq!(expected.len(), 31);
    // Both explicit arrays and flat generated rows retain the complete document.
    for schema in [schema.clone(), schema.repeating()] {
        assert_compact(&encoded(&schema), &rows, expected);
    }
}

#[test]
fn compact_output_preserves_field_order_escaped_whitespace_and_unicode() {
    let schema = encoded(&SchemaNode::group(
        "Root",
        vec![
            SchemaNode::scalar("Zeta", ScalarType::String),
            SchemaNode::scalar("Alpha", ScalarType::Int),
        ],
    ));
    let instance = Instance::Group(
        vec![
            ("Alpha".into(), Instance::Scalar(Value::String("7".into()))),
            (
                "Zeta".into(),
                Instance::Scalar(Value::String(" \t雪😀e\u{301}\n\"\\\r\u{8}\u{c}".into())),
            ),
        ]
        .into(),
    );
    assert_compact(
        &schema,
        &instance,
        r#"{"Zeta":" \t雪😀é\n\"\\\r\b\f","Alpha":7}"#,
    );
    let pretty = "{\n  \"Zeta\": \" \\t雪😀e\u{301}\\n\\\"\\\\\\r\\b\\f\",\n  \"Alpha\": 7\n}\n";
    assert_eq!(serialize_json(&schema, &instance).as_deref(), Ok(pretty));
    assert_eq!(
        serialize_json_bytes(&schema, &instance).as_deref(),
        Ok(pretty.as_bytes())
    );
}

#[test]
fn compact_numbers_retain_signed_zero_and_difficult_canonical_spellings() {
    let integer = encoded(&SchemaNode::scalar("Value", ScalarType::Int));
    for (value, expected) in [
        (i64::MIN, "-9223372036854775808"),
        (i64::MAX, "9223372036854775807"),
        (9_007_199_254_740_993, "9007199254740993"),
    ] {
        assert_compact(&integer, &Instance::Scalar(Value::Int(value)), expected);
    }
    let float = encoded(&SchemaNode::scalar("Value", ScalarType::Float));
    // Literal bits and spellings from the existing normalization/uniqueness contracts.
    for (value, expected) in [
        (0.0, "0.0"),
        (-0.0, "-0.0"),
        (f64::from_bits(1), "5e-324"),
        (f64::from_bits(0x0031_fa18_2c40_c60d), "1e-307"),
        (
            f64::from_bits(0x0031_fa18_2c40_c60e),
            "1.0000000000000001e-307",
        ),
        (f64::from_bits(0x3fef_ffff_ffff_fc19), "0.9999999999998891"),
        (f64::from_bits(0x3fef_ffff_ffff_fc1a), "0.9999999999998892"),
        (0.30000000000000004, "0.30000000000000004"),
    ] {
        assert_compact(&float, &Instance::Scalar(Value::Float(value)), expected);
    }
}

#[test]
fn compact_output_validates_typed_coercions_and_portable_constraints() {
    let count = SchemaNode::scalar("Count", ScalarType::Int)
        .with_numeric_range(NumericRange::Integer(
            IntegerRange::new(Some(5), Some(8)).unwrap(),
        ))
        .unwrap()
        .with_json_allowed_values(
            JsonAllowedValues::new([JsonAllowedValue::Int(6), JsonAllowedValue::Int(7)]).unwrap(),
        )
        .unwrap();
    let count = encoded(&count);
    assert_compact(&count, &Instance::Scalar(Value::String("7".into())), "7");
    assert_output_error(
        &count,
        &Instance::Scalar(Value::String("8".into())),
        "`Count` is not one of its JSON Schema allowed values",
    );
    assert_output_error(
        &count,
        &Instance::Scalar(Value::String("9".into())),
        "`Count` requires numeric range [5, 8], got 9",
    );
    assert_output_error(
        &count,
        &Instance::Group(Vec::new().into()),
        "`Count`: expected integer, got object",
    );

    let patterned = SchemaNode::scalar("Code", ScalarType::String)
        .with_json_patterns(JsonPatternConstraints::new([["^7$"]]).unwrap())
        .unwrap();
    let patterned = encoded(&patterned);
    assert_compact(&patterned, &Instance::Scalar(Value::Int(7)), r#""7""#);
    assert_output_error(
        &patterned,
        &Instance::Scalar(Value::Int(8)),
        "`Code` does not match its JSON Schema pattern constraints",
    );

    let multiple = JsonMultipleOf::from_decimal_lexical("0.25").unwrap();
    let amount = SchemaNode::scalar("Amount", ScalarType::Float)
        .with_json_multiple_of(JsonMultipleOfConstraints::new([[multiple]]).unwrap())
        .unwrap();
    let amount = encoded(&amount);
    assert_compact(
        &amount,
        &Instance::Scalar(Value::String("1.50".into())),
        "1.5",
    );
    assert_output_error(
        &amount,
        &Instance::Scalar(Value::String("1.3".into())),
        "`Amount` is not an exact multiple of its JSON Schema multipleOf constraints",
    );
    assert_output_error(
        &amount,
        &Instance::Scalar(Value::Float(f64::NAN)),
        "`Amount`: expected finite number, got non-finite float",
    );

    let unique = encoded(
        &SchemaNode::scalar("Values", ScalarType::Float)
            .repeating()
            .with_json_unique_items()
            .unwrap(),
    );
    let values = |first: &str, second: &str| {
        Instance::Repeated(vec![
            Instance::Scalar(Value::String(first.into())),
            Instance::Scalar(Value::String(second.into())),
        ])
    };
    assert_compact(&unique, &values("1", "2.0"), "[1.0,2.0]");
    assert_output_error(
        &unique,
        &values("9007199254740993.0", "9007199254740992"),
        "`Values` requires unique JSON array items, but indexes 1 and 2 are equal",
    );
}

#[test]
fn compact_output_checks_float_bits_without_reparsing_and_decodes_v2_metadata() {
    let low = Instance::Scalar(Value::Float(f64::from_bits(0x0031_fa18_2c40_c60d)));
    let high = Instance::Scalar(Value::Float(f64::from_bits(0x0031_fa18_2c40_c60e)));
    let multiple = r#"{"name":"Value","json_multiple_of":{"any_of":[[{"coefficient":1,"decimal_exponent":-307}]]},"kind":{"kind":"scalar","ty":"float"}}"#;
    assert_compact(multiple, &low, "1e-307");
    assert_output_error(
        multiple,
        &high,
        "`Value` is not an exact multiple of its JSON Schema multipleOf constraints",
    );

    let allowed = r#"{"name":"Value","json_allowed_values":[{"type":"float","value":1.0000000000000001e-307},{"type":"float","value":1.5}],"kind":{"kind":"scalar","ty":"float"}}"#;
    let v2 = format!(
        "{}{}",
        codegen_schema::V2_PREFIX,
        // V2 requires markers at both FiniteF64 positions, including exact 1.5.
        allowed
            .replace(
                "1.0000000000000001e-307",
                r#""FERRULE-F64-BITS:0031fa182c40c60e""#
            )
            .replace(
                r#""value":1.5"#,
                r#""value":"FERRULE-F64-BITS:3ff8000000000000""#
            )
    );
    for schema in [allowed, v2.as_str()] {
        assert_compact(schema, &high, "1.0000000000000001e-307");
        assert_output_error(
            schema,
            &low,
            "`Value` is not one of its JSON Schema allowed values",
        );
    }
}

#[test]
fn legacy_pretty_root_documents_and_schema_errors_remain_exact() {
    let schema = encoded(&SchemaNode::group(
        "Root",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    ));
    let pretty = "{\n  \"Value\": \"P.root\"\n}\n";
    assert_eq!(
        serialize_json(&schema, &row("P.root")).as_deref(),
        Ok(pretty)
    );
    assert_eq!(
        serialize_json_bytes(&schema, &row("P.root")).as_deref(),
        Ok(pretty.as_bytes())
    );
    let rows = Instance::Repeated(vec![row("N1"), row("N2")]);
    let pretty = "[\n  {\n    \"Value\": \"N1\"\n  },\n  {\n    \"Value\": \"N2\"\n  }\n]\n";
    assert_eq!(serialize_json(&schema, &rows).as_deref(), Ok(pretty));
    assert_eq!(
        serialize_json_bytes(&schema, &rows).as_deref(),
        Ok(pretty.as_bytes())
    );

    for descriptor in [
        "{}".to_string(),
        " ".repeat(MAX_EMBEDDED_JSON_SCHEMA_BYTES + 1),
    ] {
        let legacy = serialize_json(&descriptor, &rows).expect_err("invalid schema");
        assert!(matches!(
            legacy,
            JsonBoundaryError::InvalidEmbeddedSchema { .. }
                | JsonBoundaryError::EmbeddedSchemaTooLarge { .. }
        ));
        assert_eq!(serialize_json_compact(&descriptor, &rows), Err(legacy));
    }
}

#[test]
fn compact_size_limit_counts_no_lf_and_precedes_boundary_output_constraints() {
    let plain = encoded(&SchemaNode::scalar("Value", ScalarType::String));
    let constrained = encoded(
        &SchemaNode::scalar("Value", ScalarType::String)
            .with_json_allowed_values(
                JsonAllowedValues::new([
                    JsonAllowedValue::String("allowed".into()),
                    JsonAllowedValue::String("also-allowed".into()),
                ])
                .unwrap(),
            )
            .unwrap(),
    );
    let mut instance = Instance::Scalar(Value::String("a".repeat(MAX_JSON_DOCUMENT_BYTES - 2)));
    {
        let document = serialize_json_compact(&plain, &instance).expect("exact compact byte limit");
        assert_eq!(document.len(), MAX_JSON_DOCUMENT_BYTES);
        assert!(document.starts_with('"') && document.ends_with('"'));
        assert!(!document.ends_with('\n'));
    }
    let too_large = || JsonBoundaryError::OutputTooLarge {
        bytes: MAX_JSON_DOCUMENT_BYTES + 1,
        max: MAX_JSON_DOCUMENT_BYTES,
    };
    assert_eq!(serialize_json(&constrained, &instance), Err(too_large()));
    assert!(matches!(serialize_json_compact(&constrained, &instance),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message == "`Value` is not one of its JSON Schema allowed values"));
    let Instance::Scalar(Value::String(value)) = &mut instance else {
        panic!("string fixture")
    };
    value.push('a');
    assert_eq!(
        serialize_json_compact(&constrained, &instance),
        Err(too_large())
    );
}
