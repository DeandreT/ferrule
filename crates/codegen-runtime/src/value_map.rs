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
        .map(|value| match value {
            Value::JsonNull(_) => Value::Null,
            value => value,
        })
        .unwrap_or(input);
    table
        .iter()
        .find(|(candidate, _)| *candidate == input)
        .map(|(_, output)| output.clone())
        .or(default)
        .unwrap_or(Value::Null)
}

/// Applies an isolated user-function value map. An integer is converted to
/// Float only when f64 retains its exact value; failed conversion keeps the
/// original tag before the same ordered lookup and default handling.
pub fn value_map_user_function(
    input: Value,
    input_type: Option<ScalarType>,
    table: &[(Value, Value)],
    default: Option<Value>,
) -> Value {
    let converted = match (input_type, &input) {
        (Some(ScalarType::Float), Value::Int(value)) => {
            let converted = *value as f64;
            ((converted as i128) == i128::from(*value)).then_some(Value::Float(converted))
        }
        (Some(target), value) => coerce_input(value, target),
        (None, _) => None,
    };
    let input = match converted {
        Some(value) => value,
        None => input,
    };
    value_map(input, None, table, default)
}

pub(crate) fn coerce_input(value: &Value, target: ScalarType) -> Option<Value> {
    match (target, value) {
        (_, Value::Null) => Some(Value::Null),
        (_, Value::JsonNull(value)) => Some(Value::JsonNull(*value)),
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

    #[test]
    fn isolated_float_input_conversion_retains_inexact_integer_tags() {
        for (integer, rounded, exact) in [
            (9_007_199_254_740_991, 9_007_199_254_740_991.0, true),
            (9_007_199_254_740_992, 9_007_199_254_740_992.0, true),
            (9_007_199_254_740_993, 9_007_199_254_740_992.0, false),
            (9_007_199_254_740_994, 9_007_199_254_740_994.0, true),
            (-9_007_199_254_740_991, -9_007_199_254_740_991.0, true),
            (-9_007_199_254_740_992, -9_007_199_254_740_992.0, true),
            (-9_007_199_254_740_993, -9_007_199_254_740_992.0, false),
            (i64::MIN, -9_223_372_036_854_775_808.0, true),
            (i64::MIN + 1, -9_223_372_036_854_775_808.0, false),
            (i64::MAX, 9_223_372_036_854_775_808.0, false),
        ] {
            let table = [
                (Value::Int(integer), Value::String("original".into())),
                (Value::Int(integer), Value::String("later duplicate".into())),
                (Value::Float(rounded), Value::String("converted".into())),
            ];
            assert_eq!(
                value_map(Value::Int(integer), Some(ScalarType::Float), &table, None),
                Value::String("converted".into()),
                "ordinary input {integer}"
            );
            assert_eq!(
                value_map_user_function(Value::Int(integer), Some(ScalarType::Float), &table, None),
                Value::String(if exact { "converted" } else { "original" }.into()),
                "isolated input {integer}"
            );
        }
    }

    #[test]
    fn isolated_lookup_preserves_presence_defaults_and_float_equality() {
        for input in [
            Value::Null,
            Value::json_null(),
            Value::xml_nil(),
            Value::Bool(true),
            Value::String("not-a-number".into()),
            Value::Float(f64::INFINITY),
        ] {
            assert_eq!(
                value_map_user_function(
                    input.clone(),
                    Some(ScalarType::Float),
                    &[(input, Value::String("first".into()))],
                    None
                ),
                Value::String("first".into())
            );
        }
        let zeroes = [
            (Value::Float(0.0), Value::String("first zero".into())),
            (Value::Float(-0.0), Value::String("second zero".into())),
        ];
        assert_eq!(
            value_map_user_function(Value::Float(-0.0), Some(ScalarType::Float), &zeroes, None),
            Value::String("first zero".into())
        );
        assert_eq!(
            value_map_user_function(
                Value::Float(f64::NAN),
                Some(ScalarType::Float),
                &[(Value::Float(f64::NAN), Value::String("unreachable".into()))],
                Some(Value::String("miss".into()))
            ),
            Value::String("miss".into())
        );
        assert_eq!(
            value_map_user_function(
                Value::Int(1),
                None,
                &[(Value::Float(1.0), Value::String("wrong tag".into()))],
                None
            ),
            Value::Null
        );
        for default in [None, Some(Value::Null)] {
            assert_eq!(
                value_map_user_function(Value::Int(1), Some(ScalarType::Float), &[], default),
                Value::Null
            );
        }
        assert_eq!(
            value_map_user_function(
                Value::Int(1),
                Some(ScalarType::Float),
                &[],
                Some(Value::String(String::new()))
            ),
            Value::String(String::new())
        );
    }

    #[test]
    fn ordinary_declared_json_null_normalization_preserves_untyped_and_isolated_markers() {
        let table = [
            (Value::Null, Value::String("null-first".into())),
            (Value::Null, Value::String("null-second".into())),
            (Value::json_null(), Value::String("json-first".into())),
            (Value::json_null(), Value::String("json-second".into())),
            (Value::xml_nil(), Value::String("nil-first".into())),
            (Value::xml_nil(), Value::String("nil-second".into())),
        ];
        for target in [
            ScalarType::String,
            ScalarType::Int,
            ScalarType::Float,
            ScalarType::Bool,
        ] {
            for (input, ordinary, preserved) in [
                (Value::Null, "null-first", "null-first"),
                (Value::json_null(), "null-first", "json-first"),
                (Value::xml_nil(), "nil-first", "nil-first"),
            ] {
                let declared = value_map(
                    input.clone(),
                    Some(target),
                    &table,
                    Some(Value::String("miss".into())),
                );
                let untyped = value_map(
                    input.clone(),
                    None,
                    &table,
                    Some(Value::String("miss".into())),
                );
                let isolated = value_map_user_function(
                    input.clone(),
                    Some(target),
                    &table,
                    Some(Value::String("miss".into())),
                );
                eprintln!(
                    "marker {input:?}, target {target:?}: declared={declared:?}, untyped={untyped:?}, isolated={isolated:?}"
                );
                assert_eq!(declared, Value::String(ordinary.into()));
                assert_eq!(untyped, Value::String(preserved.into()));
                assert_eq!(isolated, Value::String(preserved.into()));
            }
            let only_json = [
                (Value::json_null(), Value::String("json-first".into())),
                (Value::json_null(), Value::String("json-second".into())),
            ];
            let declared = value_map(
                Value::json_null(),
                Some(target),
                &only_json,
                Some(Value::String("miss".into())),
            );
            let untyped = value_map(
                Value::json_null(),
                None,
                &only_json,
                Some(Value::String("miss".into())),
            );
            let isolated = value_map_user_function(
                Value::json_null(),
                Some(target),
                &only_json,
                Some(Value::String("miss".into())),
            );
            eprintln!(
                "only-json target {target:?}: declared={declared:?}, untyped={untyped:?}, isolated={isolated:?}"
            );
            assert_eq!(declared, Value::String("miss".into()));
            assert_eq!(untyped, Value::String("json-first".into()));
            assert_eq!(isolated, Value::String("json-first".into()));
            for default in [
                None,
                Some(Value::Null),
                Some(Value::json_null()),
                Some(Value::xml_nil()),
                Some(Value::String(String::new())),
            ] {
                let expected = default.clone().unwrap_or(Value::Null);
                let declared = value_map(Value::json_null(), Some(target), &[], default.clone());
                let untyped = value_map(Value::json_null(), None, &[], default.clone());
                let isolated =
                    value_map_user_function(Value::json_null(), Some(target), &[], default.clone());
                eprintln!(
                    "empty-table target {target:?}, default {default:?}: declared={declared:?}, untyped={untyped:?}, isolated={isolated:?}"
                );
                assert_eq!(declared, expected);
                assert_eq!(untyped, expected);
                assert_eq!(isolated, expected);
            }
            for output in [Value::json_null(), Value::xml_nil()] {
                let actual = value_map(
                    Value::json_null(),
                    Some(target),
                    &[(Value::Null, output.clone())],
                    None,
                );
                eprintln!("selected-output target {target:?}: {actual:?}");
                assert_eq!(actual, output);
            }
        }
    }
}
