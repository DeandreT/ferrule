use ir::{SchemaKind, SchemaNode};

use super::{files, parse_required_fields, unsupported_object, unsupported_union};
use crate::JsonFormatError;

// This is a bound on the newly admitted reference-sibling subset, not on
// ordinary required declarations or legacy ignored reference siblings.
const MAX_REQUIRED_REFERENCE_NAMES: usize = 256;

fn unsupported_shape(name: &str) -> JsonFormatError {
    unsupported_union(
        name,
        "modern `$ref` sibling `required` requires an unsupported intersection and cannot be ignored",
    )
}

fn required_budget(length: usize) -> Result<(), JsonFormatError> {
    if length > MAX_REQUIRED_REFERENCE_NAMES {
        return Err(JsonFormatError::SchemaResourceLimit {
            kind: "required reference sibling names",
            limit: MAX_REQUIRED_REFERENCE_NAMES,
        });
    }
    Ok(())
}

pub(super) fn validate_siblings(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    if let Some(values) = schema.get("required").and_then(serde_json::Value::as_array) {
        required_budget(values.len())?;
    }
    parse_required_fields(name, schema)?;
    let Some(object) = schema.as_object() else {
        return Err(unsupported_shape(name));
    };
    for keyword in object.keys() {
        if files::is_internal_ref_keyword(keyword)
            || matches!(
                keyword.as_str(),
                "$ref"
                    | "required"
                    | "$schema"
                    | "$id"
                    | "$defs"
                    | "definitions"
                    | "title"
                    | "description"
                    | "$comment"
                    | "default"
                    | "examples"
                    | "deprecated"
                    | "readOnly"
                    | "writeOnly"
            )
        {
            continue;
        }
        return Err(unsupported_union(
            name,
            &format!(
                "modern `$ref` sibling `{keyword}` cannot be combined with the required-only object subset"
            ),
        ));
    }
    Ok(())
}

pub(super) fn require_concrete_object(
    name: &str,
    terminal: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let explicit_object = match terminal.get("type") {
        Some(serde_json::Value::String(ty)) => ty == "object",
        Some(serde_json::Value::Array(types)) if types.len() == 2 => {
            (types[0] == "object" && types[1] == "null")
                || (types[0] == "null" && types[1] == "object")
        }
        _ => false,
    };
    if !explicit_object
        || ["$ref", "allOf", "anyOf", "oneOf"]
            .into_iter()
            .any(|keyword| terminal.get(keyword).is_some())
        || terminal
            .get("properties")
            .is_some_and(|properties| !properties.is_object())
    {
        return Err(unsupported_shape(name));
    }
    Ok(())
}

pub(super) fn reject_unresolved(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    if schema.get("required").is_some() {
        return Err(unsupported_shape(name));
    }
    Ok(())
}

pub(super) fn apply(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
) -> Result<(), JsonFormatError> {
    if schema.get("required").is_none() {
        return Ok(());
    }
    let SchemaKind::Group {
        children,
        alternatives,
        required,
        ..
    } = &node.kind
    else {
        return Err(unsupported_shape(name));
    };
    if node.repeating || !alternatives.is_empty() {
        return Err(unsupported_shape(name));
    }
    required_budget(required.len())?;
    let names = parse_required_fields(name, schema)?;
    let mut merged = required.clone();
    for property in names {
        if !children.iter().any(|child| child.name == property) {
            return Err(unsupported_object(
                name,
                &format!(
                    "modern `$ref` sibling `required` name `{property}` must identify a declared property"
                ),
            ));
        }
        if !merged.contains(&property) {
            required_budget(merged.len() + 1)?;
            merged.push(property);
        }
    }
    if !node.set_required_fields(merged) {
        return Err(unsupported_object(
            name,
            "modern `$ref` sibling `required` conflicts with the referenced object's presence constraints",
        ));
    }
    Ok(())
}
