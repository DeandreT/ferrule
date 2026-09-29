//! JSON Schema import and JSON instance read/write.
//!
//! Shaping rules: a [`SchemaKind::Group`] is a JSON object; a child marked
//! `repeating` holds a JSON array of that child's shape (a missing repeating
//! field reads as empty, matching the XML reader's zero-match behavior);
//! scalars map per [`ScalarType`], with explicit JSON `null` accepted only
//! when the scalar or its object/array container is nullable. Absent nullable
//! containers, explicit nulls, and empty objects/arrays remain distinct.
//! Unconstrained dynamic properties retain arbitrary values as canonical JSON
//! text in the graph's string domain and restore them at the output boundary.
//! A root array of objects can reuse a non-repeating object schema as flat
//! rows; nested arrays still require a repeating child schema.

mod json5_unique;
pub mod json_schema;
mod pattern_runtime;

use std::io::Read;
use std::path::Path;

use ir::{
    GroupAlternativeConstraintValue, GroupAlternativeMode, Instance, ScalarType, ScalarTypeSet,
    SchemaKind, SchemaNode, Value,
};
use thiserror::Error;

use pattern_runtime::PatternRuntime;

pub(crate) const MAX_JSON5_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum JsonFormatError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("JSON5 error: {0}")]
    Json5(#[from] json5::Error),
    #[error("JSON5 nesting exceeds {limit} levels")]
    Json5NestingLimit { limit: usize },
    #[error("JSON5 document exceeds the {limit}-byte input limit")]
    Json5DocumentLimit { limit: usize },
    #[error("JSON5 exact validation exceeds the {limit}-byte document limit")]
    Json5ExactLimit { limit: usize },
    #[error("JSON5 exact validation cannot normalize this document: {reason}")]
    Json5ExactUnsupported { reason: &'static str },
    #[error("`{name}`: expected {expected}, got {got}")]
    Shape {
        name: String,
        expected: &'static str,
        got: &'static str,
    },
    #[error("JSON Schema union `{name}` is not representable: {reason}")]
    UnsupportedSchemaUnion { name: String, reason: String },
    #[error("JSON Schema object `{name}` is not representable: {reason}")]
    UnsupportedSchemaObject { name: String, reason: String },
    #[error("JSON Schema resource `{reference}` from `{base}` cannot be loaded: {reason}")]
    SchemaResource {
        reference: String,
        base: std::path::PathBuf,
        reason: String,
    },
    #[error("JSON Schema resource graph exceeds the {limit} {kind} limit")]
    SchemaResourceLimit { kind: &'static str, limit: usize },
    #[error("object `{name}` matches no declared schema alternative")]
    NoMatchingAlternative { name: String },
    #[error("object `{name}` matches more than one declared schema alternative")]
    AmbiguousAlternative { name: String },
    #[error("object `{object}` contains duplicate property `{property}`")]
    DuplicateProperty { object: String, property: String },
    #[error("object `{object}` requires property `{property}`")]
    MissingRequiredProperty { object: String, property: String },
    #[error(
        "object `{object}` contains trigger property `{trigger}` but requires dependent property `{property}`"
    )]
    MissingDependentProperty {
        object: String,
        trigger: String,
        property: String,
    },
    #[error(
        "object `{object}` contains trigger property `{trigger}` but does not satisfy its dependent schema"
    )]
    DependentSchemaMismatch { object: String, trigger: String },
    #[error("object `{object}` contains property name `{property}` rejected by its schema")]
    InvalidPropertyName { object: String, property: String },
    #[error("closed object `{object}` does not declare property `{property}`")]
    UndeclaredProperty { object: String, property: String },
    #[error(
        "object `{object}` contains dynamic property `{property}` that matches no patternProperties selector"
    )]
    UnmatchedPatternProperty { object: String, property: String },
    #[error("`{name}` requires constant {expected}, got {got}")]
    ConstantMismatch {
        name: String,
        expected: String,
        got: String,
    },
    #[error("`{name}` is not one of its allowed JSON values, got {got}")]
    AllowedValueMismatch { name: String, got: String },
    #[error("`{name}` requires numeric range {range}, got {got}")]
    RangeMismatch {
        name: String,
        range: String,
        got: String,
    },
    #[error("`{name}` requires a value divisible by {divisors}, got {got}")]
    MultipleOfMismatch {
        name: String,
        divisors: String,
        got: String,
    },
    #[error("`{name}` requires {range} array items, got {got}")]
    ItemCountMismatch {
        name: String,
        range: String,
        got: usize,
    },
    #[error("`{name}` requires {range} items matching its contains predicate, got {got}")]
    ContainsCountMismatch {
        name: String,
        range: String,
        got: usize,
    },
    #[error("`{name}` requires {range} object properties, got {got}")]
    PropertyCountMismatch {
        name: String,
        range: String,
        got: usize,
    },
    #[error(
        "`{name}` requires unique array items, but indexes {first_index} and {duplicate_index} are equal"
    )]
    UniqueItemsMismatch {
        name: String,
        first_index: usize,
        duplicate_index: usize,
    },
    #[error("JSON uniqueItems validation for `{name}` exceeds the {max} {resource} limit")]
    UniqueItemsLimit {
        name: String,
        resource: &'static str,
        max: usize,
    },
    #[error("`{name}` requires string length {range}, got {got} Unicode scalar values")]
    StringLengthMismatch {
        name: String,
        range: String,
        got: usize,
    },
    #[error("`{name}` does not match its JSON Schema pattern constraints")]
    PatternMismatch { name: String },
    #[error("JSON pattern metadata is invalid: {reason}")]
    InvalidPatternMetadata { reason: String },
    #[error("JSON allowed-value metadata is invalid: {reason}")]
    InvalidAllowedValuesMetadata { reason: String },
    #[error("JSON multipleOf metadata is invalid: {reason}")]
    InvalidMultipleOfMetadata { reason: String },
    #[error("JSON numeric-range metadata is invalid: {reason}")]
    InvalidNumericRangeMetadata { reason: String },
    #[error("JSON uniqueItems metadata is invalid: {reason}")]
    InvalidUniqueItemsMetadata { reason: String },
    #[error("JSON property-count metadata is invalid: {reason}")]
    InvalidPropertyCountMetadata { reason: String },
    #[error("JSON property-dependency metadata is invalid: {reason}")]
    InvalidPropertyDependenciesMetadata { reason: String },
    #[error("JSON dependent-schema metadata is invalid: {reason}")]
    InvalidDependentSchemasMetadata { reason: String },
    #[error("JSON property-name metadata is invalid: {reason}")]
    InvalidPropertyNameMetadata { reason: String },
    #[error("JSON contains metadata is invalid: {reason}")]
    InvalidContainsMetadata { reason: String },
    #[error("JSON pattern matching for `{name}` exceeds the bounded work limit")]
    PatternWorkLimit { name: String },
    #[error("JSON Lines cannot encode nullable array container `{name}`")]
    NullableJsonLinesContainer { name: String },
}

