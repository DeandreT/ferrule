use std::collections::BTreeSet;

use super::{FUNCTION_NAMES, function_library, map_name, unmap_function_name};

#[test]
fn every_import_alias_maps_to_its_ferrule_name_without_ambiguity() {
    let mut ferrule_names = BTreeSet::new();
    let mut aliases = BTreeSet::new();
    for row in FUNCTION_NAMES {
        assert!(ferrule_names.insert(row.ferrule), "{}", row.ferrule);
        for alias in row.import_aliases {
            assert!(aliases.insert(alias), "{alias}");
            assert_eq!(map_name(alias), Some(row.ferrule), "{alias}");
        }
    }
    // The pre-table importer accepted these exact 86 spellings for 75 names.
    assert_eq!(aliases.len(), 86);
    assert_eq!(
        FUNCTION_NAMES
            .iter()
            .filter(|row| !row.import_aliases.is_empty())
            .count(),
        75
    );
}

#[test]
fn every_export_name_retains_its_name_only_round_trip_or_known_asymmetry() {
    for row in FUNCTION_NAMES {
        let exported = unmap_function_name(row.ferrule);
        assert_eq!(exported, row.mfd, "{}", row.ferrule);
        let expected = match row.ferrule {
            // Library-aware component import, not map_name, handles these.
            "edifact_to_datetime" | "to_number" | "isbn10_to_isbn13" | "sql_like"
            | "sqlite_multiply" | "json_serialize_object" | "json_parse_field"
            | "json_parse_object_exists" | "json_parse_array_length" | "json_parse_array_field"
            | "flextext_parse_field" | "coerce_datetime" | "coerce_float"
            // Native export selects the MFD rounding name by arity.
            | "round_half_ceiling" => None,
            "replace_literal" => Some("replace"),
            _ => Some(row.ferrule),
        };
        assert_eq!(map_name(&exported), expected, "{}", row.ferrule);
    }
}

#[test]
fn libraries_match_the_existing_export_contract() {
    // This independent list records library choices, including identity exports.
    let lang = BTreeSet::from([
        "left",
        "right",
        "upper",
        "lower",
        "left_trim",
        "right_trim",
        "pad_string_left",
        "pad_string_right",
        "format_guid_string",
        "replace_literal",
        "is_numeric",
        "is_empty",
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
        "create_guid",
        "random",
        "delay_passthrough",
    ]);
    let ferrule = BTreeSet::from([
        "isbn10_to_isbn13",
        "sql_like",
        "sqlite_multiply",
        "to_number",
        "json_serialize_object",
        "json_parse_field",
        "json_parse_object_exists",
        "json_parse_array_length",
        "json_parse_array_field",
        "flextext_parse_field",
        "coerce_datetime",
        "coerce_float",
    ]);
    for row in FUNCTION_NAMES {
        let expected = if lang.contains(row.ferrule) {
            "lang"
        } else if ferrule.contains(row.ferrule) {
            "ferrule"
        } else if row.ferrule == "edifact_to_datetime" {
            "edifact"
        } else {
            "core"
        };
        assert_eq!(function_library(row.ferrule), expected, "{}", row.ferrule);
        assert_eq!(
            crate::canonical_function::is_internal(row.ferrule),
            ferrule.contains(row.ferrule)
        );
    }
    for name in lang.iter().chain(&ferrule) {
        assert!(
            FUNCTION_NAMES.iter().any(|row| row.ferrule == *name),
            "{name}"
        );
    }
}

#[test]
fn aliases_and_fallbacks_do_not_gain_normalization_or_admission() {
    for (alias, ferrule, exported) in [
        ("greater-equal", "greater_or_equal", "equal-or-greater"),
        ("greater-or-equal", "greater_or_equal", "equal-or-greater"),
        ("less-equal", "less_or_equal", "equal-or-less"),
        ("less-or-equal", "less_or_equal", "equal-or-less"),
        ("upper-case", "upper", "uppercase"),
        ("lower-case", "lower", "lowercase"),
        (
            "hours-from-datetime",
            "hours_from_datetime",
            "hour-from-datetime",
        ),
        (
            "minutes-from-datetime",
            "minutes_from_datetime",
            "minute-from-datetime",
        ),
        ("edifact-to-datetime", "edifact_to_datetime", "to-datetime"),
        (
            "substitute-null",
            "substitute_missing",
            "substitute-missing",
        ),
        ("convertToISBN13", "isbn10_to_isbn13", "isbn10_to_isbn13"),
        ("convertToEAN", "isbn10_to_isbn13", "isbn10_to_isbn13"),
    ] {
        assert_eq!(map_name(alias), Some(ferrule), "{alias}");
        assert_eq!(unmap_function_name(ferrule), exported, "{ferrule}");
    }
    for name in [
        "",
        "unknown-function",
        "to-datetime",
        "replace_literal",
        "to_number",
        "isbn10_to_isbn13",
        "round_half_ceiling",
        "format-datetime",
        "parse-datetime",
        "hours_from_datetime",
        "UPPERCASE",
        " uppercase",
        "uppercase ",
    ] {
        assert_eq!(map_name(name), None, "{name}");
    }
    for name in [
        "",
        "unknown-function",
        "round",
        "number",
        "to-number",
        "upper-case",
    ] {
        assert_eq!(unmap_function_name(name), name, "{name}");
        assert_eq!(function_library(name), "core", "{name}");
    }
    assert_eq!(map_name("round"), Some("round_half_ceiling"));
    assert_eq!(map_name("round-precision"), Some("round_half_ceiling"));
}
