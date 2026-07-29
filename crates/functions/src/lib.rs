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

mod builtins;
mod catalog;
mod datetime;
mod datetime_add;
mod decimal;
mod filepath;
mod flextext;
mod format_number;
mod json;
mod scalar;

pub use catalog::{
    BuiltinArity, BuiltinCategory, BuiltinDefinition, BuiltinExposure, BuiltinNames,
    BuiltinParameter, ScalarDomain, catalog as builtin_catalog, find as builtin,
};

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

/// Authoring builtin names in stable editor display order.
pub const BUILTIN_NAMES: BuiltinNames = BuiltinNames;

/// Whether `name` identifies a scalar builtin accepted by [`call`].
pub fn is_known(name: &str) -> bool {
    builtin(name).is_some()
}

/// Dispatches a built-in function call by name.
pub fn call(name: &str, args: &[Value]) -> Result<Value, FunctionError> {
    let Some(definition) = builtin(name) else {
        return Err(FunctionError::UnknownFunction(name.to_string()));
    };
    if !definition.accepts_arity(args.len()) {
        return Err(FunctionError::ArityMismatch {
            function: definition.native_name,
            expected: definition.arity.minimum(),
            got: args.len(),
        });
    }
    builtins::call_builtin(definition, args)
}
