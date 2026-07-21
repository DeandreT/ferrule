use ir::{ScalarType, SchemaNode};
use mapping::{FormatOptions, TabularBoundaryKind};

use super::function::{function_library, unmap_function_name};
use super::schema::{KeyAlloc, PortMatch, PortTree, SideFormat, side_format};

#[test]
fn pathless_json_boundary_does_not_fall_back_to_xml() {
    let options = FormatOptions {
        json_document: true,
        ..FormatOptions::default()
    };

    assert!(matches!(side_format(&None, &options), SideFormat::Json));
}

#[test]
fn xml_boundary_overrides_a_neutral_instance_extension() {
    let options = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };

    assert!(matches!(
        side_format(&Some("captured.data".into()), &options),
        SideFormat::Xml
    ));
}

#[test]
fn recognized_instance_extension_overrides_a_fallback_document_marker() {
    let options = FormatOptions {
        xml_document: true,
        ..FormatOptions::default()
    };

    assert!(matches!(
        side_format(&Some("records.jsonl".into()), &options),
        SideFormat::Json
    ));
    assert!(matches!(
        side_format(&Some("records.csv".into()), &options),
        SideFormat::Csv
    ));
}

#[test]
fn tabular_identity_is_a_fallback_for_neutral_or_missing_paths() {
    let options = FormatOptions {
        tabular_kind: Some(TabularBoundaryKind::Xlsx),
        ..FormatOptions::default()
    };

    assert!(matches!(side_format(&None, &options), SideFormat::Xlsx));
    assert!(matches!(
        side_format(&Some("report.capture".into()), &options),
        SideFormat::Xlsx
    ));
    assert!(matches!(
        side_format(&Some("report.csv".into()), &options),
        SideFormat::Csv
    ));
}

#[test]
fn canonical_scalar_names_export_as_mapforce_core_functions() {
    assert_eq!(unmap_function_name("string"), "string");
    assert_eq!(unmap_function_name("format_number"), "format-number");
    assert_eq!(function_library("string"), "core");
    assert_eq!(function_library("format_number"), "core");
    assert_eq!(
        unmap_function_name("time_from_datetime"),
        "time-from-datetime"
    );
    assert_eq!(function_library("time_from_datetime"), "lang");
    assert_eq!(
        unmap_function_name("year_from_datetime"),
        "year-from-datetime"
    );
    assert_eq!(function_library("year_from_datetime"), "lang");
    assert_eq!(
        unmap_function_name("day_from_datetime"),
        "day-from-datetime"
    );
    assert_eq!(function_library("day_from_datetime"), "lang");
    assert_eq!(
        unmap_function_name("hours_from_datetime"),
        "hour-from-datetime"
    );
    assert_eq!(function_library("hours_from_datetime"), "lang");
    assert_eq!(
        unmap_function_name("minutes_from_datetime"),
        "minute-from-datetime"
    );
    assert_eq!(function_library("minutes_from_datetime"), "lang");
    assert_eq!(
        unmap_function_name("datetime_from_date_and_time"),
        "datetime-from-date-and-time"
    );
    assert_eq!(function_library("datetime_from_date_and_time"), "lang");
    assert_eq!(
        unmap_function_name("datetime_from_parts"),
        "datetime-from-parts"
    );
    assert_eq!(
        unmap_function_name("duration_from_parts"),
        "duration-from-parts"
    );
    assert_eq!(function_library("datetime_from_parts"), "lang");
    assert_eq!(function_library("duration_from_parts"), "lang");
    assert_eq!(unmap_function_name("datetime_add"), "datetime-add");
    assert_eq!(function_library("datetime_add"), "lang");
    assert_eq!(unmap_function_name("parse_date"), "parse-date");
    assert_eq!(unmap_function_name("parse_datetime"), "parse-dateTime");
    assert_eq!(
        unmap_function_name("substitute_missing"),
        "substitute-missing"
    );
}

#[test]
fn suffix_matching_rejects_ambiguous_source_leaves() {
    let schema = SchemaNode::group(
        "Root",
        vec![
            SchemaNode::group("Customer", vec![SchemaNode::scalar("Id", ScalarType::Int)]),
            SchemaNode::group("Order", vec![SchemaNode::scalar("Id", ScalarType::Int)]),
        ],
    );
    let mut keys = KeyAlloc { next: 1 };
    let ports = PortTree::build(&schema, &mut keys);

    assert!(matches!(
        ports.match_suffix(&["Id".to_string()]),
        PortMatch::Ambiguous
    ));
    assert!(matches!(
        ports.match_suffix(&["Customer".to_string(), "Id".to_string()]),
        PortMatch::Unique(_)
    ));
    assert!(matches!(ports.match_suffix(&[]), PortMatch::Unique(_)));
}
