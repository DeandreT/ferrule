use ir::{ScalarType, Value};
use mapping::AggregateOp;

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
    crate::function_names::unmap_function_name(name)
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
        "round" => Some(
            "`round` rounds exact halves away from zero on binary floating-point values, \
             while the built-in `round` and `round-precision` round them toward positive \
             infinity on decimals; use `round_half_ceiling` for the built-in rounding",
        ),
        _ => None,
    };
    if let Some(reason) = refusal {
        return Err(reason);
    }
    // The built-in `round` has one input; rounding to decimals is
    // `round-precision`. Import reads both back as `round_half_ceiling`.
    if function == "round_half_ceiling" {
        let name = if arity == 2 {
            "round-precision"
        } else {
            "round"
        };
        return Ok((name.to_string(), "core"));
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
    crate::function_names::function_library(name)
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
            ("round_half_ceiling", 1, Some(("round", "core"))),
            ("round_half_ceiling", 2, Some(("round-precision", "core"))),
            ("round", 1, None),
            ("round", 2, None),
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
            ("replace", 4, None),
            ("replace_literal", 3, Some(("replace", "lang"))),
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