fn json_type_name(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

fn exact_f64_from_i64(value: i64) -> Option<f64> {
    let converted = value as f64;
    ((converted as i128) == i128::from(value)).then_some(converted)
}

fn exact_f64_from_u64(value: u64) -> Option<f64> {
    let converted = value as f64;
    ((converted as u128) == u128::from(value)).then_some(converted)
}

pub(crate) fn exact_f64_from_json_number(number: &serde_json::Number) -> Option<f64> {
    if let Some(value) = number.as_i64() {
        return exact_f64_from_i64(value);
    }
    if let Some(value) = number.as_u64() {
        return exact_f64_from_u64(value);
    }
    number.as_f64().filter(|value| value.is_finite())
}

fn json_number_matches_f64(number: &serde_json::Number, expected: f64) -> bool {
    if let Some(value) = number.as_i64() {
        return exact_f64_from_i64(value) == Some(expected);
    }
    if let Some(value) = number.as_u64() {
        return exact_f64_from_u64(value) == Some(expected);
    }
    number.as_f64() == Some(expected)
}

/// Reads a JSON file into an [`Instance`] tree shaped by `schema`.
pub fn read(path: &Path, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    let text = std::fs::read_to_string(path)?;
    from_str(&text, schema)
}

/// Reads a JSON5 document using the same typed schema boundary as JSON.
pub fn read_json5(path: &Path, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take((MAX_JSON5_DOCUMENT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_JSON5_DOCUMENT_BYTES {
        return Err(JsonFormatError::Json5DocumentLimit {
            limit: MAX_JSON5_DOCUMENT_BYTES,
        });
    }
    let text = String::from_utf8(bytes).map_err(|error| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, error.utf8_error())
    })?;
    from_json5_str(&text, schema)
}

/// Reads a JSON Lines file into a repeated instance, one item per non-empty
/// line.
pub fn read_lines(path: &Path, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    let text = std::fs::read_to_string(path)?;
    from_lines(&text, schema)
}

/// Reads JSON text into an [`Instance`] tree shaped by `schema`.
///
/// This is the in-memory equivalent of [`read`], suitable for hosts without
/// filesystem access such as WebAssembly applications. A root array under a
/// non-repeating group schema is read as flat rows, matching [`to_string`].
pub fn from_str(text: &str, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    let text = strip_utf8_bom(text);
    json_schema::unique_items::validate_raw_json_unique_items(schema, text)?;
    let value: serde_json::Value = serde_json::from_str(text)?;
    from_value(&value, schema)
}

/// Reads JSON5 text into an [`Instance`] tree. JSON5 comments are syntax only;
/// they are not represented in the mapping graph or reproduced on output.
pub fn from_json5_str(text: &str, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    let text = strip_utf8_bom(text);
    if text.len() > MAX_JSON5_DOCUMENT_BYTES {
        return Err(JsonFormatError::Json5DocumentLimit {
            limit: MAX_JSON5_DOCUMENT_BYTES,
        });
    }
    check_json5_nesting(text)?;
    let value: serde_json::Value = json5::from_str(text)?;
    json5_unique::validate(schema, text)?;
    from_value(&value, schema)
}

fn from_value(value: &serde_json::Value, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    let mut patterns = PatternRuntime::new(schema)?;
    if schema.repeating {
        read_repeated(value, schema, &mut patterns)
    } else if let (SchemaKind::Group { .. }, serde_json::Value::Array(rows)) = (&schema.kind, value)
    {
        // Flat-row sources retain a non-repeating row schema while presenting
        // the root records as one repeated instance, like the JSON writer.
        let mut parsed = Vec::with_capacity(rows.len());
        for row in rows {
            parsed.push(read_node_with_patterns(row, schema, &mut patterns)?);
        }
        Ok(Instance::Repeated(parsed))
    } else {
        read_node_with_patterns(value, schema, &mut patterns)
    }
}

fn check_json5_nesting(text: &str) -> Result<(), JsonFormatError> {
    const MAX_DEPTH: usize = 128;
    let bytes = text.as_bytes();
    let mut index = 0;
    let mut depth = 0usize;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' | b'"' => {
                let quote = bytes[index];
                index += 1;
                while index < bytes.len() {
                    match bytes[index] {
                        b'\\' => index = (index + 2).min(bytes.len()),
                        byte if byte == quote => {
                            index += 1;
                            break;
                        }
                        _ => index += 1,
                    }
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'/') => {
                index += 2;
                while index < bytes.len()
                    && !matches!(bytes[index], b'\n' | b'\r')
                    && !bytes[index..].starts_with("\u{2028}".as_bytes())
                    && !bytes[index..].starts_with("\u{2029}".as_bytes())
                {
                    index += 1;
                }
            }
            b'/' if bytes.get(index + 1) == Some(&b'*') => {
                index += 2;
                while index + 1 < bytes.len() && !(bytes[index] == b'*' && bytes[index + 1] == b'/')
                {
                    index += 1;
                }
                index = (index + 2).min(bytes.len());
            }
            b'{' | b'[' => {
                depth += 1;
                if depth > MAX_DEPTH {
                    return Err(JsonFormatError::Json5NestingLimit { limit: MAX_DEPTH });
                }
                index += 1;
            }
            b'}' | b']' => {
                depth = depth.saturating_sub(1);
                index += 1;
            }
            _ => index += 1,
        }
    }
    Ok(())
}

/// Reads JSON Lines text into a repeated instance.
pub fn from_lines(text: &str, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    reject_nullable_json_lines_container(schema)?;
    let mut patterns = PatternRuntime::new(schema)?;
    let lines = strip_utf8_bom(text)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    json_schema::unique_items::validate_raw_json_lines_unique_items(schema, &lines)?;
    let mut values = Vec::new();
    for line in lines {
        values.push(serde_json::from_str(line)?);
    }
    if schema.repeating {
        json_schema::item_counts::validate_len(schema, values.len())?;
    }
    let mut items = Vec::with_capacity(values.len());
    for value in &values {
        items.push(read_node_with_patterns(value, schema, &mut patterns)?);
    }
    if schema.repeating {
        json_schema::contains::validate_values(schema, &values, &mut patterns)?;
    }
    Ok(Instance::Repeated(items))
}

fn strip_utf8_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn read_repeated(
    value: &serde_json::Value,
    schema: &SchemaNode,
    patterns: &mut PatternRuntime,
) -> Result<Instance, JsonFormatError> {
    if value.is_null() && schema.container_nullable {
        return Ok(Instance::Scalar(Value::json_null()));
    }
    let serde_json::Value::Array(items) = value else {
        return Err(JsonFormatError::Shape {
            name: schema.name.clone(),
            expected: "array",
            got: json_type_name(value),
        });
    };
    json_schema::item_counts::validate_len(schema, items.len())?;
    let mut parsed = Vec::with_capacity(items.len());
    for item in items {
        parsed.push(read_node_with_patterns(item, schema, patterns)?);
    }
    json_schema::contains::validate_values(schema, items, patterns)?;
    let items = parsed;
    Ok(Instance::Repeated(items))
}

#[cfg(test)]
fn read_node(value: &serde_json::Value, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    let mut patterns = PatternRuntime::new(schema)?;
    read_node_with_patterns(value, schema, &mut patterns)
}

fn read_node_with_patterns(
    value: &serde_json::Value,
    schema: &SchemaNode,
    patterns: &mut PatternRuntime,
) -> Result<Instance, JsonFormatError> {
    if schema.json_any {
        return Ok(Instance::Scalar(Value::String(serde_json::to_string(
            value,
        )?)));
    }
    if value.is_null() && schema.container_nullable {
        return Ok(Instance::Scalar(Value::json_null()));
    }
    match &schema.kind {
        SchemaKind::Scalar { ty } => {
            let parsed = read_scalar(value, *ty, schema.nullable, &schema.name)?;
            json_schema::constraints::validate_json(schema, value)?;
            json_schema::allowed_values::validate_value(schema, &parsed)?;
            json_schema::ranges::validate_json(schema, value)?;
            json_schema::multiples::validate_json(schema, value)?;
            json_schema::string_lengths::validate_json(schema, value)?;
            patterns.validate_json(schema, value)?;
            Ok(Instance::Scalar(parsed))
        }
        SchemaKind::ScalarUnion { types } => {
            let parsed = read_scalar_union(value, *types, schema.nullable, &schema.name)?;
            json_schema::allowed_values::validate_value(schema, &parsed)?;
            json_schema::multiples::validate_json(schema, value)?;
            json_schema::string_lengths::validate_json(schema, value)?;
            patterns.validate_json(schema, value)?;
            Ok(Instance::Scalar(parsed))
        }
        SchemaKind::Group {
            children,
            alternatives,
            required,
            dynamic,
            ..
        } => {
            let serde_json::Value::Object(fields) = value else {
                return Err(JsonFormatError::Shape {
                    name: schema.name.clone(),
                    expected: "object",
                    got: json_type_name(value),
                });
            };
            json_schema::property_counts::validate_len(schema, fields.len())?;
            for property in fields.keys() {
                patterns.validate_property_name(schema, property)?;
            }
            validate_required_fields(schema, required, |name| fields.contains_key(name))?;
            json_schema::property_dependencies::validate_properties(
                schema,
                fields.keys().map(String::as_str),
            )?;
            json_schema::dependent_schemas::validate_object(
                schema,
                value,
                fields.keys(),
                patterns,
            )?;
            validate_alternative_fields(schema, alternatives, fields)?;
            if dynamic.is_some() && !alternatives.is_empty() {
                return Err(JsonFormatError::UnsupportedSchemaUnion {
                    name: schema.name.clone(),
                    reason: "open objects cannot use closed object alternatives".to_string(),
                });
            }
            if dynamic.is_none()
                && let Some(property) = fields.keys().find(|name| {
                    !children
                        .iter()
                        .any(|child| child.name.as_str() == name.as_str())
                })
            {
                return Err(JsonFormatError::UndeclaredProperty {
                    object: schema.name.clone(),
                    property: property.clone(),
                });
            }
            if let Some(dynamic) = dynamic {
                let mut out = Vec::with_capacity(fields.len().max(children.len()));
                for (name, field_value) in fields {
                    let field_schema =
                        if let Some(child) = children.iter().find(|child| child.name == *name) {
                            child
                        } else {
                            patterns.validate_dynamic_property_name(schema, name)?;
                            dynamic
                        };
                    let field = if field_schema.repeating {
                        read_repeated(field_value, field_schema, patterns)?
                    } else {
                        read_node_with_patterns(field_value, field_schema, patterns)?
                    };
                    out.push((name.clone(), field));
                }
                for child in children {
                    if !fields.contains_key(&child.name) {
                        out.push((child.name.clone(), missing_instance(child)));
                    }
                }
                return Ok(Instance::Group(out));
            }
            let mut out = Vec::with_capacity(children.len());
            for child in children {
                match fields.get(&child.name) {
                    Some(field_value) if child.repeating => {
                        out.push((
                            child.name.clone(),
                            read_repeated(field_value, child, patterns)?,
                        ));
                    }
                    Some(field_value) => {
                        out.push((
                            child.name.clone(),
                            read_node_with_patterns(field_value, child, patterns)?,
                        ));
                    }
                    None if child.repeating && !child.container_nullable => {
                        out.push((child.name.clone(), Instance::Repeated(Vec::new())));
                    }
                    // Absent properties are normal instance data (JSON
                    // objects routinely omit optional keys), not errors:
                    // scalars read as Null, objects as empty.
                    None => {
                        out.push((child.name.clone(), missing_instance(child)));
                    }
                }
            }
            Ok(Instance::Group(out))
        }
    }
}

