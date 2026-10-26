use std::collections::BTreeSet;

use ir::{PropertyCountRange, SchemaNode};

use super::PatternRuntime;
use crate::JsonFormatError;

pub(crate) fn validate_property_count(
    schema: &SchemaNode,
    length: usize,
) -> Result<(), JsonFormatError> {
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

pub(crate) fn validate_property_dependencies<'a>(
    schema: &SchemaNode,
    properties: impl IntoIterator<Item = &'a str>,
) -> Result<(), JsonFormatError> {
    let Some(dependencies) = &schema.json_property_dependencies else {
        return Ok(());
    };
    let present = properties.into_iter().collect::<BTreeSet<_>>();
    for (trigger, requirements) in dependencies.rules() {
        if present.contains(trigger.as_str())
            && let Some(property) = requirements
                .iter()
                .find(|property| !present.contains(property.as_str()))
        {
            return Err(JsonFormatError::MissingDependentProperty {
                object: schema.name.clone(),
                trigger: trigger.clone(),
                property: property.clone(),
            });
        }
    }
    Ok(())
}

pub(crate) fn validate_dependent_schemas(
    schema: &SchemaNode,
    value: &serde_json::Value,
    properties: impl IntoIterator<Item = impl AsRef<str>>,
    patterns: &mut PatternRuntime,
) -> Result<(), JsonFormatError> {
    let Some(constraints) = &schema.json_dependent_schemas else {
        return Ok(());
    };
    let present = properties
        .into_iter()
        .map(|property| property.as_ref().to_string())
        .collect::<BTreeSet<_>>();
    for constraint in constraints.as_slice() {
        if present.contains(constraint.trigger())
            && !super::predicate::matches(constraint.predicate(), value, patterns)?
        {
            return Err(JsonFormatError::DependentSchemaMismatch {
                object: schema.name.clone(),
                trigger: constraint.trigger().to_string(),
            });
        }
    }
    Ok(())
}

fn describe(range: PropertyCountRange) -> String {
    match (range.minimum(), range.maximum()) {
        (minimum, Some(maximum)) if minimum == maximum => format!("exactly {minimum}"),
        (0, Some(maximum)) => format!("at most {maximum}"),
        (minimum, None) => format!("at least {minimum}"),
        (minimum, Some(maximum)) => format!("between {minimum} and {maximum}"),
    }
}
