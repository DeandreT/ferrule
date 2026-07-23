//! Scalar, composite, and delimiter encoding for schema-guided EDI output.

use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};

use super::{WriteOptions, WriteStyle};
use crate::EdiFormatError;

pub(super) fn write_component(
    schema: &SchemaNode,
    instance: Option<&Instance>,
    opts: &WriteOptions,
) -> Result<String, EdiFormatError> {
    if schema.repeating {
        return Err(EdiFormatError::UnsupportedSchema(format!(
            "component `{}` repeats below an EDI element",
            schema.name
        )));
    }
    match &schema.kind {
        SchemaKind::Scalar { .. } => {
            let text = scalar_or_fixed(schema, instance.and_then(Instance::as_scalar))?;
            escape(&text, &schema.name, opts, None)
        }
        SchemaKind::Group { children, .. } => {
            let WriteStyle::Hl7 { subcomponent } = opts.style else {
                return Err(EdiFormatError::UnsupportedSchema(schema.name.clone()));
            };
            let mut parts = children
                .iter()
                .map(|child| {
                    let SchemaKind::Scalar { .. } = child.kind else {
                        return Err(EdiFormatError::UnsupportedSchema(child.name.clone()));
                    };
                    let text = scalar_or_fixed(
                        child,
                        instance
                            .and_then(|value| value.field(&child.name))
                            .and_then(Instance::as_scalar),
                    )?;
                    escape(&text, &child.name, opts, None)
                })
                .collect::<Result<Vec<_>, _>>()?;
            while parts.last().is_some_and(String::is_empty) {
                parts.pop();
            }
            Ok(parts.join(&subcomponent.to_string()))
        }
    }
}

/// The serialized text for one element/component: the instance value, or
/// the schema's `fixed` value when the instance doesn't provide one.
pub(super) fn scalar_or_fixed(
    schema: &SchemaNode,
    value: Option<&Value>,
) -> Result<String, EdiFormatError> {
    let missing = value.is_none_or(|value| {
        matches!(value, Value::Null) || matches!(value, Value::String(text) if text.is_empty())
    });
    let Some(fixed) = &schema.fixed else {
        if missing {
            return Ok(String::new());
        }
        let Some(value) = value else {
            return Ok(String::new());
        };
        return format_value(schema, value);
    };

    let normalized_fixed = format_value(schema, &Value::String(fixed.clone()))?;
    if missing {
        return Ok(fixed.clone());
    }
    let Some(value) = value else {
        return Ok(fixed.clone());
    };
    let normalized_value = format_value(schema, value)?;
    if semantically_equal(schema, &normalized_fixed, &normalized_value) {
        Ok(fixed.clone())
    } else {
        Err(EdiFormatError::FixedValueMismatch {
            element: schema.name.clone(),
            expected: fixed.clone(),
            found: normalized_value,
        })
    }
}

fn semantically_equal(schema: &SchemaNode, left: &str, right: &str) -> bool {
    match schema.kind {
        SchemaKind::Scalar {
            ty: ScalarType::Float,
        } => left
            .parse::<f64>()
            .ok()
            .zip(right.parse::<f64>().ok())
            .is_some_and(|(left, right)| left == right),
        SchemaKind::Scalar { .. } => left == right,
        SchemaKind::Group { .. } => false,
    }
}

pub(super) fn escape(
    text: &str,
    element: &str,
    opts: &WriteOptions,
    allowed_reserved: Option<char>,
) -> Result<String, EdiFormatError> {
    if text.chars().count() == 1 && text.chars().next() == allowed_reserved {
        return Ok(text.to_string());
    }
    if matches!(opts.style, WriteStyle::Hl7 { .. }) {
        return escape_hl7(text, opts);
    }
    let Some(release) = opts.release else {
        if let Some(delimiter) = text.chars().find(|character| {
            *character == opts.element
                || *character == opts.component
                || *character == opts.terminator
                || (matches!(opts.style, WriteStyle::Assigned) && *character == '=')
                || opts.repetition == Some(*character)
        }) {
            return Err(EdiFormatError::UnescapableDelimiter {
                element: element.to_string(),
                delimiter,
            });
        }
        return Ok(text.to_string());
    };
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        if character == release
            || character == opts.element
            || character == opts.component
            || character == opts.terminator
            || (matches!(opts.style, WriteStyle::Assigned) && character == '=')
            || opts.repetition == Some(character)
        {
            out.push(release);
        }
        out.push(character);
    }
    Ok(out)
}

