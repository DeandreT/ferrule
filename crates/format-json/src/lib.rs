//! JSON Schema import and JSON instance read/write.
//!
//! Shaping rules: a [`SchemaKind::Group`] is a JSON object; a child marked
//! `repeating` holds a JSON array of that child's shape (a missing repeating
//! field reads as empty, matching the XML reader's zero-match behavior);
//! scalars map per [`ScalarType`], with explicit JSON `null` accepted only
//! when the schema node is nullable.
//! Absent object properties read as Null scalars / empty groups. Null values
//! on non-nullable scalar properties are omitted on write, while nullable
//! scalar properties serialize them as explicit JSON `null`.

pub mod json_schema;

use std::path::Path;

use ir::{
    GroupAlternativeConstraintValue, GroupAlternativeMode, Instance, ScalarType, SchemaKind,
    SchemaNode, Value,
};
use thiserror::Error;

const MAX_EXACT_F64_INTEGER: u64 = 1_u64 << f64::MANTISSA_DIGITS;

#[derive(Debug, Error)]
pub enum JsonFormatError {
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
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
    #[error("object `{name}` matches no declared schema alternative")]
    NoMatchingAlternative { name: String },
    #[error("object `{name}` matches more than one declared schema alternative")]
    AmbiguousAlternative { name: String },
    #[error("object `{object}` contains duplicate property `{property}`")]
    DuplicateProperty { object: String, property: String },
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

/// Reads a JSON file into an [`Instance`] tree shaped by `schema`.
pub fn read(path: &Path, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    let text = std::fs::read_to_string(path)?;
    from_str(&text, schema)
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
/// filesystem access such as WebAssembly applications.
pub fn from_str(text: &str, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    let value: serde_json::Value = serde_json::from_str(strip_utf8_bom(text))?;
    if schema.repeating {
        read_repeated(&value, schema)
    } else {
        read_node(&value, schema)
    }
}

/// Reads JSON Lines text into a repeated instance.
pub fn from_lines(text: &str, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    let items = strip_utf8_bom(text)
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let value: serde_json::Value = serde_json::from_str(line)?;
            read_node(&value, schema)
        })
        .collect::<Result<Vec<_>, JsonFormatError>>()?;
    Ok(Instance::Repeated(items))
}

fn strip_utf8_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn read_repeated(
    value: &serde_json::Value,
    schema: &SchemaNode,
) -> Result<Instance, JsonFormatError> {
    let serde_json::Value::Array(items) = value else {
        return Err(JsonFormatError::Shape {
            name: schema.name.clone(),
            expected: "array",
            got: json_type_name(value),
        });
    };
    let items = items
        .iter()
        .map(|item| read_node(item, schema))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Instance::Repeated(items))
}

