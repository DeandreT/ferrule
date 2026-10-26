use ir::{
    JsonMultipleOfConstraints, NumericRange, ScalarType, SchemaNode, StringLengthRange, Value,
};

use crate::JsonFormatError;

pub(crate) fn validate_allowed_value(
    schema: &SchemaNode,
    value: &Value,
) -> Result<(), JsonFormatError> {
    let Some(values) = &schema.json_allowed_values else {
        return Ok(());
    };
    if !schema.json_allowed_values_are_valid() {
        return Err(JsonFormatError::InvalidAllowedValuesMetadata {
            reason: format!("`{}` has incompatible allowed values", schema.name),
        });
    }
    if values.matches(value) {
        return Ok(());
    }
    Err(JsonFormatError::AllowedValueMismatch {
        name: schema.name.clone(),
        got: format!("{value:?}"),
    })
}

pub(crate) fn validate_numeric_range(
    schema: &SchemaNode,
    value: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let Some(range) = schema.numeric_range else {
        return Ok(());
    };
    // Explicit null is admitted by the scalar value layer, not by the range.
    if value.is_null() && crate::null_layers::layers(schema)?.value_null {
        return Ok(());
    }
    if value.is_string()
        && ((matches!(range, NumericRange::Integer(_)) && schema.is_string_int_union())
            || (matches!(range, NumericRange::Number(_)) && schema.is_string_float_union()))
    {
        return Ok(());
    }
    let matches = match (range, value) {
        (NumericRange::Integer(range), serde_json::Value::Number(value)) => {
            value.as_i64().is_some_and(|value| range.contains(value))
        }
        (NumericRange::Number(range), serde_json::Value::Number(value)) => {
            crate::exact_f64_from_json_number(value).is_some_and(|value| range.contains(value))
        }
        _ => false,
    };
    if matches {
        return Ok(());
    }
    Err(JsonFormatError::RangeMismatch {
        name: schema.name.clone(),
        range: display_range(range),
        got: value.to_string(),
    })
}

pub(crate) fn validate_multiple_of(
    schema: &SchemaNode,
    value: &serde_json::Value,
) -> Result<(), JsonFormatError> {
    let Some(constraints) = &schema.json_multiple_of else {
        return Ok(());
    };
    // `multipleOf` constrains numbers only; whether null is admitted is
    // decided by the scalar decode.
    let matches = match value {
        serde_json::Value::Number(number)
            if schema.accepts_scalar_type(ScalarType::Int)
                && number
                    .as_i64()
                    .is_some_and(|value| constraints.matches_i64(value)) =>
        {
            true
        }
        serde_json::Value::Number(number) if schema.accepts_scalar_type(ScalarType::Float) => {
            crate::exact_f64_from_json_number(number)
                .is_some_and(|value| constraints.matches_f64(value))
        }
        serde_json::Value::Number(_) => false,
        _ => true,
    };
    if matches {
        return Ok(());
    }
    Err(JsonFormatError::MultipleOfMismatch {
        name: schema.name.clone(),
        divisors: display_divisors(constraints),
        got: value.to_string(),
    })
}

pub(crate) fn validate_string_length(
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
        range: describe_length(range),
        got: length,
    })
}

fn display_range(range: NumericRange) -> String {
    match range {
        NumericRange::Integer(range) => format!(
            "[{}, {}]",
            range
                .minimum()
                .map_or_else(|| "-inf".into(), |value| value.to_string()),
            range
                .maximum()
                .map_or_else(|| "inf".into(), |value| value.to_string())
        ),
        NumericRange::Number(range) => {
            let minimum = range.minimum();
            let maximum = range.maximum();
            format!(
                "{}{}, {}{}",
                minimum.map_or('(', |bound| if bound.is_exclusive() { '(' } else { '[' }),
                minimum.map_or_else(|| "-inf".into(), |bound| bound.value().get().to_string()),
                maximum.map_or_else(|| "inf".into(), |bound| bound.value().get().to_string()),
                maximum.map_or(')', |bound| if bound.is_exclusive() { ')' } else { ']' }),
            )
        }
    }
}

fn display_divisors(constraints: &JsonMultipleOfConstraints) -> String {
    constraints
        .any_of()
        .iter()
        .map(|alternative| {
            alternative
                .iter()
                .map(|divisor| divisor.to_decimal_lexical())
                .collect::<Vec<_>>()
                .join(" and ")
        })
        .collect::<Vec<_>>()
        .join(" or ")
}

fn describe_length(range: StringLengthRange) -> String {
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
