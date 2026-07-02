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
}

/// Dispatches a built-in function call by name.
pub fn call(name: &str, args: &[Value]) -> Result<Value, FunctionError> {
    match name {
        "concat" => Ok(concat(args)),
        "upper" => unary_string(args, "upper", str::to_uppercase),
        "lower" => unary_string(args, "lower", str::to_lowercase),
        "trim" => unary_string(args, "trim", |s| s.trim().to_string()),
        "add" => add(args),
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

fn add(args: &[Value]) -> Result<Value, FunctionError> {
    match args {
        [Value::Int(a), Value::Int(b)] => Ok(Value::Int(a + b)),
        [Value::Float(a), Value::Float(b)] => Ok(Value::Float(a + b)),
        [Value::Int(a), Value::Float(b)] => Ok(Value::Float(*a as f64 + b)),
        [Value::Float(a), Value::Int(b)] => Ok(Value::Float(a + *b as f64)),
        [Value::Int(_) | Value::Float(_), b] => Err(FunctionError::TypeMismatch {
            function: "add",
            got: b.type_name(),
        }),
        [a, _] => Err(FunctionError::TypeMismatch {
            function: "add",
            got: a.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function: "add",
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
}
