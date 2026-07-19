use crate::{FunctionError, RuntimeError, Value};

pub const MAX_GENERATED_SEQUENCE_ITEMS: u128 = 1_000_000;

/// Splits a string around one literal delimiter while preserving empty items.
pub fn tokenize(input: Value, delimiter: Value) -> Result<Vec<Value>, RuntimeError> {
    let input = sequence_string(input, "tokenize")?;
    let delimiter = sequence_string(delimiter, "tokenize")?;
    if delimiter.is_empty() {
        return Err(FunctionError::InvalidArgument {
            function: "tokenize",
            message: "requires a non-empty delimiter",
        }
        .into());
    }
    Ok(input
        .split(&delimiter)
        .map(|value| Value::String(value.to_string()))
        .collect())
}

/// Chunks a string by Unicode scalar count, retaining a final short item.
pub fn tokenize_by_length(input: Value, length: Value) -> Result<Vec<Value>, RuntimeError> {
    let input = sequence_string(input, "tokenize-by-length")?;
    let length = match length {
        Value::Int(value) => Some(value),
        Value::Float(value) if value.is_finite() => Some(value.trunc() as i64),
        Value::String(value) => value.trim().parse().ok(),
        Value::Null | Value::XmlNil(_) | Value::Bool(_) | Value::Float(_) => None,
    }
    .filter(|length| *length > 0)
    .ok_or(FunctionError::InvalidArgument {
        function: "tokenize-by-length",
        message: "requires a positive integer length",
    })? as usize;

    let chars = input.chars().collect::<Vec<_>>();
    Ok(chars
        .chunks(length)
        .map(|chunk| Value::String(chunk.iter().collect()))
        .collect())
}

/// Generates an inclusive integer range with the engine's one-million-item
/// materialization bound. An absent lower bound defaults to one.
pub fn generate_sequence(from: Option<Value>, to: Value) -> Result<Vec<Value>, RuntimeError> {
    let from = from.map_or(Ok(1), sequence_integer)?;
    let to = sequence_integer(to)?;
    if from > to {
        return Ok(Vec::new());
    }
    let requested = (i128::from(to) - i128::from(from) + 1) as u128;
    if requested > MAX_GENERATED_SEQUENCE_ITEMS {
        return Err(RuntimeError::GeneratedSequenceTooLarge {
            requested,
            max: MAX_GENERATED_SEQUENCE_ITEMS,
        });
    }
    Ok((from..=to).map(Value::Int).collect())
}

fn sequence_string(value: Value, function: &'static str) -> Result<String, RuntimeError> {
    match value {
        Value::String(value) => Ok(value),
        value => Err(FunctionError::TypeMismatch {
            function,
            got: value.type_name(),
        }
        .into()),
    }
}

fn sequence_integer(value: Value) -> Result<i64, RuntimeError> {
    let coerced = match &value {
        Value::Int(value) => Some(*value),
        Value::Float(value) => exact_float_integer(*value),
        Value::String(value) => value.trim().parse::<i64>().ok().or_else(|| {
            value
                .trim()
                .parse::<f64>()
                .ok()
                .and_then(exact_float_integer)
        }),
        Value::Null | Value::XmlNil(_) | Value::Bool(_) => None,
    };
    coerced.ok_or_else(|| {
        FunctionError::TypeMismatch {
            function: "generate-sequence",
            got: value.type_name(),
        }
        .into()
    })
}

fn exact_float_integer(value: f64) -> Option<i64> {
    (value.is_finite()
        && value.fract() == 0.0
        && value >= i64::MIN as f64
        && value < i64::MAX as f64)
        .then_some(value as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_tokenize_preserves_empty_items_and_typed_failures() {
        assert_eq!(
            tokenize(Value::String("a,,b,".into()), Value::String(",".into())),
            Ok(vec![
                Value::String("a".into()),
                Value::String(String::new()),
                Value::String("b".into()),
                Value::String(String::new()),
            ])
        );
        assert_eq!(
            tokenize(Value::String(String::new()), Value::String("/".into())),
            Ok(vec![Value::String(String::new())])
        );
        assert!(matches!(
            tokenize(Value::Int(1), Value::String(",".into())),
            Err(RuntimeError::Function(FunctionError::TypeMismatch {
                function: "tokenize",
                got: "int"
            }))
        ));
        assert!(matches!(
            tokenize(Value::String("a".into()), Value::String(String::new())),
            Err(RuntimeError::Function(FunctionError::InvalidArgument {
                function: "tokenize",
                ..
            }))
        ));
    }

    #[test]
    fn length_tokenize_uses_unicode_scalars_and_engine_coercions() {
        assert_eq!(
            tokenize_by_length(Value::String("aé🙂z".into()), Value::Float(2.9)),
            Ok(vec![
                Value::String("aé".into()),
                Value::String("🙂z".into()),
            ])
        );
        assert_eq!(
            tokenize_by_length(Value::String(String::new()), Value::Int(2)),
            Ok(Vec::new())
        );
        assert!(matches!(
            tokenize_by_length(Value::String("abc".into()), Value::String("2.0".into())),
            Err(RuntimeError::Function(FunctionError::InvalidArgument {
                function: "tokenize-by-length",
                ..
            }))
        ));
    }

    #[test]
    fn inclusive_ranges_default_descend_and_bound_without_overflow() {
        assert_eq!(
            generate_sequence(None, Value::Int(3)),
            Ok(vec![Value::Int(1), Value::Int(2), Value::Int(3)])
        );
        assert_eq!(
            generate_sequence(Some(Value::String("-2.0".into())), Value::Float(0.0)),
            Ok(vec![Value::Int(-2), Value::Int(-1), Value::Int(0)])
        );
        assert_eq!(
            generate_sequence(Some(Value::Int(3)), Value::Int(2)),
            Ok(Vec::new())
        );
        assert_eq!(
            generate_sequence(Some(Value::Int(i64::MIN)), Value::Int(i64::MAX)),
            Err(RuntimeError::GeneratedSequenceTooLarge {
                requested: 1_u128 << 64,
                max: MAX_GENERATED_SEQUENCE_ITEMS,
            })
        );
    }
}
