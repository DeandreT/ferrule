//! Lossless, bounded schema descriptors shared by generated mapping hosts.
//!
//! Ordinary descriptors retain the existing `SchemaNode` JSON format. The
//! shared parser correctly rounds finite binary64 metadata. The encoder checks
//! complete schema equality and canonical text before choosing ordinary JSON.
//! Versioned descriptors remain supported for exact-bit metadata: their JSON
//! tree keeps the ordinary schema shape while the three known `FiniteF64`
//! metadata families become bit strings. This adds no JSON container depth and
//! never changes ordinary string fields.

use ir::{SchemaKind, SchemaNode};
use serde_json::{Number, Value};
use std::fmt;

/// Prefix of the lossless descriptor. The rest of the descriptor is one JSON
/// schema tree with exact floating-point marker strings at typed positions.
pub const V2_PREFIX: &str = "FERRULE-EMBEDDED-SCHEMA/2\n";
/// Marker used only at schema positions whose IR type is `FiniteF64`.
pub const FLOAT_BITS_MARKER_PREFIX: &str = "FERRULE-F64-BITS:";
const VERSION_STEM: &str = "FERRULE-EMBEDDED-SCHEMA/";
// Every nested schema introduces at least one JSON container. A schema with
// more levels than serde_json's default 128-container limit could never have
// been read from a v1 descriptor. This guard also keeps serialization and the
// marked-value walk from recursing through an unbounded constructed tree.
const MAX_SCHEMA_NODE_DEPTH: usize = 128;

/// Failure to encode or read a bounded, lossless generated schema descriptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodecError {
    Serialization(String),
    Deserialization(String),
    InvalidShape(String),
    InvalidFloatMarker(String),
    UnsupportedVersion,
    MetadataChanged,
    DepthLimit { depth: usize, max: usize },
    TooLarge { bytes: usize, max: usize },
}

impl fmt::Display for CodecError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialization(message) => {
                write!(formatter, "cannot serialize embedded schema: {message}")
            }
            Self::Deserialization(message) => {
                write!(formatter, "cannot read embedded schema: {message}")
            }
            Self::InvalidShape(path) => {
                write!(formatter, "invalid embedded schema shape at {path}")
            }
            Self::InvalidFloatMarker(path) => {
                write!(formatter, "invalid finite-float marker at {path}")
            }
            Self::UnsupportedVersion => {
                formatter.write_str("unsupported embedded schema descriptor version")
            }
            Self::MetadataChanged => {
                formatter.write_str("embedded schema metadata changed during decoding")
            }
            Self::DepthLimit { depth, max } => {
                write!(
                    formatter,
                    "embedded schema depth is {depth}; maximum is {max}"
                )
            }
            Self::TooLarge { bytes, max } => {
                write!(
                    formatter,
                    "embedded schema is {bytes} bytes; maximum is {max}"
                )
            }
        }
    }
}

impl std::error::Error for CodecError {}

/// Encode one schema. Stable schemas keep their byte-for-byte v1 JSON form;
/// only schemas changed by default JSON parsing receive the v2 descriptor.
pub fn encode(schema: &SchemaNode, max_bytes: usize) -> Result<String, CodecError> {
    preflight_depth(schema)?;
    let plain = serde_json::to_string(schema)
        .map_err(|error| CodecError::Serialization(error.to_string()))?;
    check_size(plain.len(), max_bytes)?;
    if let Ok(decoded) = serde_json::from_str::<SchemaNode>(&plain) {
        let canonical = serde_json::to_string(&decoded)
            .map_err(|error| CodecError::Serialization(error.to_string()))?;
        // SchemaNode equality alone cannot distinguish the signs of zero.
        if decoded == *schema && canonical == plain {
            return Ok(plain);
        }
    }

    let payload = marked_value(schema)?;
    let payload = serde_json::to_string(&payload)
        .map_err(|error| CodecError::Serialization(error.to_string()))?;
    let mut descriptor = String::with_capacity(V2_PREFIX.len() + payload.len());
    descriptor.push_str(V2_PREFIX);
    descriptor.push_str(&payload);
    check_size(descriptor.len(), max_bytes)?;

    // The encoded descriptor must round-trip through exactly the public
    // decoder before any generated artifact is emitted.
    let decoded = decode(&descriptor, max_bytes)?;
    if decoded != *schema || marked_value(&decoded)? != marked_value(schema)? {
        return Err(CodecError::MetadataChanged);
    }
    Ok(descriptor)
}

