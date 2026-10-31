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

/// Graph-level JSON functions take plain `SchemaNode` text without a file
/// capability or descriptor version, so an older reader would silently drop
/// tuple frame metadata; frames therefore never enter these strings.
pub(crate) fn encode_schema(
    schema: &SchemaNode,
    unresolved_schema_reference: Option<&str>,
) -> Result<String, serde_json::Error> {
    if schema.contains_json_tuple() {
        return Err(serde::ser::Error::custom(format!(
            "schema `{}` contains a JSON tuple frame; JSON string parser schemas do not support JSON tuple frames",
            schema.name
        )));
    }
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

#[cfg(test)]
mod tests {
    use ir::{ItemCountRange, JsonTupleLayout, JsonTupleTail, ScalarType, SchemaNode};

    use super::{decode_schema, encode_schema};

    fn document(framed: bool) -> SchemaNode {
        let mut point = SchemaNode::group(
            "Point",
            vec![
                SchemaNode::scalar("item1", ScalarType::String),
                SchemaNode::scalar("item2", ScalarType::Int),
            ],
        );
        if framed {
            point = point
                .with_json_tuple(JsonTupleLayout {
                    length: ItemCountRange::new(0, Some(2)),
                    tail: JsonTupleTail::Closed,
                })
                .unwrap();
        }
        SchemaNode::group(
            "Document",
            vec![SchemaNode::scalar("Head", ScalarType::String), point],
        )
    }

    #[test]
    fn tuple_frames_never_enter_parser_schema_strings() {
        for reference in [None, Some("source.schema.json")] {
            let error = encode_schema(&document(true), reference).unwrap_err();
            assert!(error.to_string().contains("JSON tuple frame"), "{error}");

            let ordinary = document(false);
            let encoded = encode_schema(&ordinary, reference).unwrap();
            assert_eq!(
                decode_schema(&encoded).unwrap(),
                (ordinary, reference.map(str::to_string))
            );
        }
    }
}
