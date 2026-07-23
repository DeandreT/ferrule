//! EDI schema-guided instance read/write, covering ANSI X12, UN/EDIFACT,
//! HL7 v2, and TRADACOMS.
//!
//! EDI files are flat segment streams whose hierarchy (loops) exists only
//! in an implementation guide, so ferrule expresses that hierarchy in the
//! ordinary [`ir::SchemaNode`] tree and parses by recursive descent over
//! it -- the exact schema conventions are documented in [`segments`], and
//! the dialect-specific tokenizing lives in [`x12`] and [`edifact`].

mod autocomplete;
pub mod config;
pub mod edifact;
pub mod hl7;
pub mod idoc;
mod segments;
pub mod swift;
pub mod tradacoms;
mod validation;
pub mod x12;

pub use validation::{
    EdiConstraintViolation, EdiValidationIssue, EdiValidationReport, validate_values,
};

use std::io::Read;
use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{EdiImpliedDecimal, EdiLexicalFormat, EdiLexicalKind};
use thiserror::Error;

pub(crate) const MAX_RUNTIME_INPUT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum EdiFormatError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("not an X12 interchange: {0}")]
    NotX12(&'static str),
    #[error("not an EDIFACT interchange: {0}")]
    NotEdifact(&'static str),
    #[error("not an HL7 v2 message stream: {0}")]
    NotHl7(&'static str),
    #[error("not a TRADACOMS interchange: {0}")]
    NotTradacoms(&'static str),
    #[error("IDoc record {index}: unrecognized segment `{found}`")]
    UnrecognizedIdocSegment { index: usize, found: String },
    #[error("IDoc record {record}, field `{field}` is not valid UTF-8")]
    InvalidIdocText { record: usize, field: String },
    #[error("IDoc input exceeds the {0} limit")]
    IdocLimit(&'static str),
    #[error("not a SWIFT MT message stream: {0}")]
    NotSwift(&'static str),
    #[error("SWIFT message type `{0}` has no embedded layout")]
    UnknownSwiftMessage(String),
    #[error("SWIFT field `{tag}` value does not match its embedded grammar")]
    SwiftFieldParse { tag: String },
    #[error("SWIFT input exceeds the {0} limit")]
    SwiftLimit(&'static str),
    #[error("segment {index}: expected `{expected}`, found `{found}`")]
    UnexpectedSegment {
        index: usize,
        expected: String,
        found: String,
    },
    #[error("segment {index}: `{id}` not consumed by the schema")]
    TrailingSegment { index: usize, id: String },
    #[error("segment `{segment}` element {element}: cannot parse `{value}` as {expected:?}")]
    ElementParse {
        segment: String,
        element: usize,
        expected: ScalarType,
        value: String,
    },
    #[error(
        "element `{element}` contains reserved delimiter `{delimiter}`, but this EDI dialect \
         has no release character"
    )]
    UnescapableDelimiter { element: String, delimiter: char },
    #[error(
        "ISA element `{element}` declares separator `{found}`, but the writer uses `{expected}`"
    )]
    EnvelopeSeparatorMismatch {
        element: String,
        expected: char,
        found: String,
    },
    #[error("invalid X12 separator configuration: {0}")]
    InvalidX12Separators(String),
    #[error(
        "configured X12 {kind} separator `{expected}` does not match interchange separator `{found}`"
    )]
    X12SeparatorMismatch {
        kind: &'static str,
        expected: char,
        found: char,
    },
    #[error("ISA element `{element}` has invalid value `{value}`: {reason}")]
    InvalidEnvelopeElement {
        element: String,
        value: String,
        reason: &'static str,
    },
    #[error("element `{element}` cannot serialize a non-finite float")]
    NonFiniteFloat { element: String },
    #[error("element `{element}` expected {expected:?}, got {got}")]
    ValueType {
        element: String,
        expected: ScalarType,
        got: &'static str,
    },
    #[error("element `{element}` must equal fixed value `{expected}`, got `{found}`")]
    FixedValueMismatch {
        element: String,
        expected: String,
        found: String,
    },
    #[error("EDI node `{name}` expected {expected}, got {got}")]
    InstanceShape {
        name: String,
        expected: &'static str,
        got: &'static str,
    },
    #[error("EDI group `{group}` has unexpected field `{field}`")]
    UnexpectedField { group: String, field: String },
    #[error("EDI group `{group}` has duplicate field `{field}`")]
    DuplicateField { group: String, field: String },
    #[error("cannot autocomplete the {dialect} envelope: {reason}")]
    EnvelopeAutocomplete {
        dialect: &'static str,
        reason: &'static str,
    },
    #[error(
        "unsupported schema shape at `{0}`: a group named like a segment ID holds \
         scalars/composites, any other group is a loop/container of groups"
    )]
    UnsupportedSchema(String),
    #[error("EDI implied-decimal path `{path}` is invalid: {reason}")]
    InvalidImpliedDecimalLayout { path: String, reason: &'static str },
    #[error("EDI implied-decimal path `{path}` expected a number, got {got}")]
    ImpliedDecimalValue { path: String, got: &'static str },
    #[error("EDI lexical-format path `{path}` is invalid: {reason}")]
    InvalidLexicalFormatLayout { path: String, reason: &'static str },
    #[error("EDI lexical-format path `{path}` cannot format `{value}`: {reason}")]
    LexicalFormatValue {
        path: String,
        value: String,
        reason: &'static str,
    },
    #[error("EDI value-constraint path `{path}` is invalid: {reason}")]
    InvalidValueConstraintLayout { path: String, reason: &'static str },
    #[error("{0}")]
    Validation(EdiValidationReport),
}

/// Applies fixed implied fractional places retained from an EDI
/// configuration to a parsed instance.
pub fn apply_implied_decimals(
    instance: &mut Instance,
    formats: &[EdiImpliedDecimal],
) -> Result<(), EdiFormatError> {
    let mut paths = std::collections::BTreeSet::new();
    for format in formats {
        let path = format.path();
        if !paths.insert(path) {
            return Err(EdiFormatError::InvalidImpliedDecimalLayout {
                path: path.join("/"),
                reason: "duplicate path",
            });
        }
        apply_implied_decimal(instance, path, path, format.places())?;
    }
    Ok(())
}

fn apply_implied_decimal(
    instance: &mut Instance,
    remaining: &[String],
    full_path: &[String],
    places: u8,
) -> Result<(), EdiFormatError> {
    match instance {
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            for item in items {
                apply_implied_decimal(item, remaining, full_path, places)?;
            }
            Ok(())
        }
        Instance::Group(fields) => {
            let Some((segment, tail)) = remaining.split_first() else {
                return Err(EdiFormatError::InvalidImpliedDecimalLayout {
                    path: full_path.join("/"),
                    reason: "path ends at a group",
                });
            };
            let Some((_, value)) = fields.iter_mut().find(|(name, _)| name == segment) else {
                return Ok(());
            };
            apply_implied_decimal(value, tail, full_path, places)
        }
        Instance::Scalar(value) if remaining.is_empty() => match value {
            Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => Ok(()),
            Value::Float(number) => {
                *number /= 10_f64.powi(i32::from(places));
                Ok(())
            }
            Value::Int(number) => {
                *value = Value::Float(*number as f64 / 10_f64.powi(i32::from(places)));
                Ok(())
            }
            other => Err(EdiFormatError::ImpliedDecimalValue {
                path: full_path.join("/"),
                got: other.type_name(),
            }),
        },
        Instance::Scalar(_) => Err(EdiFormatError::InvalidImpliedDecimalLayout {
            path: full_path.join("/"),
            reason: "path crosses a scalar",
        }),
        Instance::DocumentSet(_) => Err(EdiFormatError::InvalidImpliedDecimalLayout {
            path: full_path.join("/"),
            reason: "path crosses a document set",
        }),
    }
}

/// Compacts XML date/time lexical values into the representations declared by
/// an EDI configuration before serialization.
pub fn apply_output_lexical_formats(
    instance: &mut Instance,
    formats: &[EdiLexicalFormat],
) -> Result<(), EdiFormatError> {
    let mut paths = std::collections::BTreeSet::new();
    for format in formats {
        let path = format.path();
        if !paths.insert(path) {
            return Err(EdiFormatError::InvalidLexicalFormatLayout {
                path: path.join("/"),
                reason: "duplicate path",
            });
        }
        apply_output_lexical_format(instance, path, path, format.kind())?;
    }
    Ok(())
}

fn apply_output_lexical_format(
    instance: &mut Instance,
    remaining: &[String],
    full_path: &[String],
    kind: EdiLexicalKind,
) -> Result<(), EdiFormatError> {
    match instance {
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            for item in items {
                apply_output_lexical_format(item, remaining, full_path, kind)?;
            }
            Ok(())
        }
        Instance::Group(fields) => {
            let Some((segment, tail)) = remaining.split_first() else {
                return Err(EdiFormatError::InvalidLexicalFormatLayout {
                    path: full_path.join("/"),
                    reason: "path ends at a group",
                });
            };
            let Some((_, value)) = fields.iter_mut().find(|(name, _)| name == segment) else {
                return Ok(());
            };
            apply_output_lexical_format(value, tail, full_path, kind)
        }
        Instance::Scalar(value) if remaining.is_empty() => match value {
            Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => Ok(()),
            Value::Float(number) => match kind {
                EdiLexicalKind::Decimal { max_chars } => {
                    *number = canonical_edi_decimal(*number, usize::from(max_chars)).map_err(
                        |reason| EdiFormatError::LexicalFormatValue {
                            path: full_path.join("/"),
                            value: number.to_string(),
                            reason,
                        },
                    )?;
                    Ok(())
                }
                _ => Err(EdiFormatError::LexicalFormatValue {
                    path: full_path.join("/"),
                    value: number.to_string(),
                    reason: "expected a string date/time lexical value",
                }),
            },
            Value::Int(number) => match kind {
                EdiLexicalKind::Decimal { max_chars }
                    if number.to_string().len() <= usize::from(max_chars) =>
                {
                    Ok(())
                }
                EdiLexicalKind::Decimal { .. } => Err(EdiFormatError::LexicalFormatValue {
                    path: full_path.join("/"),
                    value: number.to_string(),
                    reason: "integer exceeds the declared EDI decimal length",
                }),
                _ => Err(EdiFormatError::LexicalFormatValue {
                    path: full_path.join("/"),
                    value: number.to_string(),
                    reason: "expected a string date/time lexical value",
                }),
            },
            Value::String(text) => {
                *text = if let EdiLexicalKind::Decimal { max_chars } = kind {
                    validate_edi_decimal_string(text, usize::from(max_chars))
                } else {
                    compact_lexical(text, kind)
                }
                .map_err(|reason| EdiFormatError::LexicalFormatValue {
                    path: full_path.join("/"),
                    value: text.clone(),
                    reason,
                })?;
                Ok(())
            }
            other => Err(EdiFormatError::LexicalFormatValue {
                path: full_path.join("/"),
                value: format!("{other:?}"),
                reason: "expected a string date/time lexical value",
            }),
        },
        Instance::Scalar(_) => Err(EdiFormatError::InvalidLexicalFormatLayout {
            path: full_path.join("/"),
            reason: "path crosses a scalar",
        }),
        Instance::DocumentSet(_) => Err(EdiFormatError::InvalidLexicalFormatLayout {
            path: full_path.join("/"),
            reason: "path crosses a document set",
        }),
    }
}

fn compact_lexical(value: &str, kind: EdiLexicalKind) -> Result<String, &'static str> {
    match kind {
        EdiLexicalKind::CompactDate6 => compact_date(value, true),
        EdiLexicalKind::CompactDate8 => compact_date(value, false),
        EdiLexicalKind::CompactTime {
            min_digits,
            max_digits,
        } => compact_time(value, usize::from(min_digits), usize::from(max_digits)),
        EdiLexicalKind::Decimal { .. } => Err("expected a numeric value for an EDI decimal field"),
    }
}

