//! SWIFT MT output driven by a target's saved format options.
//!
//! The mapped instance carries neither header values nor, usually, an
//! application-header direction, so both come from
//! [`FormatOptions::swift_mt_output`]. Implied decimals are scaled back to
//! their wire values exactly or refused, so the output re-reads to the mapped
//! values through the ordinary SWIFT reader.

use std::collections::BTreeSet;
use std::path::Path;

use ir::{Instance, SchemaKind, SchemaNode, Value};
use mapping::{
    EdiImpliedDecimal, FormatOptions, SwiftCharset, SwiftMtDirection, SwiftMtLayout,
    SwiftMtOutputOptions, SwiftValueExpr,
};

use super::output::{check_shadowed_tag, output_schema, validate_message_layout};
use super::{SwiftHeaders, SwiftWriteError, to_bytes};
use crate::EdiFormatError;

const MESSAGE: &str = "Message";
const APPLICATION_HEADER: &str = "Application Header";
const DIRECTIONS: [&str; 2] = ["Input", "Output"];

fn write_error(error: SwiftWriteError) -> EdiFormatError {
    EdiFormatError::SwiftWrite(Box::new(error))
}

/// Checks a SWIFT MT target's saved options without an instance: the layout
/// and headers are present, every header set parses for the configured
/// direction without repeating an earlier one, the schema has a repeating
/// `Message` group offering that direction, every layout message passes the
/// writer's instance-independent checks, and every implied decimal names a
/// decimal capture. Tags reused by filled fields are checked by
/// [`validate_filled_fields`].
pub fn validate_output(schema: &SchemaNode, options: &FormatOptions) -> Result<(), EdiFormatError> {
    crate::refuse_json_tuple(schema)?;
    let (layout, output) = configured(options)?;
    headers(output)?;
    let (_, message_schema) = output_schema(schema).map_err(write_error)?;
    for message in layout.messages() {
        validate_message_layout(message).map_err(write_error)?;
    }
    let direction = output.direction().group_name();
    let offers_direction = message_schema
        .child(APPLICATION_HEADER)
        .and_then(|header| header.child(direction))
        .is_some_and(|group| {
            !group.repeating
                && matches!(&group.kind, SchemaKind::Group { children, .. } if children.is_empty())
        });
    if !offers_direction {
        return Err(EdiFormatError::SwiftOutputConfiguration(
            "the schema has no empty application-header group for the configured direction",
        ));
    }
    let captures = decimal_captures(layout);
    let mut seen = BTreeSet::new();
    for implied in &options.edi_implied_decimals {
        if !seen.insert(implied.path()) {
            return Err(EdiFormatError::InvalidImpliedDecimalLayout {
                path: implied.path().join("/"),
                reason: "duplicate path",
            });
        }
        if !captures.contains(implied.path()) {
            return Err(EdiFormatError::SwiftImpliedDecimalLayout {
                path: implied.path().join("/"),
            });
        }
    }
    Ok(())
}

/// Renders `instance` with the target's layout, direction and headers.
///
/// Each message's `Application Header` may be absent, empty, or name the
/// configured direction (alone or beside the other one, as a mapping that
/// constructs both empty groups does); it is rewritten to the configured
/// direction. A message that names only the other direction is refused.
/// Anything else is left for the writer to refuse.
pub fn to_bytes_with_options(
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
) -> Result<Vec<u8>, EdiFormatError> {
    validate_output(schema, options)?;
    let (layout, output) = configured(options)?;
    let headers = headers(output)?;
    let mut prepared = instance.clone();
    select_direction(&mut prepared, output.direction())?;
    for implied in &options.edi_implied_decimals {
        scale_implied_decimal(&mut prepared, implied.path(), implied)?;
    }
    to_bytes(schema, &prepared, layout, &headers).map_err(write_error)
}

/// [`to_bytes_with_options`], then writes `path`. A refused instance never
/// replaces an existing destination.
pub fn write_with_options(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
    options: &FormatOptions,
) -> Result<(), EdiFormatError> {
    let bytes = to_bytes_with_options(schema, instance, options)?;
    std::fs::write(path, bytes)?;
    Ok(())
}

/// Refuses filled layout fields that the reader would read as an earlier
/// field with the same tag, as the writer does. `filled` says whether the
/// target mapping fills the field at a `Message`-relative layout path; a
/// layout that reuses a tag stays valid while only its first field is filled.
pub fn validate_filled_fields(
    layout: &SwiftMtLayout,
    filled: impl Fn(&[String]) -> bool,
) -> Result<(), EdiFormatError> {
    for message in layout.messages() {
        for (position, field) in message.fields().iter().enumerate() {
            if filled(field.path()) {
                check_shadowed_tag(message.fields(), position).map_err(write_error)?;
            }
        }
    }
    Ok(())
}

