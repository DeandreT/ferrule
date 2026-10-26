//! Reads JSON, JSON5 and JSON Lines documents into schema-shaped instances.

use std::io::Read;
use std::path::Path;

use ir::{Instance, ScalarType, ScalarTypeSet, SchemaKind, SchemaNode, Value};

use crate::{
    JsonFormatError, MAX_JSON5_DOCUMENT_BYTES, PatternPrograms, PatternRuntime, RecursiveSchemas,
    exact_f64_from_json_number, json_type_name, json5_finite, json5_unique, null_layers,
    reject_nullable_json_lines_container, runtime_constraints, validate_alternative_fields,
    validate_required_fields,
};

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
    runtime_constraints::validate_raw_json_unique_items(schema, text)?;
    let value: serde_json::Value = serde_json::from_str(text)?;
    from_value(&value, schema)
}

/// Reads JSON5 text into an [`Instance`] tree. JSON5 comments are syntax only;
/// they are not represented in the mapping graph or reproduced on output.
pub fn from_json5_str(text: &str, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    if text.len() > MAX_JSON5_DOCUMENT_BYTES {
        return Err(JsonFormatError::Json5DocumentLimit {
            limit: MAX_JSON5_DOCUMENT_BYTES,
        });
    }
    let text = strip_utf8_bom(text);
    check_json5_nesting(text)?;
    let value = json5_finite::from_str(text)?;
    json5_unique::validate(schema, text)?;
    from_value(&value, schema)
}

fn from_value(value: &serde_json::Value, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    let recursive = RecursiveSchemas::new(schema)?;
    let programs = PatternPrograms::new(schema)?;
    from_value_with(value, schema, &recursive, &mut programs.runtime())
}

