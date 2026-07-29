use ir::{PropertyCountRange, SchemaKind, SchemaNode};

use crate::JsonFormatError;

pub(super) fn apply(
    name: &str,
    schema: &serde_json::Value,
    node: &mut SchemaNode,
    shape_is_ambiguous: bool,
) -> Result<(), JsonFormatError> {
    if !has_keywords(schema) {
        return Ok(());
    }
    let candidate = selected(name, schema)?;
    if candidate.is_none() {
        return Ok(());
    }
    if matches!(node.kind, SchemaKind::Group { .. }) {
        node.property_count_range = intersect(name, node.property_count_range, candidate)?;
        ensure_feasible(name, node)?;
    } else if shape_is_ambiguous {
        return Err(unsupported(
            name,
            "property-count constraint without a concrete object type also admits unconstrained non-object values",
        ));
    }
    Ok(())
}

pub(super) fn validate_ignored(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    if has_keywords(schema) {
        selected(name, schema)?;
    }
    Ok(())
}

pub(super) fn has_keywords(schema: &serde_json::Value) -> bool {
    schema.get("minProperties").is_some() || schema.get("maxProperties").is_some()
}

pub(super) fn is_effectively_constrained(
    name: &str,
    schema: &serde_json::Value,
) -> Result<bool, JsonFormatError> {
    Ok(selected(name, schema)?.is_some())
}

pub(super) fn intersect(
    name: &str,
    left: Option<PropertyCountRange>,
    right: Option<PropertyCountRange>,
) -> Result<Option<PropertyCountRange>, JsonFormatError> {
    match (left, right) {
        (None, range) | (range, None) => Ok(range),
        (Some(left), Some(right)) => left.intersection(right).map(Some).ok_or_else(|| {
            unsupported(
                name,
                "object property-count constraints have an empty intersection",
            )
        }),
    }
}

pub(super) fn ensure_feasible(name: &str, node: &SchemaNode) -> Result<(), JsonFormatError> {
    if node.property_count_range_is_valid() {
        return Ok(());
    }
    Err(unsupported(
        name,
        "object property-count constraints conflict with required properties or the closed set of declared properties",
    ))
}

pub(crate) fn validate_len(schema: &SchemaNode, length: usize) -> Result<(), JsonFormatError> {
    let Some(range) = schema.property_count_range else {
        return Ok(());
    };
    if range.contains_len(length) {
        return Ok(());
    }
    Err(JsonFormatError::PropertyCountMismatch {
        name: schema.name.clone(),
        range: describe(range),
        got: length,
    })
}

pub(super) fn render(node: &SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(range) = node.property_count_range else {
        return;
    };
    if range.minimum() > 0 {
        out.insert("minProperties".into(), range.minimum().into());
    }
    if let Some(maximum) = range.maximum() {
        out.insert("maxProperties".into(), maximum.into());
    }
}

pub(super) fn selected(
    name: &str,
    schema: &serde_json::Value,
) -> Result<Option<PropertyCountRange>, JsonFormatError> {
    let minimum = schema
        .get("minProperties")
        .map(|value| exact_count(name, "minProperties", value))
        .transpose()?
        .unwrap_or(0);
    let maximum = schema
        .get("maxProperties")
        .map(|value| exact_count(name, "maxProperties", value))
        .transpose()?;
    if minimum == 0 && maximum.is_none() {
        return Ok(None);
    }
    PropertyCountRange::new(minimum, maximum)
        .map(Some)
        .ok_or_else(|| unsupported(name, "`minProperties` must not exceed `maxProperties`"))
}

fn exact_count(
    name: &str,
    keyword: &str,
    value: &serde_json::Value,
) -> Result<u64, JsonFormatError> {
    value.as_u64().ok_or_else(|| {
        unsupported(
            name,
            &format!("`{keyword}` must be an exact non-negative integer JSON token"),
        )
    })
}

fn describe(range: PropertyCountRange) -> String {
    match (range.minimum(), range.maximum()) {
        (minimum, Some(maximum)) if minimum == maximum => format!("exactly {minimum}"),
        (0, Some(maximum)) => format!("at most {maximum}"),
        (minimum, None) => format!("at least {minimum}"),
        (minimum, Some(maximum)) => format!("between {minimum} and {maximum}"),
    }
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaUnion {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}
