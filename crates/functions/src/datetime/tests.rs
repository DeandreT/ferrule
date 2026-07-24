use super::*;

fn text(value: &str) -> Value {
    Value::String(value.to_string())
}

#[test]
fn parses_documented_numeric_and_named_date_pictures() {
    assert_eq!(
        parse_date(&[text("09-12-2014"), text("[D]-[M]-[Y]")]).unwrap(),
        text("2014-12-09")
    );
    assert_eq!(
        parse_date(&[text("01 Apr 2015"), text("[D01] [MNn,3-3] [Y]")]).unwrap(),
        text("2015-04-01")
    );
    assert_eq!(
        parse_date(&[text("01 December 2015"), text("[D01] [MNn,3] [Y]")]).unwrap(),
        text("2015-12-01")
    );
    assert_eq!(
        parse_date(&[text("315 2004 +01:00"), text("[d] [Y] [Z]")]).unwrap(),
        text("2004-11-10+01:00")
    );
}

#[test]
fn parses_documented_datetime_pictures() {
    assert_eq!(
        parse_datetime(&[text("09-12-2014 13:56:24"), text("[M]-[D]-[Y] [H]:[m]:[s]"),]).unwrap(),
        text("2014-09-12T13:56:24")
    );
    assert_eq!(
        parse_datetime(&[
            text("1.December.10 03:2:39 p.m. +01:00"),
            text("[D].[MNn].[Y,2-2] [h]:[m]:[s] [P] [Z]"),
        ])
        .unwrap(),
        text("2010-12-01T15:02:39+01:00")
    );
    assert_eq!(
        parse_datetime(&[text("20110620"), text("[Y,4-4][M,2-2][D,2-2]")]).unwrap(),
        text("2011-06-20T00:00:00")
    );
}

#[test]
fn converts_supported_edifact_2379_datetime_codes() {
    for (value, code, expected) in [
        ("20240229", "102", "2024-02-29T00:00:00"),
        ("202402291305", "203", "2024-02-29T13:05:00"),
        ("20240229130547", "204", "2024-02-29T13:05:47"),
        ("202402291305+0530", "205", "2024-02-29T13:05:00+05:30"),
        ("202402291305PDT", "303", "2024-02-29T13:05:00-09:00"),
        ("20240229130547UTC", "304", "2024-02-29T13:05:47Z"),
    ] {
        assert_eq!(
            edifact_to_datetime(&[text(value), text(code)]).unwrap(),
            text(expected)
        );
    }
}

#[test]
fn rejects_unsupported_or_invalid_edifact_datetime_values() {
    let unsupported = edifact_to_datetime(&[text("2402291305"), text("201")])
        .unwrap_err()
        .to_string();
    assert!(unsupported.contains("supports UN/EDIFACT 2379 codes"));
    assert!(edifact_to_datetime(&[text("202302291305"), text("203")]).is_err());
    let zone = edifact_to_datetime(&[text("202402291305XYZ"), text("303")])
        .unwrap_err()
        .to_string();
    assert!(zone.contains("supports UTC, GMT"));
}

#[test]
fn parses_time_with_fraction_and_gmt_offset() {
    assert_eq!(
        parse_time(&[
            text("03:2:39.25 p.m. GMT+01:00"),
            text("[h]:[m]:[s].[f] [P] [z]"),
        ])
        .unwrap(),
        text("15:02:39.25+01:00")
    );
}

#[test]
fn formats_iso_values_with_the_supported_picture_grammar() {
    assert_eq!(
        format_date(&[text("2015-12-01+01:00"), text("[D01] [MNn,3-3] [Y] [Z]"),]).unwrap(),
        text("01 Dec 2015 +01:00")
    );
    assert_eq!(
        format_datetime(&[
            text("2010-12-01T15:02:39.25+01:00"),
            text("[D].[MNn].[Y,2-2] [h]:[m01]:[s01].[f] [P] [z]"),
        ])
        .unwrap(),
        text("1.December.10 3:02:39.25 PM GMT+01:00")
    );
    assert_eq!(
        format_time(&[text("00:09:08Z"), text("[H01]:[m01]:[s01] [P] [Z]")]).unwrap(),
        text("00:09:08 AM Z")
    );
    assert_eq!(
        format_date(&[
            text("2004-11-10"),
            text("[d,3-3] [Y]"),
            Value::Null,
            Value::Null,
            Value::Null,
        ])
        .unwrap(),
        text("315 2004")
    );
}

#[test]
fn formatters_reject_invalid_values_pictures_and_nondefault_locale_arguments() {
    assert!(format_date(&[text("2023-02-29"), text("[Y]-[M]-[D]")]).is_err());
    assert!(format_time(&[text("24:00:00"), text("[H]:[m]:[s]")]).is_err());
    assert!(format_datetime(&[text("2024-01-01"), text("[Y]")]).is_err());
    assert!(
        format_date(&[
            text("2024-01-01"),
            text("[Y]"),
            text("fr"),
            Value::Null,
            Value::Null,
        ])
        .is_err()
    );
}

#[test]
fn parse_time_uses_atomic_numeric_lexical_forms() {
    assert_eq!(
        parse_time(&[Value::Float(953.0), text("[H,1-1][m,2-2]"),]).unwrap(),
        text("09:53:00")
    );
    assert_eq!(
        parse_time(&[Value::Int(1703), text("[H,2-2][m,2-2]")]).unwrap(),
        text("17:03:00")
    );
}