fn canonical_edi_decimal(number: f64, max_chars: usize) -> Result<f64, &'static str> {
    if !number.is_finite() {
        return Err("EDI decimals must be finite");
    }
    let shortest = number.to_string();
    if shortest.len() <= max_chars && !shortest.contains(['e', 'E']) {
        return Ok(number);
    }
    let tolerance = f64::EPSILON * number.abs().max(1.0) * 8.0;
    for places in 0..=15 {
        let fixed = format!("{number:.places$}");
        let canonical = if fixed.contains('.') {
            fixed
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        } else {
            fixed
        };
        if canonical.len() > max_chars {
            continue;
        }
        let parsed = canonical
            .parse::<f64>()
            .map_err(|_| "could not normalize the EDI decimal")?;
        if (parsed - number).abs() <= tolerance {
            return Ok(parsed);
        }
    }
    Err("decimal precision exceeds the declared EDI field length")
}

fn validate_edi_decimal_string(value: &str, max_chars: usize) -> Result<String, &'static str> {
    let value = value.trim();
    if value.is_empty() {
        return Err("decimal lexical value is empty");
    }
    let unsigned = value.strip_prefix(['+', '-']).unwrap_or(value);
    let mut parts = unsigned.split('.');
    let integer = parts.next().unwrap_or_default();
    let fraction = parts.next();
    if parts.next().is_some()
        || (integer.is_empty() && fraction.is_none_or(str::is_empty))
        || !integer.bytes().all(|byte| byte.is_ascii_digit())
        || fraction.is_some_and(|fraction| {
            fraction.is_empty() || !fraction.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err("expected a plain EDI decimal without exponent notation");
    }
    let number = value
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite())
        .ok_or("EDI decimal is outside the finite runtime range")?;
    if value.len() <= max_chars {
        return Ok(value.to_string());
    }

    // Target scalar adaptation can turn a computed float into its shortest
    // round-trip string before this schema-guided EDI formatting pass. Only
    // compact that exact runtime spelling; explicit lexical choices such as
    // leading zeros remain significant and fail rather than being rewritten.
    if number.to_string() != value {
        return Err("decimal lexical value exceeds the declared EDI field length");
    }
    let canonical = canonical_edi_decimal(number, max_chars)?.to_string();
    if canonical.len() > max_chars || canonical.contains(['e', 'E']) {
        return Err("decimal lexical value exceeds the declared EDI field length");
    }
    Ok(canonical)
}

