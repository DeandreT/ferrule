use functions::{FunctionError, call};
use ir::{Decimal96, DecimalError, Value};

#[test]
fn nullable_text_conversion_keeps_exact_plain_and_explicit_exponent_policies() {
    for (text, expected) in [
        ("  0.1\t", "0.1"),
        ("9007199254740993", "9007199254740993"),
        ("2.5000", "2.5"),
        (
            "0.12345678901234567890123456785",
            "0.1234567890123456789012345678",
        ),
        ("0.00000000000000000000000000005", "0"),
        ("1e2", "100"),
        ("9007199254740993e0", "9007199254740990"),
    ] {
        assert_eq!(
            call("decimal_from_nullable_text", &[Value::String(text.into())]),
            Ok(Value::decimal(Decimal96::parse_exact(expected).unwrap())),
            "{text}"
        );
    }
    for value in [Value::Null, Value::json_null()] {
        assert_eq!(
            call("decimal_from_nullable_text", &[value]),
            Ok(Value::decimal(Decimal96::ZERO))
        );
    }
}

#[test]
fn nullable_text_conversion_does_not_hide_invalid_input_or_admit_other_scalar_types() {
    for text in [
        "",
        " ",
        "abc",
        "1,000",
        "NaN",
        "INF",
        "1e9999",
        "79228162514264337593543950336",
    ] {
        assert!(
            matches!(
                call("decimal_from_nullable_text", &[Value::String(text.into())]),
                Err(FunctionError::DecimalConversion { .. })
            ),
            "{text}"
        );
    }
    for value in [
        Value::Int(1),
        Value::Float(0.1),
        Value::Bool(false),
        Value::xml_nil(),
        Value::decimal(Decimal96::ZERO),
    ] {
        assert!(matches!(
            call("decimal_from_nullable_text", &[value]),
            Err(FunctionError::TypeMismatch { .. })
        ));
    }
    assert_eq!(
        call(
            "decimal_from_nullable_text",
            &[Value::String("0".repeat(4097))]
        ),
        Err(FunctionError::DecimalConversion {
            function: "decimal_from_nullable_text",
            source: DecimalError::InputTooLong
        })
    );
    assert!(matches!(
        call("decimal_from_nullable_text", &[]),
        Err(FunctionError::ArityMismatch {
            expected: 1,
            got: 0,
            ..
        })
    ));
}
