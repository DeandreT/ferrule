//! JSON Schema import and JSON instance read/write.
//!
//! Shaping rules: a [`SchemaKind::Group`] is a JSON object; a child marked
//! `repeating` holds a JSON array of that child's shape (a missing repeating
//! field reads as empty, matching the XML reader's zero-match behavior);
//! scalars map per [`ScalarType`], with JSON `null` allowed for any of them.

pub mod json_schema;

use std::path::Path;

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use thiserror::Error;

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
    #[error("missing required field `{0}`")]
    MissingField(String),
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
    let value: serde_json::Value = serde_json::from_str(&text)?;
    if schema.repeating {
        read_repeated(&value, schema)
    } else {
        read_node(&value, schema)
    }
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
        SchemaKind::Scalar { ty } => Ok(Instance::Scalar(read_scalar(value, *ty, &schema.name)?)),
        SchemaKind::Group { children } => {
            let serde_json::Value::Object(fields) = value else {
                return Err(JsonFormatError::Shape {
                    name: schema.name.clone(),
                    expected: "object",
                    got: json_type_name(value),
                });
            };
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
                    None => return Err(JsonFormatError::MissingField(child.name.clone())),
                }
            }
            Ok(Instance::Group(out))
        }
    }
}

fn read_scalar(
    value: &serde_json::Value,
    ty: ScalarType,
    name: &str,
) -> Result<Value, JsonFormatError> {
    let bad = |expected: &'static str| JsonFormatError::Shape {
        name: name.to_string(),
        expected,
        got: json_type_name(value),
    };
    match (ty, value) {
        (_, serde_json::Value::Null) => Ok(Value::Null),
        (ScalarType::String, serde_json::Value::String(s)) => Ok(Value::String(s.clone())),
        (ScalarType::Int, serde_json::Value::Number(n)) => {
            n.as_i64().map(Value::Int).ok_or_else(|| bad("integer"))
        }
        (ScalarType::Float, serde_json::Value::Number(n)) => {
            n.as_f64().map(Value::Float).ok_or_else(|| bad("number"))
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
    let value = write_node(schema, instance);
    let mut text = serde_json::to_string_pretty(&value)?;
    text.push('\n');
    std::fs::write(path, text)?;
    Ok(())
}

fn write_node(schema: &SchemaNode, instance: &Instance) -> serde_json::Value {
    match instance {
        Instance::Repeated(items) => {
            serde_json::Value::Array(items.iter().map(|item| write_node(schema, item)).collect())
        }
        Instance::Scalar(value) => write_scalar(value, schema),
        Instance::Group(fields) => {
            let mut out = serde_json::Map::with_capacity(fields.len());
            if let SchemaKind::Group { children } = &schema.kind {
                for child_schema in children {
                    if let Some((_, child_instance)) =
                        fields.iter().find(|(n, _)| n == &child_schema.name)
                    {
                        out.insert(
                            child_schema.name.clone(),
                            write_node(child_schema, child_instance),
                        );
                    }
                }
            }
            serde_json::Value::Object(out)
        }
    }
}

fn write_scalar(value: &Value, schema: &SchemaNode) -> serde_json::Value {
    // A string flowing into a typed leaf (common when the source format is
    // untyped text/XML) is coerced so the output matches the schema.
    if let (Value::String(s), SchemaKind::Scalar { ty }) = (value, &schema.kind) {
        let coerced = match ty {
            ScalarType::Int => s.trim().parse().map(Value::Int).ok(),
            ScalarType::Float => s.trim().parse().map(Value::Float).ok(),
            ScalarType::Bool => s.trim().parse().map(Value::Bool).ok(),
            ScalarType::String => None,
        };
        if let Some(coerced) = coerced {
            return write_scalar(&coerced, schema);
        }
    }
    match value {
        Value::Null => serde_json::Value::Null,
        Value::Bool(b) => serde_json::Value::Bool(*b),
        Value::Int(i) => serde_json::Value::Number((*i).into()),
        Value::Float(f) => serde_json::Number::from_f64(*f)
            .map_or(serde_json::Value::Null, serde_json::Value::Number),
        Value::String(s) => serde_json::Value::String(s.clone()),
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

    #[test]
    fn write_then_read_roundtrips_nested_repeating_groups() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!(
            "ferrule_format_json_test_{}.json",
            std::process::id()
        ));

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

        write(&path, &schema(), &instance).unwrap();
        let read_back = read(&path, &schema()).unwrap();
        std::fs::remove_file(&path).unwrap();

        assert_eq!(read_back, instance);
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
}
