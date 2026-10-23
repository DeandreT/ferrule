use functions::call;
use ir::{Decimal96, Value};

#[test]
fn native_integer_picture_rounds_in_the_decimal_domain() {
    for (input, expected) in [
        ("0", "0"),
        ("-0", "0"),
        ("0.1", "0"),
        ("0.49", "0"),
        ("0.5", "1"),
        ("0.5000000000000000000000000001", "1"),
        ("0.999", "1"),
        ("1.5", "2"),
        ("2.5", "3"),
        ("-0.5", "-1"),
        ("-1.5", "-2"),
        ("9007199254740993.5", "9007199254740994"),
        ("-0.1", "-0"),
        ("-0.49", "-0"),
        ("-0.4999999999999999999999999999", "-0"),
        ("0.4999999999999999999999999999", "0"),
        ("1.0000000000000000000000000001", "1"),
        ("-1.0000000000000000000000000001", "-1"),
        ("-0.0000000000000000000000000001", "-0"),
        (
            "79228162514264337593543950335",
            "79228162514264337593543950335",
        ),
        (
            "-79228162514264337593543950335",
            "-79228162514264337593543950335",
        ),
    ] {
        let value = Value::decimal(Decimal96::parse_exact(input).unwrap());
        assert_eq!(
            call("format_decimal_integer", &[value]).unwrap(),
            Value::String(expected.into())
        );
    }
}

#[test]
fn exact_integer_picture_is_not_an_implicit_legacy_numeric_conversion() {
    assert!(call("format_decimal_integer", &[Value::Int(1)]).is_err());
    assert!(
        call(
            "format_number",
            &[
                Value::decimal(Decimal96::parse_exact("1").unwrap()),
                Value::String("#".into())
            ]
        )
        .is_err()
    );
}
