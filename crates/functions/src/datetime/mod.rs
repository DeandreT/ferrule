use std::borrow::Cow;

use ir::Value;

use crate::FunctionError;

const INVALID_PICTURE: &str = "requires a value matching a supported date/time picture";
const EDIFACT_DATETIME_INVALID: &str =
    "requires a value matching its UN/EDIFACT 2379 date/time format code";
const EDIFACT_DATETIME_UNSUPPORTED: &str =
    "supports UN/EDIFACT 2379 codes 102, 203, 204, 205, 303, and 304";
const EDIFACT_ZONE_UNSUPPORTED: &str =
    "supports UTC, GMT, EST, EDT, CST, CDT, MST, MDT, PST, and PDT named zones";

#[derive(Clone, Copy)]
enum Field {
    Year,
    Month,
    MonthName,
    Day,
    DayOfYear,
    Hour24,
    Hour12,
    Period,
    Minute,
    Second,
    Fraction,
    Timezone,
    GmtTimezone,
}

struct Component {
    field: Field,
    min_width: usize,
    max_width: usize,
    fixed_width: Option<usize>,
}

enum Part {
    Literal(String),
    Component(Component),
}

#[derive(Default)]
struct Parsed {
    year: Option<(u32, usize)>,
    month: Option<u32>,
    day: Option<u32>,
    day_of_year: Option<u32>,
    hour24: Option<u32>,
    hour12: Option<u32>,
    period: Option<bool>,
    minute: Option<u32>,
    second: Option<u32>,
    fraction: Option<String>,
    timezone: Option<String>,
}

pub(super) fn parse_date(args: &[Value]) -> Result<Value, FunctionError> {
    let (value, picture) = string_pair(args, "parse_date")?;
    let parsed = parse_picture(value, picture, "parse_date")?;
    let (year, month, day) = parsed.date("parse_date")?;
    let mut output = format!("{year:04}-{month:02}-{day:02}");
    if let Some(timezone) = parsed.timezone {
        output.push_str(&timezone);
    }
    Ok(Value::String(output))
}

pub(super) fn parse_datetime(args: &[Value]) -> Result<Value, FunctionError> {
    let (value, picture) = string_pair(args, "parse_datetime")?;
    let parsed = parse_picture(value, picture, "parse_datetime")?;
    let (year, month, day) = parsed.date("parse_datetime")?;
    let (hour, minute, second) = parsed.time("parse_datetime", true)?;
    let mut output = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}");
    append_time_suffix(&mut output, &parsed);
    Ok(Value::String(output))
}

pub(super) fn parse_time(args: &[Value]) -> Result<Value, FunctionError> {
    let (value, picture) = lexical_pair(args, "parse_time")?;
    let parsed = parse_picture(&value, picture, "parse_time")?;
    let (hour, minute, second) = parsed.time("parse_time", false)?;
    let mut output = format!("{hour:02}:{minute:02}:{second:02}");
    append_time_suffix(&mut output, &parsed);
    Ok(Value::String(output))
}

pub(super) fn edifact_to_datetime(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "edifact_to_datetime";
    let (value, code) = match args {
        [Value::String(value), Value::String(code)] => (value.as_str(), code.as_str()),
        [first, second] => {
            let bad = if matches!(first, Value::String(_)) {
                second
            } else {
                first
            };
            return Err(FunctionError::TypeMismatch {
                function: FUNCTION,
                got: bad.type_name(),
            });
        }
        _ => {
            return Err(FunctionError::ArityMismatch {
                function: FUNCTION,
                expected: 2,
                got: args.len(),
            });
        }
    };
    if !value.is_ascii() {
        return edifact_datetime_invalid();
    }

    let (base_len, has_seconds, zone) = match code {
        "102" if value.len() == 8 => (8, false, String::new()),
        "203" if value.len() == 12 => (12, false, String::new()),
        "204" if value.len() == 14 => (14, true, String::new()),
        "205" => {
            if value.len() != 17 {
                return edifact_datetime_invalid();
            }
            (12, false, numeric_edifact_zone(&value[12..])?)
        }
        "303" => {
            if value.len() != 15 {
                return edifact_datetime_invalid();
            }
            (12, false, named_edifact_zone(&value[12..])?.to_string())
        }
        "304" => {
            if value.len() != 17 {
                return edifact_datetime_invalid();
            }
            (14, true, named_edifact_zone(&value[14..])?.to_string())
        }
        "102" | "203" | "204" => return edifact_datetime_invalid(),
        _ => {
            return Err(FunctionError::InvalidArgument {
                function: FUNCTION,
                message: EDIFACT_DATETIME_UNSUPPORTED,
            });
        }
    };
    if value.len() < base_len {
        return edifact_datetime_invalid();
    }
    let base = &value[..base_len];
    if !base.bytes().all(|byte| byte.is_ascii_digit()) {
        return edifact_datetime_invalid();
    }
    let date = format!("{}-{}-{}", &base[..4], &base[4..6], &base[6..8]);
    validate_iso_date(&date, FUNCTION)?;
    let time = if base_len == 8 {
        "00:00:00".to_string()
    } else {
        let seconds = if has_seconds { &base[12..14] } else { "00" };
        format!("{}:{}:{seconds}", &base[8..10], &base[10..12])
    };
    validate_iso_time(&format!("{time}{zone}"), FUNCTION)?;
    Ok(Value::String(format!("{date}T{time}{zone}")))
}