pub(crate) fn from_value_with(
    value: &serde_json::Value,
    schema: &SchemaNode,
    recursive: &RecursiveSchemas<'_>,
    patterns: &mut PatternRuntime,
) -> Result<Instance, JsonFormatError> {
    if schema.repeating {
        read_repeated(value, schema, recursive, 0, patterns)
    } else if let (SchemaKind::Group { .. }, serde_json::Value::Array(rows)) = (&schema.kind, value)
    {
        // Flat-row sources retain a non-repeating row schema while presenting
        // the root records as one repeated instance, like the JSON writer.
        let mut parsed = Vec::with_capacity(rows.len());
        for row in rows {
            parsed.push(read_node_with_patterns(
                row, schema, recursive, 0, patterns,
            )?);
        }
        Ok(Instance::Repeated(parsed))
    } else {
        read_node_with_patterns(value, schema, recursive, 0, patterns)
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
    let recursive = RecursiveSchemas::new(schema)?;
    let programs = PatternPrograms::new(schema)?;
    let mut patterns = programs.runtime();
    let lines = strip_utf8_bom(text)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    runtime_constraints::unique_items::validate_raw_json_lines_unique_items(schema, &lines)?;
    let mut values = Vec::new();
    for line in lines {
        values.push(serde_json::from_str(line)?);
    }
    if schema.repeating {
        runtime_constraints::validate_item_count(schema, values.len())?;
    }
    let mut items = Vec::with_capacity(values.len());
    for value in &values {
        items.push(read_node_with_patterns(
            value,
            schema,
            &recursive,
            0,
            &mut patterns,
        )?);
    }
    if schema.repeating {
        runtime_constraints::validate_contains(schema, &values, &mut patterns)?;
    }
    Ok(Instance::Repeated(items))
}

pub(crate) fn strip_utf8_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

pub(crate) fn read_repeated(
    value: &serde_json::Value,
    schema: &SchemaNode,
    recursive: &RecursiveSchemas<'_>,
    recursion_depth: usize,
    patterns: &mut PatternRuntime,
) -> Result<Instance, JsonFormatError> {
    let resolved;
    let (schema, recursion_depth) = if schema.recursive_ref.is_some() {
        let (value, next_depth) = recursive.resolve(schema, recursion_depth)?;
        resolved = value;
        (&resolved, next_depth)
    } else {
        (schema, recursion_depth)
    };
    if value.is_null() && null_layers::instance_null(schema)?.whole {
        return Ok(Instance::Scalar(Value::json_null()));
    }
    let serde_json::Value::Array(items) = value else {
        return Err(JsonFormatError::Shape {
            name: schema.name.clone(),
            expected: "array",
            got: json_type_name(value),
        });
    };
    runtime_constraints::validate_item_count(schema, items.len())?;
    let mut parsed = Vec::with_capacity(items.len());
    for item in items {
        parsed.push(read_node_with_patterns(
            item,
            schema,
            recursive,
            recursion_depth,
            patterns,
        )?);
    }
    runtime_constraints::validate_contains(schema, items, patterns)?;
    let items = parsed;
    Ok(Instance::Repeated(items))
}

#[cfg(test)]
pub(crate) fn read_node(
    value: &serde_json::Value,
    schema: &SchemaNode,
) -> Result<Instance, JsonFormatError> {
    let recursive = RecursiveSchemas::new(schema)?;
    let programs = PatternPrograms::new(schema)?;
    let mut patterns = programs.runtime();
    read_node_with_patterns(value, schema, &recursive, 0, &mut patterns)
}

pub(crate) fn read_node_with_patterns(
    value: &serde_json::Value,
    schema: &SchemaNode,
    recursive: &RecursiveSchemas<'_>,
    recursion_depth: usize,
    patterns: &mut PatternRuntime,
) -> Result<Instance, JsonFormatError> {
    let resolved;
    let (schema, recursion_depth) = if schema.recursive_ref.is_some() {
        let (value, next_depth) = recursive.resolve(schema, recursion_depth)?;
        resolved = value;
        (&resolved, next_depth)
    } else {
        (schema, recursion_depth)
    };
    if let Some(item) = schema.json_array_wrapper_item() {
        let value = read_repeated(value, item, recursive, recursion_depth, patterns)?;
        return Ok(Instance::Group(vec![(item.name.clone(), value)].into()));
    }
    if schema.json_any {
        return Ok(Instance::Scalar(Value::String(serde_json::to_string(
            value,
        )?)));
    }
    let null = null_layers::instance_null(schema)?;
    if value.is_null() && null.single {
        return Ok(Instance::Scalar(Value::json_null()));
    }
    match &schema.kind {
        SchemaKind::Scalar { ty } => {
            let parsed = read_scalar(value, *ty, null.value, &schema.name)?;
            runtime_constraints::validate_constant(schema, value)?;
            runtime_constraints::validate_allowed_value(schema, &parsed)?;
            runtime_constraints::validate_numeric_range(schema, value)?;
            runtime_constraints::validate_multiple_of(schema, value)?;
            runtime_constraints::validate_string_length(schema, value)?;
            patterns.validate_json(schema, value)?;
            Ok(Instance::Scalar(parsed))
        }
        SchemaKind::ScalarUnion { types } => {
            let parsed = read_scalar_union(value, *types, null.value, &schema.name)?;
            runtime_constraints::validate_allowed_value(schema, &parsed)?;
            runtime_constraints::validate_numeric_range(schema, value)?;
            runtime_constraints::validate_multiple_of(schema, value)?;
            runtime_constraints::validate_string_length(schema, value)?;
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
            runtime_constraints::validate_property_count(schema, fields.len())?;
            for property in fields.keys() {
                patterns.validate_property_name(schema, property)?;
            }
            validate_required_fields(schema, required, |name| fields.contains_key(name))?;
            runtime_constraints::validate_property_dependencies(
                schema,
                fields.keys().map(String::as_str),
            )?;
            runtime_constraints::validate_dependent_schemas(
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
                        read_repeated(
                            field_value,
                            field_schema,
                            recursive,
                            recursion_depth,
                            patterns,
                        )?
                    } else {
                        read_node_with_patterns(
                            field_value,
                            field_schema,
                            recursive,
                            recursion_depth,
                            patterns,
                        )?
                    };
                    out.push((name.clone(), field));
                }
                for child in children {
                    if !fields.contains_key(&child.name) {
                        out.push((child.name.clone(), missing_instance(child)?));
                    }
                }
                return Ok(Instance::Group((out).into()));
            }
            let mut out = Vec::with_capacity(children.len());
            for child in children {
                match fields.get(&child.name) {
                    Some(field_value) if child.repeating => {
                        out.push((
                            child.name.clone(),
                            read_repeated(
                                field_value,
                                child,
                                recursive,
                                recursion_depth,
                                patterns,
                            )?,
                        ));
                    }
                    Some(field_value) => {
                        out.push((
                            child.name.clone(),
                            read_node_with_patterns(
                                field_value,
                                child,
                                recursive,
                                recursion_depth,
                                patterns,
                            )?,
                        ));
                    }
                    // Absent properties are normal instance data (JSON
                    // objects routinely omit optional keys), not errors:
                    // scalars read as Null, objects and arrays as empty.
                    None => {
                        out.push((child.name.clone(), missing_instance(child)?));
                    }
                }
            }
            Ok(Instance::Group((out).into()))
        }
    }
}

fn missing_instance(schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    // A missing nullable object or array is absent, not an empty container.
    if null_layers::instance_null(schema)?.whole {
        return Ok(Instance::Scalar(Value::Null));
    }
    Ok(if schema.repeating {
        Instance::Repeated(Vec::new())
    } else {
        match schema.kind {
            SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } => {
                Instance::Scalar(Value::Null)
            }
            SchemaKind::Group { .. } => Instance::Group((Vec::new()).into()),
        }
    })
}

pub(crate) fn read_scalar_union(
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

pub(crate) fn read_scalar(
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
