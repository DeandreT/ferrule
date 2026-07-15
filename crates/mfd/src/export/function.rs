use ir::{ScalarType, Value};
use mapping::AggregateOp;

pub(super) fn aggregate_component_name(op: AggregateOp) -> &'static str {
    match op {
        AggregateOp::Count => "count",
        AggregateOp::Sum => "sum",
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
        Value::Bool(value) => (value.to_string(), "boolean"),
        Value::Int(value) => (value.to_string(), "integer"),
        Value::Float(value) => (value.to_string(), "decimal"),
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
        Value::Null | Value::XmlNil(_) => None,
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
        "greater_or_equal" => "greater-equal",
        "less_or_equal" => "less-equal",
        "and" => "logical-and",
        "or" => "logical-or",
        "not" => "logical-not",
        "length" => "string-length",
        "starts_with" => "starts-with",
        "upper" => "upper-case",
        "lower" => "lower-case",
        "left_trim" => "left-trim",
        "right_trim" => "right-trim",
        "pad_string_left" => "pad-string-left",
        "pad_string_right" => "pad-string-right",
        "substring_before" => "substring-before",
        "substring_after" => "substring-after",
        "get_folder" => "get-folder",
        "remove_folder" => "remove-folder",
        "resolve_filepath" => "resolve-filepath",
        "is_xml_nil" => "is-xsi-nil",
        "date_from_datetime" => "date-from-datetime",
        "year_from_datetime" => "year-from-datetime",
        "month_from_datetime" => "month-from-datetime",
        "day_from_datetime" => "day-from-datetime",
        "hours_from_datetime" => "hour-from-datetime",
        "minutes_from_datetime" => "minute-from-datetime",
        "time_from_datetime" => "time-from-datetime",
        "datetime_from_date_and_time" => "datetime-from-date-and-time",
        "datetime_from_parts" => "datetime-from-parts",
        "datetime_add" => "datetime-add",
        "parse_date" => "parse-date",
        "parse_datetime" => "parse-dateTime",
        "parse_time" => "parse-time",
        "edifact_to_datetime" => "to-datetime",
        "substitute_missing" => "substitute-missing",
        "format_number" => "format-number",
        other => other,
    }
    .to_string()
}

pub(super) fn function_library(name: &str) -> &'static str {
    match name {
        "left_trim"
        | "right_trim"
        | "pad_string_left"
        | "pad_string_right"
        | "year_from_datetime"
        | "month_from_datetime"
        | "day_from_datetime"
        | "hours_from_datetime"
        | "minutes_from_datetime"
        | "time_from_datetime"
        | "datetime_from_date_and_time"
        | "datetime_from_parts"
        | "datetime_add" => "lang",
        "edifact_to_datetime" => "edifact",
        _ => "core",
    }
}
