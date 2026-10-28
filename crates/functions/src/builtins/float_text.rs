use ir::Value;

use crate::FunctionError;

/// Materialize the XML float writer's String conversion at a proved binding.
/// Other tags retain the existing target adaptation and serialization policy.
pub(super) fn coerce_float(args: &[Value]) -> Result<Value, FunctionError> {
    let [value] = args else {
        return Err(FunctionError::ArityMismatch {
            function: "coerce_float",
            expected: 1,
            got: args.len(),
        });
    };
    let Value::String(text) = value else {
        return Ok(value.clone());
    };
    text.trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
        .map(Value::Float)
        .ok_or(FunctionError::InvalidArgument {
            function: "coerce_float",
            message: "requires finite floating-point text",
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lexical_conversion_preserves_float_bits_without_integer_first_parsing() {
        for (text, expected) in [
            (" -0 ", -0.0_f64),
            (" 1.25e2\n", 125.0),
            ("9007199254740993", 9007199254740992.0),
            ("9223372036854775807", 9223372036854775808.0),
        ] {
            let Value::Float(actual) = coerce_float(&[Value::String(text.into())]).unwrap() else {
                panic!("expected float");
            };
            assert_eq!(actual.to_bits(), expected.to_bits(), "{text}");
        }
    }

    #[test]
    fn invalid_text_and_arity_keep_exact_errors() {
        for text in ["", " ", "not-a-number", "NaN", "inf", "-inf", "1e9999"] {
            assert_eq!(
                coerce_float(&[Value::String(text.into())]),
                Err(FunctionError::InvalidArgument {
                    function: "coerce_float",
                    message: "requires finite floating-point text",
                })
            );
        }
        assert_eq!(
            coerce_float(&[]),
            Err(FunctionError::ArityMismatch {
                function: "coerce_float",
                expected: 1,
                got: 0,
            })
        );
    }

    #[test]
    fn non_strings_remain_owned_values_without_new_conversion() {
        for value in [
            Value::Null,
            Value::json_null(),
            Value::xml_nil(),
            Value::Bool(true),
            Value::Int(i64::MAX),
            Value::Float(1.25),
            Value::Decimal(Box::new(ir::Decimal96::parse_exact("1.25").unwrap())),
        ] {
            assert_eq!(coerce_float(std::slice::from_ref(&value)).unwrap(), value);
        }
        let value = Value::Float(f64::NAN);
        let Value::Float(actual) = coerce_float(&[value]).unwrap() else {
            panic!("float tag");
        };
        assert_eq!(actual.to_bits(), f64::NAN.to_bits());
    }
}