fn missing_instance(schema: &SchemaNode) -> Instance {
    if schema.container_nullable {
        return Instance::Scalar(Value::Null);
    }
    if schema.repeating {
        Instance::Repeated(Vec::new())
    } else {
        match schema.kind {
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => {
                Instance::Scalar(Value::Null)
            }
            SchemaKind::Group { .. } => Instance::Group(Vec::new()),
        }
    }
}

fn read_scalar_union(
    value: &serde_json::Value,
    types: ScalarTypeSet,
    nullable: bool,
    name: &str,
) -> Result<Value, JsonFormatError> {
    match value {
        serde_json::Value::Null if nullable => Ok(Value::json_null()),
        serde_json::Value::String(value) if types.contains(ScalarType::String) => {
            Ok(Value::String(value.clone()))
        }
        serde_json::Value::Bool(value) if types.contains(ScalarType::Bool) => {
            Ok(Value::Bool(*value))
        }
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64()
                && types.contains(ScalarType::Int)
            {
                return Ok(Value::Int(value));
            }
            if types.contains(ScalarType::Float) {
                return read_scalar(
                    &serde_json::Value::Number(value.clone()),
                    ScalarType::Float,
                    false,
                    name,
                );
            }
            Err(JsonFormatError::Shape {
                name: name.to_string(),
                expected: "declared scalar union",
                got: "number",
            })
        }
        _ => Err(JsonFormatError::Shape {
            name: name.to_string(),
            expected: "declared scalar union",
            got: json_type_name(value),
        }),
    }
}

fn read_scalar(
    value: &serde_json::Value,
    ty: ScalarType,
    nullable: bool,
    name: &str,
) -> Result<Value, JsonFormatError> {
    let bad = |expected: &'static str| JsonFormatError::Shape {
        name: name.to_string(),
        expected,
        got: json_type_name(value),
    };
    match (ty, value) {
        (_, serde_json::Value::Null) if nullable => Ok(Value::json_null()),
        (ScalarType::String, serde_json::Value::String(s)) => Ok(Value::String(s.clone())),
        (ScalarType::Int, serde_json::Value::Number(n)) => {
            n.as_i64().map(Value::Int).ok_or_else(|| bad("integer"))
        }
        (ScalarType::Float, serde_json::Value::Number(number)) => {
            match exact_f64_from_json_number(number) {
                Some(value) => Ok(Value::Float(value)),
                None if number.as_i64().is_some() || number.as_u64().is_some() => {
                    Err(JsonFormatError::Shape {
                        name: name.to_string(),
                        expected: "number",
                        got: "integer outside the exact f64 range",
                    })
                }
                None => Err(bad("finite number")),
            }
        }
        (ScalarType::Bool, serde_json::Value::Bool(b)) => Ok(Value::Bool(*b)),
        (ScalarType::String, _) => Err(bad("string")),
        (ScalarType::Int, _) => Err(bad("integer")),
        (ScalarType::Float, _) => Err(bad("number")),
        (ScalarType::Bool, _) => Err(bad("bool")),
    }
}

/// Writes an [`Instance`] tree shaped by `schema` to a pretty-printed JSON
/// file.
pub fn write(path: &Path, schema: &SchemaNode, instance: &Instance) -> Result<(), JsonFormatError> {
    std::fs::write(path, to_string(schema, instance)?)?;
    Ok(())
}

/// Writes a JSON5 document, using ordinary JSON schema validation first.
pub fn write_json5(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<(), JsonFormatError> {
    std::fs::write(path, to_json5_string(schema, instance)?)?;
    Ok(())
}

/// Writes a repeated instance as JSON Lines using one compact value per line.
pub fn write_lines(
    path: &Path,
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<(), JsonFormatError> {
    std::fs::write(path, to_lines(schema, instance)?)?;
    Ok(())
}

/// Writes an [`Instance`] tree shaped by `schema` as pretty-printed JSON.
///
/// The returned document ends with a newline, matching [`write`]. This is
/// the in-memory counterpart used by hosts without filesystem access.
pub fn to_string(schema: &SchemaNode, instance: &Instance) -> Result<String, JsonFormatError> {
    let mut patterns = PatternRuntime::new(schema)?;
    // A root scope can produce flat rows even though the row schema itself
    // is not repeating (the same convention used by CSV). Preserve that
    // established JSON-output shape while keeping nested nodes
    // schema-directed.
    let value = match instance {
        Instance::Repeated(items) if !schema.repeating => {
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(write_single_node_with_patterns(
                    schema,
                    item,
                    &mut patterns,
                )?);
            }
            serde_json::Value::Array(values)
        }
        _ => write_node_with_patterns(schema, instance, &mut patterns)?,
    };
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text)
}

