use ir::{ScalarType, Value};
use mapping::AggregateOp;

use crate::canonical_function;

pub(super) fn aggregate_component_name(op: AggregateOp) -> &'static str {
    match op {
        AggregateOp::Count => "count",
        AggregateOp::Sum => "sum",
        AggregateOp::DecimalSum => "decimal-sum",
        AggregateOp::LastPresentText => "last-present-text",
        AggregateOp::LastDistinctText => "last-distinct-text",
        AggregateOp::Avg => "avg",
        AggregateOp::Min => "min",
        AggregateOp::Max => "max",
        AggregateOp::Join => "string-join",
        AggregateOp::ItemAt => "item-at",
    }
}

pub(super) fn constant_parts(value: &Value) -> (String, &'static str) {
    match value {
        Value::Null => (String::new(), "string"),
        Value::JsonNull(_) => (String::new(), "string"),
        Value::Bool(value) => (value.to_string(), "boolean"),
        Value::Int(value) => (value.to_string(), "integer"),
        Value::Float(value) => (value.to_string(), "decimal"),
        Value::Decimal(value) => (value.to_string(), "decimal"),
        Value::String(value) => (value.clone(), "string"),
        Value::XmlNil(_) => (String::new(), "string"),
    }
}

pub(super) fn value_text(value: &Value) -> String {
    constant_parts(value).0
}

pub(super) fn scalar_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "decimal",
        ScalarType::Bool => "boolean",
    }
}

pub(super) fn value_scalar_type(value: &Value) -> Option<ScalarType> {
    match value {
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) | Value::Decimal(_) => None,
        Value::Bool(_) => Some(ScalarType::Bool),
        Value::Int(_) => Some(ScalarType::Int),
        Value::Float(_) => Some(ScalarType::Float),
        Value::String(_) => Some(ScalarType::String),
    }
}

pub(super) fn unmap_function_name(name: &str) -> String {
    match name {
        "not_equal" => "not-equal",
        "greater_than" => "greater",
        "less_than" => "less",
        "greater_or_equal" => "equal-or-greater",
        "less_or_equal" => "equal-or-less",
        "and" => "logical-and",
        "or" => "logical-or",
        "not" => "logical-not",
        "length" => "string-length",
        "starts_with" => "starts-with",
        "ends_with" => "ends-with",
        "upper" => "uppercase",
        "lower" => "lowercase",
        "left_trim" => "left-trim",
        "right_trim" => "right-trim",
        "pad_string_left" => "pad-string-left",
        "pad_string_right" => "pad-string-right",
        "format_guid_string" => "format-guid-string",
        "substring_before" => "substring-before",
        "substring_after" => "substring-after",
        "normalize_space" => "normalize-space",
        "is_numeric" => "numeric",
        "is_empty" => "empty",
        "get_folder" => "get-folder",
        "remove_folder" => "remove-folder",
        "resolve_filepath" => "resolve-filepath",
        "is_xml_nil" => "is-xsi-nil",
        "substitute_missing_with_xml_nil" => "substitute-missing-with-xsi-nil",
        "date_from_datetime" => "date-from-datetime",
        "year_from_datetime" => "year-from-datetime",
        "month_from_datetime" => "month-from-datetime",
        "day_from_datetime" => "day-from-datetime",
        "weekday" => "weekday",
        "hours_from_datetime" => "hour-from-datetime",
        "minutes_from_datetime" => "minute-from-datetime",
        "time_from_datetime" => "time-from-datetime",
        "datetime_from_date_and_time" => "datetime-from-date-and-time",
        "datetime_from_parts" => "datetime-from-parts",
        "duration_from_parts" => "duration-from-parts",
        "datetime_add" => "datetime-add",
        "parse_date" => "parse-date",
        "parse_datetime" => "parse-dateTime",
        "parse_time" => "parse-time",
        "edifact_to_datetime" => "to-datetime",
        "substitute_missing" => "substitute-missing",
        "get_fileext" => "get-fileext",
        "delay_passthrough" => "sleep",
        "create_guid" => "create-guid",
        "format_number" => "format-number",
        "format_date" => "format-date",
        "format_datetime" => "format-dateTime",
        "format_time" => "format-time",
        other => other,
    }
    .to_string()
}