fn escape_hl7(text: &str, opts: &WriteOptions) -> Result<String, EdiFormatError> {
    let WriteStyle::Hl7 { subcomponent } = opts.style else {
        return Err(EdiFormatError::UnsupportedSchema(
            "invalid HL7 write style".to_string(),
        ));
    };
    let release = opts.release.unwrap_or('\\');
    let repetition = opts.repetition.unwrap_or('~');
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        let escape_code = if character == opts.element {
            Some('F')
        } else if character == opts.component {
            Some('S')
        } else if character == repetition {
            Some('R')
        } else if character == release {
            Some('E')
        } else if character == subcomponent {
            Some('T')
        } else {
            None
        };
        if let Some(code) = escape_code {
            out.extend([release, code, release]);
        } else {
            out.push(character);
        }
    }
    Ok(out)
}

fn format_value(schema: &SchemaNode, value: &Value) -> Result<String, EdiFormatError> {
    let SchemaKind::Scalar { ty } = schema.kind else {
        return Err(EdiFormatError::UnsupportedSchema(schema.name.clone()));
    };
    let incompatible = |got| EdiFormatError::ValueType {
        element: schema.name.clone(),
        expected: ty,
        got,
    };
    match (ty, value) {
        (_, Value::Null) => Ok(String::new()),
        (ScalarType::String, Value::Bool(value)) => Ok(value.to_string()),
        (ScalarType::String, Value::Int(value)) => Ok(value.to_string()),
        (ScalarType::String, Value::Float(value)) if value.is_finite() => Ok(value.to_string()),
        (ScalarType::String, Value::Float(_)) => Err(EdiFormatError::NonFiniteFloat {
            element: schema.name.clone(),
        }),
        (ScalarType::String, Value::String(value)) => Ok(value.clone()),
        (ScalarType::Int, Value::Int(value)) => Ok(value.to_string()),
        (ScalarType::Int, Value::String(value)) => value
            .trim()
            .parse::<i64>()
            .map(|value| value.to_string())
            .map_err(|_| incompatible("string")),
        (ScalarType::Float, Value::Float(value)) if value.is_finite() => Ok(value.to_string()),
        (ScalarType::Float, Value::Float(_)) => Err(EdiFormatError::NonFiniteFloat {
            element: schema.name.clone(),
        }),
        (ScalarType::Float, Value::Int(value)) if exact_f64(*value).is_some() => {
            Ok(value.to_string())
        }
        (ScalarType::Float, Value::Int(_)) => Err(incompatible("int outside the exact f64 range")),
        (ScalarType::Float, Value::String(value)) => value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(|value| value.to_string())
            .ok_or_else(|| incompatible("string")),
        (ScalarType::Bool, Value::Bool(value)) => Ok(value.to_string()),
        (ScalarType::Bool, Value::String(value)) => value
            .trim()
            .parse::<bool>()
            .map(|value| value.to_string())
            .map_err(|_| incompatible("string")),
        (_, other) => Err(incompatible(other.type_name())),
    }
}

fn exact_f64(value: i64) -> Option<f64> {
    let magnitude = value.unsigned_abs();
    if magnitude == 0 {
        return Some(0.0);
    }
    let significant_bits = u64::BITS - magnitude.leading_zeros() - magnitude.trailing_zeros();
    (significant_bits <= f64::MANTISSA_DIGITS).then_some(value as f64)
}