fn numeric_edifact_zone(zone: &str) -> Result<String, FunctionError> {
    if zone.len() != 5
        || !matches!(zone.as_bytes().first(), Some(b'+') | Some(b'-'))
        || !zone[1..].bytes().all(|byte| byte.is_ascii_digit())
    {
        return edifact_datetime_invalid();
    }
    if &zone[1..] == "0000" {
        return Ok("Z".to_string());
    }
    Ok(format!("{}{}:{}", &zone[..1], &zone[1..3], &zone[3..]))
}

fn named_edifact_zone(zone: &str) -> Result<&'static str, FunctionError> {
    const FUNCTION: &str = "edifact_to_datetime";
    match zone {
        "UTC" | "GMT" => Ok("Z"),
        "EST" => Ok("-05:00"),
        "EDT" => Ok("-04:00"),
        "CST" => Ok("-06:00"),
        "CDT" => Ok("-05:00"),
        "MST" => Ok("-07:00"),
        "MDT" => Ok("-06:00"),
        "PST" => Ok("-08:00"),
        // MapForce's UN/EDIFACT 2379 conversion assigns PDT this legacy
        // offset, including for format code 303. Keep it distinct from the
        // conventional civil-time offset used outside that function.
        "PDT" => Ok("-09:00"),
        _ => Err(FunctionError::InvalidArgument {
            function: FUNCTION,
            message: EDIFACT_ZONE_UNSUPPORTED,
        }),
    }
}

fn edifact_datetime_invalid<T>() -> Result<T, FunctionError> {
    Err(FunctionError::InvalidArgument {
        function: "edifact_to_datetime",
        message: EDIFACT_DATETIME_INVALID,
    })
}

pub(super) fn time_from_datetime(args: &[Value]) -> Result<Value, FunctionError> {
    let value = match args {
        [Value::String(value)] => value,
        [other] => {
            return Err(FunctionError::TypeMismatch {
                function: "time_from_datetime",
                got: other.type_name(),
            });
        }
        _ => {
            return Err(FunctionError::ArityMismatch {
                function: "time_from_datetime",
                expected: 1,
                got: args.len(),
            });
        }
    };
    let Some((date, time)) = value.split_once('T') else {
        return invalid("time_from_datetime");
    };
    validate_iso_date(date, "time_from_datetime")?;
    validate_iso_time(time, "time_from_datetime")?;
    Ok(Value::String(time.to_string()))
}

pub(super) fn datetime_from_date_and_time(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "datetime_from_date_and_time";
    let (date, time) = match args {
        [Value::String(date)] => (date.as_str(), "00:00:00"),
        [Value::String(date), Value::String(time)] => (date.as_str(), time.as_str()),
        [Value::String(date), Value::Null | Value::JsonNull(_)] => (date.as_str(), "00:00:00"),
        [first, second] => {
            let bad = if matches!(first, Value::String(_)) {
                second
            } else {
                first
            };
            return Err(FunctionError::TypeMismatch {
                function: FUNCTION,
                got: bad.type_name(),
            });
        }
        [other] => {
            return Err(FunctionError::TypeMismatch {
                function: FUNCTION,
                got: other.type_name(),
            });
        }
        _ => {
            return Err(FunctionError::ArityMismatch {
                function: FUNCTION,
                expected: 1,
                got: args.len(),
            });
        }
    };

    let (date, date_zone) = split_iso_timezone(date, FUNCTION)?;
    let (time, time_zone) = split_iso_timezone(time, FUNCTION)?;
    validate_iso_date(date, FUNCTION)?;
    validate_iso_time(time, FUNCTION)?;
    let zone = match (date_zone, time_zone) {
        (Some(date_zone), Some(time_zone))
            if timezone_offset(date_zone) != timezone_offset(time_zone) =>
        {
            return invalid(FUNCTION);
        }
        (Some(date_zone), _) => Some(date_zone),
        (None, time_zone) => time_zone,
    };

    let mut output = format!("{date}T{time}");
    if let Some(zone) = zone {
        output.push_str(zone);
    }
    Ok(Value::String(output))
}