/// Serializes a typed instance as JSON5. Safe property names are unquoted;
/// comments are intentionally not generated.
pub fn to_json5_string(
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<String, JsonFormatError> {
    let strict = to_string(schema, instance)?;
    let value: serde_json::Value = serde_json::from_str(&strict)?;
    let mut text = json5::to_string(&value)?;
    text.push('\n');
    Ok(text)
}

/// Serializes an instance as JSON Lines using one compact root value per
/// line. A non-repeated instance becomes a single line.
pub fn to_lines(schema: &SchemaNode, instance: &Instance) -> Result<String, JsonFormatError> {
    reject_nullable_json_lines_container(schema)?;
    let mut patterns = PatternRuntime::new(schema)?;
    let values = match instance {
        Instance::Repeated(items) => {
            if schema.repeating {
                json_schema::item_counts::validate_len(schema, items.len())?;
            }
            let mut values = Vec::with_capacity(items.len());
            for item in items {
                values.push(write_single_node_with_patterns(
                    schema,
                    item,
                    &mut patterns,
                )?);
            }
            values
        }
        item => {
            if schema.repeating {
                json_schema::item_counts::validate_len(schema, 1)?;
            }
            vec![write_single_node_with_patterns(
                schema,
                item,
                &mut patterns,
            )?]
        }
    };
    if schema.repeating {
        json_schema::contains::validate_values(schema, &values, &mut patterns)?;
        json_schema::unique_items::validate(schema, &values)?;
    }
    let mut text = String::new();
    for value in values {
        text.push_str(&serde_json::to_string(&value)?);
        text.push('\n');
    }
    Ok(text)
}

fn reject_nullable_json_lines_container(schema: &SchemaNode) -> Result<(), JsonFormatError> {
    if schema.repeating && schema.container_nullable {
        return Err(JsonFormatError::NullableJsonLinesContainer {
            name: schema.name.clone(),
        });
    }
    Ok(())
}

#[cfg(test)]
fn write_node(
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<serde_json::Value, JsonFormatError> {
    let mut patterns = PatternRuntime::new(schema)?;
    write_node_with_patterns(schema, instance, &mut patterns)
}

fn write_node_with_patterns(
    schema: &SchemaNode,
    instance: &Instance,
    patterns: &mut PatternRuntime,
) -> Result<serde_json::Value, JsonFormatError> {
    if schema.container_nullable && matches!(instance, Instance::Scalar(Value::JsonNull(_))) {
        return Ok(serde_json::Value::Null);
    }
    if schema.repeating {
        let items = match instance {
            Instance::Repeated(items) | Instance::MappedSequence(items) => items,
            _ => {
                return Err(write_shape_error(
                    schema,
                    "array",
                    instance_type_name(instance),
                ));
            }
        };
        json_schema::item_counts::validate_len(schema, items.len())?;
        let mut values = Vec::with_capacity(items.len());
        for item in items {
            values.push(write_single_node_with_patterns(schema, item, patterns)?);
        }
        json_schema::contains::validate_values(schema, &values, patterns)?;
        json_schema::unique_items::validate(schema, &values)?;
        return Ok(serde_json::Value::Array(values));
    }
    write_single_node_with_patterns(schema, instance, patterns)
}

fn write_single_node_with_patterns(
    schema: &SchemaNode,
    instance: &Instance,
    patterns: &mut PatternRuntime,
) -> Result<serde_json::Value, JsonFormatError> {
    let instance = match instance {
        Instance::MappedSequence(items) => match items.as_slice() {
            [item] => item,
            _ => {
                return Err(write_shape_error(
                    schema,
                    "one mapped item",
                    "mapped sequence",
                ));
            }
        },
        item => item,
    };
    if schema.json_any {
        return write_json_any(schema, instance);
    }
    if schema.container_nullable && matches!(instance, Instance::Scalar(Value::JsonNull(_))) {
        return Ok(serde_json::Value::Null);
    }
    match (&schema.kind, instance) {
        (SchemaKind::Scalar { ty }, Instance::Scalar(value)) => {
            let value = write_scalar(value, *ty, schema.nullable, &schema.name)?;
            json_schema::constraints::validate_json(schema, &value)?;
            let normalized = read_scalar(&value, *ty, schema.nullable, &schema.name)?;
            json_schema::allowed_values::validate_value(schema, &normalized)?;
            json_schema::ranges::validate_json(schema, &value)?;
            json_schema::multiples::validate_json(schema, &value)?;
            json_schema::string_lengths::validate_json(schema, &value)?;
            patterns.validate_json(schema, &value)?;
            Ok(value)
        }
        (SchemaKind::ScalarUnion { types }, Instance::Scalar(value)) => {
            let value = write_scalar_union(value, *types, schema.nullable, &schema.name)?;
            let normalized = read_scalar_union(&value, *types, schema.nullable, &schema.name)?;
            json_schema::allowed_values::validate_value(schema, &normalized)?;
            json_schema::multiples::validate_json(schema, &value)?;
            json_schema::string_lengths::validate_json(schema, &value)?;
            patterns.validate_json(schema, &value)?;
            Ok(value)
        }
        (
            SchemaKind::Group {
                children,
                alternatives,
                required,
                dynamic,
                ..
            },
            Instance::Group(fields),
        ) => {
            if dynamic.is_some() && !alternatives.is_empty() {
                return Err(JsonFormatError::UnsupportedSchemaUnion {
                    name: schema.name.clone(),
                    reason: "open objects cannot use closed object alternatives".to_string(),
                });
            }
            let mut out = serde_json::Map::with_capacity(fields.len());
            if let Some(dynamic) = dynamic {
                for (name, child_instance) in fields {
                    if out.contains_key(name) {
                        return Err(JsonFormatError::DuplicateProperty {
                            object: schema.name.clone(),
                            property: name.clone(),
                        });
                    }
                    let (child_schema, is_dynamic) =
                        match children.iter().find(|child| child.name == *name) {
                            Some(child) => (child, false),
                            None => (dynamic.as_ref(), true),
                        };
                    if is_boundary_absence(child_schema, child_instance) {
                        continue;
                    }
                    if is_dynamic {
                        patterns.validate_dynamic_property_name(schema, name)?;
                    }
                    out.insert(
                        name.clone(),
                        write_node_with_patterns(child_schema, child_instance, patterns)?,
                    );
                }
                for property in out.keys() {
                    patterns.validate_property_name(schema, property)?;
                }
                validate_required_fields(schema, required, |name| out.contains_key(name))?;
                json_schema::property_dependencies::validate_properties(
                    schema,
                    out.keys().map(String::as_str),
                )?;
                json_schema::property_counts::validate_len(schema, out.len())?;
                let value = serde_json::Value::Object(out);
                json_schema::dependent_schemas::validate_object(
                    schema,
                    &value,
                    value
                        .as_object()
                        .into_iter()
                        .flat_map(serde_json::Map::keys),
                    patterns,
                )?;
                return Ok(value);
            }
            for child_schema in children {
                if let Some((_, child_instance)) =
                    fields.iter().find(|(n, _)| n == &child_schema.name)
                {
                    // A non-nullable Null scalar is boundary-level absence.
                    // Nullable scalars retain an explicit JSON null.
                    if is_boundary_absence(child_schema, child_instance) {
                        continue;
                    }
                    out.insert(
                        child_schema.name.clone(),
                        write_node_with_patterns(child_schema, child_instance, patterns)?,
                    );
                }
            }
            for property in out.keys() {
                patterns.validate_property_name(schema, property)?;
            }
            validate_required_fields(schema, required, |name| out.contains_key(name))?;
            json_schema::property_dependencies::validate_properties(
                schema,
                out.keys().map(String::as_str),
            )?;
            validate_alternative_fields(schema, alternatives, &out)?;
            json_schema::property_counts::validate_len(schema, out.len())?;
            let value = serde_json::Value::Object(out);
            json_schema::dependent_schemas::validate_object(
                schema,
                &value,
                value
                    .as_object()
                    .into_iter()
                    .flat_map(serde_json::Map::keys),
                patterns,
            )?;
            Ok(value)
        }
        (SchemaKind::Scalar { ty }, other) => Err(write_shape_error(
            schema,
            scalar_type_name(*ty),
            instance_type_name(other),
        )),
        (SchemaKind::ScalarUnion { .. }, other) => Err(write_shape_error(
            schema,
            "declared scalar union",
            instance_type_name(other),
        )),
        (SchemaKind::Group { .. }, other) => Err(write_shape_error(
            schema,
            "object",
            instance_type_name(other),
        )),
    }
}

fn validate_required_fields(
    schema: &SchemaNode,
    required: &[String],
    contains: impl Fn(&str) -> bool,
) -> Result<(), JsonFormatError> {
    if let Some(property) = required.iter().find(|property| !contains(property)) {
        return Err(JsonFormatError::MissingRequiredProperty {
            object: schema.name.clone(),
            property: property.clone(),
        });
    }
    Ok(())
}

fn write_json_any(
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<serde_json::Value, JsonFormatError> {
    let Instance::Scalar(value) = instance else {
        return Err(write_shape_error(
            schema,
            "arbitrary JSON scalar encoding",
            instance_type_name(instance),
        ));
    };
    match value {
        Value::String(value) => Ok(serde_json::from_str(value)
            .unwrap_or_else(|_| serde_json::Value::String(value.clone()))),
        Value::Bool(value) => Ok((*value).into()),
        Value::Int(value) => Ok((*value).into()),
        Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| JsonFormatError::Shape {
                name: schema.name.clone(),
                expected: "finite JSON number",
                got: "non-finite float",
            }),
        Value::JsonNull(_) => Ok(serde_json::Value::Null),
        Value::Null | Value::XmlNil(_) => Err(JsonFormatError::Shape {
            name: schema.name.clone(),
            expected: "arbitrary JSON value",
            got: value.type_name(),
        }),
    }
}

fn is_boundary_absence(schema: &SchemaNode, instance: &Instance) -> bool {
    if let Instance::MappedSequence(items) = instance
        && items.is_empty()
        && !schema.repeating
    {
        return true;
    }
    matches!(instance, Instance::Scalar(Value::Null))
        && (schema.container_nullable || (!schema.repeating && schema.is_scalar()))
        || matches!(instance, Instance::Repeated(items) | Instance::MappedSequence(items) if items.is_empty())
            && schema
                .item_count_range
                .is_some_and(|range| range.minimum() > 0)
}

fn write_scalar_union(
    value: &Value,
    types: ScalarTypeSet,
    nullable: bool,
    name: &str,
) -> Result<serde_json::Value, JsonFormatError> {
    let allowed = match value {
        Value::String(_) => types.contains(ScalarType::String),
        Value::Int(_) => types.contains(ScalarType::Int),
        Value::Float(_) => types.contains(ScalarType::Float),
        Value::Bool(_) => types.contains(ScalarType::Bool),
        Value::JsonNull(_) => nullable,
        Value::Null | Value::XmlNil(_) => false,
    };
    if !allowed {
        if matches!(value, Value::Int(_)) && types.contains(ScalarType::Float) {
            return write_scalar(value, ScalarType::Float, false, name);
        }
        if matches!(value, Value::String(_)) && !types.contains(ScalarType::String) {
            let mut converted = None;
            for ty in [ScalarType::Int, ScalarType::Float, ScalarType::Bool] {
                if types.contains(ty)
                    && let Ok(candidate) = write_scalar(value, ty, false, name)
                {
                    if converted.is_some() {
                        return Err(JsonFormatError::Shape {
                            name: name.to_string(),
                            expected: "unambiguous declared scalar union",
                            got: "string",
                        });
                    }
                    converted = Some(candidate);
                }
            }
            if let Some(converted) = converted {
                return Ok(converted);
            }
        }
        return Err(JsonFormatError::Shape {
            name: name.to_string(),
            expected: "declared scalar union",
            got: value.type_name(),
        });
    }
    match value {
        Value::String(value) => Ok(value.clone().into()),
        Value::Int(value) => Ok((*value).into()),
        Value::Float(value) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .ok_or_else(|| JsonFormatError::Shape {
                name: name.to_string(),
                expected: "finite number",
                got: "non-finite float",
            }),
        Value::Bool(value) => Ok((*value).into()),
        Value::JsonNull(_) => Ok(serde_json::Value::Null),
        Value::Null | Value::XmlNil(_) => Err(JsonFormatError::Shape {
            name: name.to_string(),
            expected: "declared scalar union",
            got: value.type_name(),
        }),
    }
}

fn validate_alternative_fields(
    schema: &SchemaNode,
    alternatives: &[ir::GroupAlternative],
    fields: &serde_json::Map<String, serde_json::Value>,
) -> Result<(), JsonFormatError> {
    if alternatives.is_empty() {
        return Ok(());
    }
    let matches = alternatives
        .iter()
        .filter(|alternative| {
            alternative.required.iter().all(|required| {
                fields.get(required).is_some_and(|value| {
                    !value.is_null() || schema.child(required).is_some_and(|child| child.nullable)
                })
            }) && fields
                .keys()
                .all(|field| alternative.members.iter().any(|member| member == field))
                && alternative.constraints.iter().all(|constraint| {
                    fields
                        .get(&constraint.member)
                        .is_none_or(|value| constraint_matches(&constraint.value, value))
                })
        })
        .count();
    match matches {
        0 => Err(JsonFormatError::NoMatchingAlternative {
            name: schema.name.clone(),
        }),
        1 => Ok(()),
        _ if schema.alternative_mode() == GroupAlternativeMode::Exclusive => {
            Err(JsonFormatError::AmbiguousAlternative {
                name: schema.name.clone(),
            })
        }
        _ => Ok(()),
    }
}

fn constraint_matches(
    expected: &GroupAlternativeConstraintValue,
    actual: &serde_json::Value,
) -> bool {
    match (expected, actual) {
        (GroupAlternativeConstraintValue::String(expected), serde_json::Value::String(actual)) => {
            expected == actual
        }
        (GroupAlternativeConstraintValue::Int(expected), serde_json::Value::Number(actual)) => {
            actual.as_i64() == Some(*expected)
        }
        (GroupAlternativeConstraintValue::Float(expected), serde_json::Value::Number(actual)) => {
            json_number_matches_f64(actual, expected.get())
        }
        (GroupAlternativeConstraintValue::Bool(expected), serde_json::Value::Bool(actual)) => {
            expected == actual
        }
        (GroupAlternativeConstraintValue::JsonNull, serde_json::Value::Null) => true,
        _ => false,
    }
}

fn write_scalar(
    value: &Value,
    ty: ScalarType,
    nullable: bool,
    name: &str,
) -> Result<serde_json::Value, JsonFormatError> {
    if let Value::Float(value) = value
        && !value.is_finite()
    {
        return Err(JsonFormatError::Shape {
            name: name.to_string(),
            expected: "finite number",
            got: "non-finite float",
        });
    }

    let bad = || JsonFormatError::Shape {
        name: name.to_string(),
        expected: scalar_type_name(ty),
        got: value.type_name(),
    };
    match (ty, value) {
        (_, Value::JsonNull(_)) if nullable => Ok(serde_json::Value::Null),
        (ScalarType::String, Value::Bool(value)) => {
            Ok(serde_json::Value::String(value.to_string()))
        }
        (ScalarType::String, Value::Int(value)) => Ok(serde_json::Value::String(value.to_string())),
        (ScalarType::String, Value::Float(value)) => {
            Ok(serde_json::Value::String(value.to_string()))
        }
        (ScalarType::String, Value::String(value)) => Ok(serde_json::Value::String(value.clone())),
        (ScalarType::Int, Value::Int(value)) => Ok(serde_json::Value::Number((*value).into())),
        (ScalarType::Int, Value::String(value)) => value
            .trim()
            .parse::<i64>()
            .map(|value| serde_json::Value::Number(value.into()))
            .map_err(|_| bad()),
        (ScalarType::Float, Value::Int(value)) if exact_f64_from_i64(*value).is_some() => {
            Ok(serde_json::Value::Number((*value).into()))
        }
        (ScalarType::Float, Value::Int(_)) => Err(JsonFormatError::Shape {
            name: name.to_string(),
            expected: "number",
            got: "int outside the exact f64 range",
        }),
        (ScalarType::Float, Value::Float(value)) => serde_json::Number::from_f64(*value)
            .map(serde_json::Value::Number)
            .ok_or_else(bad),
        (ScalarType::Float, Value::String(value)) => {
            let parsed = value.trim().parse::<f64>().map_err(|_| bad())?;
            serde_json::Number::from_f64(parsed)
                .map(serde_json::Value::Number)
                .ok_or_else(|| JsonFormatError::Shape {
                    name: name.to_string(),
                    expected: "finite number",
                    got: "string",
                })
        }
        (ScalarType::Bool, Value::Bool(value)) => Ok(serde_json::Value::Bool(*value)),
        (ScalarType::Bool, Value::String(value)) => value
            .trim()
            .parse::<bool>()
            .map(serde_json::Value::Bool)
            .map_err(|_| bad()),
        _ => Err(bad()),
    }
}

fn scalar_type_name(ty: ScalarType) -> &'static str {
    match ty {
        ScalarType::String => "string",
        ScalarType::Int => "integer",
        ScalarType::Float => "number",
        ScalarType::Bool => "bool",
    }
}

fn instance_type_name(instance: &Instance) -> &'static str {
    match instance {
        Instance::Scalar(value) => value.type_name(),
        Instance::Group(_) => "object",
        Instance::Repeated(_) => "array",
        Instance::MappedSequence(_) => "mapped sequence",
        Instance::DocumentSet(_) => "document set",
    }
}

