//! The direct `unevaluatedItems` form (`docs/design/json-tuple-arrays.md`).
//!
//! On a concrete array schema without composition, conditionals, `contains`,
//! dependent schemas or a reference sibling, the evaluated items are known
//! from the schema alone: either every item is evaluated and
//! `unevaluatedItems` has no effect, or it is the tail after the positional
//! members. Every other active form depends on annotations and is refused.

use super::{files, parse, positional_items, unsupported_union};
use crate::JsonFormatError;

const KEYWORD: &str = "unevaluatedItems";

/// Keywords whose evaluated items or annotations the direct form cannot see.
const ANNOTATION_SOURCES: [&str; 12] = [
    "allOf",
    "anyOf",
    "oneOf",
    "not",
    "if",
    "then",
    "else",
    "dependentSchemas",
    "contains",
    "$ref",
    "$recursiveRef",
    "$dynamicRef",
];

/// Refuses an active `unevaluatedItems` outside the direct form. The schema
/// object is checked on its own, so a referenced array schema qualifies.
pub(super) fn validate_direct(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let Some(value) = active(schema) else {
        return Ok(());
    };
    if let Some(keyword) = ANNOTATION_SOURCES
        .into_iter()
        .find(|keyword| schema.get(keyword).is_some())
    {
        return Err(unsupported_union(
            name,
            &format!("`{KEYWORD}` beside `{keyword}` depends on annotations and is not supported"),
        ));
    }
    if !positional_items::is_direct_array_schema(schema) {
        return Err(unsupported_union(
            name,
            &format!("`{KEYWORD}` requires a direct concrete array schema"),
        ));
    }
    if !value.is_object() && !value.is_boolean() {
        return Err(unsupported_union(
            name,
            &format!("`{KEYWORD}` must be a JSON Schema object or boolean"),
        ));
    }
    Ok(())
}

/// The `unevaluatedItems` schema that acts as the tail of a direct array
/// schema: after its positional members, or as its item schema when it has
/// none. When every item is already evaluated, the schema is parsed for
/// validation and has no effect.
pub(super) fn tail<'a>(
    name: &str,
    schema: &'a serde_json::Value,
    doc: &files::LoadedSchema,
    active_refs: &mut Vec<String>,
) -> Result<Option<&'a serde_json::Value>, JsonFormatError> {
    let Some(value) = active(schema) else {
        return Ok(None);
    };
    if !evaluates_every_item(schema) {
        return Ok(Some(value));
    }
    if value.is_object() {
        parse(name, value, doc, active_refs)?;
    }
    Ok(None)
}

fn active(schema: &serde_json::Value) -> Option<&serde_json::Value> {
    files::validation_dialect(schema)
        .supports_unevaluated_items()
        .then(|| schema.get(KEYWORD))
        .flatten()
}

/// Schema-valued `items` evaluates every remaining item in any active
/// dialect, and so does `additionalItems` after tuple-form `items`.
fn evaluates_every_item(schema: &serde_json::Value) -> bool {
    match schema.get("items") {
        Some(serde_json::Value::Array(_)) => schema.get("additionalItems").is_some(),
        Some(_) => true,
        None => false,
    }
}