pub(super) fn coerce_datetime(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "coerce_datetime";
    let value = match args {
        [Value::Null | Value::JsonNull(_)] => return Ok(Value::Null),
        [Value::XmlNil(nil)] => return Ok(Value::XmlNil(*nil)),
        [Value::String(value)] => value,
        [other] => {
            return Err(FunctionError::TypeMismatch {
                function: FUNCTION,
                got: other.type_name(),
            });
        }
        _ => {
            return Err(FunctionError::ArityMismatch {
                function: FUNCTION,
                expected: 1,
                got: args.len(),
            });
        }
    };

    if let Some((date, time)) = value.split_once('T') {
        validate_iso_date(date, FUNCTION)?;
        validate_iso_time(time, FUNCTION)?;
        return Ok(Value::String(value.clone()));
    }

    let (date, timezone) = split_iso_timezone(value, FUNCTION)?;
    validate_iso_date(date, FUNCTION)?;
    let mut output = format!("{date}T00:00:00");
    if let Some(timezone) = timezone {
        output.push_str(timezone);
    }
    Ok(Value::String(output))
}

pub(super) fn datetime_from_parts(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "datetime_from_parts";
    if !(3..=8).contains(&args.len()) {
        return Err(FunctionError::ArityMismatch {
            function: FUNCTION,
            expected: 3,
            got: args.len(),
        });
    }

    let year = integer_part(&args[0], FUNCTION)?;
    let month = integer_part(&args[1], FUNCTION)?;
    let day = integer_part(&args[2], FUNCTION)?;
    let optional = |index: usize| -> Result<i64, FunctionError> {
        match args.get(index) {
            None | Some(Value::Null | Value::JsonNull(_)) => Ok(0),
            Some(value) => integer_part(value, FUNCTION),
        }
    };
    let hour = optional(3)?;
    let minute = optional(4)?;
    let second = optional(5)?;
    let millisecond = match args.get(6) {
        None | Some(Value::Null | Value::JsonNull(_)) => 0.0,
        Some(value) => decimal_part(value, FUNCTION)?,
    };
    let timezone = match args.get(7) {
        None | Some(Value::Null | Value::JsonNull(_)) => None,
        Some(value) => Some(integer_part(value, FUNCTION)?),
    };

    let (Ok(month), Ok(day), Ok(hour), Ok(minute), Ok(second)) = (
        u32::try_from(month),
        u32::try_from(day),
        u32::try_from(hour),
        u32::try_from(minute),
        u32::try_from(second),
    ) else {
        return invalid(FUNCTION);
    };
    if !valid_signed_date(year, month, day)
        || hour > 23
        || minute > 59
        || second > 59
        || !millisecond.is_finite()
        || !(0.0..1000.0).contains(&millisecond)
    {
        return invalid(FUNCTION);
    }

    let year = if year < 0 {
        format!("-{:04}", year.unsigned_abs())
    } else {
        format!("{year:04}")
    };
    let mut output = format!("{year}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}");
    if millisecond != 0.0 {
        let fraction = format!("{:.15}", millisecond / 1000.0);
        let fraction = fraction.trim_start_matches('0').trim_end_matches('0');
        if fraction != "." {
            output.push_str(fraction);
        }
    }
    if let Some(offset) = timezone.filter(|offset| *offset != -32_768) {
        if !(-840..=840).contains(&offset) {
            return invalid(FUNCTION);
        }
        if offset == 0 {
            output.push('Z');
        } else {
            let sign = if offset < 0 { '-' } else { '+' };
            let absolute = offset.unsigned_abs();
            output.push_str(&format!("{sign}{:02}:{:02}", absolute / 60, absolute % 60));
        }
    }
    Ok(Value::String(output))
}

