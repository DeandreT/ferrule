use ir::{SchemaKind, SchemaNode};

use super::{dependent_schemas, files};
use crate::JsonFormatError;

#[derive(Clone, Copy)]
enum ElseMode {
    Unconstrained,
    Never,
}

struct Selected<'a> {
    trigger: String,
    predicate: Option<&'a serde_json::Value>,
    condition_is_object: bool,
    else_mode: ElseMode,
}

#[derive(Clone, Copy)]
enum OuterObjectType {
    NonNullable,
    Nullable,
}

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
    let Some(selected) = selected(name, schema)? else {
        return Ok(());
    };
    let Some(outer_type) = explicit_object_type(schema) else {
        return Err(unsupported(
            name,
            "presence-based conditionals require an explicit `type: \"object\"` or `type: [\"object\", \"null\"]` schema",
        ));
    };
    if admits_non_objects || node.repeating || !matches!(&node.kind, SchemaKind::Group { .. }) {
        return Err(unsupported(
            name,
            "presence-based conditionals require a concrete object schema",
        ));
    }
    if matches!(outer_type, OuterObjectType::NonNullable) && node.container_nullable {
        return Err(unsupported(
            name,
            "a non-null object conditional cannot be attached to a nullable imported shape",
        ));
    }
    if matches!(outer_type, OuterObjectType::Nullable) && !selected.condition_is_object {
        return Err(unsupported(
            name,
            "a nullable object conditional must explicitly declare `type: \"object\"` inside `if` so null selects `else`",
        ));
    }

    match selected.else_mode {
        ElseMode::Unconstrained => {
            if let Some(predicate) = selected.predicate {
                dependent_schemas::apply_triggered_schema(
                    name,
                    &selected.trigger,
                    predicate,
                    node,
                    doc,
                    active_refs,
                )?;
            }
        }
        ElseMode::Never => {
            apply_never_else(name, selected, node, doc, active_refs)?;
        }
    }
    Ok(())
}

fn selected<'a>(
    name: &str,
    schema: &'a serde_json::Value,
) -> Result<Option<Selected<'a>>, JsonFormatError> {
    if !files::validation_dialect(schema).supports_conditionals() {
        return Ok(None);
    }
    let Some(object) = schema.as_object() else {
        return Ok(None);
    };
    let Some(condition) = object.get("if") else {
        return Ok(None);
    };

    let else_mode = match object.get("else") {
        None | Some(serde_json::Value::Bool(true)) => ElseMode::Unconstrained,
        Some(serde_json::Value::Bool(false)) => ElseMode::Never,
        Some(_) => {
            return Err(unsupported(
                name,
                "presence-based conditional lowering supports only an absent, `true`, or `false` `else` schema",
            ));
        }
    };

    let predicate = match object.get("then") {
        None | Some(serde_json::Value::Bool(true)) => None,
        Some(predicate) if predicate.is_boolean() || predicate.is_object() => Some(predicate),
        Some(_) => {
            return Err(unsupported(
                name,
                "`then` must be a boolean or object schema",
            ));
        }
    };
    if predicate.is_none() && matches!(else_mode, ElseMode::Unconstrained) {
        return Ok(None);
    }

    let condition = condition.as_object().ok_or_else(|| {
        unsupported(
            name,
            "`if` must be an object schema containing exactly one required-property presence test",
        )
    })?;
    let condition_is_object = condition.get("type").is_some();
    if condition_is_object
        && condition.get("type").and_then(serde_json::Value::as_str) != Some("object")
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
    Ok(Some(Selected {
        trigger: required.to_string(),
        predicate,
        condition_is_object,
        else_mode,
    }))
}

fn explicit_object_type(schema: &serde_json::Value) -> Option<OuterObjectType> {
    let ty = schema.get("type")?;
    if ty.as_str() == Some("object") {
        return Some(OuterObjectType::NonNullable);
    }
    let types = ty.as_array()?;
    (types.len() == 2
        && types
            .iter()
            .any(|candidate| candidate.as_str() == Some("object"))
        && types
            .iter()
            .any(|candidate| candidate.as_str() == Some("null")))
    .then_some(OuterObjectType::Nullable)
}

fn apply_never_else(
    name: &str,
    selected: Selected<'_>,
    node: &mut SchemaNode,
    doc: &serde_json::Value,
    active_refs: &mut Vec<String>,
) -> Result<(), JsonFormatError> {
    let SchemaKind::Group {
        children,
        alternatives,
        dynamic,
        ..
    } = &node.kind
    else {
        return Err(unsupported(
            name,
            "`else: false` requires a concrete object schema",
        ));
    };
    if !alternatives.is_empty() {
        return Err(unsupported(
            name,
            "`else: false` cannot yet be intersected exactly with existing object alternatives",
        ));
    }
    if dynamic.is_none() && !children.iter().any(|child| child.name == selected.trigger) {
        return Err(unsupported(
            name,
            &format!(
                "`else: false` requires trigger property `{}` but the closed object does not declare it",
                selected.trigger
            ),
        ));
    }

    let mut candidate = node.clone();
    candidate.container_nullable = false;
    let mut required = candidate.required_fields().to_vec();
    if !required.contains(&selected.trigger) {
        required.push(selected.trigger.clone());
    }
    if !candidate.set_required_fields(required) {
        return Err(unsupported(
            name,
            &format!(
                "`else: false` trigger property `{}` conflicts with object requirements or validation metadata",
                selected.trigger
            ),
        ));
    }
    if let Some(predicate) = selected.predicate {
        dependent_schemas::apply_triggered_schema(
            name,
            &selected.trigger,
            predicate,
            &mut candidate,
            doc,
            active_refs,
        )?;
    }
    *node = candidate;
    Ok(())
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaObject {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}