fn compact_date(value: &str, short: bool) -> Result<String, &'static str> {
    let width = if short { 6 } else { 8 };
    if value.len() == width && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Ok(value.to_string());
    }
    let bytes = value.as_bytes();
    if bytes.len() < 10
        || bytes.get(4) != Some(&b'-')
        || bytes.get(7) != Some(&b'-')
        || !bytes[..10]
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 4 | 7) || byte.is_ascii_digit())
        || !valid_timezone(&value[10..])
    {
        return Err("expected YYYY-MM-DD with an optional XML timezone");
    }
    let year = value[..4].parse::<u16>().map_err(|_| "invalid year")?;
    let month = value[5..7].parse::<u8>().map_err(|_| "invalid month")?;
    let day = value[8..10].parse::<u8>().map_err(|_| "invalid day")?;
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let max_day = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return Err("month is outside 01..=12"),
    };
    if day == 0 || day > max_day {
        return Err("day is outside the selected month");
    }
    let compact = format!("{}{}{}", &value[..4], &value[5..7], &value[8..10]);
    Ok(if short {
        compact[2..].to_string()
    } else {
        compact
    })
}

fn compact_time(value: &str, min: usize, max: usize) -> Result<String, &'static str> {
    if (min..=max).contains(&value.len()) && value.bytes().all(|byte| byte.is_ascii_digit()) {
        validate_compact_time(value)?;
        return Ok(value.to_string());
    }
    let bytes = value.as_bytes();
    if bytes.len() < 5
        || bytes.get(2) != Some(&b':')
        || !bytes[..5]
            .iter()
            .enumerate()
            .all(|(index, byte)| index == 2 || byte.is_ascii_digit())
    {
        return Err("expected HH:MM[:SS[.fraction]] with an optional XML timezone");
    }
    let hour = value[..2].parse::<u8>().map_err(|_| "invalid hour")?;
    let minute = value[3..5].parse::<u8>().map_err(|_| "invalid minute")?;
    if hour > 23 || minute > 59 {
        return Err("time is outside 00:00..=23:59");
    }
    let mut cursor = 5;
    let mut second = "00";
    if bytes.get(cursor) == Some(&b':') {
        if bytes.len() < cursor + 3 || !bytes[cursor + 1..cursor + 3].iter().all(u8::is_ascii_digit)
        {
            return Err("time seconds must contain two digits");
        }
        second = &value[cursor + 1..cursor + 3];
        cursor += 3;
    }
    if second.parse::<u8>().map_err(|_| "invalid second")? > 59 {
        return Err("second is outside 00..=59");
    }
    let fraction_start = (bytes.get(cursor) == Some(&b'.')).then_some(cursor + 1);
    if fraction_start.is_some() {
        cursor += 1;
        while bytes.get(cursor).is_some_and(u8::is_ascii_digit) {
            cursor += 1;
        }
    }
    if !valid_timezone(&value[cursor..]) {
        return Err("time has an invalid XML timezone");
    }
    let fraction = fraction_start.map_or("", |start| &value[start..cursor]);
    let mut compact = format!("{}{}{}", &value[..2], &value[3..5], second);
    if max == 4 {
        if second != "00" || fraction.bytes().any(|byte| byte != b'0') {
            return Err("declared four-digit time cannot represent seconds");
        }
        compact.truncate(4);
    } else {
        let capacity = max.saturating_sub(6);
        let retained = fraction
            .get(..fraction.len().min(capacity))
            .unwrap_or_default();
        if fraction[retained.len()..].bytes().any(|byte| byte != b'0') {
            return Err("fractional seconds exceed the declared EDI precision");
        }
        compact.push_str(retained);
    }
    while compact.len() < min {
        compact.push('0');
    }
    Ok(compact)
}