/// Decode v1 JSON or an exact v2 descriptor under one UTF-8 byte limit.
pub fn decode(descriptor: &str, max_bytes: usize) -> Result<SchemaNode, CodecError> {
    check_size(descriptor.len(), max_bytes)?;
    if let Some(payload) = descriptor.strip_prefix(V2_PREFIX) {
        let mut value: Value = serde_json::from_str(payload)
            .map_err(|error| CodecError::Deserialization(error.to_string()))?;
        walk_schema(&mut value, "$", Direction::Decode)?;
        // Keep the legacy SchemaNode decoder's treatment of ordinary JSON:
        // whitespace, field order, and ignored fields are not semantic. Only
        // typed floating metadata markers have a stricter v2 representation.
        return serde_json::from_value(value)
            .map_err(|error| CodecError::Deserialization(error.to_string()));
    }
    if descriptor.starts_with(VERSION_STEM) {
        return Err(CodecError::UnsupportedVersion);
    }
    serde_json::from_str(descriptor).map_err(|error| CodecError::Deserialization(error.to_string()))
}

fn check_size(bytes: usize, max: usize) -> Result<(), CodecError> {
    if bytes > max {
        Err(CodecError::TooLarge { bytes, max })
    } else {
        Ok(())
    }
}

fn preflight_depth(schema: &SchemaNode) -> Result<(), CodecError> {
    let mut pending = vec![(schema, 1_usize)];
    while let Some((node, depth)) = pending.pop() {
        if depth > MAX_SCHEMA_NODE_DEPTH {
            return Err(CodecError::DepthLimit {
                depth,
                max: MAX_SCHEMA_NODE_DEPTH,
            });
        }
        let next = depth + 1;
        if let SchemaKind::Group {
            children, dynamic, ..
        } = &node.kind
        {
            pending.extend(children.iter().map(|child| (child, next)));
            if let Some(dynamic) = dynamic {
                pending.push((dynamic, next));
            }
        }
        if let Some(constraints) = &node.json_contains {
            pending.extend(
                constraints
                    .as_slice()
                    .iter()
                    .filter_map(|constraint| constraint.predicate().as_schema())
                    .map(|schema| (schema, next)),
            );
        }
        if let Some(constraints) = &node.json_dependent_schemas {
            pending.extend(
                constraints
                    .as_slice()
                    .iter()
                    .filter_map(|constraint| constraint.predicate().as_schema())
                    .map(|schema| (schema, next)),
            );
        }
    }
    Ok(())
}

fn marked_value(schema: &SchemaNode) -> Result<Value, CodecError> {
    // `to_value` retains the original binary64 payloads without a text parse.
    let mut value = serde_json::to_value(schema)
        .map_err(|error| CodecError::Serialization(error.to_string()))?;
    walk_schema(&mut value, "$", Direction::Encode)?;
    Ok(value)
}

#[derive(Clone, Copy)]
enum Direction {
    Encode,
    Decode,
}

fn object<'a>(
    value: &'a mut Value,
    path: &str,
) -> Result<&'a mut serde_json::Map<String, Value>, CodecError> {
    value
        .as_object_mut()
        .ok_or_else(|| CodecError::InvalidShape(path.into()))
}

fn array<'a>(value: &'a mut Value, path: &str) -> Result<&'a mut Vec<Value>, CodecError> {
    value
        .as_array_mut()
        .ok_or_else(|| CodecError::InvalidShape(path.into()))
}

fn field<'a>(
    value: &'a mut serde_json::Map<String, Value>,
    name: &str,
    path: &str,
) -> Result<&'a mut Value, CodecError> {
    value
        .get_mut(name)
        .ok_or_else(|| CodecError::InvalidShape(format!("{path}.{name}")))
}

fn tag_is(value: &serde_json::Map<String, Value>, name: &str, expected: &str) -> bool {
    value.get(name).and_then(Value::as_str) == Some(expected)
}

fn walk_float(value: &mut Value, path: &str, direction: Direction) -> Result<(), CodecError> {
    match direction {
        Direction::Encode => {
            let number = value
                .as_number()
                .ok_or_else(|| CodecError::InvalidShape(path.into()))?;
            let bits = number
                .as_f64()
                .ok_or_else(|| CodecError::InvalidShape(path.into()))?
                .to_bits();
            *value = Value::String(format!("{FLOAT_BITS_MARKER_PREFIX}{bits:016x}"));
        }
        Direction::Decode => {
            let marker = value
                .as_str()
                .ok_or_else(|| CodecError::InvalidFloatMarker(path.into()))?;
            let hex = marker
                .strip_prefix(FLOAT_BITS_MARKER_PREFIX)
                .ok_or_else(|| CodecError::InvalidFloatMarker(path.into()))?;
            if hex.len() != 16
                || !hex
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(CodecError::InvalidFloatMarker(path.into()));
            }
            let bits = u64::from_str_radix(hex, 16)
                .map_err(|_| CodecError::InvalidFloatMarker(path.into()))?;
            let number = Number::from_f64(f64::from_bits(bits))
                .ok_or_else(|| CodecError::InvalidFloatMarker(path.into()))?;
            *value = Value::Number(number);
        }
    }
    Ok(())
}