fn read_node(value: &serde_json::Value, schema: &SchemaNode) -> Result<Instance, JsonFormatError> {
    match &schema.kind {
        SchemaKind::Scalar { ty } => Ok(Instance::Scalar(read_scalar(
            value,
            *ty,
            schema.nullable,
            &schema.name,
        )?)),
        SchemaKind::Group {
            children,
            alternatives,
            dynamic,
        } => {
            let serde_json::Value::Object(fields) = value else {
                return Err(JsonFormatError::Shape {
                    name: schema.name.clone(),
                    expected: "object",
                    got: json_type_name(value),
                });
            };
            validate_alternative_fields(schema, alternatives, fields)?;
            if dynamic.is_some() && !alternatives.is_empty() {
                return Err(JsonFormatError::UnsupportedSchemaUnion {
                    name: schema.name.clone(),
                    reason: "open objects cannot use closed object alternatives".to_string(),
                });
            }
            if let Some(dynamic) = dynamic {
                let mut out = Vec::with_capacity(fields.len().max(children.len()));
                for (name, field_value) in fields {
                    let field_schema = children
                        .iter()
                        .find(|child| child.name == *name)
                        .unwrap_or(dynamic);
                    let field = if field_schema.repeating {
                        read_repeated(field_value, field_schema)?
                    } else {
                        read_node(field_value, field_schema)?
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
                        out.push((child.name.clone(), read_repeated(field_value, child)?));
                    }
                    Some(field_value) => {
                        out.push((child.name.clone(), read_node(field_value, child)?));
                    }
                    None if child.repeating => {
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
    if schema.repeating {
        Instance::Repeated(Vec::new())
    } else {
        match schema.kind {
            SchemaKind::Scalar { .. } => Instance::Scalar(Value::Null),
            SchemaKind::Group { .. } => Instance::Group(Vec::new()),
        }
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
            if number
                .as_i64()
                .is_some_and(|value| value.unsigned_abs() > MAX_EXACT_F64_INTEGER)
                || number
                    .as_u64()
                    .is_some_and(|value| value > MAX_EXACT_F64_INTEGER)
            {
                return Err(JsonFormatError::Shape {
                    name: name.to_string(),
                    expected: "number",
                    got: "integer outside the exact f64 range",
                });
            }
            number
                .as_f64()
                .filter(|value| value.is_finite())
                .map(Value::Float)
                .ok_or_else(|| bad("finite number"))
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
    // A root scope can produce flat rows even though the row schema itself
    // is not repeating (the same convention used by CSV). Preserve that
    // established JSON-output shape while keeping nested nodes
    // schema-directed.
    let value = match instance {
        Instance::Repeated(items) if !schema.repeating => items
            .iter()
            .map(|item| write_single_node(schema, item))
            .collect::<Result<Vec<_>, _>>()
            .map(serde_json::Value::Array)?,
        _ => write_node(schema, instance)?,
    };
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    Ok(text)
}

/// Serializes an instance as JSON Lines using one compact root value per
/// line. A non-repeated instance becomes a single line.
pub fn to_lines(schema: &SchemaNode, instance: &Instance) -> Result<String, JsonFormatError> {
    let mut text = String::new();
    match instance {
        Instance::Repeated(items) => {
            for item in items {
                text.push_str(&serde_json::to_string(&write_single_node(schema, item)?)?);
                text.push('\n');
            }
        }
        item => {
            text.push_str(&serde_json::to_string(&write_single_node(schema, item)?)?);
            text.push('\n');
        }
    }
    Ok(text)
}

fn write_node(
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<serde_json::Value, JsonFormatError> {
    if schema.repeating {
        let Instance::Repeated(items) = instance else {
            return Err(write_shape_error(
                schema,
                "array",
                instance_type_name(instance),
            ));
        };
        return items
            .iter()
            .map(|item| write_single_node(schema, item))
            .collect::<Result<Vec<_>, _>>()
            .map(serde_json::Value::Array);
    }
    write_single_node(schema, instance)
}

fn write_single_node(
    schema: &SchemaNode,
    instance: &Instance,
) -> Result<serde_json::Value, JsonFormatError> {
    match (&schema.kind, instance) {
        (SchemaKind::Scalar { ty }, Instance::Scalar(value)) => {
            write_scalar(value, *ty, schema.nullable, &schema.name)
        }
        (
            SchemaKind::Group {
                children,
                alternatives,
                dynamic,
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
                    let child_schema = children
                        .iter()
                        .find(|child| child.name == *name)
                        .unwrap_or(dynamic);
                    if !child_schema.repeating
                        && matches!(&child_schema.kind, SchemaKind::Scalar { .. })
                        && matches!(child_instance, Instance::Scalar(Value::Null))
                    {
                        continue;
                    }
                    out.insert(name.clone(), write_node(child_schema, child_instance)?);
                }
                return Ok(serde_json::Value::Object(out));
            }
            for child_schema in children {
                if let Some((_, child_instance)) =
                    fields.iter().find(|(n, _)| n == &child_schema.name)
                {
                    // A non-nullable Null scalar is boundary-level absence.
                    // Nullable scalars retain an explicit JSON null.
                    if !child_schema.repeating
                        && matches!(&child_schema.kind, SchemaKind::Scalar { .. })
                        && matches!(child_instance, Instance::Scalar(Value::Null))
                    {
                        continue;
                    }
                    out.insert(
                        child_schema.name.clone(),
                        write_node(child_schema, child_instance)?,
                    );
                }
            }
            validate_alternative_fields(schema, alternatives, &out)?;
            Ok(serde_json::Value::Object(out))
        }
        (SchemaKind::Scalar { ty }, other) => Err(write_shape_error(
            schema,
            scalar_type_name(*ty),
            instance_type_name(other),
        )),
        (SchemaKind::Group { .. }, other) => Err(write_shape_error(
            schema,
            "object",
            instance_type_name(other),
        )),
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
                        .is_some_and(|value| constraint_matches(&constraint.value, value))
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
            actual.as_f64() == Some(expected.get())
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
        (ScalarType::Float, Value::Int(value)) if value.unsigned_abs() <= MAX_EXACT_F64_INTEGER => {
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
    fn float_leaves_only_widen_integers_that_roundtrip_exactly() {
        let schema = SchemaNode::scalar("Field", ScalarType::Float);
        let boundary = MAX_EXACT_F64_INTEGER as i64;
        let encoded = write_node(&schema, &Instance::Scalar(Value::Int(boundary))).unwrap();
        assert_eq!(
            read_node(&encoded, &schema).unwrap(),
            Instance::Scalar(Value::Float(boundary as f64))
        );

        for value in [boundary + 1, -(boundary + 1)] {
            let error = write_node(&schema, &Instance::Scalar(Value::Int(value))).unwrap_err();
            assert!(matches!(
                error,
                JsonFormatError::Shape {
                    ref name,
                    expected: "number",
                    got: "int outside the exact f64 range"
                } if name == "Field"
            ));
        }
    }

    #[test]
    fn float_leaves_reject_lossy_external_json_integers() {
        let path = std::env::temp_dir().join(format!(
            "ferrule_format_json_float_precision_{}.json",
            std::process::id()
        ));
        let schema = SchemaNode::scalar("Field", ScalarType::Float);

        std::fs::write(&path, (MAX_EXACT_F64_INTEGER + 1).to_string()).unwrap();
        let error = read(&path, &schema).unwrap_err();
        assert!(matches!(
            error,
            JsonFormatError::Shape {
                ref name,
                expected: "number",
                got: "integer outside the exact f64 range"
            } if name == "Field"
        ));

        std::fs::write(&path, "1.25").unwrap();
        assert_eq!(
            read(&path, &schema).unwrap(),
            Instance::Scalar(Value::Float(1.25))
        );
        std::fs::remove_file(path).unwrap();
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

        for mapped in [
            Instance::MappedSequence(Vec::new()),
            Instance::Group(vec![("Field".into(), Instance::MappedSequence(Vec::new()))]),
        ] {
            let (schema, instance) = match mapped {
                Instance::Group(_) => (
                    SchemaNode::group("Root", vec![SchemaNode::scalar("Field", ScalarType::Bool)]),
                    mapped,
                ),
                _ => (SchemaNode::scalar("Field", ScalarType::Bool), mapped),
            };
            let error = write_node(&schema, &instance).unwrap_err();
            assert!(matches!(
                error,
                JsonFormatError::Shape {
                    got: "mapped sequence",
                    ..
                }
            ));
        }
    }
}