fn validate_compact_time(value: &str) -> Result<(), &'static str> {
    if value.len() < 4 {
        return Err("compact time is shorter than HHMM");
    }
    let hour = value[..2].parse::<u8>().map_err(|_| "invalid hour")?;
    let minute = value[2..4].parse::<u8>().map_err(|_| "invalid minute")?;
    if hour > 23 || minute > 59 {
        return Err("time is outside 00:00..=23:59");
    }
    if value.len() >= 6 && value[4..6].parse::<u8>().map_err(|_| "invalid second")? > 59 {
        return Err("second is outside 00..=59");
    }
    Ok(())
}

fn valid_timezone(value: &str) -> bool {
    if value.is_empty() || value == "Z" {
        return true;
    }
    let bytes = value.as_bytes();
    if bytes.len() != 6
        || !matches!(bytes.first(), Some(b'+') | Some(b'-'))
        || bytes.get(3) != Some(&b':')
        || !bytes[1..3].iter().all(u8::is_ascii_digit)
        || !bytes[4..6].iter().all(u8::is_ascii_digit)
    {
        return false;
    }
    let hour = value[1..3].parse::<u8>().ok();
    let minute = value[4..6].parse::<u8>().ok();
    matches!((hour, minute), (Some(0..=14), Some(0..=59)))
        && !(hour == Some(14) && minute != Some(0))
}

