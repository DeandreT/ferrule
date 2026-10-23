use std::sync::Arc;

use codegen_schema::json5_profile::{self, Json5ProfileError};
use format_json::json5_boundary::{MAX_ORIGINAL_BYTES, normalize};
use format_json::{JsonFormatError, PreparedSchema};
use ir::{Instance, SchemaNode};

use crate::descriptor_registry::DescriptorRegistry;

use super::{Json5BoundaryError, Json5BoundaryResource};

pub(super) fn check_original_size(bytes: usize) -> Result<(), Json5BoundaryError> {
    limit(
        bytes,
        MAX_ORIGINAL_BYTES,
        Json5BoundaryResource::OriginalDocumentBytes,
    )
}

pub(super) fn check_output_size(bytes: usize) -> Result<(), Json5BoundaryError> {
    limit(
        bytes,
        crate::MAX_JSON_DOCUMENT_BYTES,
        Json5BoundaryResource::OutputDocumentBytes,
    )
}

fn limit(
    requested: usize,
    max: usize,
    resource: Json5BoundaryResource,
) -> Result<(), Json5BoundaryError> {
    if requested > max {
        Err(Json5BoundaryError::Limit {
            resource,
            requested,
            max,
        })
    } else {
        Ok(())
    }
}

/// Decoded JSON5 profile descriptors, shared by both sides of every call.
/// Each call keeps its own document, normalization and output accounting.
static PREPARED: DescriptorRegistry<PreparedSchema> =
    DescriptorRegistry::new(4 * json5_profile::MAX_DESCRIPTOR_BYTES);

fn prepare(descriptor: &str) -> Result<PreparedSchema, Json5ProfileError> {
    json5_profile::decode_schema(descriptor).map(PreparedSchema::new)
}

pub(super) fn schemas(
    source: &str,
    target: &str,
) -> Result<(Arc<PreparedSchema>, Arc<PreparedSchema>), Json5BoundaryError> {
    let source = PREPARED
        .get_or_prepare(source, prepare)
        .map_err(Json5BoundaryError::SourceSchema)?;
    let target = PREPARED
        .get_or_prepare(target, prepare)
        .map_err(Json5BoundaryError::TargetSchema)?;
    Ok((source, target))
}

pub(super) fn project(
    prepared: &PreparedSchema,
    document: &str,
) -> Result<Instance, Json5BoundaryError> {
    let schema = prepared.schema();
    // Complete normalization rejects all finite-token/syntax failures, even in
    // fields that schema projection would otherwise discard.
    let normalized = normalize(document).map_err(Json5BoundaryError::Syntax)?;
    // The successful normalizer emits one strict root token. Do not admit the
    // ordinary formatter's convenience that maps a root array to flat rows.
    if normalized.as_bytes().first() != Some(&b'{') {
        return Err(Json5BoundaryError::Input(shape(
            schema,
            root_kind(&normalized),
        )));
    }
    prepared
        .from_str(&normalized)
        .map_err(Json5BoundaryError::Input)
}

pub(super) fn serialize(
    prepared: &PreparedSchema,
    output: &Instance,
) -> Result<String, Json5BoundaryError> {
    let schema = prepared.schema();
    // Use the exact existing strict writer normalization without reparsing floats.
    let value = prepared
        .to_value(output)
        .map_err(Json5BoundaryError::Output)?;
    if !value.is_object() {
        return Err(Json5BoundaryError::Output(shape(
            schema,
            value_kind(&value),
        )));
    }
    crate::json::text::pretty_with_limit(
        &value,
        Json5BoundaryError::OutputSerialization,
        check_output_size,
    )
}

fn shape(schema: &SchemaNode, got: &'static str) -> JsonFormatError {
    JsonFormatError::Shape {
        name: schema.name.clone(),
        expected: "object",
        got,
    }
}

fn root_kind(normalized: &str) -> &'static str {
    match normalized.as_bytes().first() {
        Some(b'[') => "array",
        Some(b'"') => "string",
        Some(b't' | b'f') => "bool",
        Some(b'n') => "null",
        Some(b'-' | b'0'..=b'9') => "number",
        _ => "invalid normalized root",
    }
}

fn value_kind(value: &serde_json::Value) -> &'static str {
    match value {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}
