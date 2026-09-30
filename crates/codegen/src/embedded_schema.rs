use std::fmt;

use ir::SchemaNode;

/// Generated JSON entry points accept at most this many schema bytes.
pub const MAX_EMBEDDED_JSON_SCHEMA_BYTES: usize = 1024 * 1024;
/// Generated XML serialization expressions accept at most this many schema bytes.
pub const MAX_EMBEDDED_XML_SCHEMA_BYTES: usize = 8 * 1024 * 1024;

/// Failure to preserve a schema in the JSON descriptor used by generated code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmbeddedSchemaError {
    Serialization {
        schema: String,
        message: String,
    },
    Deserialization {
        schema: String,
        message: String,
    },
    MetadataChanged {
        schema: String,
    },
    TooLarge {
        schema: String,
        bytes: usize,
        max: usize,
    },
}

impl fmt::Display for EmbeddedSchemaError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Serialization { schema, message } => write!(
                formatter,
                "cannot serialize embedded schema {schema:?}: {message}"
            ),
            Self::Deserialization { schema, message } => write!(
                formatter,
                "generated runtime cannot read embedded schema {schema:?}: {message}"
            ),
            Self::TooLarge { schema, bytes, max } => write!(
                formatter,
                "embedded schema {schema:?} is {bytes} bytes; maximum is {max}"
            ),
            Self::MetadataChanged { schema } => write!(
                formatter,
                "embedded schema {schema:?} changes metadata when read by the generated runtime"
            ),
        }
    }
}

impl std::error::Error for EmbeddedSchemaError {}

/// Encodes a schema only if the generated runtime's default JSON parser retains
/// its complete metadata and the descriptor fits the caller's runtime limit.
///
/// In particular, the default serde JSON floating-point parser can change a
/// finite bound after its shortest decimal representation is serialized. Both
/// generated backends use this guard rather than silently changing a constraint.
pub fn serialize_embedded_schema(
    schema: &SchemaNode,
    max_bytes: usize,
) -> Result<String, EmbeddedSchemaError> {
    let descriptor =
        serde_json::to_string(schema).map_err(|error| EmbeddedSchemaError::Serialization {
            schema: schema.name.clone(),
            message: error.to_string(),
        })?;
    if descriptor.len() > max_bytes {
        return Err(EmbeddedSchemaError::TooLarge {
            schema: schema.name.clone(),
            bytes: descriptor.len(),
            max: max_bytes,
        });
    }
    let decoded: SchemaNode = serde_json::from_str(&descriptor).map_err(|error| {
        EmbeddedSchemaError::Deserialization {
            schema: schema.name.clone(),
            message: error.to_string(),
        }
    })?;
    if !metadata_is_unchanged(schema, &decoded, &descriptor).map_err(|error| {
        EmbeddedSchemaError::Serialization {
            schema: schema.name.clone(),
            message: error.to_string(),
        }
    })? {
        return Err(EmbeddedSchemaError::MetadataChanged {
            schema: schema.name.clone(),
        });
    }
    Ok(descriptor)
}

