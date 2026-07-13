//! Built-in function library (string, math, date, aggregate, node-set) used
//! by mapping graphs, plus hooks for user-defined functions.
//!
//! Covers the string/math/comparison/boolean core plus the scalar helpers
//! MapForce designs lean on (substring family, exists, round, ISO
//! date-from-datetime); more built-ins land alongside the formats/semantics
//! that need them. Aggregates (count/sum/...) are not here: they reduce
//! collections in scope context, so they live in the engine as
//! `mapping::Node::Aggregate`.

use ir::Value;
use thiserror::Error;

mod datetime;
mod datetime_add;
mod filepath;
mod format_number;

const MAX_GENERATED_PADDING_CHARS: i64 = 1_000_000;

#[derive(Debug, Error, PartialEq)]
pub enum FunctionError {
    #[error("unknown function `{0}`")]
    UnknownFunction(String),
    #[error("`{function}` expected {expected} argument(s), got {got}")]
    ArityMismatch {
        function: &'static str,
        expected: usize,
        got: usize,
    },
    #[error("`{function}` cannot accept a {got} argument")]
    TypeMismatch {
        function: &'static str,
        got: &'static str,
    },
    #[error("division by zero")]
    DivideByZero,
    #[error("`{function}` integer arithmetic overflowed")]
    IntegerOverflow { function: &'static str },
    #[error("`{function}` {message}")]
    InvalidArgument {
        function: &'static str,
        message: &'static str,
    },
}

/// Scalar builtin names accepted by [`call`], in editor display order.
pub const BUILTIN_NAMES: &[&str] = &[
    "concat",
    "upper",
    "lower",
    "trim",
    "left_trim",
    "right_trim",
    "length",
    "starts_with",
    "contains",
    "sql_like",
    "pad_string_left",
    "pad_string_right",
    "add",
    "subtract",
    "multiply",
    "divide",
    "equal",
    "not_equal",
    "less_than",
    "greater_than",
    "less_or_equal",
    "greater_or_equal",
    "and",
    "or",
    "not",
    "substring",
    "substring_before",
    "substring_after",
    "string",
    "format_number",
    "exists",
    "round",
    "date_from_datetime",
    "time_from_datetime",
    "datetime_from_date_and_time",
    "datetime_from_parts",
    "datetime_add",
    "parse_date",
    "parse_datetime",
    "parse_time",
    "edifact_to_datetime",
    "substitute_missing",
    "get_folder",
    "remove_folder",
    "resolve_filepath",
    "is_xml_nil",
];

/// Whether `name` identifies a scalar builtin accepted by [`call`].
pub fn is_known(name: &str) -> bool {
    BUILTIN_NAMES.contains(&name)
}

/// Dispatches a built-in function call by name.
pub fn call(name: &str, args: &[Value]) -> Result<Value, FunctionError> {
    match name {
        "concat" => Ok(concat(args)),
        "upper" => unary_string(args, "upper", str::to_uppercase),
        "lower" => unary_string(args, "lower", str::to_lowercase),
        "trim" => unary_string(args, "trim", |s| s.trim().to_string()),
        "left_trim" => unary_string(args, "left_trim", |s| {
            s.trim_start_matches([' ', '\t', '\r', '\n']).to_string()
        }),
        "right_trim" => unary_string(args, "right_trim", |s| {
            s.trim_end_matches([' ', '\t', '\r', '\n']).to_string()
        }),
        "length" => length(args),
        "starts_with" => binary_string(args, "starts_with", |a, b| a.starts_with(b)),
        "contains" => binary_string(args, "contains", |a, b| a.contains(b)),
        "sql_like" => binary_string(args, "sql_like", sql_like),
        "pad_string_left" => pad_string(args, "pad_string_left", true),
        "pad_string_right" => pad_string(args, "pad_string_right", false),
        "add" => numeric(args, "add", i64::checked_add, |a, b| a + b),
        "subtract" => numeric(args, "subtract", i64::checked_sub, |a, b| a - b),
        "multiply" => numeric(args, "multiply", i64::checked_mul, |a, b| a * b),
        "divide" => divide(args),
        "equal" => comparison(args, "equal", |o| o == std::cmp::Ordering::Equal),
        "not_equal" => comparison(args, "not_equal", |o| o != std::cmp::Ordering::Equal),
        "less_than" => comparison(args, "less_than", |o| o == std::cmp::Ordering::Less),
        "greater_than" => comparison(args, "greater_than", |o| o == std::cmp::Ordering::Greater),
        "less_or_equal" => comparison(args, "less_or_equal", |o| o != std::cmp::Ordering::Greater),
        "greater_or_equal" => {
            comparison(args, "greater_or_equal", |o| o != std::cmp::Ordering::Less)
        }
        "and" => binary_bool(args, "and", |a, b| a && b),
        "or" => binary_bool(args, "or", |a, b| a || b),
        "not" => unary_bool(args, "not", |a| !a),
        "substring" => substring(args),
        "substring_before" => split_string(args, "substring_before", true),
        "substring_after" => split_string(args, "substring_after", false),
        "string" => string(args),
        "format_number" => format_number::format_number(args),
        "exists" => exists(args),
        "round" => round(args),
        "date_from_datetime" => date_from_datetime(args),
        "time_from_datetime" => datetime::time_from_datetime(args),
        "datetime_from_date_and_time" => datetime::datetime_from_date_and_time(args),
        "datetime_from_parts" => datetime::datetime_from_parts(args),
        "datetime_add" => datetime_add::datetime_add(args),
        "parse_date" => datetime::parse_date(args),
        "parse_datetime" => datetime::parse_datetime(args),
        "parse_time" => datetime::parse_time(args),
        "edifact_to_datetime" => datetime::edifact_to_datetime(args),
        "substitute_missing" => substitute_missing(args),
        "get_folder" => filepath::get_folder(args),
        "remove_folder" => filepath::remove_folder(args),
        "resolve_filepath" => filepath::resolve_filepath(args),
        "is_xml_nil" => is_xml_nil(args),
        other => Err(FunctionError::UnknownFunction(other.to_string())),
    }
}

fn is_xml_nil(args: &[Value]) -> Result<Value, FunctionError> {
    let [value] = args else {
        return Err(FunctionError::ArityMismatch {
            function: "is_xml_nil",
            expected: 1,
            got: args.len(),
        });
    };
    Ok(Value::Bool(value.is_xml_nil()))
}

/// Matches a complete string using SQL LIKE's `%` (zero or more characters)
/// and `_` (exactly one character) wildcards. ASCII literals use SQLite's
/// default case-insensitive LIKE behavior; non-ASCII literals compare exactly.
fn sql_like(value: &str, pattern: &str) -> bool {
    let value = value.chars().collect::<Vec<_>>();
    let mut previous = vec![false; value.len() + 1];
    previous[0] = true;
    for token in pattern.chars() {
        let mut current = vec![false; value.len() + 1];
        match token {
            '%' => {
                current[0] = previous[0];
                for index in 1..=value.len() {
                    current[index] = previous[index] || current[index - 1];
                }
            }
            '_' => {
                current[1..].copy_from_slice(&previous[..value.len()]);
            }
            literal => {
                for index in 1..=value.len() {
                    current[index] =
                        previous[index - 1] && value[index - 1].eq_ignore_ascii_case(&literal);
                }
            }
        }
        previous = current;
    }
    previous[value.len()]
}

fn concat(args: &[Value]) -> Value {
    let mut out = String::new();
    for arg in args {
        match arg {
            Value::Null | Value::XmlNil(_) => {}
            Value::Bool(b) => out.push_str(&b.to_string()),
            Value::Int(i) => out.push_str(&i.to_string()),
            Value::Float(f) => out.push_str(&f.to_string()),
            Value::String(s) => out.push_str(s),
        }
    }
    Value::String(out)
}

fn unary_string(
    args: &[Value],
    name: &'static str,
    f: impl Fn(&str) -> String,
) -> Result<Value, FunctionError> {
    match args {
        [Value::String(s)] => Ok(Value::String(f(s))),
        [other] => Err(FunctionError::TypeMismatch {
            function: name,
            got: other.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 1,
            got: args.len(),
        }),
    }
}

fn binary_string(
    args: &[Value],
    name: &'static str,
    f: impl Fn(&str, &str) -> bool,
) -> Result<Value, FunctionError> {
    match args {
        [Value::String(a), Value::String(b)] => Ok(Value::Bool(f(a, b))),
        [a, b] => {
            let bad = if matches!(a, Value::String(_)) { b } else { a };
            Err(FunctionError::TypeMismatch {
                function: name,
                got: bad.type_name(),
            })
        }
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 2,
            got: args.len(),
        }),
    }
}

