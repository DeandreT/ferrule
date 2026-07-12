use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{Graph, NodeId, SequenceExpr};

use super::{EngineError, PositionFrame, eval_expr};

pub(super) const MAX_GENERATED_SEQUENCE_ITEMS: u128 = 1_000_000;

pub(super) fn eval_sequence(
    graph: &Graph,
    sequence: &SequenceExpr,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<Vec<Value>, EngineError> {
    let mut in_progress = HashSet::new();
    eval_sequence_in_progress(graph, sequence, context, positions, &mut in_progress)
}

pub(super) fn eval_sequence_in_progress(
    graph: &Graph,
    sequence: &SequenceExpr,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Vec<Value>, EngineError> {
    match sequence {
        SequenceExpr::Tokenize {
            input, delimiter, ..
        } => {
            let Some(input) = eval_sequence_arg(graph, *input, context, positions, in_progress)?
            else {
                return Ok(Vec::new());
            };
            let Some(delimiter) =
                eval_sequence_arg(graph, *delimiter, context, positions, in_progress)?
            else {
                return Ok(Vec::new());
            };
            tokenize(input, delimiter)
        }
        SequenceExpr::TokenizeByLength { input, length, .. } => {
            let Some(input) = eval_sequence_arg(graph, *input, context, positions, in_progress)?
            else {
                return Ok(Vec::new());
            };
            let Some(length) = eval_sequence_arg(graph, *length, context, positions, in_progress)?
            else {
                return Ok(Vec::new());
            };
            tokenize_by_length(input, length)
        }
        SequenceExpr::Generate { from, to, .. } => {
            let from = match from {
                Some(node) => {
                    let Some(value) =
                        eval_sequence_arg(graph, *node, context, positions, in_progress)?
                    else {
                        return Ok(Vec::new());
                    };
                    Some(value)
                }
                None => None,
            };
            let Some(to) = eval_sequence_arg(graph, *to, context, positions, in_progress)? else {
                return Ok(Vec::new());
            };
            generate_sequence(from, to)
        }
    }
}

pub(super) fn eval_sequence_exists(
    graph: &Graph,
    sequence: &SequenceExpr,
    predicate: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let values = eval_sequence_in_progress(graph, sequence, context, positions, in_progress)?;
    for (index, value) in values.into_iter().enumerate() {
        let item = Instance::Scalar(value);
        let mut item_context = context.to_vec();
        item_context.push(&item);
        let mut item_positions = positions.to_vec();
        item_positions.push(PositionFrame {
            collection: Vec::new(),
            index: index + 1,
            grouped: false,
            join: None,
            join_position: None,
        });
        match eval_expr(
            graph,
            predicate,
            &item_context,
            &item_positions,
            in_progress,
        )? {
            Value::Bool(true) => return Ok(Value::Bool(true)),
            Value::Bool(false) => {}
            other => {
                return Err(EngineError::NotABool {
                    node: predicate,
                    found: other.type_name(),
                });
            }
        }
    }
    Ok(Value::Bool(false))
}

fn eval_sequence_arg(
    graph: &Graph,
    node: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Option<Value>, EngineError> {
    let value = eval_expr(graph, node, context, positions, in_progress)?;
    Ok((value != Value::Null).then_some(value))
}

pub(super) fn generate_sequence(from: Option<Value>, to: Value) -> Result<Vec<Value>, EngineError> {
    let from = from.map_or(Ok(1), |value| sequence_integer(value, "generate-sequence"))?;
    let to = sequence_integer(to, "generate-sequence")?;
    if from > to {
        return Ok(Vec::new());
    }
    let requested = (i128::from(to) - i128::from(from) + 1) as u128;
    if requested > MAX_GENERATED_SEQUENCE_ITEMS {
        return Err(EngineError::GeneratedSequenceTooLarge {
            requested,
            max: MAX_GENERATED_SEQUENCE_ITEMS,
        });
    }
    let mut values = Vec::with_capacity(requested as usize);
    values.extend((from..=to).map(Value::Int));
    Ok(values)
}

fn sequence_integer(value: Value, function: &'static str) -> Result<i64, EngineError> {
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
        _ => None,
    };
    coerced.ok_or_else(|| {
        functions::FunctionError::TypeMismatch {
            function,
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

pub(super) fn tokenize(input: Value, delimiter: Value) -> Result<Vec<Value>, EngineError> {
    let input = sequence_string(input, "tokenize")?;
    let delimiter = sequence_string(delimiter, "tokenize")?;
    if delimiter.is_empty() {
        return Err(functions::FunctionError::InvalidArgument {
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

pub(super) fn tokenize_by_length(input: Value, length: Value) -> Result<Vec<Value>, EngineError> {
    let input = sequence_string(input, "tokenize-by-length")?;
    let length = match length {
        Value::Int(value) => Some(value),
        Value::Float(value) if value.is_finite() => Some(value.trunc() as i64),
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    }
    .filter(|length| *length > 0)
    .ok_or(functions::FunctionError::InvalidArgument {
        function: "tokenize-by-length",
        message: "requires a positive integer length",
    })? as usize;

    let chars: Vec<char> = input.chars().collect();
    Ok(chars
        .chunks(length)
        .map(|chunk| Value::String(chunk.iter().collect()))
        .collect())
}

fn sequence_string(value: Value, function: &'static str) -> Result<String, EngineError> {
    match value {
        Value::String(value) => Ok(value),
        other => Err(functions::FunctionError::TypeMismatch {
            function,
            got: other.type_name(),
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use ir::Value;

    use super::sequence_integer;

    #[test]
    fn range_boundaries_accept_exact_numeric_representations() {
        assert_eq!(sequence_integer(Value::Float(3.0), "test").unwrap(), 3);
        assert_eq!(
            sequence_integer(Value::String("-4.0".into()), "test").unwrap(),
            -4
        );
        assert_eq!(
            sequence_integer(Value::String(i64::MAX.to_string()), "test").unwrap(),
            i64::MAX
        );
    }

    #[test]
    fn range_boundaries_reject_lossy_or_non_finite_values() {
        for value in [
            Value::Float(1.5),
            Value::Float(f64::INFINITY),
            Value::Float(i64::MAX as f64),
            Value::String("2.5".into()),
        ] {
            assert!(sequence_integer(value, "test").is_err());
        }
    }
}