/// Native component name and library for a canonical function called with
/// `arity` arguments, or why the built-in execution language has none.
///
/// The reasons come from native runs of self-authored designs and the
/// vendor's per-function language listings (see `docs/mfd-interop.md`).
pub(super) fn native_function(
    function: &str,
    arity: usize,
) -> Result<(String, &'static str), &'static str> {
    let refusal = match function {
        "positive" => Some(
            "the built-in `positive` tests whether a number is greater than zero, \
             while this `positive` returns the number unchanged",
        ),
        "trim" => Some(
            "the built-in trim functions remove only spaces, tabs, carriage returns \
             and line feeds, while `trim` removes all Unicode whitespace",
        ),
        "ends_with" => Some("`ends-with` exists only for XSLT 2.0, XSLT 3.0 and XQuery"),
        "matches" => Some(
            "`matches` exists only for XSLT 2.0, XSLT 3.0 and XQuery, and the built-in \
             `match-pattern` must match the whole text",
        ),
        "replace" => Some(
            "regular-expression `replace` exists only for XSLT 2.0, XSLT 3.0 and XQuery; \
             the built-in `replace` replaces literal text",
        ),
        "format_date" | "format_datetime" if arity > 3 => Some(
            "the built-in function takes only value, format and language inputs, \
             so calendar and place arguments have no equivalent",
        ),
        "format_time" if arity > 2 => {
            Some("the built-in `format-time` takes only value and format inputs")
        }
        _ => None,
    };
    if let Some(reason) = refusal {
        return Err(reason);
    }
    // The built-in `round` has one input; rounding to decimals is
    // `round-precision`, which import reads back as two-argument `round`.
    if function == "round" && arity == 2 {
        return Ok(("round-precision".to_string(), "core"));
    }
    Ok((unmap_function_name(function), function_library(function)))
}

/// Library of the native component for a canonical function.
///
/// Exported designs always select the built-in execution language, so each
/// name and library pair must be one that language offers. The xpath2 library
/// (`upper-case`, `lower-case`) exists only for XSLT 2.0, XSLT 3.0 and XQuery,
/// and core has no case functions; import still accepts those spellings.
pub(super) fn function_library(name: &str) -> &'static str {
    if canonical_function::is_internal(name) {
        return "ferrule";
    }
    match name {
        "left" | "right" | "upper" | "lower" => "lang",
        "left_trim"
        | "right_trim"
        | "pad_string_left"
        | "pad_string_right"
        | "format_guid_string"
        | "is_numeric"
        | "is_empty"
        | "date_from_datetime"
        | "year_from_datetime"
        | "month_from_datetime"
        | "day_from_datetime"
        | "weekday"
        | "hours_from_datetime"
        | "minutes_from_datetime"
        | "time_from_datetime"
        | "datetime_from_date_and_time"
        | "datetime_from_parts"
        | "duration_from_parts"
        | "datetime_add"
        | "create_guid"
        | "random"
        | "delay_passthrough" => "lang",
        "edifact_to_datetime" => "edifact",
        _ => "core",
    }
}

#[cfg(test)]
mod tests {
    use super::{function_library, native_function, unmap_function_name};

    #[test]
    fn internal_whitespace_function_names_export_canonically() {
        assert_eq!(unmap_function_name("normalize_space"), "normalize-space");
        assert_eq!(unmap_function_name("ends_with"), "ends-with");
        assert_eq!(unmap_function_name("matches"), "matches");
        assert_eq!(unmap_function_name("replace"), "replace");
        assert_eq!(function_library("normalize_space"), "core");
        assert_eq!(unmap_function_name("is_numeric"), "numeric");
        assert_eq!(function_library("is_numeric"), "lang");
        assert_eq!(unmap_function_name("is_empty"), "empty");
        assert_eq!(function_library("is_empty"), "lang");
        assert_eq!(unmap_function_name("left"), "left");
        assert_eq!(function_library("left"), "lang");
        assert_eq!(unmap_function_name("right"), "right");
        assert_eq!(function_library("right"), "lang");
        assert_eq!(unmap_function_name("weekday"), "weekday");
        assert_eq!(function_library("weekday"), "lang");
        assert_eq!(
            unmap_function_name("substitute_missing_with_xml_nil"),
            "substitute-missing-with-xsi-nil"
        );
        assert_eq!(function_library("substitute_missing_with_xml_nil"), "core");
        assert_eq!(unmap_function_name("get_fileext"), "get-fileext");
        assert_eq!(function_library("get_fileext"), "core");
        assert_eq!(unmap_function_name("delay_passthrough"), "sleep");
        assert_eq!(function_library("delay_passthrough"), "lang");
        assert_eq!(unmap_function_name("create_guid"), "create-guid");
        assert_eq!(function_library("create_guid"), "lang");
        assert_eq!(
            unmap_function_name("format_guid_string"),
            "format-guid-string"
        );
        assert_eq!(function_library("format_guid_string"), "lang");
        assert_eq!(function_library("to_number"), "ferrule");
        assert_eq!(function_library("sql_like"), "ferrule");
        assert_eq!(function_library("json_serialize_object"), "ferrule");
    }

    #[test]
    fn built_in_language_functions_export_under_their_native_libraries() {
        for (function, name, library) in [
            ("upper", "uppercase", "lang"),
            ("lower", "lowercase", "lang"),
            ("date_from_datetime", "date-from-datetime", "lang"),
            ("boolean", "boolean", "core"),
            ("floor", "floor", "core"),
            ("format_date", "format-date", "core"),
            ("format_datetime", "format-dateTime", "core"),
            ("format_time", "format-time", "core"),
            ("translate", "translate", "core"),
        ] {
            assert_eq!(unmap_function_name(function), name, "{function}");
            assert_eq!(function_library(function), library, "{function}");
        }
    }

    #[test]
    fn calls_export_under_their_native_arity_or_are_refused() {
        for (function, arity, expected) in [
            ("round", 1, Some(("round", "core"))),
            ("round", 2, Some(("round-precision", "core"))),
            ("format_date", 3, Some(("format-date", "core"))),
            ("format_datetime", 3, Some(("format-dateTime", "core"))),
            ("format_time", 2, Some(("format-time", "core"))),
            ("format_date", 4, None),
            ("format_datetime", 5, None),
            ("format_time", 3, None),
            ("positive", 1, None),
            ("trim", 1, None),
            ("ends_with", 2, None),
            ("matches", 2, None),
            ("matches", 3, None),
            ("replace", 3, None),
            ("left_trim", 1, Some(("left-trim", "lang"))),
        ] {
            let actual = native_function(function, arity);
            match expected {
                Some((name, library)) => assert_eq!(
                    actual,
                    Ok((name.to_string(), library)),
                    "{function}/{arity}"
                ),
                None => assert!(actual.is_err(), "{function}/{arity}: {actual:?}"),
            }
        }
    }
}
