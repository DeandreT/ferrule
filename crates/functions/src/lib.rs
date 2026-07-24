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
mod datetime;
mod datetime_add;
mod decimal;
mod filepath;
mod flextext;
mod format_number;
mod json;
mod scalar;

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
    "left",
    "right",
    "left_trim",
    "right_trim",
    "length",
    "starts_with",
    "ends_with",
    "contains",
    "matches",
    "replace",
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
    "boolean",
    "positive",
    "floor",
    "format_number",
    "exists",
    "round",
    "date_from_datetime",
    "year_from_datetime",
    "month_from_datetime",
    "day_from_datetime",
    "weekday",
    "hours_from_datetime",
    "minutes_from_datetime",
    "time_from_datetime",
    "datetime_from_date_and_time",
    "datetime_from_parts",
    "duration_from_parts",
    "datetime_add",
    "parse_date",
    "parse_datetime",
    "parse_time",
    "format_date",
    "format_datetime",
    "format_time",
    "edifact_to_datetime",
    "substitute_missing",
    "substitute_missing_with_xml_nil",
    "get_folder",
    "remove_folder",
    "get_fileext",
    "resolve_filepath",
    "is_xml_nil",
    "isbn10_to_isbn13",
];

const INTERNAL_NAMES: &[&str] = &[
    "sqlite_multiply",
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
    builtins::call(name, args)
}
