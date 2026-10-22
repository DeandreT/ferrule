use functions::{FunctionError, call};
use ir::{Decimal96, Value};

#[test]
fn decimal_text_is_exact_and_legacy_numeric_calls_refuse_it() {
    let value = Value::decimal(Decimal96::parse_exact("9007199254740993.1").unwrap());
    assert_eq!(
        call("string", std::slice::from_ref(&value)),
        Ok(Value::String("9007199254740993.1".into()))
    );
    assert_eq!(
        call("concat", &[value.clone(), Value::String("!".into())]),
        Ok(Value::String("9007199254740993.1!".into()))
    );
    assert_eq!(
        call("is_numeric", std::slice::from_ref(&value)),
        Ok(Value::Bool(true))
    );
    for function in ["to_number", "boolean", "positive", "floor", "format_number"] {
        let arguments = if function == "format_number" {
            vec![value.clone(), Value::String("0.00".into())]
        } else {
            vec![value.clone()]
        };
        assert!(
            matches!(
                call(function, &arguments),
                Err(FunctionError::TypeMismatch { got: "decimal", .. })
            ),
            "{function}"
        );
    }
    for function in [
        "add",
        "sqlite_multiply",
        "subtract",
        "multiply",
        "divide",
        "equal",
        "not_equal",
        "less_than",
        "greater_than",
    ] {
        for arguments in [
            vec![value.clone(), Value::Int(1)],
            vec![Value::Int(1), value.clone()],
        ] {
            assert!(
                matches!(
                    call(function, &arguments),
                    Err(FunctionError::TypeMismatch { got: "decimal", .. })
                ),
                "{function}"
            );
        }
    }
    for absent in [Value::Null, Value::json_null()] {
        for arguments in [
            vec![value.clone(), absent.clone()],
            vec![absent, value.clone()],
        ] {
            assert_eq!(
                call("sqlite_multiply", &arguments),
                Err(FunctionError::TypeMismatch {
                    function: "sqlite_multiply",
                    got: "decimal"
                })
            );
        }
    }
}
