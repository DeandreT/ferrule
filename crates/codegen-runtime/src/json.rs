use std::fmt;

use ir::{Instance, SchemaNode};

use crate::RuntimeError;

pub const MAX_EMBEDDED_JSON_SCHEMA_BYTES: usize = 1024 * 1024;
pub const MAX_JSON_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

/// Structured failure from a generated mapping's JSON document boundary.
#[derive(Debug, PartialEq)]
pub enum JsonBoundaryError {
    EmbeddedSchemaTooLarge { bytes: usize, max: usize },
    InvalidEmbeddedSchema { message: String },
    InputTooLarge { bytes: usize, max: usize },
    InvalidInput { message: String },
    Execution(RuntimeError),
    InvalidOutput { message: String },
    OutputTooLarge { bytes: usize, max: usize },
}

impl fmt::Display for JsonBoundaryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmbeddedSchemaTooLarge { bytes, max } => write!(
                formatter,
                "embedded JSON schema is {bytes} bytes; maximum is {max}"
            ),
            Self::InvalidEmbeddedSchema { message } => {
                write!(formatter, "embedded JSON schema is invalid: {message}")
            }
            Self::InputTooLarge { bytes, max } => {
                write!(formatter, "JSON input is {bytes} bytes; maximum is {max}")
            }
            Self::InvalidInput { message } => write!(formatter, "JSON input is invalid: {message}"),
            Self::Execution(error) => error.fmt(formatter),
            Self::InvalidOutput { message } => {
                write!(formatter, "JSON output is invalid: {message}")
            }
            Self::OutputTooLarge { bytes, max } => {
                write!(formatter, "JSON output is {bytes} bytes; maximum is {max}")
            }
        }
    }
}

impl std::error::Error for JsonBoundaryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Execution(error) => Some(error),
            Self::EmbeddedSchemaTooLarge { .. }
            | Self::InvalidEmbeddedSchema { .. }
            | Self::InputTooLarge { .. }
            | Self::InvalidInput { .. }
            | Self::InvalidOutput { .. }
            | Self::OutputTooLarge { .. } => None,
        }
    }
}

impl From<RuntimeError> for JsonBoundaryError {
    fn from(error: RuntimeError) -> Self {
        Self::Execution(error)
    }
}

/// Parses one bounded JSON document using an emitter-owned schema.
pub fn parse_json(schema: &str, document: &str) -> Result<Instance, JsonBoundaryError> {
    check_input_size(document.len())?;
    let schema = parse_schema(schema)?;
    format_json::from_str(document, &schema).map_err(|error| JsonBoundaryError::InvalidInput {
        message: error.to_string(),
    })
}

/// Parses one bounded UTF-8 JSON payload using an emitter-owned schema.
pub fn parse_json_bytes(schema: &str, document: &[u8]) -> Result<Instance, JsonBoundaryError> {
    check_input_size(document.len())?;
    let document =
        std::str::from_utf8(document).map_err(|error| JsonBoundaryError::InvalidInput {
            message: format!("document is not UTF-8: {error}"),
        })?;
    parse_json(schema, document)
}

fn check_input_size(bytes: usize) -> Result<(), JsonBoundaryError> {
    if bytes > MAX_JSON_DOCUMENT_BYTES {
        return Err(JsonBoundaryError::InputTooLarge {
            bytes,
            max: MAX_JSON_DOCUMENT_BYTES,
        });
    }
    Ok(())
}

/// Serializes one instance as a bounded pretty-printed JSON document.
pub fn serialize_json(schema: &str, instance: &Instance) -> Result<String, JsonBoundaryError> {
    let schema = parse_schema(schema)?;
    let document = format_json::to_string(&schema, instance).map_err(|error| {
        JsonBoundaryError::InvalidOutput {
            message: error.to_string(),
        }
    })?;
    if document.len() > MAX_JSON_DOCUMENT_BYTES {
        return Err(JsonBoundaryError::OutputTooLarge {
            bytes: document.len(),
            max: MAX_JSON_DOCUMENT_BYTES,
        });
    }
    Ok(document)
}

/// Serializes one instance as a bounded pretty-printed UTF-8 JSON payload.
pub fn serialize_json_bytes(
    schema: &str,
    instance: &Instance,
) -> Result<Vec<u8>, JsonBoundaryError> {
    serialize_json(schema, instance).map(String::into_bytes)
}