fn write_shape_error(
    schema: &SchemaNode,
    expected: &'static str,
    got: &'static str,
) -> JsonFormatError {
    JsonFormatError::Shape {
        name: schema.name.clone(),
        expected,
        got,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json5_reads_and_writes_schema_shaped_documents() {
        let schema = schema();
        let source = "// source comment\n{Name:'Ada',Tag:[{Value:'x',Weight:0x10},],}";
        let instance = from_json5_str(source, &schema).unwrap();
        let rendered = to_json5_string(&schema, &instance).unwrap();
        assert!(rendered.contains("Name:"));
        assert!(rendered.contains("Weight:"));
        assert!(!rendered.contains("source comment"));
        assert_eq!(from_json5_str(&rendered, &schema).unwrap(), instance);
    }

    #[test]
    fn json5_rejects_nonfinite_values_and_deep_nesting() {
        let schema = SchemaNode::group("Root", vec![SchemaNode::scalar("n", ScalarType::Float)]);
        for source in ["{n: Infinity}", "{n: -Infinity}", "{n: NaN}"] {
            assert!(from_json5_str(source, &schema).is_err(), "{source}");
        }
        let deep = format!("{}0{}", "[".repeat(129), "]".repeat(129));
        assert!(matches!(
            from_json5_str(&deep, &schema),
            Err(JsonFormatError::Json5NestingLimit { limit: 128 })
        ));
        for separator in ['\u{2028}', '\u{2029}'] {
            let shallow = format!("// comment{separator}{{n: 1}}");
            assert!(from_json5_str(&shallow, &schema).is_ok());
            let deep = format!(
                "// comment{separator}{}0{}",
                "[".repeat(129),
                "]".repeat(129)
            );
            assert!(matches!(
                from_json5_str(&deep, &schema),
                Err(JsonFormatError::Json5NestingLimit { limit: 128 })
            ));
        }
    }

    #[test]
    fn json5_checks_unique_items_on_decoded_arrays() {
        let mut schema = SchemaNode::scalar("items", ScalarType::Int).repeating();
        schema.json_unique_items = true;
        assert!(matches!(
            from_json5_str("[1, /* duplicate */ 1,]", &schema),
            Err(JsonFormatError::UniqueItemsMismatch { .. })
        ));
        let mut float_schema = SchemaNode::scalar("floats", ScalarType::Float).repeating();
        float_schema.json_unique_items = true;
        assert!(matches!(
            from_json5_str("[1.0, 1.00]", &float_schema),
            Err(JsonFormatError::UniqueItemsMismatch { .. })
        ));
        for source in [
            "[0x10, 16]",
            "[+1, 1.0]",
            "[.1e1, 1.]",
            "[-0x0, 0]",
            "[1e+2, 100]",
        ] {
            assert!(
                matches!(
                    from_json5_str(source, &float_schema),
                    Err(JsonFormatError::UniqueItemsMismatch { .. })
                ),
                "{source}"
            );
        }
        // The decoded f64 values are the same, but the source decimals are
        // mathematically distinct. Exact lexical validation must allow them.
        assert!(from_json5_str("[1.00000000000000000001, 1.0]", &float_schema).is_ok());
        assert!(from_json5_str("[9007199254740992.0, 9007199254740993.0]", &float_schema).is_ok());
    }

    #[test]
    fn json5_unique_items_compare_objects_without_key_order() {
        let mut schema = SchemaNode::group(
            "entry",
            vec![
                SchemaNode::scalar("ab", ScalarType::Int),
                SchemaNode::scalar("other", ScalarType::String),
            ],
        )
        .repeating();
        schema.json_unique_items = true;
        let source = "[{'ab':1,other:'x',},{other:'x',a\\u0062:0x1,}]";
        assert!(matches!(
            from_json5_str(source, &schema),
            Err(JsonFormatError::UniqueItemsMismatch { .. })
        ));
    }

    fn pattern_property_schema(
        sources: impl IntoIterator<Item = &'static str>,
    ) -> Result<SchemaNode, Box<dyn std::error::Error>> {
        let mut dynamic = SchemaNode::scalar("*", ScalarType::Int);
        dynamic.nullable = true;
        let selectors = ir::JsonPatternPropertyNames::new(sources)?;
        SchemaNode::group(
            "Object",
            vec![SchemaNode::scalar("fixed", ScalarType::String)],
        )
        .with_dynamic_fields(dynamic)
        .and_then(|schema| schema.with_json_pattern_property_names(selectors))
        .ok_or_else(|| "patternProperties test schema is valid".into())
    }

    fn schema() -> SchemaNode {
        SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::group(
                    "Tag",
                    vec![
                        SchemaNode::scalar("Value", ScalarType::String),
                        SchemaNode::scalar("Weight", ScalarType::Float),
                    ],
                )
                .repeating(),
            ],
        )
    }

    fn alternative_schema() -> SchemaNode {
        SchemaNode::group(
            "Address",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("state", ScalarType::String),
                SchemaNode::scalar("postcode", ScalarType::String),
            ],
        )
        .with_alternatives(vec![
            ir::GroupAlternative {
                name: "domestic".into(),
                members: vec!["name".into(), "state".into()],
                required: vec!["name".into(), "state".into()],
                constraints: Vec::new(),
            },
            ir::GroupAlternative {
                name: "international".into(),
                members: vec!["name".into(), "postcode".into()],
                required: vec!["name".into(), "postcode".into()],
                constraints: Vec::new(),
            },
        ])
        .unwrap()
    }

    #[test]
    fn hybrid_open_objects_preserve_order_and_reject_duplicates() {
        let schema = SchemaNode::group("Object", vec![SchemaNode::scalar("id", ScalarType::Int)])
            .with_dynamic_fields(SchemaNode::scalar("value", ScalarType::String))
            .unwrap();
        let value = serde_json::json!({"before": "B", "id": 7, "after": "A"});
        let instance = read_node(&value, &schema).unwrap();
        let Instance::Group(fields) = &instance else {
            panic!("open object should read as a group")
        };
        assert_eq!(
            fields
                .iter()
                .map(|(name, _)| name.as_str())
                .collect::<Vec<_>>(),
            ["before", "id", "after"]
        );
        assert_eq!(write_node(&schema, &instance).unwrap(), value);

        let duplicate = Instance::Group(vec![
            (
                "name".into(),
                Instance::Scalar(Value::String("first".into())),
            ),
            (
                "name".into(),
                Instance::Scalar(Value::String("second".into())),
            ),
        ]);
        assert!(matches!(
            write_node(&schema, &duplicate),
            Err(JsonFormatError::DuplicateProperty { ref property, .. }) if property == "name"
        ));
    }

    #[test]
    fn pattern_properties_select_only_dynamic_names_and_preserve_fixed_precedence()
    -> Result<(), Box<dyn std::error::Error>> {
        let schema = pattern_property_schema(["^x-", "meta"])?;
        let value = serde_json::json!({
            "fixed": "uses its declared string schema",
            "x-value": 1,
            "x-meta": 2,
            "meta-value": null
        });
        let instance = read_node(&value, &schema)?;
        assert_eq!(write_node(&schema, &instance)?, value);

        assert!(matches!(
            read_node(&serde_json::json!({"other": 1}), &schema),
            Err(JsonFormatError::UnmatchedPatternProperty { ref property, .. })
                if property == "other"
        ));
        assert!(matches!(
            write_node(
                &schema,
                &Instance::Group(vec![(
                    "other".into(),
                    Instance::Scalar(Value::Int(1)),
                )]),
            ),
            Err(JsonFormatError::UnmatchedPatternProperty { ref property, .. })
                if property == "other"
        ));
        assert_eq!(
            write_node(
                &schema,
                &Instance::Group(vec![("other".into(), Instance::Scalar(Value::Null),)]),
            )?,
            serde_json::json!({})
        );
        Ok(())
    }

    #[test]
    fn pattern_properties_apply_to_json_lines_with_one_shared_work_budget()
    -> Result<(), Box<dyn std::error::Error>> {
        let schema = pattern_property_schema(["^x-"])?;
        let rows = from_lines("{\"x-a\":1}\n{\"x-b\":null}\n", &schema)?;
        assert_eq!(to_lines(&schema, &rows)?, "{\"x-a\":1}\n{\"x-b\":null}\n");
        assert!(matches!(
            from_lines("{\"x-a\":1}\n{\"other\":2}\n", &schema),
            Err(JsonFormatError::UnmatchedPatternProperty { ref property, .. })
                if property == "other"
        ));

        let expensive = pattern_property_schema(["^(a?){8000}$"])?;
        let key = "a".repeat(8_000);
        assert!(matches!(
            from_str(&format!("{{{key:?}:1}}"), &expensive),
            Err(JsonFormatError::PatternWorkLimit { ref name }) if name == "Object"
        ));
        Ok(())
    }

    #[test]
    fn object_alternatives_validate_and_preserve_each_projection() {
        let schema = alternative_schema();
        for value in [
            serde_json::json!({"name": "A", "state": "WA"}),
            serde_json::json!({"name": "B", "postcode": "SW1"}),
        ] {
            let instance = read_node(&value, &schema).unwrap();
            assert_eq!(write_node(&schema, &instance).unwrap(), value);
        }
        assert!(matches!(
            read_node(&serde_json::json!({"name": "A"}), &schema),
            Err(JsonFormatError::NoMatchingAlternative { .. })
        ));
        assert!(matches!(
            read_node(
                &serde_json::json!({"name": "A", "state": "WA", "postcode": "SW1"}),
                &schema
            ),
            Err(JsonFormatError::NoMatchingAlternative { .. })
        ));
        for invalid in [
            serde_json::json!({"name": "A", "state": null}),
            serde_json::json!({"name": "A", "state": "WA", "extra": true}),
        ] {
            assert!(matches!(
                read_node(&invalid, &schema),
                Err(JsonFormatError::NoMatchingAlternative { .. })
            ));
        }
    }

    #[test]
    fn object_alternatives_match_required_string_constraints() {
        let schema = SchemaNode::group(
            "Event",
            vec![
                SchemaNode::scalar("kind", ScalarType::String),
                SchemaNode::scalar("value", ScalarType::String),
            ],
        )
        .with_alternatives(vec![
            ir::GroupAlternative {
                name: "created".into(),
                members: vec!["kind".into(), "value".into()],
                required: vec!["kind".into(), "value".into()],
                constraints: vec![ir::GroupAlternativeConstraint {
                    member: "kind".into(),
                    value: GroupAlternativeConstraintValue::String("created".into()),
                }],
            },
            ir::GroupAlternative {
                name: "deleted".into(),
                members: vec!["kind".into(), "value".into()],
                required: vec!["kind".into(), "value".into()],
                constraints: vec![ir::GroupAlternativeConstraint {
                    member: "kind".into(),
                    value: GroupAlternativeConstraintValue::String("deleted".into()),
                }],
            },
        ])
        .unwrap();

        for value in [
            serde_json::json!({"kind": "created", "value": "one"}),
            serde_json::json!({"kind": "deleted", "value": "two"}),
        ] {
            let instance = read_node(&value, &schema).unwrap();
            assert_eq!(write_node(&schema, &instance).unwrap(), value);
        }
        for value in [
            serde_json::json!({"kind": "changed", "value": "three"}),
            serde_json::json!({"kind": null, "value": "four"}),
            serde_json::json!({"value": "five"}),
        ] {
            assert!(matches!(
                read_node(&value, &schema),
                Err(JsonFormatError::NoMatchingAlternative { .. })
            ));
        }
    }

    #[test]
    fn text_io_roundtrips_nested_repeating_groups() {
        let tag = |v: &str, w: f64| {
            Instance::Group(vec![
                ("Value".into(), Instance::Scalar(Value::String(v.into()))),
                ("Weight".into(), Instance::Scalar(Value::Float(w))),
            ])
        };
        let instance = Instance::Group(vec![
            (
                "Name".into(),
                Instance::Scalar(Value::String("Jane".into())),
            ),
            (
                "Tag".into(),
                Instance::Repeated(vec![tag("a", 1.5), tag("b", 2.0)]),
            ),
        ]);

        let text = to_string(&schema(), &instance).unwrap();
        let read_back = from_str(&text, &schema()).unwrap();

        assert!(text.ends_with('\n'));
        assert_eq!(read_back, instance);
    }

    #[test]
    fn text_io_supports_repeating_roots() {
        let schema = SchemaNode::scalar("Value", ScalarType::Int).repeating();
        let instance = Instance::Repeated(vec![
            Instance::Scalar(Value::Int(1)),
            Instance::Scalar(Value::Int(2)),
        ]);

        let text = to_string(&schema, &instance).unwrap();

        assert_eq!(from_str(&text, &schema).unwrap(), instance);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap(),
            serde_json::json!([1, 2])
        );
    }

    #[test]
    fn scalar_union_writes_only_unambiguous_existing_scalar_coercions()
    -> Result<(), JsonFormatError> {
        let Some(types) = ScalarTypeSet::new([ScalarType::Float, ScalarType::String]) else {
            panic!("test scalar union must contain distinct types");
        };
        let float_or_string = SchemaNode::scalar_union("value", types);
        assert_eq!(
            write_node(&float_or_string, &Instance::Scalar(Value::Int(42)))?,
            serde_json::json!(42)
        );

        let Some(types) = ScalarTypeSet::new([ScalarType::Int, ScalarType::String]) else {
            panic!("test scalar union must contain distinct types");
        };
        let int_or_string = SchemaNode::scalar_union("value", types);
        assert_eq!(
            write_node(
                &int_or_string,
                &Instance::Scalar(Value::String("42".into()))
            )?,
            serde_json::json!("42")
        );

        let Some(types) = ScalarTypeSet::new([ScalarType::Float, ScalarType::Bool]) else {
            panic!("test scalar union must contain distinct types");
        };
        let float_or_bool = SchemaNode::scalar_union("value", types);
        assert!(matches!(
            write_node(
                &float_or_bool,
                &Instance::Scalar(Value::Int((1_i64 << f64::MANTISSA_DIGITS) + 1))
            ),
            Err(JsonFormatError::Shape {
                got: "int outside the exact f64 range",
                ..
            })
        ));
        let Some(types) = ScalarTypeSet::new([ScalarType::Int, ScalarType::Float]) else {
            panic!("test scalar union must contain distinct types");
        };
        let ambiguous_numeric = SchemaNode::scalar_union("value", types);
        assert!(matches!(
            write_node(
                &ambiguous_numeric,
                &Instance::Scalar(Value::String("1".into()))
            ),
            Err(JsonFormatError::Shape {
                expected: "unambiguous declared scalar union",
                got: "string",
                ..
            })
        ));
        Ok(())
    }

    #[test]
    fn json_lines_roundtrips_rows_without_an_enclosing_array() {
        let schema = SchemaNode::group(
            "Row",
            vec![
                SchemaNode::scalar("name", ScalarType::String),
                SchemaNode::scalar("count", ScalarType::Int),
            ],
        );
        let text = "{\"name\":\"first\",\"count\":1}\n\n{\"name\":\"second\",\"count\":2}\n";

        let rows = from_lines(text, &schema).unwrap();
        assert_eq!(
            to_lines(&schema, &rows).unwrap(),
            text.replace("\n\n", "\n")
        );
        assert_eq!(rows.as_repeated().map(<[Instance]>::len), Some(2));
    }

    #[test]
    fn leading_utf8_bom_is_accepted_for_json_documents_and_lines() {
        let scalar = SchemaNode::scalar("Value", ScalarType::Int);
        assert_eq!(
            from_str("\u{feff}42", &scalar).unwrap(),
            Instance::Scalar(Value::Int(42))
        );
        assert_eq!(
            from_lines("\u{feff}1\n2\n", &scalar).unwrap(),
            Instance::Repeated(vec![
                Instance::Scalar(Value::Int(1)),
                Instance::Scalar(Value::Int(2)),
            ])
        );
    }

    #[test]
    fn to_string_preserves_flat_rows_for_a_non_repeating_root() {
        let schema = SchemaNode::group("Row", vec![SchemaNode::scalar("Name", ScalarType::String)]);
        let rows = Instance::Repeated(vec![
            Instance::Group(vec![(
                "Name".into(),
                Instance::Scalar(Value::String("first".into())),
            )]),
            Instance::Group(vec![(
                "Name".into(),
                Instance::Scalar(Value::String("second".into())),
            )]),
        ]);

        let text = to_string(&schema, &rows).unwrap();

        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&text).unwrap(),
            serde_json::json!([{"Name": "first"}, {"Name": "second"}])
        );
    }

    #[test]
    fn missing_repeating_field_reads_as_empty() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_format_json_test_empty_{}.json",
            std::process::id()
        ));
        std::fs::write(&path, r#"{ "Name": "Jane" }"#).unwrap();

        let instance = read(&path, &schema()).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(instance.field("Tag"), Some(&Instance::Repeated(Vec::new())));
    }

    #[test]
    fn absent_properties_read_as_null_and_are_omitted_on_write() {
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("Nick", ScalarType::String),
                SchemaNode::group(
                    "Extra",
                    vec![SchemaNode::scalar("Note", ScalarType::String)],
                ),
            ],
        );
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_json_test_optional_{}.json",
            std::process::id()
        ));
        std::fs::write(&path, r#"{ "Name": "Jane" }"#).unwrap();

        let instance = read(&path, &schema).unwrap();
        assert_eq!(instance.field("Nick"), Some(&Instance::Scalar(Value::Null)));
        assert_eq!(instance.field("Extra"), Some(&Instance::Group(vec![])));

        // Writing the Null back omits the key instead of emitting `null`.
        write(&path, &schema, &instance).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert!(!text.contains("Nick"), "{text}");
    }

    #[test]
    fn explicit_null_requires_nullable_scalar_metadata() {
        let scalar = SchemaNode::scalar("Value", ScalarType::String);
        assert!(matches!(
            from_str("null", &scalar),
            Err(JsonFormatError::Shape {
                expected: "string",
                got: "null",
                ..
            })
        ));
        assert!(matches!(
            to_string(&scalar, &Instance::Scalar(Value::Null)),
            Err(JsonFormatError::Shape {
                expected: "string",
                got: "null",
                ..
            })
        ));

        let nullable = scalar.nullable().unwrap();
        let instance = from_str("null", &nullable).unwrap();
        assert_eq!(instance, Instance::Scalar(Value::json_null()));
        assert_eq!(to_string(&nullable, &instance).unwrap(), "null\n");
    }

    #[test]
    fn nullable_object_properties_distinguish_absence_from_explicit_null() {
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Optional", ScalarType::String),
                SchemaNode::scalar("Nullable", ScalarType::String)
                    .nullable()
                    .unwrap(),
            ],
        );
        let instance = from_str("{}", &schema).unwrap();
        let value: serde_json::Value =
            serde_json::from_str(&to_string(&schema, &instance).unwrap()).unwrap();
        assert_eq!(value, serde_json::json!({}));

        let instance = from_str(r#"{"Nullable":null}"#, &schema).unwrap();
        assert_eq!(
            instance.field("Nullable"),
            Some(&Instance::Scalar(Value::json_null()))
        );
        let value: serde_json::Value =
            serde_json::from_str(&to_string(&schema, &instance).unwrap()).unwrap();
        assert_eq!(value, serde_json::json!({"Nullable": null}));
    }

    #[test]
    fn null_only_omits_scalar_leaves() {
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::group(
                    "Object",
                    vec![SchemaNode::scalar("Value", ScalarType::String)],
                ),
                SchemaNode::scalar("Items", ScalarType::String).repeating(),
            ],
        );

        for field in ["Object", "Items"] {
            let instance =
                Instance::Group(vec![(field.to_string(), Instance::Scalar(Value::Null))]);
            let error = write_node(&schema, &instance).unwrap_err();
            assert!(matches!(
                error,
                JsonFormatError::Shape { ref name, got: "null", .. } if name == field
            ));
        }
    }

    #[test]
    fn wrong_shape_is_reported_with_field_name() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_format_json_test_bad_{}.json",
            std::process::id()
        ));
        std::fs::write(&path, r#"{ "Name": 42, "Tag": [] }"#).unwrap();

        let err = read(&path, &schema()).unwrap_err();
        std::fs::remove_file(&path).unwrap();

        assert!(
            matches!(err, JsonFormatError::Shape { ref name, expected: "string", .. } if name == "Name")
        );
    }

    fn write_scalar_value(
        ty: ScalarType,
        value: Value,
    ) -> Result<serde_json::Value, JsonFormatError> {
        write_node(&SchemaNode::scalar("Field", ty), &Instance::Scalar(value))
    }

    #[test]
    fn string_leaves_serialize_every_finite_scalar_as_json_text() {
        for (value, expected) in [
            (Value::Bool(true), "true"),
            (Value::Int(-42), "-42"),
            (Value::Float(2.5), "2.5"),
            (Value::String("value".into()), "value"),
        ] {
            assert_eq!(
                write_scalar_value(ScalarType::String, value).unwrap(),
                serde_json::Value::String(expected.into())
            );
        }
    }

    #[test]
    fn typed_leaves_coerce_text_and_widen_integers_to_numbers() {
        assert_eq!(
            write_scalar_value(ScalarType::Int, Value::String(" 42 ".into())).unwrap(),
            serde_json::json!(42)
        );
        assert_eq!(
            write_scalar_value(ScalarType::Float, Value::Int(42)).unwrap(),
            serde_json::json!(42)
        );
        assert_eq!(
            write_scalar_value(ScalarType::Float, Value::String("2.5".into())).unwrap(),
            serde_json::json!(2.5)
        );
        assert_eq!(
            write_scalar_value(ScalarType::Bool, Value::String("true".into())).unwrap(),
            serde_json::json!(true)
        );
    }

    #[test]
    fn float_leaves_only_widen_integers_that_roundtrip_exactly() -> Result<(), JsonFormatError> {
        let schema = SchemaNode::scalar("Field", ScalarType::Float);
        for value in [
            1_i64 << f64::MANTISSA_DIGITS,
            (1_i64 << f64::MANTISSA_DIGITS) + 2,
            i64::MIN,
        ] {
            let encoded = write_node(&schema, &Instance::Scalar(Value::Int(value)))?;
            assert_eq!(
                read_node(&encoded, &schema)?,
                Instance::Scalar(Value::Float(value as f64))
            );
        }

        for value in [
            (1_i64 << f64::MANTISSA_DIGITS) + 1,
            -((1_i64 << f64::MANTISSA_DIGITS) + 1),
            i64::MAX,
        ] {
            let Err(error) = write_node(&schema, &Instance::Scalar(Value::Int(value))) else {
                panic!("inexact integer must not widen to float");
            };
            assert!(matches!(
                error,
                JsonFormatError::Shape {
                    ref name,
                    expected: "number",
                    got: "int outside the exact f64 range"
                } if name == "Field"
            ));
        }
        Ok(())
    }

    #[test]
    fn float_leaves_accept_exact_sparse_external_integers() -> Result<(), Box<dyn std::error::Error>>
    {
        let schema = SchemaNode::scalar("Field", ScalarType::Float);

        for text in [
            ((1_u64 << f64::MANTISSA_DIGITS) + 2).to_string(),
            i64::MIN.to_string(),
            (1_u64 << 63).to_string(),
        ] {
            let expected = text.parse::<f64>()?;
            assert_eq!(
                from_str(&text, &schema)?,
                Instance::Scalar(Value::Float(expected))
            );
        }

        let Err(error) = from_str(&((1_u64 << f64::MANTISSA_DIGITS) + 1).to_string(), &schema)
        else {
            panic!("inexact external integer must not narrow to float");
        };
        assert!(matches!(
            error,
            JsonFormatError::Shape {
                ref name,
                expected: "number",
                got: "integer outside the exact f64 range"
            } if name == "Field"
        ));

        assert_eq!(
            from_str("1.25", &schema)?,
            Instance::Scalar(Value::Float(1.25))
        );
        Ok(())
    }

    #[test]
    fn incompatible_and_non_finite_values_return_typed_errors() {
        let incompatible = write_scalar_value(ScalarType::Int, Value::Float(2.0)).unwrap_err();
        assert!(matches!(
            incompatible,
            JsonFormatError::Shape {
                ref name,
                expected: "integer",
                got: "float"
            } if name == "Field"
        ));

        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let error = write_scalar_value(ScalarType::Float, Value::Float(value)).unwrap_err();
            assert!(matches!(
                error,
                JsonFormatError::Shape {
                    ref name,
                    expected: "finite number",
                    got: "non-finite float"
                } if name == "Field"
            ));
        }

        let wrong_shape = write_node(
            &SchemaNode::scalar("Field", ScalarType::Bool),
            &Instance::Group(Vec::new()),
        )
        .unwrap_err();
        assert!(matches!(
            wrong_shape,
            JsonFormatError::Shape {
                ref name,
                expected: "bool",
                got: "object"
            } if name == "Field"
        ));

        let error = write_node(
            &SchemaNode::scalar("Field", ScalarType::Bool),
            &Instance::MappedSequence(Vec::new()),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            JsonFormatError::Shape {
                got: "mapped sequence",
                ..
            }
        ));
    }

    #[test]
    fn json_output_normalizes_bounded_mapped_sequences() -> Result<(), Box<dyn std::error::Error>> {
        let scalar = SchemaNode::scalar("Field", ScalarType::String);
        let root = SchemaNode::group("Root", vec![scalar.clone()]);
        let item = |value: &str| Instance::Scalar(Value::String(value.into()));
        let field =
            |items| Instance::Group(vec![("Field".into(), Instance::MappedSequence(items))]);

        let omitted: serde_json::Value = serde_json::from_str(&to_string(&root, &field(vec![]))?)?;
        assert_eq!(omitted, serde_json::json!({}));
        let one: serde_json::Value =
            serde_json::from_str(&to_string(&root, &field(vec![item("one")]))?)?;
        assert_eq!(one, serde_json::json!({"Field": "one"}));
        let error = to_string(&root, &field(vec![item("one"), item("two")])).unwrap_err();
        assert!(matches!(
            error,
            JsonFormatError::Shape {
                ref name,
                expected: "one mapped item",
                got: "mapped sequence"
            } if name == "Field"
        ));

        let required = root
            .clone()
            .with_required_fields(vec!["Field".into()])
            .expect("valid required field");
        assert!(matches!(
            to_string(&required, &field(vec![])),
            Err(JsonFormatError::MissingRequiredProperty { ref property, .. }) if property == "Field"
        ));

        let repeating = scalar.repeating();
        for (items, expected) in [
            (vec![], serde_json::json!([])),
            (vec![item("one")], serde_json::json!(["one"])),
            (
                vec![item("one"), item("two")],
                serde_json::json!(["one", "two"]),
            ),
        ] {
            let actual: serde_json::Value =
                serde_json::from_str(&to_string(&repeating, &Instance::MappedSequence(items))?)?;
            assert_eq!(actual, expected);
        }
        Ok(())
    }

    #[test]
    fn root_row_arrays_validate_every_object_and_nested_assertion()
    -> Result<(), Box<dyn std::error::Error>> {
        let range = ir::StringLengthRange::new(2, Some(5)).expect("valid length range");
        let name = SchemaNode::scalar("Name", ScalarType::String)
            .with_string_length_range(range)
            .expect("string length applies to string");
        let tags = SchemaNode::scalar("Tags", ScalarType::Int)
            .repeating()
            .with_json_unique_items()
            .expect("uniqueItems applies to arrays");
        let nested = SchemaNode::group("Nested", vec![]);
        let schema = SchemaNode::group("Row", vec![name, tags, nested])
            .with_required_fields(vec!["Name".into()])
            .expect("required field exists");

        assert_eq!(from_str("[]", &schema)?, Instance::Repeated(vec![]));
        let rows = from_str(
            r#"[{"Name":"Ada","Tags":[1,2]},{"Name":"Eve","Tags":[3]}]"#,
            &schema,
        )?;
        assert!(matches!(rows, Instance::Repeated(ref items) if items.len() == 2));
        assert!(matches!(
            from_str(r#"[{"Name":"Ada"},{}]"#, &schema),
            Err(JsonFormatError::MissingRequiredProperty { ref property, .. }) if property == "Name"
        ));
        assert!(matches!(
            from_str(r#"[{"Name":"Ada"},{"Name":"X"}]"#, &schema),
            Err(JsonFormatError::StringLengthMismatch { .. })
        ));
        assert!(matches!(
            from_str(r#"[{"Name":"Ada"},{"Name":"Eve","Nested":[]}]"#, &schema),
            Err(JsonFormatError::Shape { ref name, expected: "object", got: "array" })
                if name == "Nested"
        ));
        assert!(matches!(
            from_str(r#"[{"Name":"Ada"},{"Name":"Eve","Tags":[1,1]}]"#, &schema),
            Err(JsonFormatError::UniqueItemsMismatch { .. })
        ));
        assert!(matches!(
            from_json5_str("[{Name: 'Ada'}, {Name: 'Eve', Tags: [1, 1]}]", &schema),
            Err(JsonFormatError::UniqueItemsMismatch { .. })
        ));
        Ok(())
    }
}
