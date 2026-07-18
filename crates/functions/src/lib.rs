//! Built-in function library (string, math, date, aggregate, node-set) used
//! by mapping graphs, plus hooks for user-defined functions.
//!
//! Covers the string/math/comparison/boolean core plus the scalar helpers
//! MapForce designs lean on (substring family, exists, round, ISO
//! date/time component extraction); more built-ins land alongside the formats/semantics
//! that need them. Aggregates (count/sum/...) are not here: they reduce
//! collections in scope context, so they live in the engine as
//! `mapping::Node::Aggregate`.

use ir::Value;
use thiserror::Error;

mod datetime;
mod datetime_add;
mod decimal;
mod filepath;
mod flextext;
mod format_number;
mod json;

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
    "normalize_space",
    "is_empty",
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
    "is_numeric",
    "to_number",
    "format_number",
    "exists",
    "round",
    "date_from_datetime",
    "year_from_datetime",
    "month_from_datetime",
    "day_from_datetime",
    "hours_from_datetime",
    "minutes_from_datetime",
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
    "isbn10_to_isbn13",
];

const INTERNAL_NAMES: &[&str] = &[
    "json_serialize_object",
    "json_parse_field",
    "flextext_parse_field",
    "delay_passthrough",
    "coerce_datetime",
];

/// Whether `name` identifies a scalar builtin accepted by [`call`].
pub fn is_known(name: &str) -> bool {
    BUILTIN_NAMES.contains(&name) || INTERNAL_NAMES.contains(&name)
}

/// Dispatches a built-in function call by name.
pub fn call(name: &str, args: &[Value]) -> Result<Value, FunctionError> {
    match name {
        "concat" => Ok(concat(args)),
        "upper" => unary_string(args, "upper", str::to_uppercase),
        "lower" => unary_string(args, "lower", str::to_lowercase),
        "normalize_space" => unary_string(args, "normalize_space", normalize_space),
        "is_empty" => unary_string_predicate(args, "is_empty", str::is_empty),
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
        "multiply" => multiply(args),
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
        "is_numeric" => is_numeric(args),
        "to_number" => to_number(args),
        "format_number" => format_number::format_number(args),
        "exists" => exists(args),
        "round" => round(args),
        "delay_passthrough" => delay_passthrough(args),
        "date_from_datetime" => date_from_datetime(args),
        "year_from_datetime" => year_from_datetime(args),
        "month_from_datetime" => month_from_datetime(args),
        "day_from_datetime" => day_from_datetime(args),
        "hours_from_datetime" => hours_from_datetime(args),
        "minutes_from_datetime" => minutes_from_datetime(args),
        "time_from_datetime" => datetime::time_from_datetime(args),
        "datetime_from_date_and_time" => datetime::datetime_from_date_and_time(args),
        "datetime_from_parts" => datetime::datetime_from_parts(args),
        "datetime_add" => datetime_add::datetime_add(args),
        "parse_date" => datetime::parse_date(args),
        "parse_datetime" => datetime::parse_datetime(args),
        "parse_time" => datetime::parse_time(args),
        "edifact_to_datetime" => datetime::edifact_to_datetime(args),
        "coerce_datetime" => datetime::coerce_datetime(args),
        "substitute_missing" => substitute_missing(args),
        "get_folder" => filepath::get_folder(args),
        "remove_folder" => filepath::remove_folder(args),
        "resolve_filepath" => filepath::resolve_filepath(args),
        "is_xml_nil" => is_xml_nil(args),
        "isbn10_to_isbn13" => isbn10_to_isbn13(args),
        "json_serialize_object" => json::serialize_object(args),
        "json_parse_field" => json::parse_field(args),
        "flextext_parse_field" => flextext::parse_field(args),
        other => Err(FunctionError::UnknownFunction(other.to_string())),
    }
}

