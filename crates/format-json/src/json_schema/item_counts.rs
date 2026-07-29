use ir::{ItemCountRange, SchemaNode};

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
    let candidate = parse(name, schema)?;
    if candidate.is_none() {
        return Ok(());
    }
    if node.repeating {
        node.item_count_range = intersect(name, node.item_count_range, candidate)?;
    } else if shape_is_ambiguous {
        return Err(unsupported(
            name,
            "item-count constraint without a concrete array type also admits unconstrained non-array values",
        ));
    }
    Ok(())
}

pub(super) fn validate_ignored(
    name: &str,
    schema: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    if has_keywords(schema) {
        parse(name, schema)?;
    }
    Ok(())
}

pub(super) fn has_keywords(schema: &serde_json::Value) -> bool {
    schema.get("minItems").is_some() || schema.get("maxItems").is_some()
}

pub(super) fn is_effectively_constrained(
    name: &str,
    schema: &serde_json::Value,
) -> Result<bool, JsonFormatError> {
    Ok(parse(name, schema)?.is_some())
}

pub(super) fn intersect(
    name: &str,
    left: Option<ItemCountRange>,
    right: Option<ItemCountRange>,
) -> Result<Option<ItemCountRange>, JsonFormatError> {
    match (left, right) {
        (None, range) | (range, None) => Ok(range),
        (Some(left), Some(right)) => left.intersection(right).map(Some).ok_or_else(|| {
            unsupported(
                name,
                "array item-count constraints have an empty intersection",
            )
        }),
    }
}

pub(crate) fn validate_len(schema: &SchemaNode, length: usize) -> Result<(), JsonFormatError> {
    let Some(range) = schema.item_count_range else {
        return Ok(());
    };
    if range.contains_len(length) {
        return Ok(());
    }
    Err(JsonFormatError::ItemCountMismatch {
        name: schema.name.clone(),
        range: describe(range),
        got: length,
    })
}

pub(super) fn render(node: &SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(range) = node.item_count_range else {
        return;
    };
    if range.minimum() > 0 {
        out.insert("minItems".into(), range.minimum().into());
    }
    if let Some(maximum) = range.maximum() {
        out.insert("maxItems".into(), maximum.into());
    }
}

fn parse(
    name: &str,
    schema: &serde_json::Value,
) -> Result<Option<ItemCountRange>, JsonFormatError> {
    let minimum = schema
        .get("minItems")
        .map(|value| exact_count(name, "minItems", value))
        .transpose()?
        .unwrap_or(0);
    let maximum = schema
        .get("maxItems")
        .map(|value| exact_count(name, "maxItems", value))
        .transpose()?;
    if minimum == 0 && maximum.is_none() {
        return Ok(None);
    }
    ItemCountRange::new(minimum, maximum)
        .map(Some)
        .ok_or_else(|| unsupported(name, "`minItems` must not exceed `maxItems`"))
}

pub(super) fn selected(
    name: &str,
    schema: &serde_json::Value,
) -> Result<Option<ItemCountRange>, JsonFormatError> {
    parse(name, schema)
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

fn describe(range: ItemCountRange) -> String {
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