fn metadata_is_unchanged(
    original: &SchemaNode,
    decoded: &SchemaNode,
    descriptor: &str,
) -> Result<bool, serde_json::Error> {
    // Structural equality alone treats negative and positive floating zero as
    // equal. Canonical serialization also retains the sign of every float.
    Ok(decoded == original && serde_json::to_string(decoded)? == descriptor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::{
        FiniteF64, JsonAllowedValue, JsonAllowedValues, NumberBound, NumberRange, NumericRange,
        ScalarType,
    };

    fn physical_scalar(metadata: &str) -> SchemaNode {
        serde_json::from_str(&format!(
            r#"{{"name":"Amount",{metadata},"kind":{{"kind":"scalar","ty":"float"}}}}"#
        ))
        .expect("physical schema is valid")
    }

    #[test]
    fn rejects_changed_nested_floating_metadata_without_changing_the_schema() {
        let cases = [
            r#""numeric_range":{"kind":"number","bounds":{"minimum":{"value":1e-307}}}"#,
            r#""json_allowed_values":[{"type":"int","value":0},{"type":"float","value":1e-307}]"#,
        ];
        for metadata in cases {
            let schema = SchemaNode::group("Root", vec![physical_scalar(metadata)]);
            let original = schema.clone();
            assert_eq!(
                serialize_embedded_schema(&schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES),
                Err(EmbeddedSchemaError::MetadataChanged {
                    schema: "Root".into()
                })
            );
            assert_eq!(schema, original);
        }
        let alternative: SchemaNode = serde_json::from_str(
            r#"{"name":"AlternativeRoot","kind":{"kind":"group","children":[{"name":"Out","kind":{"kind":"scalar","ty":"float"}}],"alternatives":[{"name":"Selected","members":["Out"],"constraints":[{"member":"Out","value":{"type":"float","value":1e-307}}]}]}}"#,
        )
        .expect("physical alternative schema is valid");
        assert_eq!(
            serialize_embedded_schema(&alternative, MAX_EMBEDDED_JSON_SCHEMA_BYTES),
            Err(EmbeddedSchemaError::MetadataChanged {
                schema: "AlternativeRoot".into()
            })
        );
    }

    #[test]
    fn retains_stable_metadata_and_library_constructed_values() {
        for value in [0.0, -0.0, 1.5, f64::MAX, f64::MIN_POSITIVE] {
            let finite = FiniteF64::new(value).expect("test value is finite");
            let values =
                JsonAllowedValues::new([JsonAllowedValue::Int(1), JsonAllowedValue::Float(finite)])
                    .expect("test values are canonical and distinct");
            let schema = SchemaNode::scalar("Amount", ScalarType::Float)
                .with_json_allowed_values(values)
                .expect("allowed value has the scalar domain");
            let encoded = serialize_embedded_schema(&schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES)
                .expect("stable schema encodes");
            let decoded: SchemaNode = serde_json::from_str(&encoded).expect("schema decodes");
            assert_eq!(schema, decoded);

            let range = NumberRange::new(Some(NumberBound::inclusive(finite)), None)
                .expect("inclusive minimum is valid");
            let schema = SchemaNode::scalar("Amount", ScalarType::Float)
                .with_numeric_range(NumericRange::Number(range))
                .expect("range has the scalar domain");
            let encoded = serialize_embedded_schema(&schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES)
                .expect("stable range encodes");
            let decoded: SchemaNode = serde_json::from_str(&encoded).expect("range decodes");
            let Some(NumericRange::Number(range)) = decoded.numeric_range else {
                panic!("decoded range is present");
            };
            assert_eq!(
                range.minimum().unwrap().value().get().to_bits(),
                value.to_bits(),
                "signed zero and finite endpoints retain their bits"
            );
        }
        let schema = physical_scalar(
            r#""numeric_range":{"kind":"number","bounds":{"minimum":{"value":1.5},"maximum":{"value":2.5,"exclusive":true}}}"#,
        );
        assert!(matches!(
            schema.numeric_range,
            Some(NumericRange::Number(_))
        ));
        assert!(serialize_embedded_schema(&schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES).is_ok());
    }

    #[test]
    fn canonical_descriptor_comparison_distinguishes_signed_zero() {
        let make = |value| {
            SchemaNode::scalar("Amount", ScalarType::Float)
                .with_numeric_range(NumericRange::Number(
                    NumberRange::new(
                        Some(NumberBound::inclusive(FiniteF64::new(value).unwrap())),
                        None,
                    )
                    .unwrap(),
                ))
                .unwrap()
        };
        let negative = make(-0.0);
        let positive = make(0.0);
        assert_eq!(negative, positive, "structural equality hides the sign");
        let descriptor = serde_json::to_string(&negative).unwrap();
        assert!(!metadata_is_unchanged(&negative, &positive, &descriptor).unwrap());
        assert!(metadata_is_unchanged(&negative, &negative, &descriptor).unwrap());
    }

    #[test]
    fn enforces_selected_schema_byte_limit_before_parsing() {
        let make = |bytes| {
            let mut schema = SchemaNode::scalar("Padding", ScalarType::String);
            schema.fixed = Some(String::new());
            let overhead = serde_json::to_string(&schema).unwrap().len();
            schema.fixed = Some("x".repeat(bytes - overhead));
            assert_eq!(serde_json::to_string(&schema).unwrap().len(), bytes);
            schema
        };
        for max in [
            MAX_EMBEDDED_JSON_SCHEMA_BYTES,
            MAX_EMBEDDED_XML_SCHEMA_BYTES,
        ] {
            assert_eq!(
                serialize_embedded_schema(&make(max), max).unwrap().len(),
                max
            );
            assert_eq!(
                serialize_embedded_schema(&make(max + 1), max),
                Err(EmbeddedSchemaError::TooLarge {
                    schema: "Padding".into(),
                    bytes: max + 1,
                    max
                })
            );
        }
        let schema = make(MAX_EMBEDDED_JSON_SCHEMA_BYTES + 1);
        assert!(serialize_embedded_schema(&schema, MAX_EMBEDDED_XML_SCHEMA_BYTES).is_ok());
    }
}
