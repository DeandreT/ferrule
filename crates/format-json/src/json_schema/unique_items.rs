use ir::SchemaNode;

use super::unsupported_union;
use crate::JsonFormatError;

// Compatibility paths for downstream users; execution has one runtime owner.
pub use crate::runtime_constraints::{
    UniqueItemsValidationError, validate_raw_json_unique_items, validate_unique_json_items,
};

pub(super) fn has_keyword(schema: &serde_json::Value) -> bool {
    schema.get("uniqueItems").is_some()
}

pub(super) fn selected(name: &str, schema: &serde_json::Value) -> Result<bool, JsonFormatError> {
    match schema.get("uniqueItems") {
        None => Ok(false),
        Some(serde_json::Value::Bool(value)) => Ok(*value),
        Some(_) => Err(unsupported_union(name, "uniqueItems must be a boolean")),
    }
}

pub(super) fn apply(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
    ambiguous_without_array: bool,
) -> Result<(), JsonFormatError> {
    if !selected(name, schema)? {
        return Ok(());
    }
    if node.repeating {
        node.json_unique_items = true;
    } else if ambiguous_without_array {
        return Err(unsupported_union(
            name,
            "uniqueItems: true without a concrete array type also admits unconstrained non-array values",
        ));
    }
    Ok(())
}

pub(super) fn validate_ignored(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    selected(name, schema)?;
    Ok(())
}

pub(super) fn render(node: &SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    if node.json_unique_items {
        out.insert("uniqueItems".into(), true.into());
    }
}
