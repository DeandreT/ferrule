use codegen_schema::json5_profile;
use format_json::JsonFormatError;
use format_json::json5_boundary::{MAX_ORIGINAL_BYTES, normalize};
use ir::{Instance, SchemaNode};

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

pub(super) fn schemas(
    source: &str,
    target: &str,
) -> Result<(SchemaNode, SchemaNode), Json5BoundaryError> {
    let source = json5_profile::decode_schema(source).map_err(Json5BoundaryError::SourceSchema)?;
    let target = json5_profile::decode_schema(target).map_err(Json5BoundaryError::TargetSchema)?;
    Ok((source, target))
}

pub(super) fn project(schema: &SchemaNode, document: &str) -> Result<Instance, Json5BoundaryError> {
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
    format_json::from_str(&normalized, schema).map_err(Json5BoundaryError::Input)
}

pub(super) fn serialize(
    schema: &SchemaNode,
    output: &Instance,
) -> Result<String, Json5BoundaryError> {
    // Use the exact existing strict writer normalization without reparsing floats.
    let value = format_json::to_value(schema, output).map_err(Json5BoundaryError::Output)?;
    if !value.is_object() {
        return Err(Json5BoundaryError::Output(shape(
            schema,
            value_kind(&value),
        )));
    }
    let mut document =
        serde_json::to_string_pretty(&value).map_err(Json5BoundaryError::OutputSerialization)?;
    document.push('\n');
    check_output_size(document.len())?;
    Ok(document)
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