pub(super) fn duration_from_parts(args: &[Value]) -> Result<Value, FunctionError> {
    const FUNCTION: &str = "duration_from_parts";
    if !(3..=8).contains(&args.len()) {
        return Err(FunctionError::ArityMismatch {
            function: FUNCTION,
            expected: 3,
            got: args.len(),
        });
    }
    let required = [
        integer_part(&args[0], FUNCTION)?,
        integer_part(&args[1], FUNCTION)?,
        integer_part(&args[2], FUNCTION)?,
    ];
    let optional_integer = |index: usize| -> Result<i64, FunctionError> {
        match args.get(index) {
            None | Some(Value::Null | Value::JsonNull(_)) => Ok(0),
            Some(value) => integer_part(value, FUNCTION),
        }
    };
    let hour = optional_integer(3)?;
    let minute = optional_integer(4)?;
    let second = optional_integer(5)?;
    let millisecond = match args.get(6) {
        None | Some(Value::Null | Value::JsonNull(_)) => 0.0,
        Some(value) => decimal_part(value, FUNCTION)?,
    };
    let negative = match args.get(7) {
        None | Some(Value::Null | Value::JsonNull(_)) => false,
        Some(Value::Bool(value)) => *value,
        Some(value) => {
            return Err(FunctionError::TypeMismatch {
                function: FUNCTION,
                got: value.type_name(),
            });
        }
    };
    if required
        .into_iter()
        .chain([hour, minute, second])
        .any(|part| part < 0)
        || !millisecond.is_finite()
        || !(0.0..1000.0).contains(&millisecond)
    {
        return invalid(FUNCTION);
    }
    let [year, month, day] = required;
    let has_date = year != 0 || month != 0 || day != 0;
    let has_time = hour != 0 || minute != 0 || second != 0 || millisecond != 0.0;
    let mut output = if negative {
        "-P".to_string()
    } else {
        "P".to_string()
    };
    if year != 0 {
        output.push_str(&format!("{year}Y"));
    }
    if month != 0 {
        output.push_str(&format!("{month}M"));
    }
    if day != 0 {
        output.push_str(&format!("{day}D"));
    }
    if has_time {
        output.push('T');
        if hour != 0 {
            output.push_str(&format!("{hour}H"));
        }
        if minute != 0 {
            output.push_str(&format!("{minute}M"));
        }
        if second != 0 || millisecond != 0.0 {
            output.push_str(&second.to_string());
            if millisecond != 0.0 {
                let Some(fraction) = shifted_decimal_fraction(millisecond, 3) else {
                    return invalid(FUNCTION);
                };
                output.push_str(&fraction);
            }
            output.push('S');
        }
    }
    if !has_date && !has_time {
        output.push_str("T0S");
    }
    Ok(Value::String(output))
}

fn shifted_decimal_fraction(value: f64, places: i32) -> Option<String> {
    let lexical = value.to_string();
    let (mantissa, exponent) = match lexical.split_once(['e', 'E']) {
        Some((mantissa, exponent)) => (mantissa, exponent.parse::<i32>().ok()?),
        None => (lexical.as_str(), 0),
    };
    let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let mut digits = String::with_capacity(whole.len() + fraction.len());
    digits.push_str(whole);
    digits.push_str(fraction);
    let decimal_position = i32::try_from(whole.len())
        .ok()?
        .checked_add(exponent)?
        .checked_sub(places)?;
    let mut shifted = String::from(".");
    if decimal_position < 0 {
        shifted.push_str(&"0".repeat(decimal_position.unsigned_abs() as usize));
        shifted.push_str(&digits);
    } else {
        let decimal_position = usize::try_from(decimal_position).ok()?;
        if decimal_position >= digits.len() {
            shifted.push_str(&digits);
            shifted.push_str(&"0".repeat(decimal_position - digits.len()));
        } else {
            shifted.push_str(&digits[..decimal_position]);
            shifted.push_str(&digits[decimal_position..]);
        }
    }
    while shifted.ends_with('0') {
        shifted.pop();
    }
    (shifted != ".").then_some(shifted)
}

fn integer_part(value: &Value, function: &'static str) -> Result<i64, FunctionError> {
    let parsed = match value {
        Value::Int(value) => Some(*value),
        Value::Float(value)
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i64::MIN as f64
                && *value < -(i64::MIN as f64) =>
        {
            Some(*value as i64)
        }
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    };
    parsed.ok_or(FunctionError::TypeMismatch {
        function,
        got: value.type_name(),
    })
}