/// Converts a validated ISBN-10 into its equivalent Bookland ISBN-13/EAN-13.
/// ASCII spaces and hyphens are accepted as presentation separators.
fn isbn10_to_isbn13(args: &[Value]) -> Result<Value, FunctionError> {
    let [Value::String(input)] = args else {
        return match args {
            [_] => Err(FunctionError::TypeMismatch {
                function: "isbn10_to_isbn13",
                got: args[0].type_name(),
            }),
            _ => Err(FunctionError::ArityMismatch {
                function: "isbn10_to_isbn13",
                expected: 1,
                got: args.len(),
            }),
        };
    };
    let normalized = input
        .bytes()
        .filter(|byte| !matches!(byte, b' ' | b'-'))
        .collect::<Vec<_>>();
    if normalized.len() != 10
        || !normalized[..9].iter().all(u8::is_ascii_digit)
        || !(normalized[9].is_ascii_digit() || normalized[9].eq_ignore_ascii_case(&b'X'))
    {
        return Err(FunctionError::InvalidArgument {
            function: "isbn10_to_isbn13",
            message: "expected a 10-character ISBN with an optional final X check digit",
        });
    }
    let isbn10_sum = normalized[..9]
        .iter()
        .enumerate()
        .map(|(index, byte)| u32::from(byte - b'0') * (10 - index as u32))
        .sum::<u32>()
        + if normalized[9].eq_ignore_ascii_case(&b'X') {
            10
        } else {
            u32::from(normalized[9] - b'0')
        };
    if isbn10_sum % 11 != 0 {
        return Err(FunctionError::InvalidArgument {
            function: "isbn10_to_isbn13",
            message: "ISBN-10 check digit is invalid",
        });
    }

    let mut output = Vec::with_capacity(13);
    output.extend_from_slice(b"978");
    output.extend_from_slice(&normalized[..9]);
    let weighted = output
        .iter()
        .enumerate()
        .map(|(index, byte)| u32::from(byte - b'0') * if index % 2 == 0 { 1 } else { 3 })
        .sum::<u32>();
    output.push(b'0' + ((10 - weighted % 10) % 10) as u8);
    let converted = String::from_utf8(output).map_err(|_| FunctionError::InvalidArgument {
        function: "isbn10_to_isbn13",
        message: "converted ISBN was not valid UTF-8",
    })?;
    Ok(Value::String(converted))
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

fn unary_string_predicate(
    args: &[Value],
    name: &'static str,
    predicate: impl Fn(&str) -> bool,
) -> Result<Value, FunctionError> {
    match args {
        [Value::String(value)] => Ok(Value::Bool(predicate(value))),
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

fn normalize_space(value: &str) -> String {
    value
        .split([' ', '\t', '\r', '\n'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
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
        [value] => Ok(Value::Int(scalar_text(value).chars().count() as i64)),
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

fn is_numeric(args: &[Value]) -> Result<Value, FunctionError> {
    let [value] = args else {
        return Err(FunctionError::ArityMismatch {
            function: "is_numeric",
            expected: 1,
            got: args.len(),
        });
    };
    let numeric = match value {
        Value::Int(_) => true,
        Value::Float(value) => value.is_finite(),
        Value::String(value) => value
            .trim()
            .parse::<f64>()
            .is_ok_and(|value| value.is_finite()),
        Value::Null | Value::XmlNil(_) | Value::Bool(_) => false,
    };
    Ok(Value::Bool(numeric))
}

fn to_number(args: &[Value]) -> Result<Value, FunctionError> {
    let [value] = args else {
        return Err(FunctionError::ArityMismatch {
            function: "to_number",
            expected: 1,
            got: args.len(),
        });
    };
    match value {
        Value::Null => Ok(Value::Null),
        Value::Int(value) => Ok(Value::Int(*value)),
        Value::Float(value) if value.is_finite() => Ok(Value::Float(*value)),
        Value::String(value) => {
            let value = value.trim();
            if let Ok(value) = value.parse::<i64>() {
                return Ok(Value::Int(value));
            }
            value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(Value::Float)
                .ok_or(FunctionError::InvalidArgument {
                    function: "to_number",
                    message: "requires a finite numeric value",
                })
        }
        Value::Float(_) | Value::Bool(_) | Value::XmlNil(_) => {
            Err(FunctionError::InvalidArgument {
                function: "to_number",
                message: "requires a finite numeric value",
            })
        }
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
    let Some((first, rest)) = args.split_first().filter(|(_, rest)| !rest.is_empty()) else {
        return Err(FunctionError::ArityMismatch {
            function: name,
            expected: 2,
            got: args.len(),
        });
    };
    let mut operands = Vec::with_capacity(args.len());
    operands.push(numeric_operand(first, name)?);
    for value in rest {
        operands.push(numeric_operand(value, name)?);
    }

    if operands
        .iter()
        .any(|operand| matches!(operand, NumericOperand::Float(_)))
    {
        let mut operands = operands.into_iter();
        let mut result = operands.next().map_or(0.0, NumericOperand::as_f64);
        for operand in operands {
            result = f_float(result, operand.as_f64());
        }
        return Ok(Value::Float(result));
    }

    let mut operands = operands.into_iter();
    let mut result = match operands.next() {
        Some(NumericOperand::Int(value)) => value,
        Some(NumericOperand::Float(_)) | None => unreachable!("numeric operands were prevalidated"),
    };
    for operand in operands {
        let NumericOperand::Int(value) = operand else {
            unreachable!("floating operands were handled before integer folding");
        };
        result = f_int(result, value).ok_or(FunctionError::IntegerOverflow { function: name })?;
    }
    Ok(Value::Int(result))
}

fn multiply(args: &[Value]) -> Result<Value, FunctionError> {
    let binary = numeric(args, "multiply", i64::checked_mul, |a, b| a * b)?;
    let Value::Float(binary) = binary else {
        return Ok(binary);
    };
    Ok(Value::Float(decimal::product(args).unwrap_or(binary)))
}

#[cfg(test)]
fn assert_numeric_call(name: &str, args: &[Value], expected: Value) {
    assert_eq!(call(name, args), Ok(expected));
}

#[cfg(test)]
mod growable_arithmetic_tests {
    use super::*;

    #[test]
    fn folds_growable_arithmetic_inputs_from_left_to_right() {
        assert_numeric_call(
            "add",
            &[Value::Int(20), Value::Int(10), Value::Int(12)],
            Value::Int(42),
        );
        assert_numeric_call(
            "subtract",
            &[Value::Int(50), Value::Int(5), Value::Int(3)],
            Value::Int(42),
        );
        assert_numeric_call(
            "multiply",
            &[
                Value::String("2.5".into()),
                Value::Int(4),
                Value::Float(2.0),
            ],
            Value::Float(20.0),
        );
        assert_numeric_call(
            "add",
            &[Value::Int(i64::MAX), Value::Int(1), Value::Float(0.5)],
            Value::Float(i64::MAX as f64 + 1.5),
        );
        assert_eq!(
            call("add", &[Value::Int(i64::MAX), Value::Int(1)]),
            Err(FunctionError::IntegerOverflow { function: "add" })
        );
    }
}

fn divide(args: &[Value]) -> Result<Value, FunctionError> {
    let (a, b) = match args {
        [a, b] => (
            numeric_operand(a, "divide")?.as_f64(),
            numeric_operand(b, "divide")?.as_f64(),
        ),
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

#[derive(Clone, Copy)]
enum NumericOperand {
    Int(i64),
    Float(f64),
}

impl NumericOperand {
    fn as_f64(self) -> f64 {
        match self {
            Self::Int(value) => value as f64,
            Self::Float(value) => value,
        }
    }
}

fn numeric_operand(value: &Value, function: &'static str) -> Result<NumericOperand, FunctionError> {
    match value {
        Value::Int(value) => Ok(NumericOperand::Int(*value)),
        Value::Float(value) => Ok(NumericOperand::Float(*value)),
        Value::String(value) => {
            let value = value.trim();
            if let Ok(value) = value.parse::<i64>() {
                return Ok(NumericOperand::Int(value));
            }
            value
                .parse::<f64>()
                .ok()
                .filter(|value| value.is_finite())
                .map(NumericOperand::Float)
                .ok_or(FunctionError::TypeMismatch {
                    function,
                    got: "string",
                })
        }
        other => Err(FunctionError::TypeMismatch {
            function,
            got: other.type_name(),
        }),
    }
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

/// Retains MapForce's value dependency across a `sleep(value, seconds)`
/// component. External calls remain captured-response boundaries, so the
/// pure engine validates the delay but does not pause wall-clock execution.
fn delay_passthrough(args: &[Value]) -> Result<Value, FunctionError> {
    let [value, duration] = args else {
        return Err(FunctionError::ArityMismatch {
            function: "delay_passthrough",
            expected: 2,
            got: args.len(),
        });
    };
    let duration = number_arg(duration, "delay_passthrough")?;
    if !duration.is_finite() || duration < 0.0 {
        return Err(FunctionError::InvalidArgument {
            function: "delay_passthrough",
            message: "requires a finite nonnegative duration",
        });
    }
    Ok(value.clone())
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

fn nullable_string_arg<'a>(
    args: &'a [Value],
    function: &'static str,
) -> Result<Option<&'a str>, FunctionError> {
    let value = match args {
        [Value::Null] => return Ok(None),
        [Value::String(value)] => value,
        [other] => {
            return Err(FunctionError::TypeMismatch {
                function,
                got: other.type_name(),
            });
        }
        _ => {
            return Err(FunctionError::ArityMismatch {
                function,
                expected: 1,
                got: args.len(),
            });
        }
    };
    Ok(Some(value))
}

/// The local year component of an ISO date or dateTime, without timezone adjustment.
fn year_from_datetime(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "year_from_datetime";
    let Some(value) = nullable_string_arg(args, FUNCTION)? else {
        return Ok(Value::Null);
    };
    let date = validated_local_date(value, FUNCTION)?;
    let mut year = date
        .year
        .parse::<i64>()
        .map_err(|_| FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "requires a year within the signed 64-bit integer range",
        })?;
    if date.rolls_year() {
        year = if year == -1 {
            1
        } else {
            year.checked_add(1).ok_or(FunctionError::InvalidArgument {
                function: FUNCTION,
                message: "requires a year within the signed 64-bit integer range",
            })?
        };
    }
    Ok(Value::Int(year))
}

/// The local month component of an ISO date or dateTime, without timezone adjustment.
fn month_from_datetime(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "month_from_datetime";
    let Some(value) = nullable_string_arg(args, FUNCTION)? else {
        return Ok(Value::Null);
    };
    let date = validated_local_date(value, FUNCTION)?;
    let (month, _) = date.normalized_month_day();
    Ok(Value::Int(i64::from(month)))
}

/// The local day component of an ISO date or dateTime, without timezone adjustment.
fn day_from_datetime(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "day_from_datetime";
    let Some(value) = nullable_string_arg(args, FUNCTION)? else {
        return Ok(Value::Null);
    };
    let date = validated_local_date(value, FUNCTION)?;
    let (_, day) = date.normalized_month_day();
    Ok(Value::Int(i64::from(day)))
}

/// The local hour component of an ISO dateTime, without timezone adjustment.
fn hours_from_datetime(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "hours_from_datetime";
    let Some(value) = nullable_string_arg(args, FUNCTION)? else {
        return Ok(Value::Null);
    };
    let time = validated_local_datetime_time(value, FUNCTION)?;
    if time.end_of_day {
        return Ok(Value::Int(0));
    }
    let hour = time.value[..2]
        .parse::<i64>()
        .map_err(|_| FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "requires a valid ISO dateTime",
        })?;
    Ok(Value::Int(hour))
}

/// The local minute component of an ISO dateTime, without timezone adjustment.
fn minutes_from_datetime(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "minutes_from_datetime";
    let Some(value) = nullable_string_arg(args, FUNCTION)? else {
        return Ok(Value::Null);
    };
    let time = validated_local_datetime_time(value, FUNCTION)?;
    if time.end_of_day {
        return Ok(Value::Int(0));
    }
    let minute = time.value[3..5]
        .parse::<i64>()
        .map_err(|_| FunctionError::InvalidArgument {
            function: FUNCTION,
            message: "requires a valid ISO dateTime",
        })?;
    Ok(Value::Int(minute))
}

struct LocalDate<'a> {
    year: &'a str,
    month: u32,
    day: u32,
    end_of_day: bool,
}

impl LocalDate<'_> {
    fn rolls_year(&self) -> bool {
        self.end_of_day && self.month == 12 && self.day == 31
    }

    fn normalized_month_day(&self) -> (u32, u32) {
        if !self.end_of_day {
            return (self.month, self.day);
        }
        let last_day = iso_days_in_month(self.year, self.month);
        if self.day < last_day {
            (self.month, self.day + 1)
        } else if self.month < 12 {
            (self.month + 1, 1)
        } else {
            (1, 1)
        }
    }
}

fn validated_local_date<'a>(
    value: &'a str,
    function: &'static str,
) -> Result<LocalDate<'a>, FunctionError> {
    if !value.is_ascii() {
        return Err(FunctionError::InvalidArgument {
            function,
            message: "requires a valid ISO date or dateTime",
        });
    }
    let (date, end_of_day) = if let Some((date, time)) = value.split_once('T') {
        let time = validated_local_time(time, function)?;
        (date, time.end_of_day)
    } else {
        let (date, _) = datetime::split_iso_timezone(value, function)?;
        (date, false)
    };
    datetime::validate_iso_date(date, function)?;
    let year_end = date
        .len()
        .checked_sub(6)
        .ok_or(FunctionError::InvalidArgument {
            function,
            message: "requires a valid ISO date or dateTime",
        })?;
    let month = date[year_end + 1..date.len() - 3]
        .parse::<u32>()
        .map_err(|_| FunctionError::InvalidArgument {
            function,
            message: "requires a valid ISO date or dateTime",
        })?;
    let day =
        date[date.len() - 2..]
            .parse::<u32>()
            .map_err(|_| FunctionError::InvalidArgument {
                function,
                message: "requires a valid ISO date or dateTime",
            })?;
    Ok(LocalDate {
        year: &date[..year_end],
        month,
        day,
        end_of_day,
    })
}

struct LocalTime<'a> {
    value: &'a str,
    end_of_day: bool,
}

fn validated_local_datetime_time<'a>(
    value: &'a str,
    function: &'static str,
) -> Result<LocalTime<'a>, FunctionError> {
    let (date, time) = value
        .split_once('T')
        .ok_or(FunctionError::InvalidArgument {
            function,
            message: "requires a valid ISO dateTime",
        })?;
    datetime::validate_iso_date(date, function)?;
    validated_local_time(time, function)
}

