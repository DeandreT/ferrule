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
}

/// Dispatches a built-in function call by name.
pub fn call(name: &str, args: &[Value]) -> Result<Value, FunctionError> {
    match name {
        "concat" => Ok(concat(args)),
        "upper" => unary_string(args, "upper", str::to_uppercase),
        "lower" => unary_string(args, "lower", str::to_lowercase),
        "trim" => unary_string(args, "trim", |s| s.trim().to_string()),
        "length" => length(args),
        "starts_with" => binary_string(args, "starts_with", |a, b| a.starts_with(b)),
        "contains" => binary_string(args, "contains", |a, b| a.contains(b)),
        "add" => numeric(args, "add", |a, b| a + b, |a, b| a + b),
        "subtract" => numeric(args, "subtract", |a, b| a - b, |a, b| a - b),
        "multiply" => numeric(args, "multiply", |a, b| a * b, |a, b| a * b),
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
        "exists" => exists(args),
        "round" => round(args),
        "date_from_datetime" => date_from_datetime(args),
        other => Err(FunctionError::UnknownFunction(other.to_string())),
    }
}

fn concat(args: &[Value]) -> Value {
    let mut out = String::new();
    for arg in args {
        match arg {
            Value::Null => {}
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

fn numeric(
    args: &[Value],
    name: &'static str,
    f_int: impl Fn(i64, i64) -> i64,
    f_float: impl Fn(f64, f64) -> f64,
) -> Result<Value, FunctionError> {
    match args {
        [Value::Int(a), Value::Int(b)] => Ok(Value::Int(f_int(*a, *b))),
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
        Some(len) => Some(start + number_arg(len, "substring")?.round() as i64),
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

fn value_ordering(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
        (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
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
    }

    #[test]
    fn comparisons_coerce_int_and_float() {
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
