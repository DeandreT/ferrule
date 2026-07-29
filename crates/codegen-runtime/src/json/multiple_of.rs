use ir::{ScalarType, SchemaKind, SchemaNode, Value};

use super::ConstraintValidationError;

pub(super) fn validate_instance_value(
    schema: &SchemaNode,
    value: &Value,
) -> Result<(), ConstraintValidationError> {
    let Some(constraints) = &schema.json_multiple_of else {
        return Ok(());
    };
    let matches = match value {
        Value::Int(value) => constraints.matches_i64(*value),
        Value::Float(value) => constraints.matches_f64(*value),
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) | Value::Bool(_) | Value::String(_) => {
            true
        }
    };
    if matches {
        Ok(())
    } else {
        Err(mismatch(schema))
    }
}

pub(super) fn validate_json_value(
    schema: &SchemaNode,
    value: &serde_json::Number,
) -> Result<(), ConstraintValidationError> {
    let Some(constraints) = &schema.json_multiple_of else {
        return Ok(());
    };
    let matches = match &schema.kind {
        SchemaKind::Scalar {
            ty: ScalarType::Int,
        } => value
            .as_i64()
            .is_some_and(|value| constraints.matches_i64(value)),
        SchemaKind::Scalar {
            ty: ScalarType::Float,
        } => value
            .as_f64()
            .is_some_and(|value| constraints.matches_f64(value)),
        SchemaKind::ScalarUnion { types }
            if types.contains(ScalarType::Int) && value.as_i64().is_some() =>
        {
            value
                .as_i64()
                .is_some_and(|value| constraints.matches_i64(value))
        }
        SchemaKind::ScalarUnion { types } if types.contains(ScalarType::Float) => value
            .as_f64()
            .is_some_and(|value| constraints.matches_f64(value)),
        SchemaKind::Scalar { .. } | SchemaKind::ScalarUnion { .. } | SchemaKind::Group { .. } => {
            true
        }
    };
    if matches {
        Ok(())
    } else {
        Err(mismatch(schema))
    }
}

fn mismatch(schema: &SchemaNode) -> ConstraintValidationError {
    ConstraintValidationError::MultipleOfMismatch {
        name: schema.name.clone(),
    }
}
