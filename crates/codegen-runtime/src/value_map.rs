use crate::{ScalarType, Value};

/// Applies one ordered value-map table using the engine's declared-input
/// coercion rules. Failed coercion retains the original input, a miss uses the
/// configured default, and a miss without a default produces Null.
pub fn value_map(
    input: Value,
    input_type: Option<ScalarType>,
    table: &[(Value, Value)],
    default: Option<Value>,
) -> Value {
    let input = input_type
        .and_then(|target| coerce_input(&input, target))
        .unwrap_or(input);
    table
        .iter()
        .find(|(candidate, _)| *candidate == input)
        .map(|(_, output)| output.clone())
        .or(default)
        .unwrap_or(Value::Null)
}

fn coerce_input(value: &Value, target: ScalarType) -> Option<Value> {
    match (target, value) {
        (_, Value::Null) => Some(Value::Null),
        (_, Value::XmlNil(value)) => Some(Value::XmlNil(*value)),
        (ScalarType::String, Value::String(value)) => Some(Value::String(value.clone())),
        (ScalarType::String, Value::Bool(value)) => Some(Value::String(value.to_string())),
        (ScalarType::String, Value::Int(value)) => Some(Value::String(value.to_string())),
        (ScalarType::String, Value::Float(value)) if value.is_finite() => {
            Some(Value::String(value.to_string()))
        }
        (ScalarType::String, Value::Float(_)) => None,
        (ScalarType::Int, Value::Int(value)) => Some(Value::Int(*value)),
        (ScalarType::Int, Value::Float(value))
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i64::MIN as f64
                && *value < -(i64::MIN as f64) =>
        {
            Some(Value::Int(*value as i64))
        }
        (ScalarType::Int, Value::String(value)) => value.trim().parse::<i64>().ok().map(Value::Int),
        (ScalarType::Float, Value::Float(value)) if value.is_finite() => Some(Value::Float(*value)),
        (ScalarType::Float, Value::Int(value)) => Some(Value::Float(*value as f64)),
        (ScalarType::Float, Value::String(value)) => value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Value::Float),
        (ScalarType::Bool, Value::Bool(value)) => Some(Value::Bool(*value)),
        (ScalarType::Bool, Value::String(value)) => match value.trim() {
            "true" | "1" => Some(Value::Bool(true)),
            "false" | "0" => Some(Value::Bool(false)),
            _ => None,
        },
        (ScalarType::Int | ScalarType::Float | ScalarType::Bool, _) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapped(
        input: Value,
        input_type: Option<ScalarType>,
        table: Vec<(Value, Value)>,
        default: Option<Value>,
    ) -> Value {
        value_map(input, input_type, &table, default)
    }

    #[test]
    fn ordered_lookup_uses_first_match_then_default_or_null() {
        let key = Value::String("same".into());
        assert_eq!(
            mapped(
                key.clone(),
                None,
                vec![
                    (key.clone(), Value::String("first".into())),
                    (key, Value::String("second".into())),
                ],
                Some(Value::String("default".into())),
            ),
            Value::String("first".into())
        );
        assert_eq!(
            mapped(
                Value::String("missing".into()),
                None,
                Vec::new(),
                Some(Value::Int(7)),
            ),
            Value::Int(7)
        );
        assert_eq!(
            mapped(Value::String("missing".into()), None, Vec::new(), None),
            Value::Null
        );
        assert_eq!(
            mapped(
                Value::Int(1),
                None,
                vec![(Value::String("1".into()), Value::String("coerced".into()))],
                Some(Value::String("type-sensitive".into())),
            ),
            Value::String("type-sensitive".into())
        );
    }

    #[test]
    fn null_and_xml_nil_survive_every_declared_coercion() {
        for target in [
            ScalarType::String,
            ScalarType::Int,
            ScalarType::Float,
            ScalarType::Bool,
        ] {
            assert_eq!(
                mapped(
                    Value::Null,
                    Some(target),
                    vec![(Value::Null, Value::String("null".into()))],
                    None,
                ),
                Value::String("null".into())
            );
            assert_eq!(
                mapped(
                    Value::xml_nil(),
                    Some(target),
                    vec![(Value::xml_nil(), Value::String("nil".into()))],
                    None,
                ),
                Value::String("nil".into())
            );
        }
    }

    #[test]
    fn string_coercion_formats_scalars_and_retains_non_finite_floats() {
        for (input, text) in [
            (Value::Bool(true), "true".to_string()),
            (Value::Int(i64::MIN), i64::MIN.to_string()),
            (Value::Float(-0.0), (-0.0_f64).to_string()),
            (Value::Float(f64::MAX), f64::MAX.to_string()),
        ] {
            assert_eq!(
                mapped(
                    input,
                    Some(ScalarType::String),
                    vec![(Value::String(text), Value::String("matched".into()))],
                    None,
                ),
                Value::String("matched".into())
            );
        }

        assert_eq!(
            mapped(
                Value::Float(f64::INFINITY),
                Some(ScalarType::String),
                vec![(
                    Value::Float(f64::INFINITY),
                    Value::String("retained".into()),
                )],
                None,
            ),
            Value::String("retained".into())
        );
        assert_eq!(
            mapped(
                Value::Float(f64::NAN),
                Some(ScalarType::String),
                vec![(Value::Float(f64::NAN), Value::String("impossible".into()))],
                Some(Value::String("default".into())),
            ),
            Value::String("default".into())
        );
    }

    #[test]
    fn integer_coercion_accepts_exact_in_range_values() {
        let largest_exact_below_upper_bound = -(i64::MIN as f64) - 1024.0;
        for (input, expected) in [
            (Value::Float(i64::MIN as f64), i64::MIN),
            (
                Value::Float(largest_exact_below_upper_bound),
                largest_exact_below_upper_bound as i64,
            ),
            (Value::String(format!("  {}  ", i64::MAX)), i64::MAX),
            (Value::String(i64::MIN.to_string()), i64::MIN),
        ] {
            assert_eq!(
                mapped(
                    input,
                    Some(ScalarType::Int),
                    vec![(Value::Int(expected), Value::String("matched".into()))],
                    None,
                ),
                Value::String("matched".into())
            );
        }
    }

    #[test]
    fn integer_coercion_failure_retains_fractional_out_of_range_and_text_values() {
        for input in [
            Value::Float(1.5),
            Value::Float(i64::MIN as f64 - 2048.0),
            Value::Float(-(i64::MIN as f64)),
            Value::Float(f64::NEG_INFINITY),
            Value::String("-9223372036854775809".into()),
            Value::String("9223372036854775808".into()),
            Value::String("1.0".into()),
            Value::Bool(true),
        ] {
            assert_eq!(
                mapped(
                    input.clone(),
                    Some(ScalarType::Int),
                    vec![(input, Value::String("retained".into()))],
                    None,
                ),
                Value::String("retained".into())
            );
        }
    }

    #[test]
    fn float_coercion_accepts_finite_numbers_and_retains_failed_inputs() {
        for (input, expected) in [
            (Value::Int(i64::MIN), i64::MIN as f64),
            (Value::Int(i64::MAX), i64::MAX as f64),
            (Value::String(" 1.25 ".into()), 1.25),
            (Value::Float(-0.0), -0.0),
        ] {
            assert_eq!(
                mapped(
                    input,
                    Some(ScalarType::Float),
                    vec![(Value::Float(expected), Value::String("matched".into()))],
                    None,
                ),
                Value::String("matched".into())
            );
        }

        for input in [
            Value::String("NaN".into()),
            Value::String("inf".into()),
            Value::String("not-a-number".into()),
            Value::Float(f64::INFINITY),
            Value::Bool(false),
        ] {
            assert_eq!(
                mapped(
                    input.clone(),
                    Some(ScalarType::Float),
                    vec![(input, Value::String("retained".into()))],
                    None,
                ),
                Value::String("retained".into())
            );
        }
    }

    #[test]
    fn bool_coercion_accepts_only_the_engine_lexical_forms() {
        for (text, expected) in [
            (" true ", true),
            ("1", true),
            ("false", false),
            (" 0 ", false),
        ] {
            assert_eq!(
                mapped(
                    Value::String(text.into()),
                    Some(ScalarType::Bool),
                    vec![(Value::Bool(expected), Value::String("matched".into()))],
                    None,
                ),
                Value::String("matched".into())
            );
        }

        for input in [
            Value::String("TRUE".into()),
            Value::String("yes".into()),
            Value::Int(1),
            Value::Float(0.0),
        ] {
            assert_eq!(
                mapped(
                    input.clone(),
                    Some(ScalarType::Bool),
                    vec![(input, Value::String("retained".into()))],
                    None,
                ),
                Value::String("retained".into())
            );
        }
    }
}