fn validated_local_time<'a>(
    value: &'a str,
    function: &'static str,
) -> Result<LocalTime<'a>, FunctionError> {
    if !value.is_ascii() {
        return Err(FunctionError::InvalidArgument {
            function,
            message: "requires a valid ISO dateTime",
        });
    }
    let (time, _) = datetime::split_iso_timezone(value, function)?;
    let end_of_day = time.starts_with("24:");
    if end_of_day {
        let normalized = format!("00{}", &time[2..]);
        datetime::validate_iso_time(&normalized, function)?;
        let (_, minute_second) = time.split_once(':').ok_or(FunctionError::InvalidArgument {
            function,
            message: "requires a valid ISO dateTime",
        })?;
        let (minute, second) =
            minute_second
                .split_once(':')
                .ok_or(FunctionError::InvalidArgument {
                    function,
                    message: "requires a valid ISO dateTime",
                })?;
        let (second, fraction) = second
            .split_once('.')
            .map_or((second, ""), |(second, fraction)| (second, fraction));
        if minute != "00" || second != "00" || fraction.bytes().any(|digit| digit != b'0') {
            return Err(FunctionError::InvalidArgument {
                function,
                message: "requires a valid ISO dateTime",
            });
        }
    } else {
        datetime::validate_iso_time(value, function)?;
    }
    Ok(LocalTime {
        value: time,
        end_of_day,
    })
}

fn iso_days_in_month(year: &str, month: u32) -> u32 {
    let digits = year.strip_prefix('-').unwrap_or(year);
    let decimal_mod = |modulus| {
        digits.bytes().fold(0, |value, digit| {
            (value * 10 + u32::from(digit - b'0')) % modulus
        })
    };
    let leap = decimal_mod(400) == 0 || decimal_mod(4) == 0 && decimal_mod(100) != 0;
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    }
}

fn substitute_missing(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [Value::Null | Value::XmlNil(_), replacement] => Ok(replacement.clone()),
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
        [Value::Null | Value::XmlNil(_), _] | [_, Value::Null | Value::XmlNil(_)] => {
            Ok(Value::Bool(false))
        }
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
}
