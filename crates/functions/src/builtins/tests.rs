use super::*;
use crate::{BUILTIN_NAMES, is_known};

#[test]
fn concat_joins_mixed_scalar_types() {
    let result = call(
        "concat",
        &[
            Value::String("Jane".to_string()),
            Value::String(" ".to_string()),
            Value::String("Doe".to_string()),
        ],
    )
    .unwrap();
    assert_eq!(result, Value::String("Jane Doe".to_string()));
}

#[test]
fn isbn10_converts_to_bookland_isbn13_and_validates_check_digit() {
    assert_eq!(
        call("isbn10_to_isbn13", &[Value::String("0-7645-4964-2".into())]),
        Ok(Value::String("9780764549649".into()))
    );
    assert_eq!(
        call("isbn10_to_isbn13", &[Value::String("080442957X".into())]),
        Ok(Value::String("9780804429573".into()))
    );
    assert!(matches!(
        call("isbn10_to_isbn13", &[Value::String("0764549643".into())]),
        Err(FunctionError::InvalidArgument {
            function: "isbn10_to_isbn13",
            message: "ISBN-10 check digit is invalid"
        })
    ));
}

#[test]
fn add_promotes_int_and_float() {
    assert_eq!(
        call("add", &[Value::Int(29), Value::Int(1)]).unwrap(),
        Value::Int(30)
    );
    assert_eq!(
        call("add", &[Value::Int(1), Value::Float(1.5)]).unwrap(),
        Value::Float(2.5)
    );
}

#[test]
fn numeric_predicate_accepts_finite_numbers_and_numeric_strings() {
    for value in [
        Value::Int(-7),
        Value::Float(12.5),
        Value::String(" 42 ".into()),
        Value::String("-1.25e2".into()),
    ] {
        assert_eq!(call("is_numeric", &[value]).unwrap(), Value::Bool(true));
    }
    for value in [
        Value::Null,
        Value::Bool(true),
        Value::Float(f64::INFINITY),
        Value::String("forty-two".into()),
        Value::String("NaN".into()),
    ] {
        assert_eq!(call("is_numeric", &[value]).unwrap(), Value::Bool(false));
    }
}

#[test]
fn number_conversion_preserves_integers_and_parses_finite_decimals() {
    assert_eq!(
        call("to_number", &[Value::String(" 42 ".into())]).unwrap(),
        Value::Int(42)
    );
    assert_eq!(
        call("to_number", &[Value::String("12.5".into())]).unwrap(),
        Value::Float(12.5)
    );
    assert_eq!(
        call("to_number", &[Value::Float(6.25)]).unwrap(),
        Value::Float(6.25)
    );
    assert!(call("to_number", &[Value::String("NaN".into())]).is_err());
    assert!(call("to_number", &[Value::Bool(true)]).is_err());
}

#[test]
fn integer_arithmetic_reports_overflow() {
    for (function, left, right) in [
        ("add", i64::MAX, 1),
        ("subtract", i64::MIN, 1),
        ("multiply", i64::MIN, -1),
    ] {
        assert_eq!(
            call(function, &[Value::Int(left), Value::Int(right)]),
            Err(FunctionError::IntegerOverflow { function })
        );
    }
}

#[test]
fn numeric_arithmetic_coerces_finite_lexical_numbers() {
    assert_eq!(
        call(
            "add",
            &[Value::String(" 20 ".into()), Value::String("22".into())]
        ),
        Ok(Value::Int(42))
    );
    assert_eq!(
        call("multiply", &[Value::String("2.5".into()), Value::Int(4)]),
        Ok(Value::Float(10.0))
    );
    assert_eq!(
        call(
            "divide",
            &[Value::String("12.5".into()), Value::String("2.5".into())]
        ),
        Ok(Value::Float(5.0))
    );
    assert!(matches!(
        call(
            "subtract",
            &[Value::String("not-a-number".into()), Value::Int(1)]
        ),
        Err(FunctionError::TypeMismatch {
            function: "subtract",
            got: "string"
        })
    ));
}

#[test]
fn decimal_multiplication_does_not_expose_binary_float_artifacts() {
    let result = call("multiply", &[Value::Float(0.09), Value::Float(15.0)]).unwrap();

    assert_eq!(result, Value::Float(1.35));
    assert_eq!(scalar_text(&result), "1.35");
    assert_eq!(
        call(
            "multiply",
            &[Value::Float(1.234_567_890_123_456_7), Value::Float(1.0),],
        ),
        Ok(Value::Float(1.234_567_890_123_456_7))
    );
}