pub(crate) fn read_bounded_input(
    path: &Path,
    too_large: EdiFormatError,
) -> Result<Vec<u8>, EdiFormatError> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((MAX_RUNTIME_INPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_RUNTIME_INPUT_BYTES {
        return Err(too_large);
    }
    Ok(bytes)
}

/// The EDI dialect a schema describes, decided by its first trigger
/// segment: `ISA` means X12, while `UNB` or standalone `UNH` means EDIFACT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    X12,
    Edifact,
    Hl7,
    Tradacoms,
}

pub fn dialect_of(schema: &SchemaNode) -> Result<Dialect, EdiFormatError> {
    if matches!(schema.name.as_str(), "MFD-X12" | "MFD-EDIFACT") {
        return Err(EdiFormatError::UnsupportedSchema(
            "an MFD entry-tree schema has no reliable element positions; supply a complete EDI schema before execution"
                .to_string(),
        ));
    }
    match segments::root_trigger(schema)? {
        "ISA" => Ok(Dialect::X12),
        "UNB" | "UNH" => Ok(Dialect::Edifact),
        "FHS" | "BHS" | "MSH" => Ok(Dialect::Hl7),
        "STX" => Ok(Dialect::Tradacoms),
        other => Err(EdiFormatError::UnsupportedSchema(format!(
            "schema must start with ISA (X12), UNB/UNH (EDIFACT), an HL7 header, or STX (TRADACOMS), found `{other}`"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_roots_fail_while_nested_composites_select_the_dialect() {
        let scalar_root = SchemaNode::scalar("ISA", ScalarType::String);
        assert!(matches!(
            dialect_of(&scalar_root),
            Err(EdiFormatError::UnsupportedSchema(_))
        ));

        let nested_composite = SchemaNode::group(
            "X12",
            vec![SchemaNode::group(
                "ISA",
                vec![SchemaNode::group(
                    "element",
                    vec![SchemaNode::group(
                        "nested",
                        vec![SchemaNode::scalar("value", ScalarType::String)],
                    )],
                )],
            )],
        );
        assert_eq!(dialect_of(&nested_composite).unwrap(), Dialect::X12);
    }

    #[test]
    fn importer_entry_tree_schemas_are_graph_only_until_repaired() {
        let x12 = SchemaNode::group(
            "MFD-X12",
            vec![SchemaNode::group(
                "Message",
                vec![SchemaNode::group(
                    "BEG",
                    vec![SchemaNode::scalar("01", ScalarType::String)],
                )],
            )],
        );
        let edifact = SchemaNode::group(
            "MFD-EDIFACT",
            vec![SchemaNode::group(
                "Message",
                vec![SchemaNode::group(
                    "BGM",
                    vec![SchemaNode::scalar("1004", ScalarType::String)],
                )],
            )],
        );

        for schema in [&x12, &edifact] {
            assert!(matches!(
                dialect_of(schema),
                Err(EdiFormatError::UnsupportedSchema(message))
                    if message.contains("no reliable element positions")
            ));
        }
    }

    #[test]
    fn implied_decimals_scale_every_repeated_value_once() {
        let mut instance = Instance::Group(vec![(
            "Rows".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![(
                    "Amount".into(),
                    Instance::Scalar(Value::Float(72_345.0)),
                )]),
                Instance::Group(vec![("Amount".into(), Instance::Scalar(Value::Null))]),
            ]),
        )]);
        let format = EdiImpliedDecimal::new(vec!["Rows".into(), "Amount".into()], 3).unwrap();

        apply_implied_decimals(&mut instance, std::slice::from_ref(&format)).unwrap();
        assert_eq!(
            instance
                .field("Rows")
                .and_then(Instance::as_repeated)
                .and_then(|rows| rows[0].field("Amount"))
                .and_then(Instance::as_scalar),
            Some(&Value::Float(72.345))
        );
        assert!(apply_implied_decimals(&mut instance, &[format.clone(), format]).is_err());
    }

    #[test]
    fn output_lexical_formats_compact_dates_and_times_across_repetition() {
        let mut instance = Instance::Group(vec![(
            "Rows".into(),
            Instance::Repeated(vec![Instance::Group(vec![
                (
                    "Date".into(),
                    Instance::Scalar(Value::String("2004-04-30-09:00".into())),
                ),
                (
                    "Time".into(),
                    Instance::Scalar(Value::String("17:42:00.120-09:00".into())),
                ),
                (
                    "ShortDate".into(),
                    Instance::Scalar(Value::String("2026-07-18Z".into())),
                ),
                (
                    "ShortTime".into(),
                    Instance::Scalar(Value::String("12:34:00Z".into())),
                ),
                (
                    "Decimal".into(),
                    Instance::Scalar(Value::Float(1.35_f64 / 7.5_f64)),
                ),
                (
                    "DecimalCode".into(),
                    Instance::Scalar(Value::String("01".into())),
                ),
                (
                    "DecimalFraction".into(),
                    Instance::Scalar(Value::String(".09".into())),
                ),
                (
                    "DecimalArtifact".into(),
                    Instance::Scalar(Value::String("0.18000000000000002".into())),
                ),
            ])]),
        )]);
        let formats = [
            EdiLexicalFormat::new(
                vec!["Rows".into(), "Date".into()],
                EdiLexicalKind::CompactDate8,
            )
            .unwrap(),
            EdiLexicalFormat::new(
                vec!["Rows".into(), "Decimal".into()],
                EdiLexicalKind::Decimal { max_chars: 10 },
            )
            .unwrap(),
            EdiLexicalFormat::new(
                vec!["Rows".into(), "DecimalCode".into()],
                EdiLexicalKind::Decimal { max_chars: 2 },
            )
            .unwrap(),
            EdiLexicalFormat::new(
                vec!["Rows".into(), "DecimalFraction".into()],
                EdiLexicalKind::Decimal { max_chars: 3 },
            )
            .unwrap(),
            EdiLexicalFormat::new(
                vec!["Rows".into(), "DecimalArtifact".into()],
                EdiLexicalKind::Decimal { max_chars: 10 },
            )
            .unwrap(),
            EdiLexicalFormat::new(
                vec!["Rows".into(), "ShortDate".into()],
                EdiLexicalKind::CompactDate6,
            )
            .unwrap(),
            EdiLexicalFormat::new(
                vec!["Rows".into(), "ShortTime".into()],
                EdiLexicalKind::CompactTime {
                    min_digits: 4,
                    max_digits: 4,
                },
            )
            .unwrap(),
            EdiLexicalFormat::new(
                vec!["Rows".into(), "Time".into()],
                EdiLexicalKind::CompactTime {
                    min_digits: 4,
                    max_digits: 8,
                },
            )
            .unwrap(),
        ];

        apply_output_lexical_formats(&mut instance, &formats).unwrap();

        let row = &instance
            .field("Rows")
            .and_then(Instance::as_repeated)
            .unwrap()[0];
        assert_eq!(
            row.field("Date").and_then(Instance::as_scalar),
            Some(&Value::String("20040430".into()))
        );
        assert_eq!(
            row.field("Time").and_then(Instance::as_scalar),
            Some(&Value::String("17420012".into()))
        );
        assert_eq!(
            row.field("ShortDate").and_then(Instance::as_scalar),
            Some(&Value::String("260718".into()))
        );
        assert_eq!(
            row.field("ShortTime").and_then(Instance::as_scalar),
            Some(&Value::String("1234".into()))
        );
        assert_eq!(
            row.field("Decimal").and_then(Instance::as_scalar),
            Some(&Value::Float(0.18))
        );
        assert_eq!(
            row.field("DecimalCode").and_then(Instance::as_scalar),
            Some(&Value::String("01".into()))
        );
        assert_eq!(
            row.field("DecimalFraction").and_then(Instance::as_scalar),
            Some(&Value::String(".09".into()))
        );
        assert_eq!(
            row.field("DecimalArtifact").and_then(Instance::as_scalar),
            Some(&Value::String("0.18".into()))
        );
    }

    #[test]
    fn output_lexical_formats_reject_lossy_or_duplicate_formats() {
        let format = EdiLexicalFormat::new(
            vec!["Time".into()],
            EdiLexicalKind::CompactTime {
                min_digits: 4,
                max_digits: 4,
            },
        )
        .unwrap();
        let mut lossy = Instance::Group(vec![(
            "Time".into(),
            Instance::Scalar(Value::String("17:42:01".into())),
        )]);
        assert!(matches!(
            apply_output_lexical_formats(&mut lossy, std::slice::from_ref(&format)),
            Err(EdiFormatError::LexicalFormatValue { .. })
        ));
        let mut valid = Instance::Group(vec![(
            "Time".into(),
            Instance::Scalar(Value::String("17:42:00".into())),
        )]);
        assert!(matches!(
            apply_output_lexical_formats(&mut valid, &[format.clone(), format]),
            Err(EdiFormatError::InvalidLexicalFormatLayout { .. })
        ));

        let decimal = EdiLexicalFormat::new(
            vec!["Amount".into()],
            EdiLexicalKind::Decimal { max_chars: 4 },
        )
        .unwrap();
        let mut over_precise = Instance::Group(vec![(
            "Amount".into(),
            Instance::Scalar(Value::Float(1.234_567)),
        )]);
        assert!(matches!(
            apply_output_lexical_formats(&mut over_precise, &[decimal]),
            Err(EdiFormatError::LexicalFormatValue { .. })
        ));

        let decimal = EdiLexicalFormat::new(
            vec!["Amount".into()],
            EdiLexicalKind::Decimal { max_chars: 4 },
        )
        .unwrap();
        for lexical in ["1.234567", "00001"] {
            let mut significant_lexical = Instance::Group(vec![(
                "Amount".into(),
                Instance::Scalar(Value::String(lexical.into())),
            )]);
            assert!(matches!(
                apply_output_lexical_formats(
                    &mut significant_lexical,
                    std::slice::from_ref(&decimal)
                ),
                Err(EdiFormatError::LexicalFormatValue { .. })
            ));
            assert_eq!(
                significant_lexical
                    .field("Amount")
                    .and_then(Instance::as_scalar),
                Some(&Value::String(lexical.into()))
            );
        }
    }
}
