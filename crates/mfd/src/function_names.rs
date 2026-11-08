//! Scalar name spellings shared by MFD import and export.
//!
//! Import aliases are explicit, not inferred from the export name. Component
//! reading still owns library/kind checks, EDIFACT name normalization and
//! literal replacement; native export still owns arity-dependent rounding and
//! semantic refusals. In particular, a name-only lookup must not start accepting
//! internal Ferrule names or the unqualified EDIFACT `to-datetime` spelling.

struct FunctionName {
    ferrule: &'static str,
    mfd: &'static str,
    import_aliases: &'static [&'static str],
    library: &'static str,
}

impl FunctionName {
    const fn new(
        ferrule: &'static str,
        mfd: &'static str,
        import_aliases: &'static [&'static str],
        library: &'static str,
    ) -> Self {
        Self {
            ferrule,
            mfd,
            import_aliases,
            library,
        }
    }
}

const FUNCTION_NAMES: &[FunctionName] = &[
    FunctionName::new("concat", "concat", &["concat"], "core"),
    FunctionName::new("add", "add", &["add"], "core"),
    FunctionName::new("subtract", "subtract", &["subtract"], "core"),
    FunctionName::new("multiply", "multiply", &["multiply"], "core"),
    FunctionName::new("divide", "divide", &["divide"], "core"),
    FunctionName::new("equal", "equal", &["equal"], "core"),
    FunctionName::new("not_equal", "not-equal", &["not-equal"], "core"),
    FunctionName::new("greater_than", "greater", &["greater"], "core"),
    FunctionName::new("less_than", "less", &["less"], "core"),
    FunctionName::new(
        "greater_or_equal",
        "equal-or-greater",
        &["greater-equal", "greater-or-equal", "equal-or-greater"],
        "core",
    ),
    FunctionName::new(
        "less_or_equal",
        "equal-or-less",
        &["less-equal", "less-or-equal", "equal-or-less"],
        "core",
    ),
    FunctionName::new("and", "logical-and", &["logical-and"], "core"),
    FunctionName::new("or", "logical-or", &["logical-or"], "core"),
    FunctionName::new("not", "logical-not", &["logical-not"], "core"),
    FunctionName::new("length", "string-length", &["string-length"], "core"),
    FunctionName::new("contains", "contains", &["contains"], "core"),
    FunctionName::new("starts_with", "starts-with", &["starts-with"], "core"),
    FunctionName::new("ends_with", "ends-with", &["ends-with"], "core"),
    FunctionName::new("matches", "matches", &["matches"], "core"),
    FunctionName::new("replace", "replace", &["replace"], "core"),
    FunctionName::new("upper", "uppercase", &["upper-case", "uppercase"], "lang"),
    FunctionName::new("lower", "lowercase", &["lower-case", "lowercase"], "lang"),
    FunctionName::new("string", "string", &["string"], "core"),
    FunctionName::new("to_number", "to_number", &["number"], "ferrule"),
    FunctionName::new("is_numeric", "numeric", &["numeric"], "lang"),
    FunctionName::new("boolean", "boolean", &["boolean"], "core"),
    FunctionName::new("positive", "positive", &["positive"], "core"),
    FunctionName::new("floor", "floor", &["floor"], "core"),
    FunctionName::new("create_guid", "create-guid", &["create-guid"], "lang"),
    FunctionName::new("random", "random", &["random"], "lang"),
    FunctionName::new(
        "format_guid_string",
        "format-guid-string",
        &["format-guid-string"],
        "lang",
    ),
    FunctionName::new("format_number", "format-number", &["format-number"], "core"),
    FunctionName::new("format_date", "format-date", &["format-date"], "core"),
    FunctionName::new(
        "format_datetime",
        "format-dateTime",
        &["format-dateTime"],
        "core",
    ),
    FunctionName::new("format_time", "format-time", &["format-time"], "core"),
    FunctionName::new("trim", "trim", &["trim"], "core"),
    FunctionName::new("left", "left", &["left"], "lang"),
    FunctionName::new("right", "right", &["right"], "lang"),
    FunctionName::new("left_trim", "left-trim", &["left-trim"], "lang"),
    FunctionName::new("right_trim", "right-trim", &["right-trim"], "lang"),
    FunctionName::new(
        "pad_string_left",
        "pad-string-left",
        &["pad-string-left"],
        "lang",
    ),
    FunctionName::new(
        "pad_string_right",
        "pad-string-right",
        &["pad-string-right"],
        "lang",
    ),
    FunctionName::new("substring", "substring", &["substring"], "core"),
    FunctionName::new(
        "substring_before",
        "substring-before",
        &["substring-before"],
        "core",
    ),
    FunctionName::new(
        "substring_after",
        "substring-after",
        &["substring-after"],
        "core",
    ),
    FunctionName::new("translate", "translate", &["translate"], "core"),
    FunctionName::new(
        "normalize_space",
        "normalize-space",
        &["normalize-space"],
        "core",
    ),
    FunctionName::new("is_empty", "empty", &["empty"], "lang"),
    FunctionName::new("get_folder", "get-folder", &["get-folder"], "core"),
    FunctionName::new("remove_folder", "remove-folder", &["remove-folder"], "core"),
    FunctionName::new(
        "resolve_filepath",
        "resolve-filepath",
        &["resolve-filepath"],
        "core",
    ),
    FunctionName::new("is_xml_nil", "is-xsi-nil", &["is-xsi-nil"], "core"),
    FunctionName::new(
        "substitute_missing_with_xml_nil",
        "substitute-missing-with-xsi-nil",
        &["substitute-missing-with-xsi-nil"],
        "core",
    ),
    FunctionName::new("exists", "exists", &["exists"], "core"),
    // The name-only exporter historically passes this through. Native export
    // selects `round` or `round-precision` by arity instead; neither may become
    // Ferrule's away-from-zero `round` on import.
    FunctionName::new(
        "round_half_ceiling",
        "round_half_ceiling",
        &["round", "round-precision"],
        "core",
    ),
    FunctionName::new(
        "date_from_datetime",
        "date-from-datetime",
        &["date-from-datetime"],
        "lang",
    ),
    FunctionName::new(
        "year_from_datetime",
        "year-from-datetime",
        &["year-from-datetime"],
        "lang",
    ),
    FunctionName::new(
        "month_from_datetime",
        "month-from-datetime",
        &["month-from-datetime"],
        "lang",
    ),
    FunctionName::new(
        "day_from_datetime",
        "day-from-datetime",
        &["day-from-datetime"],
        "lang",
    ),
    FunctionName::new("weekday", "weekday", &["weekday"], "lang"),
    FunctionName::new(
        "hours_from_datetime",
        "hour-from-datetime",
        &["hour-from-datetime", "hours-from-datetime"],
        "lang",
    ),
    FunctionName::new(
        "minutes_from_datetime",
        "minute-from-datetime",
        &["minute-from-datetime", "minutes-from-datetime"],
        "lang",
    ),
    FunctionName::new(
        "time_from_datetime",
        "time-from-datetime",
        &["time-from-datetime"],
        "lang",
    ),
    FunctionName::new(
        "datetime_from_date_and_time",
        "datetime-from-date-and-time",
        &["datetime-from-date-and-time"],
        "lang",
    ),
    FunctionName::new(
        "datetime_from_parts",
        "datetime-from-parts",
        &["datetime-from-parts"],
        "lang",
    ),
    FunctionName::new(
        "duration_from_parts",
        "duration-from-parts",
        &["duration-from-parts"],
        "lang",
    ),
    FunctionName::new("datetime_add", "datetime-add", &["datetime-add"], "lang"),
    FunctionName::new("parse_date", "parse-date", &["parse-date"], "core"),
    FunctionName::new(
        "parse_datetime",
        "parse-dateTime",
        &["parse-dateTime"],
        "core",
    ),
    FunctionName::new("parse_time", "parse-time", &["parse-time"], "core"),
    FunctionName::new(
        "edifact_to_datetime",
        "to-datetime",
        &["edifact-to-datetime"],
        "edifact",
    ),
    FunctionName::new(
        "substitute_missing",
        "substitute-missing",
        &["substitute-missing", "substitute-null"],
        "core",
    ),
    FunctionName::new("get_fileext", "get-fileext", &["get-fileext"], "core"),
    FunctionName::new(
        "isbn10_to_isbn13",
        "isbn10_to_isbn13",
        &["convertToISBN13", "convertToEAN"],
        "ferrule",
    ),
    FunctionName::new("delay_passthrough", "sleep", &["sleep"], "lang"),
    // `replace` imports as regex replacement by name alone. The component's
    // `lang` library selects literal replacement separately.
    FunctionName::new("replace_literal", "replace", &[], "lang"),
    // These names require a kind-5 component in the explicit Ferrule library;
    // they are not additional accepted aliases for the name-only importer.
    FunctionName::new("sql_like", "sql_like", &[], "ferrule"),
    FunctionName::new("sqlite_multiply", "sqlite_multiply", &[], "ferrule"),
    FunctionName::new(
        "json_serialize_object",
        "json_serialize_object",
        &[],
        "ferrule",
    ),
    FunctionName::new("json_parse_field", "json_parse_field", &[], "ferrule"),
    FunctionName::new(
        "json_parse_object_exists",
        "json_parse_object_exists",
        &[],
        "ferrule",
    ),
    FunctionName::new(
        "json_parse_array_length",
        "json_parse_array_length",
        &[],
        "ferrule",
    ),
    FunctionName::new(
        "json_parse_array_field",
        "json_parse_array_field",
        &[],
        "ferrule",
    ),
    FunctionName::new(
        "flextext_parse_field",
        "flextext_parse_field",
        &[],
        "ferrule",
    ),
    FunctionName::new("coerce_datetime", "coerce_datetime", &[], "ferrule"),
    FunctionName::new("coerce_float", "coerce_float", &[], "ferrule"),
];

pub(crate) fn map_name(name: &str) -> Option<&'static str> {
    FUNCTION_NAMES
        .iter()
        .find(|row| row.import_aliases.contains(&name))
        .map(|row| row.ferrule)
}

pub(crate) fn unmap_function_name(name: &str) -> String {
    FUNCTION_NAMES
        .iter()
        .find(|row| row.ferrule == name)
        .map_or(name, |row| row.mfd)
        .to_string()
}

pub(crate) fn function_library(name: &str) -> &'static str {
    FUNCTION_NAMES
        .iter()
        .find(|row| row.ferrule == name)
        .map_or("core", |row| row.library)
}

#[cfg(test)]
pub(crate) fn export_names() -> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    FUNCTION_NAMES
        .iter()
        .map(|row| (row.ferrule, row.mfd, row.library))
}

#[cfg(test)]
mod tests;
