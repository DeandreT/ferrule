//! Built-in function library (string, math, date, aggregate, node-set) used
//! by mapping graphs, plus hooks for user-defined functions.
//!
//! This first cut only has the string/math functions needed to demonstrate
//! a real transformation end to end; more built-ins land alongside the
//! formats/semantics that need them.

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
