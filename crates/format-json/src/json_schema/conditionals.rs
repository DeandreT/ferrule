use ir::{SchemaKind, SchemaNode};

use super::{dependent_schemas, files};
use crate::JsonFormatError;

pub(super) fn has_keywords(schema: &serde_json::Value) -> bool {
    schema.get("if").is_some() || schema.get("then").is_some() || schema.get("else").is_some()
}

pub(super) fn is_effectively_constrained(
    name: &str,
    schema: &serde_json::Value,
) -> Result<bool, JsonFormatError> {
    selected(name, schema).map(|selected| selected.is_some())
}

pub(super) fn apply(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
    admits_non_objects: bool,
) -> Result<(), JsonFormatError> {
    let Some((trigger, predicate)) = selected(name, schema)? else {
        return Ok(());
    };
    if schema.get("type").and_then(serde_json::Value::as_str) != Some("object")
        || admits_non_objects
        || node.repeating
        || node.container_nullable
        || !matches!(&node.kind, SchemaKind::Group { .. })
    {
        return Err(unsupported(
            name,
            "presence-based conditionals require an explicit non-null `type: \"object\"` schema",
        ));
    }
    dependent_schemas::apply_triggered_schema(name, &trigger, predicate, node, doc, active_refs)
}

fn selected<'a>(
    name: &str,
    schema: &'a serde_json::Value,
) -> Result<Option<(String, &'a serde_json::Value)>, JsonFormatError> {
    if !files::validation_dialect(schema).supports_conditionals() {
        return Ok(None);
    }
    let Some(object) = schema.as_object() else {
        return Ok(None);
    };
    let Some(condition) = object.get("if") else {
        return Ok(None);
    };

    match object.get("else") {
        None | Some(serde_json::Value::Bool(true)) => {}
        Some(_) => {
            return Err(unsupported(
                name,
                "presence-based conditional lowering supports only an absent or `true` `else` schema",
            ));
        }
    }

    let Some(predicate) = object.get("then") else {
        return Ok(None);
    };
    if predicate == &serde_json::Value::Bool(true) {
        return Ok(None);
    }
    if !predicate.is_boolean() && !predicate.is_object() {
        return Err(unsupported(
            name,
            "`then` must be a boolean or object schema",
        ));
    }

    let condition = condition.as_object().ok_or_else(|| {
        unsupported(
            name,
            "`if` must be an object schema containing exactly one required-property presence test",
        )
    })?;
    if condition
        .get("type")
        .is_some_and(|value| value.as_str() != Some("object"))
    {
        return Err(unsupported(
            name,
            "the supported `if` condition may declare only `type: \"object\"`",
        ));
    }
    if condition.keys().any(|keyword| {
        !files::is_internal_ref_keyword(keyword)
            && !matches!(
                keyword.as_str(),
                "type"
                    | "required"
                    | "$schema"
                    | "$id"
                    | "id"
                    | "$anchor"
                    | "$dynamicAnchor"
                    | "$comment"
                    | "$defs"
                    | "definitions"
                    | "title"
                    | "description"
                    | "default"
                    | "deprecated"
                    | "readOnly"
                    | "writeOnly"
                    | "examples"
            )
    }) {
        return Err(unsupported(
            name,
            "the supported `if` condition must test only one required property's presence; value-sensitive and composed conditions are not representable",
        ));
    }
    let required = condition
        .get("required")
        .and_then(serde_json::Value::as_array)
        .filter(|required| required.len() == 1)
        .and_then(|required| required.first())
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            unsupported(
                name,
                "the supported `if` condition must contain `required` with exactly one property name",
            )
        })?;
    Ok(Some((required.to_string(), predicate)))
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaObject {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}