fn configured(
    options: &FormatOptions,
) -> Result<(&SwiftMtLayout, &SwiftMtOutputOptions), EdiFormatError> {
    let layout = options
        .swift_mt
        .as_ref()
        .ok_or(EdiFormatError::SwiftOutputConfiguration(
            "the target has no embedded `swift_mt` layout",
        ))?;
    let output =
        options
            .swift_mt_output
            .as_ref()
            .ok_or(EdiFormatError::SwiftOutputConfiguration(
                "the target has no `swift_mt_output` message headers",
            ))?;
    Ok((layout, output))
}

fn headers(output: &SwiftMtOutputOptions) -> Result<Vec<SwiftHeaders>, EdiFormatError> {
    let (parsed, refusals) = parse_headers(output);
    match refusals.into_iter().next() {
        Some((_, refusal)) => Err(write_error(refusal)),
        None => Ok(parsed),
    }
}

/// Every saved header set the writer would refuse, with its one-based
/// message number: values outside their FIN positions for the configured
/// direction, and basic headers or output message input references that
/// repeat an earlier message's.
pub fn header_refusals(output: &SwiftMtOutputOptions) -> Vec<(usize, SwiftWriteError)> {
    parse_headers(output).1
}

fn parse_headers(
    output: &SwiftMtOutputOptions,
) -> (Vec<SwiftHeaders>, Vec<(usize, SwiftWriteError)>) {
    let mut basic_headers = BTreeSet::new();
    let mut input_references = BTreeSet::new();
    let mut parsed = Vec::with_capacity(output.messages().len());
    let mut refusals = Vec::new();
    for (index, message) in output.messages().iter().enumerate() {
        let number = index + 1;
        let headers = SwiftHeaders::new(message.basic_header.as_str()).and_then(|headers| {
            match output.direction() {
                SwiftMtDirection::Input => headers.with_input(message.application_header.as_str()),
                SwiftMtDirection::Output => {
                    headers.with_output(message.application_header.as_str())
                }
            }
        });
        let headers = match headers {
            Ok(headers) => headers,
            Err(refusal) => {
                refusals.push((number, refusal));
                continue;
            }
        };
        // The writer refuses these repeats too; finding them here reports a
        // saved configuration that can never be written.
        let repeated = if !basic_headers.insert(headers.basic_header().to_string()) {
            Some("basic header")
        } else if let Some(reference) = headers.message_input_reference()
            && output.direction() == SwiftMtDirection::Output
            && !input_references.insert(reference.to_string())
        {
            Some("message input reference")
        } else {
            None
        };
        if let Some(block) = repeated {
            refusals.push((
                number,
                SwiftWriteError::RepeatedHeader {
                    message: number,
                    block,
                },
            ));
        }
        parsed.push(headers);
    }
    (parsed, refusals)
}

fn select_direction(
    instance: &mut Instance,
    direction: SwiftMtDirection,
) -> Result<(), EdiFormatError> {
    let configured = direction.group_name();
    let Instance::Group(root) = instance else {
        return Ok(());
    };
    let Some((_, Instance::Repeated(messages))) = root.iter_mut().find(|(name, _)| name == MESSAGE)
    else {
        return Ok(());
    };
    let selected = || -> Instance {
        Instance::Group(vec![(configured.to_string(), Instance::Group(Vec::new().into()))].into())
    };
    for (index, message) in messages.iter_mut().enumerate() {
        let Instance::Group(fields) = message else {
            continue;
        };
        let Some((_, header)) = fields
            .iter_mut()
            .find(|(name, _)| name == APPLICATION_HEADER)
        else {
            fields.insert(0, (APPLICATION_HEADER.to_string(), selected()));
            continue;
        };
        let Instance::Group(groups) = header else {
            continue;
        };
        let names = groups
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        let only_directions = groups.iter().all(|(name, value)| {
            DIRECTIONS.contains(&name.as_str())
                && matches!(value, Instance::Group(group) if group.is_empty())
        });
        let distinct = names.iter().collect::<BTreeSet<_>>().len() == names.len();
        if !only_directions || !distinct {
            continue;
        }
        if !names.is_empty() && !names.contains(&configured) {
            let mapped = DIRECTIONS
                .into_iter()
                .find(|direction| *direction != configured)
                .unwrap_or(configured);
            return Err(EdiFormatError::SwiftDirection {
                message: index + 1,
                configured,
                mapped,
            });
        }
        *header = selected();
    }
    Ok(())
}

