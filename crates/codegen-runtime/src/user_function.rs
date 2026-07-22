use ir::{ScalarType, Value};

use crate::RuntimeError;

/// Adapts one value at a generated user-function boundary.
pub fn adapt_user_function_value(
    value: Value,
    expected: ScalarType,
    function: u64,
    parameter: Option<u64>,
) -> Result<Value, RuntimeError> {
    let found = value.type_name();
    let adapted = match (expected, value) {
        (_, value @ (Value::Null | Value::XmlNil(_))) => Some(value),
        (ScalarType::String, value @ Value::String(_))
        | (ScalarType::Int, value @ Value::Int(_))
        | (ScalarType::Float, value @ Value::Float(_))
        | (ScalarType::Bool, value @ Value::Bool(_)) => Some(value),
        (ScalarType::String, Value::Bool(value)) => Some(Value::String(value.to_string())),
        (ScalarType::String, Value::Int(value)) => Some(Value::String(value.to_string())),
        (ScalarType::String, Value::Float(value)) if value.is_finite() => {
            Some(Value::String(value.to_string()))
        }
        (ScalarType::Int, Value::Float(value))
            if value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value < -(i64::MIN as f64) =>
        {
            Some(Value::Int(value as i64))
        }
        (ScalarType::Int, Value::String(value)) => value.trim().parse::<i64>().ok().map(Value::Int),
        (ScalarType::Float, Value::Int(value)) => {
            let converted = value as f64;
            ((converted as i128) == i128::from(value)).then_some(Value::Float(converted))
        }
        (ScalarType::Float, Value::String(value)) => value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Value::Float),
        (ScalarType::Bool, Value::String(value)) => match value.trim() {
            "true" | "1" => Some(Value::Bool(true)),
            "false" | "0" => Some(Value::Bool(false)),
            _ => None,
        },
        _ => None,
    };
    adapted.ok_or(RuntimeError::UserFunctionType {
        function,
        parameter,
        expected,
        found,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapts_exact_scalar_boundaries_and_preserves_missing_values() {
        assert_eq!(
            adapt_user_function_value(Value::String(" 42 ".into()), ScalarType::Int, 7, Some(3),),
            Ok(Value::Int(42))
        );
        assert_eq!(
            adapt_user_function_value(Value::Int(42), ScalarType::Float, 7, None),
            Ok(Value::Float(42.0))
        );
        assert_eq!(
            adapt_user_function_value(Value::Null, ScalarType::Bool, 7, Some(3)),
            Ok(Value::Null)
        );
    }

    #[test]
    fn rejects_lossy_or_invalid_scalar_boundaries_with_function_context() {
        assert_eq!(
            adapt_user_function_value(Value::Float(1.5), ScalarType::Int, 7, Some(3)),
            Err(RuntimeError::UserFunctionType {
                function: 7,
                parameter: Some(3),
                expected: ScalarType::Int,
                found: "float",
            })
        );
    }
}
