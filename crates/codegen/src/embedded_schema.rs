use std::fmt;

use ir::SchemaNode;

/// Generated JSON entry points accept at most this many schema bytes.
pub const MAX_EMBEDDED_JSON_SCHEMA_BYTES: usize = 1024 * 1024;
/// Generated XML serialization expressions accept at most this many schema bytes.
pub const MAX_EMBEDDED_XML_SCHEMA_BYTES: usize = 8 * 1024 * 1024;

/// Failure to preserve a schema in the descriptor used by generated code.
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
    Codec {
        schema: String,
        source: codegen_schema::CodecError,
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
            Self::Codec { schema, source } => {
                write!(formatter, "embedded schema {schema:?} is invalid: {source}")
            }
        }
    }
}

impl std::error::Error for EmbeddedSchemaError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Codec { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Encodes a schema losslessly within the caller's generated runtime limit.
///
/// In particular, the default serde JSON floating-point parser can change a
/// finite bound after its shortest decimal representation is serialized. Both
/// generated backends use a versioned descriptor for those schemas. Stable
/// schemas retain their ordinary JSON descriptor byte for byte.
pub fn serialize_embedded_schema(
    schema: &SchemaNode,
    max_bytes: usize,
) -> Result<String, EmbeddedSchemaError> {
    codegen_schema::encode(schema, max_bytes).map_err(|source| {
        let schema = schema.name.clone();
        match source {
            codegen_schema::CodecError::Serialization(message) => {
                EmbeddedSchemaError::Serialization { schema, message }
            }
            codegen_schema::CodecError::Deserialization(message) => {
                EmbeddedSchemaError::Deserialization { schema, message }
            }
            codegen_schema::CodecError::MetadataChanged => {
                EmbeddedSchemaError::MetadataChanged { schema }
            }
            codegen_schema::CodecError::TooLarge { bytes, max } => {
                EmbeddedSchemaError::TooLarge { schema, bytes, max }
            }
            source => EmbeddedSchemaError::Codec { schema, source },
        }
    })
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
    fn preserves_changed_nested_floating_metadata_without_changing_the_schema() {
        let cases = [
            r#""numeric_range":{"kind":"number","bounds":{"minimum":{"value":1e-307}}}"#,
            r#""json_allowed_values":[{"type":"int","value":0},{"type":"float","value":1e-307}]"#,
        ];
        for metadata in cases {
            let schema = SchemaNode::group("Root", vec![physical_scalar(metadata)]);
            let original = schema.clone();
            let encoded = serialize_embedded_schema(&schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES)
                .expect("unstable metadata encodes losslessly");
            assert!(encoded.starts_with(codegen_schema::V2_PREFIX));
            let decoded = codegen_schema::decode(&encoded, MAX_EMBEDDED_JSON_SCHEMA_BYTES)
                .expect("lossless metadata decodes");
            assert_eq!(
                serde_json::to_value(&schema).unwrap(),
                serde_json::to_value(&decoded).unwrap()
            );
            assert_eq!(schema, original);
        }
        let alternative: SchemaNode = serde_json::from_str(
            r#"{"name":"AlternativeRoot","kind":{"kind":"group","children":[{"name":"Out","kind":{"kind":"scalar","ty":"float"}}],"alternatives":[{"name":"Selected","members":["Out"],"constraints":[{"member":"Out","value":{"type":"float","value":1e-307}}]}]}}"#,
        )
        .expect("physical alternative schema is valid");
        let encoded = serialize_embedded_schema(&alternative, MAX_EMBEDDED_JSON_SCHEMA_BYTES)
            .expect("floating alternative encodes");
        assert!(encoded.starts_with(codegen_schema::V2_PREFIX));
        let decoded = codegen_schema::decode(&encoded, MAX_EMBEDDED_JSON_SCHEMA_BYTES).unwrap();
        assert_eq!(
            serde_json::to_value(&alternative).unwrap(),
            serde_json::to_value(&decoded).unwrap()
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
            assert_eq!(encoded, serde_json::to_string(&schema).unwrap());
            let decoded = codegen_schema::decode(&encoded, MAX_EMBEDDED_JSON_SCHEMA_BYTES)
                .expect("schema decodes");
            assert_eq!(schema, decoded);

            let range = NumberRange::new(Some(NumberBound::inclusive(finite)), None)
                .expect("inclusive minimum is valid");
            let schema = SchemaNode::scalar("Amount", ScalarType::Float)
                .with_numeric_range(NumericRange::Number(range))
                .expect("range has the scalar domain");
            let encoded = serialize_embedded_schema(&schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES)
                .expect("stable range encodes");
            let decoded = codegen_schema::decode(&encoded, MAX_EMBEDDED_JSON_SCHEMA_BYTES)
                .expect("range decodes");
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
    fn lossless_descriptors_distinguish_signed_zero() {
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
        let negative =
            serialize_embedded_schema(&negative, MAX_EMBEDDED_JSON_SCHEMA_BYTES).unwrap();
        let positive =
            serialize_embedded_schema(&positive, MAX_EMBEDDED_JSON_SCHEMA_BYTES).unwrap();
        assert_ne!(negative, positive, "descriptor preserves the sign of zero");
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

    #[test]
    fn counts_lossless_prefix_and_markers_in_selected_byte_limit() {
        let mut padding = SchemaNode::scalar("Padding", ScalarType::String);
        padding.fixed = Some(String::new());
        let base = SchemaNode::group(
            "Root",
            vec![
                physical_scalar(
                    r#""numeric_range":{"kind":"number","bounds":{"minimum":{"value":1e-307}}}"#,
                ),
                padding,
            ],
        );
        let overhead = serialize_embedded_schema(&base, usize::MAX).unwrap().len();
        for max in [
            MAX_EMBEDDED_JSON_SCHEMA_BYTES,
            MAX_EMBEDDED_XML_SCHEMA_BYTES,
        ] {
            let mut schema = base.clone();
            let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
                unreachable!()
            };
            children[1].fixed = Some("x".repeat(max - overhead));
            let encoded = serialize_embedded_schema(&schema, max).unwrap();
            assert!(encoded.starts_with(codegen_schema::V2_PREFIX));
            assert_eq!(encoded.len(), max);
            let ir::SchemaKind::Group { children, .. } = &mut schema.kind else {
                unreachable!()
            };
            children[1].fixed.as_mut().unwrap().push('x');
            assert_eq!(
                serialize_embedded_schema(&schema, max),
                Err(EmbeddedSchemaError::TooLarge {
                    schema: "Root".into(),
                    bytes: max + 1,
                    max,
                })
            );
        }
    }

    #[test]
    fn codec_failures_retain_schema_identity_and_structured_source() {
        let mut schema = SchemaNode::scalar("Leaf", ScalarType::Int);
        for _ in 0..128 {
            schema = SchemaNode::group("Nested", vec![schema]);
        }
        schema.name = "DeepRoot".into();
        let error = serialize_embedded_schema(&schema, MAX_EMBEDDED_JSON_SCHEMA_BYTES).unwrap_err();
        assert!(matches!(error, EmbeddedSchemaError::Codec {
            ref schema,
            source: codegen_schema::CodecError::DepthLimit { depth: 129, max: 128 },
        } if schema == "DeepRoot"));
        assert!(error.to_string().contains("DeepRoot"));
        assert!(std::error::Error::source(&error).is_some());
    }
}
