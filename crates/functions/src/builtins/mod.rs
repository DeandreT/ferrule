use ir::Value;

use super::{
    FunctionError, datetime, datetime_add, decimal, filepath, flextext, format_number, json,
    scalar::text as scalar_text,
};

mod isbn;
mod regex_match;

const MAX_GENERATED_PADDING_CHARS: i64 = 1_000_000;

pub(super) fn call(name: &str, args: &[Value]) -> Result<Value, FunctionError> {
    match name {
        "concat" => Ok(concat(args)),
        "upper" => unary_string(args, "upper", str::to_uppercase),
        "lower" => unary_string(args, "lower", str::to_lowercase),
        "normalize_space" => unary_string(args, "normalize_space", normalize_space),
        "is_empty" => unary_string_predicate(args, "is_empty", str::is_empty),
        "trim" => unary_string(args, "trim", |s| s.trim().to_string()),
        "left" => edge_chars(args, "left", true),
        "right" => edge_chars(args, "right", false),
        "left_trim" => unary_string(args, "left_trim", |s| {
            s.trim_start_matches([' ', '\t', '\r', '\n']).to_string()
        }),
        "right_trim" => unary_string(args, "right_trim", |s| {
            s.trim_end_matches([' ', '\t', '\r', '\n']).to_string()
        }),
        "length" => length(args),
        "starts_with" => binary_scalar_string(args, "starts_with", |a, b| a.starts_with(b)),
        "ends_with" => binary_scalar_string(args, "ends_with", |a, b| a.ends_with(b)),
        "contains" => binary_scalar_string(args, "contains", |a, b| a.contains(b)),
        "matches" => regex_match::matches(args),
        "replace" => regex_match::replace(args),
        "sql_like" => binary_string(args, "sql_like", sql_like),
        "pad_string_left" => pad_string(args, "pad_string_left", true),
        "pad_string_right" => pad_string(args, "pad_string_right", false),
        "add" => numeric(args, "add", i64::checked_add, |a, b| a + b),
        "subtract" => numeric(args, "subtract", i64::checked_sub, |a, b| a - b),
        "multiply" => multiply(args),
        "sqlite_multiply" => sqlite_multiply(args),
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
        "weekday" => weekday(args),
        "hours_from_datetime" => hours_from_datetime(args),
        "minutes_from_datetime" => minutes_from_datetime(args),
        "time_from_datetime" => datetime::time_from_datetime(args),
        "datetime_from_date_and_time" => datetime::datetime_from_date_and_time(args),
        "datetime_from_parts" => datetime::datetime_from_parts(args),
        "duration_from_parts" => datetime::duration_from_parts(args),
        "datetime_add" => datetime_add::datetime_add(args),
        "parse_date" => datetime::parse_date(args),
        "parse_datetime" => datetime::parse_datetime(args),
        "parse_time" => datetime::parse_time(args),
        "edifact_to_datetime" => datetime::edifact_to_datetime(args),
        "coerce_datetime" => datetime::coerce_datetime(args),
        "substitute_missing" => substitute_missing(args),
        "substitute_missing_with_xml_nil" => substitute_missing_with_xml_nil(args),
        "get_folder" => filepath::get_folder(args),
        "remove_folder" => filepath::remove_folder(args),
        "get_fileext" => filepath::get_fileext(args),
        "resolve_filepath" => filepath::resolve_filepath(args),
        "is_xml_nil" => is_xml_nil(args),
        "isbn10_to_isbn13" => isbn::isbn10_to_isbn13(args),
        "json_serialize_object" => json::serialize_object(args),
        "json_parse_field" => json::parse_field(args),
        "flextext_parse_field" => flextext::parse_field(args),
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
            Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => {}
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
        [value] => Ok(Value::Bool(predicate(&scalar_text(value)))),
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

fn binary_scalar_string(
    args: &[Value],
    name: &'static str,
    f: impl Fn(&str, &str) -> bool,
) -> Result<Value, FunctionError> {
    match args {
        [a, b] => Ok(Value::Bool(f(&scalar_text(a), &scalar_text(b)))),
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
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) | Value::Bool(_) => false,
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
        Value::Null | Value::JsonNull(_) => Ok(Value::Null),
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

fn edge_chars(args: &[Value], name: &'static str, left: bool) -> Result<Value, FunctionError> {
    let [Value::String(value), count] = args else {
        return match args {
            [other, _] if !matches!(other, Value::String(_)) => Err(FunctionError::TypeMismatch {
                function: name,
                got: other.type_name(),
            }),
            [_, other] => Err(FunctionError::TypeMismatch {
                function: name,
                got: other.type_name(),
            }),
            _ => Err(FunctionError::ArityMismatch {
                function: name,
                expected: 2,
                got: args.len(),
            }),
        };
    };
    let count = match count {
        Value::Int(count) => *count,
        Value::Float(count) if count.is_finite() => *count as i64,
        Value::Float(_) => {
            return Err(FunctionError::InvalidArgument {
                function: name,
                message: "requires a finite character count",
            });
        }
        other => {
            return Err(FunctionError::TypeMismatch {
                function: name,
                got: other.type_name(),
            });
        }
    };
    if count <= 0 {
        return Ok(Value::String(String::new()));
    }

    let length = value.chars().count();
    let Ok(count) = usize::try_from(count) else {
        return Ok(Value::String(value.clone()));
    };
    if count >= length {
        return Ok(Value::String(value.clone()));
    }
    let result = if left {
        value.chars().take(count).collect()
    } else {
        value.chars().skip(length - count).collect()
    };
    Ok(Value::String(result))
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

fn sqlite_multiply(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "sqlite_multiply";
    let [left, right] = args else {
        return Err(FunctionError::ArityMismatch {
            function: FUNCTION,
            expected: 2,
            got: args.len(),
        });
    };
    if matches!(left, Value::Null | Value::JsonNull(_))
        || matches!(right, Value::Null | Value::JsonNull(_))
    {
        return Ok(Value::Null);
    }
    match (
        numeric_operand(left, FUNCTION)?,
        numeric_operand(right, FUNCTION)?,
    ) {
        (NumericOperand::Int(left), NumericOperand::Int(right)) => Ok(left
            .checked_mul(right)
            .map_or_else(|| Value::Float(left as f64 * right as f64), Value::Int)),
        (left, right) => Ok(Value::Float(left.as_f64() * right.as_f64())),
    }
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
        [value] => Ok(Value::Bool(!matches!(
            value,
            Value::Null | Value::JsonNull(_)
        ))),
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
        [Value::Null | Value::JsonNull(_)] => return Ok(None),
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

/// The ISO weekday of the local date, where Monday is 1 and Sunday is 7.
fn weekday(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "weekday";
    let Some(value) = nullable_string_arg(args, FUNCTION)? else {
        return Ok(Value::Null);
    };
    let date = validated_local_date(value, FUNCTION)?;
    let (month, day) = date.normalized_month_day();
    let mut year = year_mod_400(date.year);
    if date.rolls_year() {
        year = (year + 1) % 400;
    }
    if month < 3 {
        year = (year + 399) % 400;
    }
    const MONTH_OFFSETS: [u32; 12] = [0, 3, 2, 5, 0, 3, 5, 1, 4, 6, 2, 4];
    let sunday_based =
        (year + year / 4 - year / 100 + year / 400 + MONTH_OFFSETS[month as usize - 1] + day) % 7;
    Ok(Value::Int(i64::from(if sunday_based == 0 {
        7
    } else {
        sunday_based
    })))
}

fn year_mod_400(year: &str) -> u32 {
    let (negative, digits) = year
        .strip_prefix('-')
        .map_or((false, year), |digits| (true, digits));
    let magnitude = digits.bytes().fold(0, |value, digit| {
        (value * 10 + u32::from(digit - b'0')) % 400
    });
    if negative {
        (401 - magnitude) % 400
    } else {
        magnitude
    }
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
        [
            Value::Null | Value::JsonNull(_) | Value::XmlNil(_),
            replacement,
        ] => Ok(replacement.clone()),
        [value, _] => Ok(value.clone()),
        _ => Err(FunctionError::ArityMismatch {
            function: "substitute_missing",
            expected: 2,
            got: args.len(),
        }),
    }
}

fn substitute_missing_with_xml_nil(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [Value::Null | Value::JsonNull(_)] => Ok(Value::xml_nil()),
        [value] => Ok(value.clone()),
        _ => Err(FunctionError::ArityMismatch {
            function: "substitute_missing_with_xml_nil",
            expected: 1,
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
        (Value::String(_), Value::Null | Value::JsonNull(_) | Value::XmlNil(_))
        | (Value::Null | Value::JsonNull(_) | Value::XmlNil(_), Value::String(_)) => None,
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
        [Value::Null | Value::JsonNull(_) | Value::XmlNil(_), _]
        | [_, Value::Null | Value::JsonNull(_) | Value::XmlNil(_)] => Ok(Value::Bool(false)),
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
mod tests;