#[test]
fn upper_rejects_non_string_argument() {
    assert_eq!(
        call("upper", &[Value::Int(1)]),
        Err(FunctionError::TypeMismatch {
            function: "upper",
            got: "int"
        })
    );
}

#[test]
fn delay_passthrough_validates_duration_without_sleeping() {
    assert_eq!(
        call(
            "delay_passthrough",
            &[Value::String("response".into()), Value::Float(3.0)]
        ),
        Ok(Value::String("response".into()))
    );
    assert!(
        call(
            "delay_passthrough",
            &[Value::String("response".into()), Value::Int(-1)]
        )
        .is_err()
    );
}

#[test]
fn unknown_function_is_reported() {
    assert_eq!(
        call("nope", &[]),
        Err(FunctionError::UnknownFunction("nope".to_string()))
    );
    assert!(!is_known("nope"));
    assert!(is_known("concat"));
    assert!(is_known("flextext_parse_field"));
    for name in BUILTIN_NAMES {
        assert!(
            !matches!(call(name, &[]), Err(FunctionError::UnknownFunction(_))),
            "catalog entry `{name}` is not dispatched"
        );
    }
}

#[test]
fn comparisons_use_numeric_widening_and_mixed_string_lexical_order() {
    assert_eq!(
        call("greater_or_equal", &[Value::Int(65), Value::Int(60)]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        call("less_than", &[Value::Int(1), Value::Float(1.5)]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        call(
            "equal",
            &[Value::String("a".into()), Value::String("b".into())]
        )
        .unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        call("equal", &[Value::String("2008".into()), Value::Int(2008)]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        call("less_than", &[Value::Int(4), Value::String("12".into())]).unwrap(),
        Value::Bool(false)
    );
    for name in [
        "equal",
        "not_equal",
        "less_than",
        "greater_than",
        "less_or_equal",
        "greater_or_equal",
    ] {
        assert_eq!(
            call(name, &[Value::Null, Value::Int(0)]).unwrap(),
            Value::Bool(false),
            "{name}"
        );
        assert_eq!(
            call(name, &[Value::Int(0), Value::xml_nil()]).unwrap(),
            Value::Bool(false),
            "{name}"
        );
    }
}

#[test]
fn divide_by_zero_is_an_error() {
    assert_eq!(
        call("divide", &[Value::Int(1), Value::Int(0)]),
        Err(FunctionError::DivideByZero)
    );
}

#[test]
fn string_predicates() {
    assert_eq!(
        call(
            "starts_with",
            &[
                Value::String("Jane Doe".into()),
                Value::String("Jane".into())
            ]
        )
        .unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        call("length", &[Value::String("Jane".into())]).unwrap(),
        Value::Int(4)
    );
    assert_eq!(call("length", &[Value::Int(120)]), Ok(Value::Int(3)));
    assert_eq!(call("length", &[Value::Bool(false)]), Ok(Value::Int(5)));
    assert_eq!(call("length", &[Value::Null]), Ok(Value::Int(0)));
}

#[test]
fn normalize_space_and_empty_follow_string_semantics() {
    assert_eq!(
        call(
            "normalize_space",
            &[Value::String(" \talpha\r\n beta  gamma ".into())]
        )
        .unwrap(),
        Value::String("alpha beta gamma".into())
    );
    assert_eq!(
        call("is_empty", &[Value::String(String::new())]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        call("is_empty", &[Value::String(" ".into())]).unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn sql_like_matches_percent_and_single_character_wildcards() {
    for (value, pattern, expected) in [
        ("Baker", "B%", true),
        ("baker", "B%", true),
        ("Baker", "%ake_", true),
        ("Baker", "B_k_r", true),
        ("Baker", "B_k", false),
        ("", "%", true),
        ("", "_", false),
        ("é", "_", true),
    ] {
        assert_eq!(
            call(
                "sql_like",
                &[
                    Value::String(value.to_string()),
                    Value::String(pattern.to_string())
                ]
            )
            .unwrap(),
            Value::Bool(expected),
            "{value:?} LIKE {pattern:?}"
        );
    }
}

#[test]
fn padding_and_directional_trim_follow_character_semantics() {
    assert_eq!(
        call(
            "pad_string_left",
            &[Value::Int(7), Value::Float(3.0), Value::String("0".into())]
        )
        .unwrap(),
        Value::String("007".into())
    );
    assert_eq!(
        call(
            "pad_string_right",
            &[
                Value::String("AP".into()),
                Value::Int(4),
                Value::String("Z".into())
            ]
        )
        .unwrap(),
        Value::String("APZZ".into())
    );
    assert_eq!(
        call(
            "pad_string_left",
            &[
                Value::String("AP".into()),
                Value::Int(-3),
                Value::String("Z".into())
            ]
        )
        .unwrap(),
        Value::String("AP".into())
    );
    assert!(matches!(
        call(
            "pad_string_left",
            &[
                Value::String("AP".into()),
                Value::Int(3),
                Value::String("YZ".into())
            ]
        ),
        Err(FunctionError::InvalidArgument { .. })
    ));
    for name in ["pad_string_left", "pad_string_right"] {
        for desired_length in [
            Value::Int(MAX_GENERATED_PADDING_CHARS + 1),
            Value::Int(i64::MAX),
            Value::Float(f64::NAN),
            Value::Float(f64::INFINITY),
            Value::Float(f64::NEG_INFINITY),
        ] {
            assert!(matches!(
                call(
                    name,
                    &[
                        Value::String(String::new()),
                        desired_length,
                        Value::String("x".into()),
                    ]
                ),
                Err(FunctionError::InvalidArgument { function, .. }) if function == name
            ));
        }
    }
    assert_eq!(
        call(
            "pad_string_left",
            &[
                Value::String("7".into()),
                Value::Float(3.9),
                Value::String("0".into()),
            ]
        )
        .unwrap(),
        Value::String("007".into())
    );
    assert_eq!(
        call("left_trim", &[Value::String(" \t\nvalue\u{a0}".into())]).unwrap(),
        Value::String("value\u{a0}".into())
    );
    assert_eq!(
        call("right_trim", &[Value::String("\u{a0}value\r\n ".into())]).unwrap(),
        Value::String("\u{a0}value".into())
    );
}

#[test]
fn string_converts_every_scalar_value() {
    assert_eq!(
        call("string", &[Value::String("ferrule".into())]).unwrap(),
        Value::String("ferrule".into())
    );
    assert_eq!(
        call("string", &[Value::Bool(true)]).unwrap(),
        Value::String("true".into())
    );
    assert_eq!(
        call("string", &[Value::Int(42)]).unwrap(),
        Value::String("42".into())
    );
    assert_eq!(
        call("string", &[Value::Null]).unwrap(),
        Value::String(String::new())
    );
}

#[test]
fn format_number_applies_decimal_grouping_and_optional_digits() {
    assert_eq!(
        call(
            "format_number",
            &[Value::Float(1234.5), Value::String("#,##0.00".into())]
        )
        .unwrap(),
        Value::String("1,234.50".into())
    );
    assert_eq!(
        call(
            "format_number",
            &[Value::Float(123.456), Value::String("#,##0.00".into())]
        )
        .unwrap(),
        Value::String("123.46".into())
    );
    assert_eq!(
        call(
            "format_number",
            &[Value::Float(0.00025), Value::String("###0.0###".into())]
        )
        .unwrap(),
        Value::String("0.0003".into())
    );
    assert_eq!(
        call(
            "format_number",
            &[Value::Int(25), Value::String("00000.00".into())]
        )
        .unwrap(),
        Value::String("00025.00".into())
    );
}

#[test]
fn format_number_supports_subformats_scaling_and_custom_separators() {
    assert_eq!(
        call(
            "format_number",
            &[Value::Float(0.736), Value::String("#00%".into())]
        )
        .unwrap(),
        Value::String("74%".into())
    );
    assert_eq!(
        call(
            "format_number",
            &[Value::Float(-3.12), Value::String("#.00;(#.00)".into())]
        )
        .unwrap(),
        Value::String("(3.12)".into())
    );
    assert_eq!(
        call(
            "format_number",
            &[
                Value::Float(1234.5),
                Value::String("#.##0,00".into()),
                Value::String(",".into()),
                Value::String(".".into()),
            ]
        )
        .unwrap(),
        Value::String("1.234,50".into())
    );
}

#[test]
fn format_number_preserves_integers_and_rounds_shortest_decimals_half_up() {
    assert_eq!(
        call(
            "format_number",
            &[
                Value::Int(9_007_199_254_740_993),
                Value::String("#,##0".into()),
            ]
        )
        .unwrap(),
        Value::String("9,007,199,254,740,993".into())
    );
    assert_eq!(
        call(
            "format_number",
            &[Value::Int(i64::MIN), Value::String("0".into())]
        )
        .unwrap(),
        Value::String("-9223372036854775808".into())
    );
    assert_eq!(
        call(
            "format_number",
            &[Value::Float(1.005), Value::String("0.00".into())]
        )
        .unwrap(),
        Value::String("1.01".into())
    );
    assert_eq!(
        call(
            "format_number",
            &[Value::Float(0.00035), Value::String("###0.0###".into()),]
        )
        .unwrap(),
        Value::String("0.0004".into())
    );
}

#[test]
fn format_number_handles_optional_zero_and_extreme_finite_values() {
    assert_eq!(
        call(
            "format_number",
            &[Value::Int(0), Value::String("#.##".into())]
        )
        .unwrap(),
        Value::String("0".into())
    );
    assert_eq!(
        call(
            "format_number",
            &[Value::Int(0), Value::String("$#.## USD".into())]
        )
        .unwrap(),
        Value::String("$0 USD".into())
    );

    let precision = 400;
    let format = format!("0.{}", "0".repeat(precision));
    let Value::String(rendered) =
        call("format_number", &[Value::Float(1.0), Value::String(format)]).unwrap()
    else {
        panic!("format_number must return a string");
    };
    assert_eq!(rendered, format!("1.{}", "0".repeat(precision)));

    let Value::String(rendered) = call(
        "format_number",
        &[Value::Float(f64::MAX), Value::String("0%".into())],
    )
    .unwrap() else {
        panic!("format_number must return a string");
    };
    assert!(rendered.ends_with('%'));
    assert!(!rendered.contains("inf"));
    assert!(!rendered.contains("NaN"));
}

#[test]
fn format_number_rejects_invalid_pictures_and_separator_collisions() {
    for format in ["#0#", ".#0", "0;0;0", "0;;0", "0%%", "0%\u{2030}", "#,,##0"] {
        assert!(matches!(
            call(
                "format_number",
                &[Value::Int(1), Value::String(format.into())]
            ),
            Err(FunctionError::InvalidArgument { .. })
        ));
    }
    assert!(matches!(
        call(
            "format_number",
            &[Value::Int(1), Value::String("0;#0#".into())]
        ),
        Err(FunctionError::InvalidArgument { .. })
    ));
    assert!(matches!(
        call(
            "format_number",
            &[
                Value::Int(1),
                Value::String("0".into()),
                Value::String("#".into()),
            ]
        ),
        Err(FunctionError::InvalidArgument { .. })
    ));
    assert_eq!(
        call(
            "format_number",
            &[Value::Int(1_234_567), Value::String("####,##0".into()),]
        )
        .unwrap(),
        Value::String("1,234,567".into())
    );
}

#[test]
fn substring_family_follows_xpath_conventions() {
    assert_eq!(
        call(
            "substring",
            &[Value::String("motor car".into()), Value::Int(6)]
        )
        .unwrap(),
        Value::String(" car".into())
    );
    assert_eq!(
        call(
            "substring",
            &[
                Value::String("metadata".into()),
                Value::Int(4),
                Value::Int(3)
            ]
        )
        .unwrap(),
        Value::String("ada".into())
    );
    assert_eq!(
        call(
            "substring_before",
            &[
                Value::String("1999-04-01".into()),
                Value::String("-".into())
            ]
        )
        .unwrap(),
        Value::String("1999".into())
    );
    assert_eq!(
        call(
            "substring_after",
            &[
                Value::String("1999-04-01".into()),
                Value::String("-".into())
            ]
        )
        .unwrap(),
        Value::String("04-01".into())
    );
    // No match yields the empty string, not an error.
    assert_eq!(
        call(
            "substring_before",
            &[Value::String("abc".into()), Value::String("|".into())]
        )
        .unwrap(),
        Value::String("".into())
    );
    assert_eq!(
        call(
            "substring",
            &[
                Value::String("abc".into()),
                Value::Int(i64::MAX),
                Value::Int(i64::MAX),
            ]
        )
        .unwrap(),
        Value::String(String::new())
    );
}

#[test]
fn exists_distinguishes_null() {
    assert_eq!(call("exists", &[Value::Null]).unwrap(), Value::Bool(false));
    assert_eq!(
        call("exists", &[Value::String("".into())]).unwrap(),
        Value::Bool(true)
    );
}

#[test]
fn xml_nil_predicate_distinguishes_nil_null_and_values() {
    assert_eq!(
        call("is_xml_nil", &[Value::xml_nil()]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        call("is_xml_nil", &[Value::Null]).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        call("is_xml_nil", &[Value::String(String::new())]).unwrap(),
        Value::Bool(false)
    );
}

#[test]
fn round_supports_precision() {
    assert_eq!(
        call("round", &[Value::Float(2.5)]).unwrap(),
        Value::Float(3.0)
    );
    assert_eq!(call("round", &[Value::Int(7)]).unwrap(), Value::Int(7));
    assert_eq!(
        call("round", &[Value::Float(1.23456), Value::Int(2)]).unwrap(),
        Value::Float(1.23)
    );
}

#[test]
fn date_from_datetime_takes_the_date_part() {
    assert_eq!(
        call(
            "date_from_datetime",
            &[Value::String("2024-03-01T10:30:00".into())]
        )
        .unwrap(),
        Value::String("2024-03-01".into())
    );
    assert_eq!(
        call("date_from_datetime", &[Value::String("2024-03-01".into())]).unwrap(),
        Value::String("2024-03-01".into())
    );
}

#[test]
fn year_from_datetime_returns_the_validated_local_year() {
    for (value, year) in [
        ("1999-12-31T19:20:00-05:00", 1999),
        ("2000-01-01T00:00:00Z", 2000),
        ("1999-12-31T24:00:00", 2000),
        ("-0001-12-31T24:00:00.0Z", 1),
        ("-0004-02-29T23:59:59.5+14:00", -4),
        ("2019-07-01-05:00", 2019),
        ("9223372036854775807-01-01", i64::MAX),
        ("-9223372036854775808-01-01", i64::MIN),
    ] {
        assert_eq!(
            call("year_from_datetime", &[Value::String(value.into())]),
            Ok(Value::Int(year))
        );
    }
    for value in [
        "9223372036854775808-01-01",
        "-9223372036854775809-01-01",
        "9223372036854775807-12-31T24:00:00",
    ] {
        assert!(matches!(
            call("year_from_datetime", &[Value::String(value.into())]),
            Err(FunctionError::InvalidArgument { .. })
        ));
    }
}

#[test]
fn month_from_datetime_returns_the_validated_local_month() {
    for (value, month) in [
        ("1999-12-31T19:20:00-05:00", 12),
        ("2000-01-01T00:00:00Z", 1),
        ("-0004-02-29T23:59:59.5+14:00", 2),
        ("2019-07-01", 7),
        ("2019-07-01Z", 7),
        ("2019-07-01-05:00", 7),
        ("2000-02-28T24:00:00", 2),
        ("2000-02-29T24:00:00", 3),
        ("1999-12-31T24:00:00", 1),
    ] {
        assert_eq!(
            call("month_from_datetime", &[Value::String(value.into())]).unwrap(),
            Value::Int(month)
        );
    }
    assert_eq!(
        call("month_from_datetime", &[Value::Null]).unwrap(),
        Value::Null
    );
    for value in [
        "2000-13-01T00:00:00",
        "2001-02-29T00:00:00",
        "2000-01-01T24:01:00",
    ] {
        assert!(call("month_from_datetime", &[Value::String(value.into())]).is_err());
    }
}

#[test]
fn day_from_datetime_returns_the_validated_local_day() {
    for (value, day) in [
        ("1999-12-31T19:20:00-05:00", 31),
        ("2000-01-01T00:00:00Z", 1),
        ("-0004-02-29T23:59:59.5+14:00", 29),
        ("2019-07-08", 8),
        ("2019-07-08Z", 8),
        ("2019-07-08-05:00", 8),
        ("2000-02-28T24:00:00", 29),
        ("2000-02-29T24:00:00", 1),
        ("1999-12-31T24:00:00", 1),
    ] {
        assert_eq!(
            call("day_from_datetime", &[Value::String(value.into())]),
            Ok(Value::Int(day))
        );
    }
    assert_eq!(call("day_from_datetime", &[Value::Null]), Ok(Value::Null));
    for value in [
        "2000-04-31T00:00:00",
        "2001-02-29T00:00:00",
        "2000-01-01T24:00:01",
    ] {
        assert!(call("day_from_datetime", &[Value::String(value.into())]).is_err());
    }
}

#[test]
fn day_from_datetime_rejects_invalid_calls() {
    assert_eq!(
        call("day_from_datetime", &[Value::Int(1)]),
        Err(FunctionError::TypeMismatch {
            function: "day_from_datetime",
            got: "int",
        })
    );
    assert_eq!(
        call("day_from_datetime", &[]),
        Err(FunctionError::ArityMismatch {
            function: "day_from_datetime",
            expected: 1,
            got: 0,
        })
    );
    assert_eq!(
        call(
            "day_from_datetime",
            &[
                Value::String("2024-01-01".into()),
                Value::String("2024-01-02".into()),
            ],
        ),
        Err(FunctionError::ArityMismatch {
            function: "day_from_datetime",
            expected: 1,
            got: 2,
        })
    );
}

#[test]
fn time_component_extractors_return_local_values() {
    for (value, hour, minute) in [
        ("1999-12-31T19:20:00-05:00", 19, 20),
        ("2000-01-01T00:01:00Z", 0, 1),
        ("-0004-02-29T23:59:59.5+14:00", 23, 59),
        ("1999-12-31T24:00:00.000-05:00", 0, 0),
    ] {
        assert_eq!(
            call("hours_from_datetime", &[Value::String(value.into())]),
            Ok(Value::Int(hour))
        );
        assert_eq!(
            call("minutes_from_datetime", &[Value::String(value.into())]),
            Ok(Value::Int(minute))
        );
    }
}

#[test]
fn new_datetime_extractors_propagate_null_and_reject_invalid_calls() {
    for function in [
        "year_from_datetime",
        "hours_from_datetime",
        "minutes_from_datetime",
    ] {
        assert_eq!(call(function, &[Value::Null]), Ok(Value::Null));
        assert_eq!(
            call(function, &[Value::Int(1)]),
            Err(FunctionError::TypeMismatch {
                function,
                got: "int",
            })
        );
        assert_eq!(
            call(function, &[]),
            Err(FunctionError::ArityMismatch {
                function,
                expected: 1,
                got: 0,
            })
        );
        assert_eq!(
            call(
                function,
                &[
                    Value::String("2024-01-01T00:00:00".into()),
                    Value::String("2024-01-02T00:00:00".into()),
                ],
            ),
            Err(FunctionError::ArityMismatch {
                function,
                expected: 1,
                got: 2,
            })
        );
    }

    for (function, values) in [
        (
            "year_from_datetime",
            &["0000-01-01", "2001-02-29T00:00:00", "2024-01💣-01"] as &[_],
        ),
        (
            "hours_from_datetime",
            &[
                "2024-01-01",
                "2024-01-01T24:00:00.1",
                "2024-01-01T24:00💣:00",
            ],
        ),
        (
            "minutes_from_datetime",
            &["2024-01-01T00:60:00", "2024-01-01T00:00:00+15:00"],
        ),
    ] {
        for value in values {
            assert!(matches!(
                call(function, &[Value::String((*value).into())]),
                Err(FunctionError::InvalidArgument { .. })
            ));
        }
    }
}

#[test]
fn substitute_missing_replaces_absent_and_xml_nil_values() {
    assert_eq!(
        call(
            "substitute_missing",
            &[Value::xml_nil(), Value::String("fallback".into())]
        )
        .unwrap(),
        Value::String("fallback".into())
    );
    assert_eq!(
        call(
            "substitute_missing",
            &[Value::Null, Value::String("fallback".into())]
        )
        .unwrap(),
        Value::String("fallback".into())
    );
    assert_eq!(
        call(
            "substitute_missing",
            &[
                Value::String(String::new()),
                Value::String("fallback".into())
            ]
        )
        .unwrap(),
        Value::String(String::new())
    );
    assert_eq!(
        call("substitute_missing", &[Value::Int(0), Value::Int(9)]).unwrap(),
        Value::Int(0)
    );
}

#[test]
fn boolean_combinators() {
    assert_eq!(
        call("and", &[Value::Bool(true), Value::Bool(false)]).unwrap(),
        Value::Bool(false)
    );
    assert_eq!(
        call("or", &[Value::Bool(true), Value::Bool(false)]).unwrap(),
        Value::Bool(true)
    );
    assert_eq!(
        call("not", &[Value::Bool(true)]).unwrap(),
        Value::Bool(false)
    );
}