fn parse_schema(schema: &str) -> Result<SchemaNode, JsonBoundaryError> {
    if schema.len() > MAX_EMBEDDED_JSON_SCHEMA_BYTES {
        return Err(JsonBoundaryError::EmbeddedSchemaTooLarge {
            bytes: schema.len(),
            max: MAX_EMBEDDED_JSON_SCHEMA_BYTES,
        });
    }
    serde_json::from_str(schema).map_err(|error| JsonBoundaryError::InvalidEmbeddedSchema {
        message: error.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use ir::{IntegerRange, ItemCountRange, NumericRange, ScalarType, ScalarTypeSet, Value};

    use super::*;

    fn schema() -> (SchemaNode, String) {
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("Count", ScalarType::Int),
            ],
        );
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        (schema, encoded)
    }

    #[test]
    fn parses_and_serializes_with_the_embedded_schema() {
        let (_, schema) = schema();
        let parsed = parse_json(&schema, r#"{"Name":"sample","Count":3}"#);
        assert_eq!(
            parsed,
            Ok(Instance::Group(vec![
                (
                    "Name".into(),
                    Instance::Scalar(Value::String("sample".into()))
                ),
                ("Count".into(), Instance::Scalar(Value::Int(3))),
            ]))
        );
        let rendered = parsed.and_then(|instance| serialize_json(&schema, &instance));
        assert_eq!(
            rendered.as_deref(),
            Ok("{\n  \"Name\": \"sample\",\n  \"Count\": 3\n}\n")
        );
    }

    #[test]
    fn enforces_embedded_numeric_ranges_on_input_and_output() {
        let Some(range) = IntegerRange::new(Some(5), Some(8)).map(NumericRange::Integer) else {
            panic!("test range is valid");
        };
        let Some(schema) = SchemaNode::scalar("Count", ScalarType::Int).with_numeric_range(range)
        else {
            panic!("test range matches its scalar type");
        };
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        assert_eq!(
            parse_json(&encoded, "5"),
            Ok(Instance::Scalar(Value::Int(5)))
        );
        assert!(matches!(
            parse_json(&encoded, "4"),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("numeric range")
        ));
        assert!(matches!(
            serialize_json(
                &encoded,
                &Instance::Scalar(Value::String("9".to_string()))
            ),
            Err(JsonBoundaryError::InvalidOutput { ref message })
                if message.contains("numeric range")
        ));
    }

    #[test]
    fn enforces_embedded_item_counts_on_input_and_output() {
        let Some(range) = ItemCountRange::new(1, Some(2)) else {
            panic!("test item-count range is valid");
        };
        let Some(schema) = SchemaNode::scalar("Values", ScalarType::Int)
            .repeating()
            .with_item_count_range(range)
        else {
            panic!("test item-count range matches a repeating node");
        };
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        assert!(matches!(
            parse_json(&encoded, "[]"),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("between 1 and 2 array items")
        ));
        assert_eq!(
            parse_json(&encoded, "[1,2]"),
            Ok(Instance::Repeated(vec![
                Instance::Scalar(Value::Int(1)),
                Instance::Scalar(Value::Int(2)),
            ]))
        );
        assert!(matches!(
            serialize_json(&encoded, &Instance::Repeated(Vec::new())),
            Err(JsonBoundaryError::InvalidOutput { ref message })
                if message.contains("between 1 and 2 array items")
        ));
        assert_eq!(
            serialize_json(
                &encoded,
                &Instance::Repeated(vec![Instance::Scalar(Value::Int(1))])
            )
            .as_deref(),
            Ok("[\n  1\n]\n")
        );

        let invalid = encoded.replace("\"repeating\":true", "\"repeating\":false");
        assert!(matches!(
            parse_json(&invalid, "1"),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { ref message })
                if message.contains("item-count range")
        ));
    }

    #[test]
    fn embedded_format_annotations_are_validated_but_not_asserted() {
        let Ok(formats) = ir::JsonFormatAnnotations::new([String::new(), "email".to_string()])
        else {
            panic!("test format annotations are bounded");
        };
        let Some(schema) =
            SchemaNode::scalar("Contact", ScalarType::String).with_json_formats(formats)
        else {
            panic!("format annotations match a string scalar");
        };
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        assert_eq!(
            parse_json(&encoded, r#""not an email""#),
            Ok(Instance::Scalar(Value::String("not an email".into())))
        );
        assert_eq!(
            serialize_json(
                &encoded,
                &Instance::Scalar(Value::String("also not an email".into()))
            )
            .as_deref(),
            Ok("\"also not an email\"\n")
        );

        let invalid_type = encoded.replace(r#""ty":"string""#, r#""ty":"int""#);
        assert!(matches!(
            parse_json(&invalid_type, "1"),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { ref message })
                if message.contains("JSON format annotations")
        ));
        let invalid_any = encoded.replace(r#""kind":"#, r#""json_any":true,"kind":"#);
        assert!(matches!(
            parse_json(&invalid_any, r#""value""#),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { ref message })
                if message.contains("JSON format annotations")
        ));
    }

    #[test]
    fn parses_and_serializes_utf8_payloads() {
        let (_, schema) = schema();
        let parsed = parse_json_bytes(
            &schema,
            b"\xEF\xBB\xBF{\"Name\":\"caf\xC3\xA9\",\"Count\":3}",
        );
        assert_eq!(
            parsed,
            Ok(Instance::Group(vec![
                (
                    "Name".into(),
                    Instance::Scalar(Value::String("café".into()))
                ),
                ("Count".into(), Instance::Scalar(Value::Int(3))),
            ]))
        );
        let rendered = parsed.and_then(|instance| serialize_json_bytes(&schema, &instance));
        assert_eq!(
            rendered.as_deref(),
            Ok(&b"{\n  \"Name\": \"caf\xC3\xA9\",\n  \"Count\": 3\n}\n"[..])
        );
        assert!(matches!(
            parse_json_bytes(&schema, b"{\"Name\":\"\xFF\"}"),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("not UTF-8")
        ));
    }

    #[test]
    fn enforces_required_properties_at_native_boundaries() {
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar("Id", ScalarType::Int),
                SchemaNode::scalar("Note", ScalarType::String)
                    .nullable()
                    .unwrap(),
            ],
        )
        .with_required_fields(vec!["Id".into(), "Note".into()])
        .unwrap();
        let encoded = serde_json::to_string(&schema).unwrap_or_default();

        assert!(matches!(
            parse_json(&encoded, r#"{"Note":null}"#),
            Err(JsonBoundaryError::InvalidInput { ref message })
                if message.contains("requires property `Id`")
        ));
        assert!(parse_json(&encoded, r#"{"Id":7,"Note":null}"#).is_ok());

        let missing = Instance::Group(vec![
            ("Id".into(), Instance::Scalar(Value::Null)),
            (
                "Note".into(),
                Instance::Scalar(Value::String("present".into())),
            ),
        ]);
        assert!(matches!(
            serialize_json(&encoded, &missing),
            Err(JsonBoundaryError::InvalidOutput { ref message })
                if message.contains("requires property `Id`")
        ));
    }

    #[test]
    fn retains_boundary_and_schema_failure_categories() {
        assert!(matches!(
            parse_json("{}", "{}"),
            Err(JsonBoundaryError::InvalidEmbeddedSchema { .. })
        ));
        assert!(matches!(
            check_input_size(MAX_JSON_DOCUMENT_BYTES.saturating_add(1)),
            Err(JsonBoundaryError::InputTooLarge { .. })
        ));
    }

    #[test]
    fn preserves_heterogeneous_scalar_union_tags_at_json_boundaries() {
        let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
            panic!("test union must contain distinct scalar types");
        };
        let schema = SchemaNode::group(
            "Root",
            vec![
                SchemaNode::scalar_union("Value", types),
                SchemaNode::scalar_union("Items", types).repeating(),
            ],
        );
        let encoded = serde_json::to_string(&schema).unwrap_or_default();
        let parsed = parse_json(&encoded, r#"{"Value":7,"Items":["A",8]}"#);
        assert_eq!(
            parsed,
            Ok(Instance::Group(vec![
                ("Value".into(), Instance::Scalar(Value::Int(7))),
                (
                    "Items".into(),
                    Instance::Repeated(vec![
                        Instance::Scalar(Value::String("A".into())),
                        Instance::Scalar(Value::Int(8)),
                    ]),
                ),
            ]))
        );
        let rendered = parsed.and_then(|instance| serialize_json(&encoded, &instance));
        assert_eq!(
            rendered.as_deref(),
            Ok("{\n  \"Value\": 7,\n  \"Items\": [\n    \"A\",\n    8\n  ]\n}\n")
        );
    }
}
