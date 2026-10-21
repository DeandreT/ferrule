use std::collections::BTreeMap;
use std::sync::{Arc, PoisonError, RwLock};

use ir::{Instance, SchemaNode};

use super::{
    JsonBoundaryError, JsonDocumentError, JsonOutputFormat, MAX_EMBEDDED_JSON_SCHEMA_BYTES,
    MAX_JSON_DOCUMENT_BYTES, check_input_size, constraint_input_error, constraint_output_error,
    parse_schema, unique_items, validate_instance_document, validate_json_document,
    without_boundary_constraints,
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
            format_json::json_schema::unique_items::validate_raw_json_unique_items(
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
        let mut document = match format {
            JsonOutputFormat::Pretty => serde_json::to_string_pretty(&value),
            JsonOutputFormat::Compact => serde_json::to_string(&value),
        }
        .map_err(|error| JsonBoundaryError::InvalidOutput {
            message: error.to_string(),
        })?;
        if matches!(format, JsonOutputFormat::Pretty) {
            document.push('\n');
        }
        if document.len() > MAX_JSON_DOCUMENT_BYTES {
            return Err(JsonBoundaryError::OutputTooLarge {
                bytes: document.len(),
                max: MAX_JSON_DOCUMENT_BYTES,
            });
        }
        // Validate normalized output values directly. Parsing the text again
        // can move a float by one ULP with serde_json's default number reader,
        // changing allowed-value, multiple-of, or structural-uniqueness results.
        unique_items::validate_document(&self.schema, &value)
            .map_err(unique_items::output_error)?;
        validate_json_document(&self.schema, &value).map_err(constraint_output_error)?;
        Ok(document)
    }
}

/// Process-wide prepared schemas for the descriptor-taking functions.
///
/// Generated mappings pass their finite set of emitter-owned descriptor
/// constants to those functions, so each descriptor is prepared once per
/// process. Only successful preparations are retained: a rejected descriptor
/// is decoded again by every call and returns its original error. The
/// registry holds at most [`MAX_SHARED_SCHEMAS`] descriptors totalling
/// [`MAX_SHARED_DESCRIPTOR_BYTES`]; admitting one more starts it over. These
/// bounds count descriptor text, not the memory of the prepared schemas,
/// which is typically an order of magnitude larger.
struct Registry {
    entries: BTreeMap<Box<str>, Arc<PreparedJsonSchema>>,
    descriptor_bytes: usize,
}

const MAX_SHARED_SCHEMAS: usize = 64;
const MAX_SHARED_DESCRIPTOR_BYTES: usize = 4 * 1024 * 1024;

static REGISTRY: RwLock<Registry> = RwLock::new(Registry {
    entries: BTreeMap::new(),
    descriptor_bytes: 0,
});

/// Returns the shared preparation of `descriptor`, preparing it on first use.
pub(super) fn shared(descriptor: &str) -> Result<Arc<PreparedJsonSchema>, JsonBoundaryError> {
    if descriptor.len() > MAX_EMBEDDED_JSON_SCHEMA_BYTES {
        // Rejected before any decoding; never retained.
        return PreparedJsonSchema::prepare(descriptor).map(Arc::new);
    }
    if let Some(prepared) = REGISTRY
        .read()
        .unwrap_or_else(PoisonError::into_inner)
        .entries
        .get(descriptor)
    {
        return Ok(Arc::clone(prepared));
    }
    // Prepare outside the lock. Concurrent first calls may both prepare; the
    // first inserted value is the one every later call shares.
    let prepared = Arc::new(PreparedJsonSchema::prepare(descriptor)?);
    let mut registry = REGISTRY.write().unwrap_or_else(PoisonError::into_inner);
    if let Some(existing) = registry.entries.get(descriptor) {
        return Ok(Arc::clone(existing));
    }
    if registry.entries.len() >= MAX_SHARED_SCHEMAS
        || registry.descriptor_bytes + descriptor.len() > MAX_SHARED_DESCRIPTOR_BYTES
    {
        registry.entries.clear();
        registry.descriptor_bytes = 0;
    }
    registry.descriptor_bytes += descriptor.len();
    registry
        .entries
        .insert(descriptor.into(), Arc::clone(&prepared));
    Ok(prepared)
}