fn decimal_part(value: &Value, function: &'static str) -> Result<f64, FunctionError> {
    let parsed = match value {
        Value::Int(value) => Some(*value as f64),
        Value::Float(value) => Some(*value),
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    };
    parsed.ok_or(FunctionError::TypeMismatch {
        function,
        got: value.type_name(),
    })
}

pub(super) fn split_iso_timezone<'a>(
    value: &'a str,
    function: &'static str,
) -> Result<(&'a str, Option<&'a str>), FunctionError> {
    if let Some(value) = value.strip_suffix('Z') {
        return Ok((value, Some("Z")));
    }
    if value.len() >= 6 {
        let start = value.len() - 6;
        let candidate = &value[start..];
        if matches!(candidate.as_bytes().first(), Some(b'+' | b'-'))
            && candidate.as_bytes().get(3) == Some(&b':')
        {
            timezone(candidate, false, function)?;
            return Ok((&value[..start], Some(candidate)));
        }
    }
    Ok((value, None))
}

fn timezone_offset(value: &str) -> i32 {
    if value == "Z" {
        return 0;
    }
    let bytes = value.as_bytes();
    let sign = if value.starts_with('-') { -1 } else { 1 };
    let hour = i32::from(bytes[1] - b'0') * 10 + i32::from(bytes[2] - b'0');
    let minute = i32::from(bytes[4] - b'0') * 10 + i32::from(bytes[5] - b'0');
    sign * (hour * 60 + minute)
}

fn string_pair<'a>(
    args: &'a [Value],
    function: &'static str,
) -> Result<(&'a str, &'a str), FunctionError> {
    match args {
        [Value::String(value), Value::String(picture)] => Ok((value, picture)),
        [first, second] => {
            let bad = if matches!(first, Value::String(_)) {
                second
            } else {
                first
            };
            Err(FunctionError::TypeMismatch {
                function,
                got: bad.type_name(),
            })
        }
        _ => Err(FunctionError::ArityMismatch {
            function,
            expected: 2,
            got: args.len(),
        }),
    }
}

/// MapForce's parse-time input boundary accepts ordinary atomic values and
/// applies their canonical lexical form before interpreting the picture. The
/// picture itself remains a required string.
fn lexical_pair<'a>(
    args: &'a [Value],
    function: &'static str,
) -> Result<(Cow<'a, str>, &'a str), FunctionError> {
    match args {
        [value, Value::String(picture)] => {
            let value = match value {
                Value::String(value) => Cow::Borrowed(value.as_str()),
                value => Cow::Owned(super::scalar::text(value)),
            };
            Ok((value, picture))
        }
        [_, other] => Err(FunctionError::TypeMismatch {
            function,
            got: other.type_name(),
        }),
        _ => Err(FunctionError::ArityMismatch {
            function,
            expected: 2,
            got: args.len(),
        }),
    }
}

fn append_time_suffix(output: &mut String, parsed: &Parsed) {
    if let Some(fraction) = &parsed.fraction {
        output.push('.');
        output.push_str(fraction);
    }
    if let Some(timezone) = &parsed.timezone {
        output.push_str(timezone);
    }
}

fn parse_picture(
    value: &str,
    picture: &str,
    function: &'static str,
) -> Result<Parsed, FunctionError> {
    let parts = picture_parts(picture).ok_or(FunctionError::InvalidArgument {
        function,
        message: INVALID_PICTURE,
    })?;
    let mut cursor = 0usize;
    let mut parsed = Parsed::default();
    for (index, part) in parts.iter().enumerate() {
        match part {
            Part::Literal(literal) => {
                if !value[cursor..].starts_with(literal) {
                    return invalid(function);
                }
                cursor += literal.len();
            }
            Part::Component(component) => {
                let remaining = &value[cursor..];
                let width = component_width(component, &parts[index + 1..], remaining).ok_or(
                    FunctionError::InvalidArgument {
                        function,
                        message: INVALID_PICTURE,
                    },
                )?;
                let field = take_chars(remaining, width).ok_or(FunctionError::InvalidArgument {
                    function,
                    message: INVALID_PICTURE,
                })?;
                cursor += field.len();
                parsed.set(component.field, field, function)?;
            }
        }
    }
    if cursor != value.len() {
        return invalid(function);
    }
    Ok(parsed)
}

