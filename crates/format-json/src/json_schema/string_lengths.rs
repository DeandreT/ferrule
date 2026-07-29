use ir::{ScalarType, SchemaNode, StringLengthRange};

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
    if shape_is_ambiguous {
        return Err(unsupported(
            name,
            "string-length constraint without a concrete string-capable type also admits unconstrained non-string values",
        ));
    }
    if node.repeating || node.json_any || !node.accepts_scalar_type(ScalarType::String) {
        return Ok(());
    }
    node.string_length_range = intersect(name, node.string_length_range, candidate)?;
    if !node.string_length_range_is_valid() {
        return Err(unsupported(
            name,
            "fixed string value falls outside the string-length range",
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
    schema.get("minLength").is_some() || schema.get("maxLength").is_some()
}

pub(super) fn is_effectively_constrained(
    name: &str,
    schema: &serde_json::Value,
) -> Result<bool, JsonFormatError> {
    Ok(parse(name, schema)?.is_some())
}

pub(super) fn intersect(
    name: &str,
    left: Option<StringLengthRange>,
    right: Option<StringLengthRange>,
) -> Result<Option<StringLengthRange>, JsonFormatError> {
    match (left, right) {
        (None, range) | (range, None) => Ok(range),
        (Some(left), Some(right)) => left.intersection(right).map(Some).ok_or_else(|| {
            unsupported(name, "string-length constraints have an empty intersection")
        }),
    }
}

pub(crate) fn validate_json(
    schema: &SchemaNode,
    value: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let Some(range) = schema.string_length_range else {
        return Ok(());
    };
    let serde_json::Value::String(value) = value else {
        return Ok(());
    };
    let length = value.chars().count();
    if range.contains_len(length) {
        return Ok(());
    }
    Err(JsonFormatError::StringLengthMismatch {
        name: schema.name.clone(),
        range: describe(range),
        got: length,
    })
}

pub(super) fn render(node: &SchemaNode, out: &mut serde_json::Map<String, serde_json::Value>) {
    let Some(range) = node.string_length_range else {
        return;
    };
    if range.minimum() > 0 {
        out.insert("minLength".into(), range.minimum().into());
    }
    if let Some(maximum) = range.maximum() {
        out.insert("maxLength".into(), maximum.into());
    }
}

pub(super) fn parse(
    name: &str,
    schema: &serde_json::Value,
) -> Result<Option<StringLengthRange>, JsonFormatError> {
    let minimum = schema
        .get("minLength")
        .map(|value| exact_length(name, "minLength", value))
        .transpose()?
        .unwrap_or(0);
    let maximum = schema
        .get("maxLength")
        .map(|value| exact_length(name, "maxLength", value))
        .transpose()?;
    if minimum == 0 && maximum.is_none() {
        return Ok(None);
    }
    StringLengthRange::new(minimum, maximum)
        .map(Some)
        .ok_or_else(|| unsupported(name, "`minLength` must not exceed `maxLength`"))
}

fn exact_length(
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

fn describe(range: StringLengthRange) -> String {
    match (range.minimum(), range.maximum()) {
        (minimum, Some(maximum)) if minimum == maximum => {
            format!("exactly {minimum} Unicode scalar values")
        }
        (0, Some(maximum)) => format!("at most {maximum} Unicode scalar values"),
        (minimum, None) => format!("at least {minimum} Unicode scalar values"),
        (minimum, Some(maximum)) => {
            format!("between {minimum} and {maximum} Unicode scalar values")
        }
    }
}

fn unsupported(name: &str, reason: &str) -> JsonFormatError {
    JsonFormatError::UnsupportedSchemaUnion {
        name: name.to_string(),
        reason: reason.to_string(),
    }
}
