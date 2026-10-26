use std::sync::Arc;

use ir::{Instance, SchemaNode};

use crate::descriptor_registry::DescriptorRegistry;

use super::{
    JsonBoundaryError, JsonDocumentError, JsonOutputFormat, MAX_JSON_DOCUMENT_BYTES,
    check_input_size, constraint_input_error, constraint_output_error, parse_schema, text,
    unique_items, validate_instance_document, validate_json_document, without_boundary_constraints,
};

/// One embedded JSON schema descriptor, decoded and validated once.
///
/// The prepared value is immutable and can be shared by repeated and
/// concurrent calls. Each call keeps its own work accounting: the document
/// byte limit, its pattern work budget, and its output byte limit. Calls
/// through a prepared value return the same results and errors as the
/// descriptor-taking functions such as [`super::parse_json`]; only the
/// descriptor's own decoding and validation are no longer repeated.
pub struct PreparedJsonSchema {
    /// The decoded schema with its boundary constraints.
    schema: SchemaNode,
    /// The same schema without boundary constraints, for the format reader
    /// and writer, which apply their own checks.
    formatter: format_json::PreparedSchema,
    unique_items: bool,
}

impl PreparedJsonSchema {
    /// Decodes and validates an emitter-owned descriptor.
    pub fn prepare(descriptor: &str) -> Result<Self, JsonBoundaryError> {
        let schema = parse_schema(descriptor)?;
        if !schema.json_pattern_budget_is_valid() {
            return Err(JsonBoundaryError::InvalidEmbeddedSchema {
                message:
                    "schema-wide JSON pattern metadata, program, or fixed-value work budget is invalid"
                        .to_string(),
            });
        }
        let unique_items = unique_items::schema_contains_assertion(&schema);
        let formatter =
            format_json::PreparedSchema::new(without_boundary_constraints(schema.clone()));
        Ok(Self {
            schema,
            formatter,
            unique_items,
        })
    }

    /// Parses one bounded JSON document and applies embedded constraints
    /// against the normalized typed instance.
    pub fn parse_json(&self, document: &str) -> Result<Instance, JsonBoundaryError> {
        self.parse_document(document, |error| JsonBoundaryError::InvalidInput {
            message: error.to_string(),
        })
    }

    /// Parses one bounded JSON document like [`super::parse_json_with_format_cause`].
    pub fn parse_json_with_format_cause(
        &self,
        document: &str,
    ) -> Result<Instance, JsonBoundaryError> {
        self.parse_document(document, |error| {
            JsonBoundaryError::InvalidDocument(JsonDocumentError::new(error))
        })
    }

    fn parse_document(
        &self,
        document: &str,
        invalid: fn(format_json::JsonFormatError) -> JsonBoundaryError,
    ) -> Result<Instance, JsonBoundaryError> {
        check_input_size(document.len())?;
        if self.unique_items {
            format_json::runtime_constraints::validate_raw_json_unique_items(
                &self.schema,
                document,
            )
            .map_err(invalid)?;
        }
        let instance = self.formatter.from_str(document).map_err(invalid)?;
        validate_instance_document(&self.schema, &instance).map_err(constraint_input_error)?;
        Ok(instance)
    }

    /// Parses one bounded UTF-8 JSON payload, like [`super::parse_json_bytes`].
    pub fn parse_json_bytes(&self, document: &[u8]) -> Result<Instance, JsonBoundaryError> {
        check_input_size(document.len())?;
        let document =
            std::str::from_utf8(document).map_err(|error| JsonBoundaryError::InvalidInput {
                message: format!("document is not UTF-8: {error}"),
            })?;
        self.parse_json(document)
    }

    /// Parses one bounded UTF-8 JSON payload, like
    /// [`super::parse_json_bytes_with_utf8_cause`].
    pub fn parse_json_bytes_with_utf8_cause(
        &self,
        document: &[u8],
    ) -> Result<Instance, JsonBoundaryError> {
        check_input_size(document.len())?;
        let document = std::str::from_utf8(document).map_err(JsonBoundaryError::InvalidUtf8)?;
        self.parse_json(document)
    }

    /// Serializes one instance as a bounded pretty-printed JSON document.
    pub fn serialize_json(&self, instance: &Instance) -> Result<String, JsonBoundaryError> {
        self.serialize(instance, JsonOutputFormat::Pretty)
    }

    /// Serializes one instance as a bounded pretty-printed UTF-8 JSON payload.
    pub fn serialize_json_bytes(&self, instance: &Instance) -> Result<Vec<u8>, JsonBoundaryError> {
        self.serialize_json(instance).map(String::into_bytes)
    }

    /// Serializes one instance as bounded compact JSON without a trailing newline.
    pub fn serialize_json_compact(&self, instance: &Instance) -> Result<String, JsonBoundaryError> {
        self.serialize(instance, JsonOutputFormat::Compact)
    }

    /// Serializes one instance as bounded compact UTF-8 JSON without a trailing newline.
    pub fn serialize_json_compact_bytes(
        &self,
        instance: &Instance,
    ) -> Result<Vec<u8>, JsonBoundaryError> {
        self.serialize_json_compact(instance)
            .map(String::into_bytes)
    }

    /// Serializes one instance and applies embedded constraints after all
    /// JSON scalar normalization and coercion.
    fn serialize(
        &self,
        instance: &Instance,
        format: JsonOutputFormat,
    ) -> Result<String, JsonBoundaryError> {
        let value = self.formatter.to_value(instance).map_err(|error| {
            JsonBoundaryError::InvalidOutput {
                message: error.to_string(),
            }
        })?;
        let invalid = |error: serde_json::Error| JsonBoundaryError::InvalidOutput {
            message: error.to_string(),
        };
        // The byte limit applies to the measured text before it is written.
        let bytes = text::measure(&value, format).map_err(invalid)?;
        if bytes > MAX_JSON_DOCUMENT_BYTES {
            return Err(JsonBoundaryError::OutputTooLarge {
                bytes,
                max: MAX_JSON_DOCUMENT_BYTES,
            });
        }
        let document = text::render(&value, format, bytes).map_err(invalid)?;
        // Validate normalized output values directly. Parsing the text again
        // can move a float by one ULP with serde_json's default number reader,
        // changing allowed-value, multiple-of, or structural-uniqueness results.
        unique_items::validate_document(&self.schema, &value)
            .map_err(unique_items::output_error)?;
        validate_json_document(&self.schema, &value).map_err(constraint_output_error)?;
        Ok(document)
    }
}

/// Process-wide prepared schemas for the descriptor-taking functions, which
/// generated mappings call with their descriptor constants.
static REGISTRY: DescriptorRegistry<PreparedJsonSchema> =
    DescriptorRegistry::new(MAX_SHARED_DESCRIPTOR_BYTES);

const MAX_SHARED_DESCRIPTOR_BYTES: usize = 4 * 1024 * 1024;

/// Returns the shared preparation of `descriptor`, preparing it on first use.
pub(super) fn shared(descriptor: &str) -> Result<Arc<PreparedJsonSchema>, JsonBoundaryError> {
    REGISTRY.get_or_prepare(descriptor, PreparedJsonSchema::prepare)
}