fn component_width(component: &Component, following: &[Part], value: &str) -> Option<usize> {
    let width = if let Some(width) = component.fixed_width {
        width
    } else if let Some(Part::Literal(literal)) = following.first() {
        if literal.is_empty() {
            return None;
        }
        value[..value.find(literal)?].chars().count()
    } else if following.is_empty() {
        value.chars().count()
    } else {
        return None;
    };
    (width >= component.min_width && width <= component.max_width).then_some(width)
}

fn take_chars(value: &str, count: usize) -> Option<&str> {
    let end = value
        .char_indices()
        .nth(count)
        .map_or(value.len(), |(index, _)| index);
    (value[..end].chars().count() == count).then_some(&value[..end])
}

fn picture_parts(picture: &str) -> Option<Vec<Part>> {
    let mut parts = Vec::new();
    let mut cursor = 0usize;
    while let Some(relative) = picture[cursor..].find('[') {
        let start = cursor + relative;
        if start > cursor {
            parts.push(Part::Literal(picture[cursor..start].to_string()));
        }
        let content_start = start + 1;
        let end = content_start + picture[content_start..].find(']')?;
        parts.push(Part::Component(parse_component(
            &picture[content_start..end],
        )?));
        cursor = end + 1;
    }
    if cursor < picture.len() {
        parts.push(Part::Literal(picture[cursor..].to_string()));
    }
    (!parts.is_empty()).then_some(parts)
}

fn parse_component(spec: &str) -> Option<Component> {
    let (presentation, width) = spec
        .split_once(',')
        .map_or((spec, None), |(presentation, width)| {
            (presentation, Some(width))
        });
    let (field, modifier) = if let Some(modifier) = presentation.strip_prefix('M') {
        if matches!(modifier, "N" | "Nn" | "n") {
            (Field::MonthName, modifier)
        } else {
            (Field::Month, modifier)
        }
    } else {
        let head = presentation.chars().next()?;
        let modifier = &presentation[head.len_utf8()..];
        let field = match head {
            'Y' => Field::Year,
            'D' => Field::Day,
            'd' => Field::DayOfYear,
            'H' => Field::Hour24,
            'h' => Field::Hour12,
            'P' => Field::Period,
            'm' => Field::Minute,
            's' => Field::Second,
            'f' => Field::Fraction,
            'Z' => Field::Timezone,
            'z' => Field::GmtTimezone,
            _ => return None,
        };
        (field, modifier)
    };
    let (default_min, default_max) = match field {
        Field::Year => (1, 9),
        Field::Month
        | Field::Day
        | Field::Hour24
        | Field::Hour12
        | Field::Minute
        | Field::Second => (1, 2),
        Field::DayOfYear => (1, 3),
        Field::Fraction => (1, 9),
        Field::MonthName => (3, 9),
        Field::Period => (2, 4),
        Field::Timezone => (1, 6),
        Field::GmtTimezone => (4, 9),
    };
    let presentation_width = match modifier {
        "" | "1" | "N" | "Nn" | "n" => None,
        digits if digits.bytes().all(|byte| byte.is_ascii_digit()) => Some(digits.len()),
        _ => return None,
    };
    let (min_width, max_width) = match width {
        Some(width) => parse_width(width, default_max)?,
        None => (default_min, default_max),
    };
    let fixed_width = (min_width == max_width)
        .then_some(min_width)
        .or(presentation_width);
    Some(Component {
        field,
        min_width,
        max_width,
        fixed_width,
    })
}

fn parse_width(width: &str, natural_max: usize) -> Option<(usize, usize)> {
    let (min, max) = width
        .split_once('-')
        .map_or((width, natural_max.to_string()), |(min, max)| {
            (min, max.to_string())
        });
    let min = min.parse().ok()?;
    let max = max.parse().ok()?;
    (min > 0 && min <= max).then_some((min, max))
}