fn binary_bool(
    args: &[Value],
    name: &'static str,
    f: impl Fn(bool, bool) -> bool,
) -> Result<Value, FunctionError> {
    match args {
        [Value::Bool(a), Value::Bool(b)] => Ok(Value::Bool(f(*a, *b))),
        [a, b] => {
            let bad = if matches!(a, Value::Bool(_)) { b } else { a };
            Err(FunctionError::TypeMismatch {
                function: name,
                got: bad.type_name(),
            })
        }
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 2,
            got: args.len(),
        }),
    }
}

fn unary_bool(
    args: &[Value],
    name: &'static str,
    f: impl Fn(bool) -> bool,
) -> Result<Value, FunctionError> {
    match args {
        [Value::Bool(a)] => Ok(Value::Bool(f(*a))),
        [other] => Err(FunctionError::TypeMismatch {
            function: name,
            got: other.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 1,
            got: args.len(),
        }),
    }
}

fn length(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [Value::String(s)] => Ok(Value::Int(s.chars().count() as i64)),
        [other] => Err(FunctionError::TypeMismatch {
            function: "length",
            got: other.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function: "length",
            expected: 1,
            got: args.len(),
        }),
    }
}

fn scalar_text(value: &Value) -> String {
    match value {
        Value::Null | Value::XmlNil(_) => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::String(value) => value.clone(),
    }
}

