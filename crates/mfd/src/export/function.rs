use ir::Value;
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
    }
}

pub(super) fn value_text(value: &Value) -> String {
    constant_parts(value).0
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
        "date_from_datetime" => "date-from-datetime",
        "time_from_datetime" => "time-from-datetime",
        "datetime_from_date_and_time" => "datetime-from-date-and-time",
        "datetime_from_parts" => "datetime-from-parts",
        "datetime_add" => "datetime-add",
        "parse_date" => "parse-date",
        "parse_datetime" => "parse-dateTime",
        "parse_time" => "parse-time",
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
        | "time_from_datetime"
        | "datetime_from_date_and_time"
        | "datetime_from_parts"
        | "datetime_add" => "lang",
        _ => "core",
    }
}