/// Absolute instance paths of every decimal capture in the layout.
fn decimal_captures(layout: &SwiftMtLayout) -> BTreeSet<Vec<String>> {
    fn visit(expression: &SwiftValueExpr, base: &[String], found: &mut BTreeSet<Vec<String>>) {
        match expression {
            SwiftValueExpr::Capture {
                path,
                charset: SwiftCharset::Decimal,
                ..
            } => {
                found.insert(base.iter().chain(path).cloned().collect());
            }
            SwiftValueExpr::Sequence { parts: children }
            | SwiftValueExpr::Alternatives { choices: children } => {
                for child in children {
                    visit(child, base, found);
                }
            }
            SwiftValueExpr::Optional { value } => visit(value, base, found),
            SwiftValueExpr::Capture { .. }
            | SwiftValueExpr::Empty
            | SwiftValueExpr::Literal { .. }
            | SwiftValueExpr::EnumCapture { .. } => {}
        }
    }
    let mut found = BTreeSet::new();
    for message in layout.messages() {
        for field in message.fields() {
            let base = std::iter::once(MESSAGE.to_string())
                .chain(field.path().iter().cloned())
                .collect::<Vec<_>>();
            visit(field.value(), &base, &mut found);
        }
    }
    found
}

/// The inverse of `apply_implied_decimal`, over the same paths.
fn scale_implied_decimal(
    instance: &mut Instance,
    remaining: &[String],
    implied: &EdiImpliedDecimal,
) -> Result<(), EdiFormatError> {
    let full_path = || implied.path().join("/");
    match instance {
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            for item in items {
                scale_implied_decimal(item, remaining, implied)?;
            }
            Ok(())
        }
        Instance::Group(fields) => {
            let Some((segment, tail)) = remaining.split_first() else {
                return Err(EdiFormatError::InvalidImpliedDecimalLayout {
                    path: full_path(),
                    reason: "path ends at a group",
                });
            };
            match fields.iter_mut().find(|(name, _)| name == segment) {
                Some((_, value)) => scale_implied_decimal(value, tail, implied),
                None => Ok(()),
            }
        }
        Instance::Scalar(value) if remaining.is_empty() => {
            if let Some(scaled) = scaled_wire_value(value, implied.places(), &full_path)? {
                *value = Value::String(scaled);
            }
            Ok(())
        }
        Instance::Scalar(_) => Err(EdiFormatError::InvalidImpliedDecimalLayout {
            path: full_path(),
            reason: "path crosses a scalar",
        }),
        Instance::DocumentSet(_) => Err(EdiFormatError::InvalidImpliedDecimalLayout {
            path: full_path(),
            reason: "path crosses a document set",
        }),
    }
}

/// The plain wire decimal whose implied-decimal reading is exactly `value`,
/// or `None` for an absent value.
fn scaled_wire_value(
    value: &Value,
    places: u8,
    path: &impl Fn() -> String,
) -> Result<Option<String>, EdiFormatError> {
    let (text, expected) = match value {
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => return Ok(None),
        Value::String(text) if text.is_empty() => return Ok(None),
        // The reader never produces negative zero.
        Value::Float(number) if *number == 0.0 => ("0".to_string(), 0.0),
        Value::Float(number) => (number.to_string(), *number),
        Value::Int(number) => (number.to_string(), *number as f64),
        Value::Decimal(number) => {
            let text = number.to_string();
            let expected = text.parse::<f64>().unwrap_or(f64::NAN);
            (text, expected)
        }
        Value::String(text) => {
            let expected = text.replace(',', ".").parse::<f64>().unwrap_or(f64::NAN);
            (text.clone(), expected)
        }
        other => {
            return Err(EdiFormatError::ImpliedDecimalValue {
                path: path(),
                got: other.type_name(),
            });
        }
    };
    let refused = || EdiFormatError::SwiftImpliedDecimalValue {
        path: path(),
        value: text.clone(),
        places,
    };
    let scaled = shift_decimal(&text, places).ok_or_else(refused)?;
    // Mirrors the reader: parse the wire value, then divide by the scale.
    let reread = scaled
        .parse::<f64>()
        .map(|wire| wire / 10_f64.powi(i32::from(places)))
        .map_err(|_| refused())?;
    if reread != expected {
        return Err(refused());
    }
    Ok(Some(scaled))
}

/// Moves the decimal point of an unsigned plain decimal `places` digits to
/// the right, without rounding.
pub(super) fn shift_decimal(text: &str, places: u8) -> Option<String> {
    let (whole, fraction) = match text.find(['.', ',']) {
        Some(index) => (&text[..index], &text[index + 1..]),
        None => (text, ""),
    };
    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return None;
    }
    let places = usize::from(places);
    let moved = fraction.get(..places).unwrap_or(fraction);
    let mut digits = format!("{whole}{moved}{}", "0".repeat(places - moved.len()));
    let leading = digits.len() - digits.trim_start_matches('0').len();
    digits.drain(..leading.min(digits.len() - 1));
    match fraction.get(places..).filter(|rest| !rest.is_empty()) {
        Some(rest) => Some(format!("{digits}.{rest}")),
        None => Some(digits),
    }
}