impl Parsed {
    fn set(
        &mut self,
        field: Field,
        value: &str,
        function: &'static str,
    ) -> Result<(), FunctionError> {
        match field {
            Field::Year => set_once(
                &mut self.year,
                (number(value, function)?, value.len()),
                function,
            ),
            Field::Month => set_once(&mut self.month, number(value, function)?, function),
            Field::MonthName => set_once(&mut self.month, month_number(value, function)?, function),
            Field::Day => set_once(&mut self.day, number(value, function)?, function),
            Field::DayOfYear => set_once(&mut self.day_of_year, number(value, function)?, function),
            Field::Hour24 => set_once(&mut self.hour24, number(value, function)?, function),
            Field::Hour12 => set_once(&mut self.hour12, number(value, function)?, function),
            Field::Period => set_once(&mut self.period, period(value, function)?, function),
            Field::Minute => set_once(&mut self.minute, number(value, function)?, function),
            Field::Second => set_once(&mut self.second, number(value, function)?, function),
            Field::Fraction => {
                if !value.bytes().all(|byte| byte.is_ascii_digit()) {
                    return invalid(function);
                }
                set_once(&mut self.fraction, value.to_string(), function)
            }
            Field::Timezone => set_once(
                &mut self.timezone,
                timezone(value, false, function)?,
                function,
            ),
            Field::GmtTimezone => set_once(
                &mut self.timezone,
                timezone(value, true, function)?,
                function,
            ),
        }
    }

    fn date(&self, function: &'static str) -> Result<(u32, u32, u32), FunctionError> {
        let Some((mut year, width)) = self.year else {
            return invalid(function);
        };
        if width == 2 {
            year += 2000;
        }
        let (month, day) = match (self.month, self.day, self.day_of_year) {
            (Some(month), Some(day), None) => (month, day),
            (None, None, Some(ordinal)) => month_day_from_ordinal(year, ordinal, function)?,
            _ => return invalid(function),
        };
        if !valid_date(year, month, day) {
            return invalid(function);
        }
        Ok((year, month, day))
    }

    fn time(
        &self,
        function: &'static str,
        allow_default: bool,
    ) -> Result<(u32, u32, u32), FunctionError> {
        let has_time = self.hour24.is_some()
            || self.hour12.is_some()
            || self.minute.is_some()
            || self.second.is_some();
        if !allow_default && !has_time {
            return invalid(function);
        }
        let hour = match (self.hour24, self.hour12, self.period) {
            (Some(hour), None, None) if hour <= 23 => hour,
            (None, Some(hour), Some(pm)) if (1..=12).contains(&hour) => {
                (hour % 12) + if pm { 12 } else { 0 }
            }
            (None, None, None) if allow_default => 0,
            _ => return invalid(function),
        };
        let minute = self.minute.unwrap_or(0);
        let second = self.second.unwrap_or(0);
        if minute > 59 || second > 59 {
            return invalid(function);
        }
        Ok((hour, minute, second))
    }
}

fn set_once<T>(
    slot: &mut Option<T>,
    value: T,
    function: &'static str,
) -> Result<(), FunctionError> {
    if slot.is_some() {
        return invalid(function);
    }
    *slot = Some(value);
    Ok(())
}

fn number(value: &str, function: &'static str) -> Result<u32, FunctionError> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return invalid(function);
    }
    value.parse().map_err(|_| FunctionError::InvalidArgument {
        function,
        message: INVALID_PICTURE,
    })
}

fn month_number(value: &str, function: &'static str) -> Result<u32, FunctionError> {
    const MONTHS: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    let value = value.to_ascii_lowercase();
    let mut matches = MONTHS
        .iter()
        .enumerate()
        .filter(|(_, month)| month.starts_with(&value));
    let Some((index, _)) = matches.next() else {
        return invalid(function);
    };
    if matches.next().is_some() {
        return invalid(function);
    }
    Ok(index as u32 + 1)
}

fn period(value: &str, function: &'static str) -> Result<bool, FunctionError> {
    let normalized: String = value
        .chars()
        .filter(char::is_ascii_alphabetic)
        .flat_map(char::to_lowercase)
        .collect();
    match normalized.as_str() {
        "am" => Ok(false),
        "pm" => Ok(true),
        _ => invalid(function),
    }
}

fn timezone(
    value: &str,
    requires_gmt: bool,
    function: &'static str,
) -> Result<String, FunctionError> {
    let value = if requires_gmt {
        value
            .strip_prefix("GMT")
            .ok_or(FunctionError::InvalidArgument {
                function,
                message: INVALID_PICTURE,
            })?
    } else {
        value
    };
    if value == "Z" {
        return Ok("Z".to_string());
    }
    let bytes = value.as_bytes();
    if bytes.len() != 6
        || !matches!(bytes[0], b'+' | b'-')
        || bytes[3] != b':'
        || !bytes[1..3].iter().all(u8::is_ascii_digit)
        || !bytes[4..6].iter().all(u8::is_ascii_digit)
    {
        return invalid(function);
    }
    let hour = number(&value[1..3], function)?;
    let minute = number(&value[4..6], function)?;
    if hour > 14 || minute > 59 || hour == 14 && minute != 0 {
        return invalid(function);
    }
    Ok(value.to_string())
}