#[test]
fn rejects_picture_mismatches_and_invalid_calendar_values() {
    assert!(parse_date(&[text("2014-02-29"), text("[Y]-[M]-[D]")]).is_err());
    assert!(parse_date(&[text("2014/01/02"), text("[Y]-[M]-[D]")]).is_err());
    assert!(
        parse_datetime(&[text("2014-01-02 24:00:00"), text("[Y]-[M]-[D] [H]:[m]:[s]")]).is_err()
    );
    assert!(parse_date(&[text("2014-01-02"), text("[]-x")]).is_err());
}

#[test]
fn extracts_and_validates_iso_time_components() {
    assert_eq!(
        time_from_datetime(&[text("2001-12-17T09:30:02.5+05:00")]).unwrap(),
        text("09:30:02.5+05:00")
    );
    for value in ["-0001-12-17T09:30:02", "12024-12-17T09:30:02"] {
        assert_eq!(
            time_from_datetime(&[text(value)]).unwrap(),
            text("09:30:02")
        );
    }
    assert!(time_from_datetime(&[text("2001-02-29T09:30:02")]).is_err());
    assert!(time_from_datetime(&[text("2001-01-01T09:30:0é")]).is_err());
}

#[test]
fn composes_datetime_from_xml_date_and_time() {
    assert_eq!(
        datetime_from_date_and_time(&[text("2024-02-29+05:30"), text("09:08:07.125+05:30"),])
            .unwrap(),
        text("2024-02-29T09:08:07.125+05:30")
    );
    assert_eq!(
        datetime_from_date_and_time(&[text("2024-02-29"), text("09:08:07-04:00")]).unwrap(),
        text("2024-02-29T09:08:07-04:00")
    );
    assert_eq!(
        datetime_from_date_and_time(&[text("2024-01-02Z")]).unwrap(),
        text("2024-01-02T00:00:00Z")
    );
    assert_eq!(
        datetime_from_date_and_time(&[text("-0001-01-02")]).unwrap(),
        text("-0001-01-02T00:00:00")
    );
    assert!(datetime_from_date_and_time(&[text("2023-02-29")]).is_err());
    assert!(
        datetime_from_date_and_time(&[text("2024-02-29+05:30"), text("09:08:07-04:00")]).is_err()
    );
}

#[test]
fn coerces_xml_date_to_datetime_without_losing_timezone() {
    assert_eq!(
        coerce_datetime(&[text("2031-08-17")]).unwrap(),
        text("2031-08-17T00:00:00")
    );
    assert_eq!(
        coerce_datetime(&[text("2031-08-17+05:45")]).unwrap(),
        text("2031-08-17T00:00:00+05:45")
    );
    assert_eq!(
        coerce_datetime(&[text("2031-08-17T06:07:08.9Z")]).unwrap(),
        text("2031-08-17T06:07:08.9Z")
    );
    assert_eq!(coerce_datetime(&[Value::Null]).unwrap(), Value::Null);
    assert!(coerce_datetime(&[text("2031-02-29")]).is_err());
    assert!(coerce_datetime(&[Value::Int(1)]).is_err());
}

#[test]
fn composes_datetime_from_typed_parts() {
    assert_eq!(
        datetime_from_parts(&[
            text("2024"),
            Value::Int(2),
            Value::Float(29.0),
            Value::Int(9),
            Value::Int(8),
            Value::Int(7),
            Value::Float(125.5),
            Value::Int(330),
        ])
        .unwrap(),
        text("2024-02-29T09:08:07.1255+05:30")
    );
    assert_eq!(
        datetime_from_parts(&[text("2024"), text("1"), text("2")]).unwrap(),
        text("2024-01-02T00:00:00")
    );
    assert_eq!(
        datetime_from_parts(&[
            text("-1"),
            text("1"),
            text("2"),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Float(f64::EPSILON),
        ])
        .unwrap(),
        text("-0001-01-02T00:00:00")
    );
    assert!(datetime_from_parts(&[text("2023"), text("2"), text("29")]).is_err());
    assert!(
        datetime_from_parts(&[Value::Float(-(i64::MIN as f64)), text("1"), text("2")]).is_err()
    );
    assert!(
        datetime_from_parts(&[
            text("2024"),
            text("1"),
            text("2"),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Int(841),
        ])
        .is_err()
    );
}

#[test]
fn composes_duration_from_typed_parts() -> Result<(), FunctionError> {
    assert_eq!(
        duration_from_parts(&[
            Value::Int(1),
            Value::Int(4),
            Value::Int(17),
            Value::Int(8),
            Value::Int(58),
            Value::Int(54),
            Value::Int(333),
            Value::Bool(true),
        ])?,
        text("-P1Y4M17DT8H58M54.333S")
    );
    assert_eq!(
        duration_from_parts(&[Value::Int(0), Value::Int(0), Value::Int(0), Value::Int(5),])?,
        text("PT5H")
    );
    assert_eq!(
        duration_from_parts(&[Value::Int(0), Value::Int(0), Value::Int(0)])?,
        text("PT0S")
    );
    assert_eq!(
        duration_from_parts(&[
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(9_007_199_254_740_993),
            Value::Int(333),
        ])?,
        text("PT9007199254740993.333S")
    );
    assert_eq!(
        duration_from_parts(&[
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Int(i64::MAX),
        ])?,
        text("PT9223372036854775807S")
    );
    assert!(duration_from_parts(&[Value::Int(0), Value::Int(0), Value::Int(-1)]).is_err());
    assert!(
        duration_from_parts(&[
            Value::Int(0),
            Value::Int(0),
            Value::Int(0),
            Value::Null,
            Value::Null,
            Value::Null,
            Value::Int(1000),
        ])
        .is_err()
    );
    Ok(())
}
