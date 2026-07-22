use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{NodeId, SequenceExpr};
use regex::RegexBuilder;

use super::EngineError;
use super::eval_expr::{EvalProgram, eval_expr};
use super::source_iteration::PositionFrame;

pub(super) const MAX_GENERATED_SEQUENCE_ITEMS: u128 = 1_000_000;
pub(super) const MAX_RECURSIVE_SEQUENCE_DEPTH: usize = 256;
const MAX_TOKENIZE_REGEX_PATTERN_BYTES: usize = 64 * 1024;
const MAX_TOKENIZE_REGEX_COMPILED_BYTES: usize = 10 * 1024 * 1024;

pub(super) fn eval_sequence(
    program: EvalProgram<'_>,
    sequence: &SequenceExpr,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<Vec<Value>, EngineError> {
    let mut in_progress = HashSet::new();
    eval_sequence_in_progress(program, sequence, context, positions, &mut in_progress)
}

pub(super) fn eval_sequence_in_progress(
    program: EvalProgram<'_>,
    sequence: &SequenceExpr,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Vec<Value>, EngineError> {
    match sequence {
        SequenceExpr::Tokenize {
            input, delimiter, ..
        } => {
            let Some(input) = eval_sequence_arg(program, *input, context, positions, in_progress)?
            else {
                return Ok(Vec::new());
            };
            let Some(delimiter) =
                eval_sequence_arg(program, *delimiter, context, positions, in_progress)?
            else {
                return Ok(Vec::new());
            };
            tokenize(input, delimiter)
        }
        SequenceExpr::TokenizeByLength { input, length, .. } => {
            let Some(input) = eval_sequence_arg(program, *input, context, positions, in_progress)?
            else {
                return Ok(Vec::new());
            };
            let Some(length) =
                eval_sequence_arg(program, *length, context, positions, in_progress)?
            else {
                return Ok(Vec::new());
            };
            tokenize_by_length(input, length)
        }
        SequenceExpr::TokenizeRegex {
            input,
            pattern,
            flags,
            ..
        } => {
            let Some(input) = eval_sequence_arg(program, *input, context, positions, in_progress)?
            else {
                return Ok(Vec::new());
            };
            let Some(pattern) =
                eval_sequence_arg(program, *pattern, context, positions, in_progress)?
            else {
                return Ok(Vec::new());
            };
            let flags = match flags {
                Some(node) => {
                    let Some(flags) =
                        eval_sequence_arg(program, *node, context, positions, in_progress)?
                    else {
                        return Ok(Vec::new());
                    };
                    Some(flags)
                }
                None => None,
            };
            tokenize_regex(input, pattern, flags)
        }
        SequenceExpr::Generate { from, to, .. } => {
            let from = match from {
                Some(node) => {
                    let Some(value) =
                        eval_sequence_arg(program, *node, context, positions, in_progress)?
                    else {
                        return Ok(Vec::new());
                    };
                    Some(value)
                }
                None => None,
            };
            let Some(to) = eval_sequence_arg(program, *to, context, positions, in_progress)? else {
                return Ok(Vec::new());
            };
            generate_sequence(from, to)
        }
        SequenceExpr::RecursiveCollect {
            collection,
            children,
            descent_value,
            values,
            value,
            prefix,
            separator,
            ..
        } => {
            let prefix = eval_sequence_arg(program, *prefix, context, positions, in_progress)?
                .map(|value| scalar_text(&value))
                .transpose()?
                .unwrap_or_default();
            let separator =
                eval_sequence_arg(program, *separator, context, positions, in_progress)?
                    .map(|value| scalar_text(&value))
                    .transpose()?
                    .unwrap_or_default();
            recursive_collect(
                context,
                collection,
                children,
                descent_value,
                values,
                value,
                &prefix,
                &separator,
            )
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn recursive_collect(
    context: &[&Instance],
    collection: &[String],
    children: &[String],
    descent_value: &[String],
    values: &[String],
    value: &[String],
    prefix: &str,
    separator: &str,
) -> Result<Vec<Value>, EngineError> {
    let Some(base) = context
        .iter()
        .rev()
        .find(|frame| {
            collection
                .first()
                .is_none_or(|first| frame.field(first).is_some())
        })
        .copied()
        .or_else(|| context.last().copied())
    else {
        return Ok(Vec::new());
    };
    let mut roots = Vec::new();
    collect_instances(base, collection, &mut roots);
    let mut output = Vec::new();
    for root in roots {
        collect_recursive_group(
            root,
            children,
            descent_value,
            values,
            value,
            prefix,
            separator,
            0,
            &mut output,
        )?;
    }
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn collect_recursive_group(
    group: &Instance,
    children: &[String],
    descent_value: &[String],
    values: &[String],
    value: &[String],
    prefix: &str,
    separator: &str,
    depth: usize,
    output: &mut Vec<Value>,
) -> Result<(), EngineError> {
    if depth >= MAX_RECURSIVE_SEQUENCE_DEPTH {
        return Err(EngineError::RecursiveSequenceDepth {
            limit: MAX_RECURSIVE_SEQUENCE_DEPTH,
        });
    }
    let Some(segment) = scalar_at(group, descent_value) else {
        return Ok(());
    };
    let current_prefix = format!("{prefix}{separator}{}", scalar_text(segment)?);
    let mut leaves = Vec::new();
    collect_instances(group, values, &mut leaves);
    for leaf in leaves {
        let Some(value) = scalar_at(leaf, value) else {
            continue;
        };
        if output.len() as u128 >= MAX_GENERATED_SEQUENCE_ITEMS {
            return Err(EngineError::RecursiveSequenceTooLarge {
                max: MAX_GENERATED_SEQUENCE_ITEMS,
            });
        }
        output.push(Value::String(format!(
            "{current_prefix}{separator}{}",
            scalar_text(value)?
        )));
    }
    let mut child_groups = Vec::new();
    collect_instances(group, children, &mut child_groups);
    for child in child_groups {
        collect_recursive_group(
            child,
            children,
            descent_value,
            values,
            value,
            &current_prefix,
            separator,
            depth + 1,
            output,
        )?;
    }
    Ok(())
}

fn collect_instances<'a>(instance: &'a Instance, path: &[String], output: &mut Vec<&'a Instance>) {
    if path.is_empty() {
        match instance {
            Instance::Repeated(items) | Instance::MappedSequence(items) => {
                output.extend(items.iter());
            }
            Instance::Scalar(_) | Instance::Group(_) => output.push(instance),
            Instance::DocumentSet(documents) => {
                output.extend(documents.iter().map(ir::DocumentMember::value));
            }
        }
        return;
    }
    match instance {
        Instance::Group(fields) => {
            if let Some((_, child)) = fields.iter().find(|(name, _)| name == &path[0]) {
                collect_instances(child, &path[1..], output);
            }
        }
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            for item in items {
                collect_instances(item, path, output);
            }
        }
        Instance::DocumentSet(documents) => {
            for document in documents {
                collect_instances(document.value(), path, output);
            }
        }
        Instance::Scalar(_) => {}
    }
}

fn scalar_at<'a>(instance: &'a Instance, path: &[String]) -> Option<&'a Value> {
    if path.is_empty() {
        return instance.as_scalar();
    }
    match instance {
        Instance::Group(fields) => fields
            .iter()
            .find(|(name, _)| name == &path[0])
            .and_then(|(_, child)| scalar_at(child, &path[1..])),
        Instance::Repeated(items) | Instance::MappedSequence(items) => {
            items.first().and_then(|item| scalar_at(item, path))
        }
        Instance::DocumentSet(documents) => documents
            .first()
            .and_then(|document| scalar_at(document.value(), path)),
        Instance::Scalar(_) => None,
    }
}

fn scalar_text(value: &Value) -> Result<String, EngineError> {
    match value {
        Value::Bool(value) => Ok(value.to_string()),
        Value::Int(value) => Ok(value.to_string()),
        Value::Float(value) if value.is_finite() => Ok(value.to_string()),
        Value::String(value) => Ok(value.clone()),
        Value::Null | Value::XmlNil(_) | Value::Float(_) => Err(EngineError::Function(
            functions::FunctionError::TypeMismatch {
                function: "recursive-collect",
                got: value.type_name(),
            },
        )),
    }
}

pub(super) fn eval_sequence_exists(
    program: EvalProgram<'_>,
    sequence: &SequenceExpr,
    predicate: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let values = eval_sequence_in_progress(program, sequence, context, positions, in_progress)?;
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
            document_path: None,
        });
        match eval_expr(
            program,
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

pub(super) fn eval_sequence_item_at(
    program: EvalProgram<'_>,
    sequence: &SequenceExpr,
    index: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let values = eval_sequence_in_progress(program, sequence, context, positions, in_progress)?;
    let index = eval_expr(program, index, context, positions, in_progress)?;
    super::aggregate::aggregate(
        mapping::AggregateOp::ItemAt,
        values.len(),
        &values,
        Some(index),
    )
}

fn eval_sequence_arg(
    program: EvalProgram<'_>,
    node: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Option<Value>, EngineError> {
    let value = eval_expr(program, node, context, positions, in_progress)?;
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

pub(super) fn tokenize_regex(
    input: Value,
    pattern: Value,
    flags: Option<Value>,
) -> Result<Vec<Value>, EngineError> {
    tokenize_regex_with_limit(input, pattern, flags, MAX_GENERATED_SEQUENCE_ITEMS as usize)
}

pub(super) fn tokenize_regex_with_limit(
    input: Value,
    pattern: Value,
    flags: Option<Value>,
    max_items: usize,
) -> Result<Vec<Value>, EngineError> {
    let input = sequence_string(input, "tokenize-regexp")?;
    let pattern = sequence_string(pattern, "tokenize-regexp")?;
    let flags = flags
        .map(|value| sequence_string(value, "tokenize-regexp"))
        .transpose()?
        .unwrap_or_default();
    if pattern.len() > MAX_TOKENIZE_REGEX_PATTERN_BYTES {
        return Err(EngineError::TokenizeRegexPatternTooLarge {
            bytes: pattern.len(),
            max: MAX_TOKENIZE_REGEX_PATTERN_BYTES,
        });
    }

    let mut builder = RegexBuilder::new(&pattern);
    for flag in flags.chars() {
        let apply: fn(&mut RegexBuilder, bool) -> &mut RegexBuilder = match flag {
            'i' => RegexBuilder::case_insensitive,
            'm' => RegexBuilder::multi_line,
            's' => RegexBuilder::dot_matches_new_line,
            'x' => RegexBuilder::ignore_whitespace,
            _ => {
                return Err(EngineError::InvalidTokenizeRegexFlags { flags });
            }
        };
        apply(&mut builder, true);
    }
    let regex = builder
        .size_limit(MAX_TOKENIZE_REGEX_COMPILED_BYTES)
        .dfa_size_limit(MAX_TOKENIZE_REGEX_COMPILED_BYTES)
        .build()
        .map_err(|error| EngineError::InvalidTokenizeRegex {
            message: error.to_string(),
        })?;
    if regex.is_match("")
        || regex
            .find_iter(&input)
            .any(|matched| matched.start() == matched.end())
    {
        return Err(EngineError::ZeroWidthTokenizeRegex);
    }
    if input.is_empty() {
        return Ok(Vec::new());
    }

    let values = regex
        .split(&input)
        .take(max_items.saturating_add(1))
        .map(|value| Value::String(value.to_string()))
        .collect::<Vec<_>>();
    if values.len() > max_items {
        return Err(EngineError::TokenizeRegexTooLarge {
            max: max_items as u128,
        });
    }
    Ok(values)
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