fn month_day_from_ordinal(
    year: u32,
    ordinal: u32,
    function: &'static str,
) -> Result<(u32, u32), FunctionError> {
    let mut remaining = ordinal;
    for month in 1..=12 {
        let days = days_in_month(year, month);
        if remaining <= days {
            return Ok((month, remaining));
        }
        remaining = remaining.saturating_sub(days);
    }
    invalid(function)
}

fn valid_date(year: u32, month: u32, day: u32) -> bool {
    year > 0 && (1..=12).contains(&month) && day > 0 && day <= days_in_month(year, month)
}

fn valid_signed_date(year: i64, month: u32, day: u32) -> bool {
    if year == 0 || !(1..=12).contains(&month) || day == 0 {
        return false;
    }
    let year = year.unsigned_abs();
    let leap = year.is_multiple_of(400) || year.is_multiple_of(4) && !year.is_multiple_of(100);
    day <= days_in_month_with_leap(month, leap)
}

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(400) || year.is_multiple_of(4) && !year.is_multiple_of(100) => 29,
        2 => 28,
        _ => 0,
    }
}

pub(super) fn validate_iso_date(value: &str, function: &'static str) -> Result<(), FunctionError> {
    if !value.is_ascii() || value.len() < 10 {
        return invalid(function);
    }
    let year_end = value.len() - 6;
    if &value[year_end..year_end + 1] != "-" || &value[value.len() - 3..value.len() - 2] != "-" {
        return invalid(function);
    }
    let year = value[..year_end]
        .strip_prefix('-')
        .unwrap_or(&value[..year_end]);
    if year.len() < 4
        || !year.bytes().all(|byte| byte.is_ascii_digit())
        || year.bytes().all(|byte| byte == b'0')
        || year.len() > 4 && year.starts_with('0')
    {
        return invalid(function);
    }
    let month = number(&value[year_end + 1..value.len() - 3], function)?;
    let day = number(&value[value.len() - 2..], function)?;
    let leap =
        decimal_mod(year, 400) == 0 || decimal_mod(year, 4) == 0 && decimal_mod(year, 100) != 0;
    if !(1..=12).contains(&month) || day == 0 || day > days_in_month_with_leap(month, leap) {
        return invalid(function);
    }
    Ok(())
}

fn decimal_mod(digits: &str, modulus: u32) -> u32 {
    digits.bytes().fold(0, |value, digit| {
        (value * 10 + u32::from(digit - b'0')) % modulus
    })
}

fn days_in_month_with_leap(month: u32, leap: bool) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    }
}

pub(super) fn validate_iso_time(value: &str, function: &'static str) -> Result<(), FunctionError> {
    if !value.is_ascii() {
        return invalid(function);
    }
    let timezone_start = value
        .char_indices()
        .skip(1)
        .find(|(_, character)| matches!(character, 'Z' | '+' | '-'))
        .map(|(index, _)| index)
        .unwrap_or(value.len());
    let (time, zone) = value.split_at(timezone_start);
    let (whole, fraction) = time
        .split_once('.')
        .map_or((time, None), |(whole, fraction)| (whole, Some(fraction)));
    if whole.len() != 8 || &whole[2..3] != ":" || &whole[5..6] != ":" {
        return invalid(function);
    }
    let hour = number(&whole[..2], function)?;
    let minute = number(&whole[3..5], function)?;
    let second = number(&whole[6..], function)?;
    if hour > 23 || minute > 59 || second > 59 {
        return invalid(function);
    }
    if fraction.is_some_and(|fraction| {
        fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    }) {
        return invalid(function);
    }
    if !zone.is_empty() {
        timezone(zone, false, function)?;
    }
    Ok(())
}

fn invalid<T>(function: &'static str) -> Result<T, FunctionError> {
    Err(FunctionError::InvalidArgument {
        function,
        message: INVALID_PICTURE,
    })
}

#[cfg(test)]
mod tests;