fn walk_schema(value: &mut Value, path: &str, direction: Direction) -> Result<(), CodecError> {
    let node = object(value, path)?;
    if let Some(range) = node
        .get_mut("numeric_range")
        .filter(|value| !value.is_null())
    {
        let range_path = format!("{path}.numeric_range");
        let range = object(range, &range_path)?;
        if tag_is(range, "kind", "number") {
            let bounds_path = format!("{range_path}.bounds");
            let bounds = object(field(range, "bounds", &range_path)?, &bounds_path)?;
            for side in ["minimum", "maximum"] {
                if let Some(bound) = bounds.get_mut(side).filter(|value| !value.is_null()) {
                    let bound_path = format!("{bounds_path}.{side}");
                    let bound = object(bound, &bound_path)?;
                    walk_float(
                        field(bound, "value", &bound_path)?,
                        &format!("{bound_path}.value"),
                        direction,
                    )?;
                }
            }
        }
    }
    if let Some(allowed) = node
        .get_mut("json_allowed_values")
        .filter(|value| !value.is_null())
    {
        let allowed_path = format!("{path}.json_allowed_values");
        for (index, entry) in array(allowed, &allowed_path)?.iter_mut().enumerate() {
            let entry_path = format!("{allowed_path}[{index}]");
            let entry = object(entry, &entry_path)?;
            if tag_is(entry, "type", "float") {
                walk_float(
                    field(entry, "value", &entry_path)?,
                    &format!("{entry_path}.value"),
                    direction,
                )?;
            }
        }
    }
    for family in ["json_contains", "json_dependent_schemas"] {
        if let Some(predicates) = node.get_mut(family).filter(|value| !value.is_null()) {
            let family_path = format!("{path}.{family}");
            for (index, entry) in array(predicates, &family_path)?.iter_mut().enumerate() {
                let entry_path = format!("{family_path}[{index}]");
                let entry = object(entry, &entry_path)?;
                let predicate_path = format!("{entry_path}.predicate");
                let predicate = object(field(entry, "predicate", &entry_path)?, &predicate_path)?;
                if tag_is(predicate, "kind", "schema") {
                    walk_schema(
                        field(predicate, "schema", &predicate_path)?,
                        &format!("{predicate_path}.schema"),
                        direction,
                    )?;
                }
            }
        }
    }
    let kind_path = format!("{path}.kind");
    let kind = object(field(node, "kind", path)?, &kind_path)?;
    if tag_is(kind, "kind", "group") {
        let children_path = format!("{kind_path}.children");
        for (index, child) in array(field(kind, "children", &kind_path)?, &children_path)?
            .iter_mut()
            .enumerate()
        {
            walk_schema(child, &format!("{children_path}[{index}]"), direction)?;
        }
        if let Some(dynamic) = kind.get_mut("dynamic").filter(|value| !value.is_null()) {
            walk_schema(dynamic, &format!("{kind_path}.dynamic"), direction)?;
        }
        if let Some(alternatives) = kind.get_mut("alternatives") {
            let alternatives_path = format!("{kind_path}.alternatives");
            for (index, alternative) in array(alternatives, &alternatives_path)?
                .iter_mut()
                .enumerate()
            {
                let alternative_path = format!("{alternatives_path}[{index}]");
                let alternative = object(alternative, &alternative_path)?;
                if let Some(constraints) = alternative.get_mut("constraints") {
                    let constraints_path = format!("{alternative_path}.constraints");
                    for (index, constraint) in array(constraints, &constraints_path)?
                        .iter_mut()
                        .enumerate()
                    {
                        let constraint_path = format!("{constraints_path}[{index}]");
                        let constraint = object(constraint, &constraint_path)?;
                        let value_path = format!("{constraint_path}.value");
                        let value =
                            object(field(constraint, "value", &constraint_path)?, &value_path)?;
                        if tag_is(value, "type", "float") {
                            walk_float(
                                field(value, "value", &value_path)?,
                                &format!("{value_path}.value"),
                                direction,
                            )?;
                        }
                    }
                }
            }
        }
    }
    Ok(())
}
