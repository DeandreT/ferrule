use ir::{ItemCountRange, SchemaNode};

use super::PatternRuntime;
use crate::JsonFormatError;

pub(crate) fn validate_item_count(
    schema: &SchemaNode,
    length: usize,
) -> Result<(), JsonFormatError> {
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

pub(crate) fn validate_contains(
    schema: &SchemaNode,
    values: &[serde_json::Value],
    patterns: &mut PatternRuntime,
) -> Result<(), JsonFormatError> {
    let Some(constraints) = &schema.json_contains else {
        return Ok(());
    };
    for constraint in constraints.as_slice() {
        let mut matched = 0_usize;
        for value in values {
            let is_match = super::predicate::matches(constraint.predicate(), value, patterns)?;
            if is_match {
                matched = matched.saturating_add(1);
                if constraint.range().maximum().is_some_and(|maximum| {
                    u64::try_from(matched).is_ok_and(|matched| matched > maximum)
                }) {
                    break;
                }
            }
        }
        if !constraint.range().contains_len(matched) {
            return Err(JsonFormatError::ContainsCountMismatch {
                name: schema.name.clone(),
                range: describe(constraint.range()),
                got: matched,
            });
        }
    }
    Ok(())
}

pub(crate) fn describe(range: ItemCountRange) -> String {
    match (range.minimum(), range.maximum()) {
        (minimum, Some(maximum)) if minimum == maximum => format!("exactly {minimum}"),
        (0, Some(maximum)) => format!("at most {maximum}"),
        (minimum, None) => format!("at least {minimum}"),
        (minimum, Some(maximum)) => format!("between {minimum} and {maximum}"),
    }
}
