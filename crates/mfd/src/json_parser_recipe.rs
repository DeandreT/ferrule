//! Provenance carried alongside a JSON string parser's embedded schema.
//!
//! The parser function still receives a `SchemaNode` JSON descriptor. Its
//! deserializer ignores this one extra top-level field, so the metadata does
//! not participate in execution or change the parsed schema.

use ir::SchemaNode;
use serde::{Deserialize, Serialize};

const METADATA_FIELD: &str = "ferrule:json-parser-recipe";
const METADATA_VERSION: u8 = 1;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct RecipeMetadata {
    version: u8,
    unresolved_schema_reference: String,
}

pub(crate) fn encode_schema(
    schema: &SchemaNode,
    unresolved_schema_reference: Option<&str>,
) -> Result<String, serde_json::Error> {
    let Some(reference) = unresolved_schema_reference else {
        return serde_json::to_string(schema);
    };
    let mut value = serde_json::to_value(schema)?;
    let object = value
        .as_object_mut()
        .expect("SchemaNode serializes as a JSON object");
    object.insert(
        METADATA_FIELD.to_string(),
        serde_json::to_value(RecipeMetadata {
            version: METADATA_VERSION,
            unresolved_schema_reference: reference.to_string(),
        })?,
    );
    serde_json::to_string(&value)
}

pub(crate) fn decode_schema(text: &str) -> Result<(SchemaNode, Option<String>), String> {
    let value: serde_json::Value =
        serde_json::from_str(text).map_err(|_| "schema descriptor is invalid".to_string())?;
    let reference = value
        .get(METADATA_FIELD)
        .map(|metadata| {
            serde_json::from_value::<RecipeMetadata>(metadata.clone())
                .map_err(|_| "JSON string parser recipe metadata is invalid".to_string())
                .and_then(|metadata| {
                    if metadata.version != METADATA_VERSION {
                        return Err(
                            "JSON string parser recipe metadata version is unsupported".to_string()
                        );
                    }
                    validate_reference(&metadata.unresolved_schema_reference)?;
                    Ok(metadata.unresolved_schema_reference)
                })
        })
        .transpose()?;
    let schema =
        serde_json::from_value(value).map_err(|_| "schema descriptor is invalid".to_string())?;
    Ok((schema, reference))
}

pub(crate) fn contains_metadata(text: &str) -> bool {
    text.contains(METADATA_FIELD)
        || serde_json::from_str::<serde_json::Value>(text)
            .ok()
            .and_then(|value| value.get(METADATA_FIELD).cloned())
            .is_some()
}

pub(crate) fn validate_reference(reference: &str) -> Result<(), String> {
    if reference.is_empty() || reference.len() > 4096 || reference.chars().any(char::is_control) {
        return Err(
            "unresolved JSON Schema reference must be 1 to 4096 bytes without control characters"
                .to_string(),
        );
    }
    Ok(())
}
