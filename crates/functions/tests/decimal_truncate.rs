use ir::{Decimal96, Value};

#[test]
fn decimal_integer_part_is_exact_and_toward_zero() {
    for (input, expected) in [
        ("1.9", "1"),
        ("-1.9", "-1"),
        ("0.9", "0"),
        ("-0.9", "0"),
        ("9223372036854775808.9", "9223372036854775808"),
        (
            "79228162514264337593543950335",
            "79228162514264337593543950335",
        ),
        ("0.0000000000000000000000000001", "0"),
    ] {
        let result = functions::call(
            "decimal_truncate",
            &[Value::decimal(Decimal96::parse_exact(input).unwrap())],
        )
        .unwrap();
        assert_eq!(
            result,
            Value::decimal(Decimal96::parse_exact(expected).unwrap())
        );
    }
    assert!(functions::call("decimal_truncate", &[]).is_err());
    assert!(functions::call("decimal_truncate", &[Value::String("1.9".into())]).is_err());
    assert!(functions::call("decimal_truncate", &[Value::Int(1)]).is_err());
}