fn string(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [value] => Ok(Value::String(scalar_text(value))),
        _ => Err(FunctionError::ArityMismatch {
            function: "string",
            expected: 1,
            got: args.len(),
        }),
    }
}

fn pad_string(args: &[Value], name: &'static str, left: bool) -> Result<Value, FunctionError> {
    let [value, desired_length, padding] = args else {
        return Err(FunctionError::ArityMismatch {
            function: name,
            expected: 3,
            got: args.len(),
        });
    };
    let desired_length = match desired_length {
        Value::Int(length) => *length,
        Value::Float(length) if length.is_finite() => *length as i64,
        Value::Float(_) => {
            return Err(FunctionError::InvalidArgument {
                function: name,
                message: "requires a finite desired length",
            });
        }
        other => {
            return Err(FunctionError::TypeMismatch {
                function: name,
                got: other.type_name(),
            });
        }
    };
    if desired_length > MAX_GENERATED_PADDING_CHARS {
        return Err(FunctionError::InvalidArgument {
            function: name,
            message: "requested output exceeds 1000000 characters",
        });
    }
    let padding = scalar_text(padding);
    let mut padding = padding.chars();
    let Some(padding_char) = padding.next() else {
        return Err(FunctionError::InvalidArgument {
            function: name,
            message: "requires one padding character",
        });
    };
    if padding.next().is_some() {
        return Err(FunctionError::InvalidArgument {
            function: name,
            message: "requires one padding character",
        });
    }

    let value = scalar_text(value);
    let count = desired_length
        .saturating_sub(value.chars().count() as i64)
        .max(0) as usize;
    let padding: String = std::iter::repeat_n(padding_char, count).collect();
    Ok(Value::String(if left {
        padding + &value
    } else {
        value + &padding
    }))
}

fn numeric(
    args: &[Value],
    name: &'static str,
    f_int: impl Fn(i64, i64) -> Option<i64>,
    f_float: impl Fn(f64, f64) -> f64,
) -> Result<Value, FunctionError> {
    match args {
        [Value::Int(a), Value::Int(b)] => f_int(*a, *b)
            .map(Value::Int)
            .ok_or(FunctionError::IntegerOverflow { function: name }),
        [Value::Float(a), Value::Float(b)] => Ok(Value::Float(f_float(*a, *b))),
        [Value::Int(a), Value::Float(b)] => Ok(Value::Float(f_float(*a as f64, *b))),
        [Value::Float(a), Value::Int(b)] => Ok(Value::Float(f_float(*a, *b as f64))),
        [Value::Int(_) | Value::Float(_), b] => Err(FunctionError::TypeMismatch {
            function: name,
            got: b.type_name(),
        }),
        [a, _] => Err(FunctionError::TypeMismatch {
            function: name,
            got: a.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 2,
            got: args.len(),
        }),
    }
}

fn divide(args: &[Value]) -> Result<Value, FunctionError> {
    let (a, b) = match args {
        [Value::Int(a), Value::Int(b)] => (*a as f64, *b as f64),
        [Value::Float(a), Value::Float(b)] => (*a, *b),
        [Value::Int(a), Value::Float(b)] => (*a as f64, *b),
        [Value::Float(a), Value::Int(b)] => (*a, *b as f64),
        [Value::Int(_) | Value::Float(_), b] => {
            return Err(FunctionError::TypeMismatch {
                function: "divide",
                got: b.type_name(),
            });
        }
        [a, _] => {
            return Err(FunctionError::TypeMismatch {
                function: "divide",
                got: a.type_name(),
            });
        }
        _ => {
            return Err(FunctionError::ArityMismatch {
                function: "divide",
                expected: 2,
                got: args.len(),
            });
        }
    };
    if b == 0.0 {
        return Err(FunctionError::DivideByZero);
    }
    Ok(Value::Float(a / b))
}

fn number_arg(value: &Value, name: &'static str) -> Result<f64, FunctionError> {
    match value {
        Value::Int(i) => Ok(*i as f64),
        Value::Float(f) => Ok(*f),
        other => Err(FunctionError::TypeMismatch {
            function: name,
            got: other.type_name(),
        }),
    }
}

/// XPath-style `substring(string, start[, length])`: positions are 1-based
/// and both numbers round to the nearest integer.
fn substring(args: &[Value]) -> Result<Value, FunctionError> {
    let (s, start, len) = match args {
        [Value::String(s), start] => (s, start, None),
        [Value::String(s), start, len] => (s, start, Some(len)),
        [other, ..] if !matches!(other, Value::String(_)) => {
            return Err(FunctionError::TypeMismatch {
                function: "substring",
                got: other.type_name(),
            });
        }
        _ => {
            return Err(FunctionError::ArityMismatch {
                function: "substring",
                expected: 2,
                got: args.len(),
            });
        }
    };
    let start = number_arg(start, "substring")?.round() as i64;
    let end = match len {
        Some(len) => Some(start.saturating_add(number_arg(len, "substring")?.round() as i64)),
        None => None,
    };
    let out: String = s
        .chars()
        .enumerate()
        .filter(|(i, _)| {
            let pos = *i as i64 + 1;
            pos >= start && end.is_none_or(|e| pos < e)
        })
        .map(|(_, c)| c)
        .collect();
    Ok(Value::String(out))
}

/// `substring-before` / `substring-after`: the part of the string on one
/// side of the first occurrence of the separator; no match yields "".
fn split_string(args: &[Value], name: &'static str, before: bool) -> Result<Value, FunctionError> {
    match args {
        [Value::String(s), Value::String(sep)] => {
            let part = match s.find(sep.as_str()) {
                Some(idx) if before => &s[..idx],
                Some(idx) => &s[idx + sep.len()..],
                None => "",
            };
            Ok(Value::String(part.to_string()))
        }
        [a, b] => {
            let bad = if matches!(a, Value::String(_)) { b } else { a };
            Err(FunctionError::TypeMismatch {
                function: name,
                got: bad.type_name(),
            })
        }
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 2,
            got: args.len(),
        }),
    }
}

/// Whether a value is present -- with the lenient readers, an absent
/// source node arrives as `Null`.
fn exists(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [value] => Ok(Value::Bool(!matches!(value, Value::Null))),
        _ => Err(FunctionError::ArityMismatch {
            function: "exists",
            expected: 1,
            got: args.len(),
        }),
    }
}

/// `round(x)` rounds to the nearest integer; `round(x, digits)` to that
/// many decimal places (MapForce's `round-precision`).
fn round(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [Value::Int(i)] => Ok(Value::Int(*i)),
        [x] => Ok(Value::Float(number_arg(x, "round")?.round())),
        [x, digits] => {
            let x = number_arg(x, "round")?;
            let factor = 10f64.powi(number_arg(digits, "round")?.round() as i32);
            Ok(Value::Float((x * factor).round() / factor))
        }
        _ => Err(FunctionError::ArityMismatch {
            function: "round",
            expected: 1,
            got: args.len(),
        }),
    }
}

/// The date part of an ISO datetime string ("2024-03-01T10:30:00" ->
/// "2024-03-01"); values without a time part pass through unchanged.
fn date_from_datetime(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [Value::String(s)] => Ok(Value::String(
            s.split('T').next().unwrap_or(s).trim().to_string(),
        )),
        [other] => Err(FunctionError::TypeMismatch {
            function: "date_from_datetime",
            got: other.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function: "date_from_datetime",
            expected: 1,
            got: args.len(),
        }),
    }
}

fn substitute_missing(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [Value::Null, replacement] => Ok(replacement.clone()),
        [value, _] => Ok(value.clone()),
        _ => Err(FunctionError::ArityMismatch {
            function: "substitute_missing",
            expected: 2,
            got: args.len(),
        }),
    }
}

fn value_ordering(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
        (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        (Value::String(_), Value::Null | Value::XmlNil(_))
        | (Value::Null | Value::XmlNil(_), Value::String(_)) => None,
        (Value::String(a), b) => Some(a.as_str().cmp(scalar_text(b).as_str())),
        (a, Value::String(b)) => Some(scalar_text(a).as_str().cmp(b.as_str())),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
        _ => None,
    }
}

fn comparison(
    args: &[Value],
    name: &'static str,
    matches: impl Fn(std::cmp::Ordering) -> bool,
) -> Result<Value, FunctionError> {
    match args {
        [a, b] => match value_ordering(a, b) {
            Some(ordering) => Ok(Value::Bool(matches(ordering))),
            None => Err(FunctionError::TypeMismatch {
                function: name,
                got: b.type_name(),
            }),
        },
        _ => Err(FunctionError::ArityMismatch {
            function: name,
            expected: 2,
            got: args.len(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn unknown_function_is_reported() {
        assert_eq!(
            call("nope", &[]),
            Err(FunctionError::UnknownFunction("nope".to_string()))
        );
        assert!(!is_known("nope"));
        assert!(is_known("concat"));
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
    fn substitute_missing_only_replaces_absent_values() {
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
}
