use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{FailureIteration, FailureRule, FailureSelection};

use crate::EngineError;
use crate::eval_expr::{EvalProgram, eval_expr};
use crate::sequence::eval_sequence;
use crate::source_iteration::{PositionFrame, WalkExtension, walk};

pub(super) fn evaluate(
    program: EvalProgram<'_>,
    rules: &[FailureRule],
    context: &[&Instance],
) -> Result<(), EngineError> {
    for (index, rule) in rules.iter().enumerate() {
        let sequence_items;
        let extensions = match &rule.iteration {
            FailureIteration::Source { collection } => source_extensions(context, collection),
            FailureIteration::Sequence { sequence } => {
                let values = eval_sequence(program, sequence, context, &[])?;
                sequence_items =
                    Instance::Repeated(values.into_iter().map(Instance::Scalar).collect());
                walk(&sequence_items, &[], &[], &[], &[])
            }
        };
        for extension in extensions {
            let mut item_context = context.to_vec();
            item_context.extend(extension.instances);
            if !is_selected(program, rule.selection, &item_context, &extension.positions)? {
                continue;
            }
            let message = rule
                .message
                .map(|message| {
                    let mut in_progress = HashSet::new();
                    eval_expr(
                        program,
                        message,
                        &item_context,
                        &extension.positions,
                        &mut in_progress,
                    )
                    .map(scalar_text)
                })
                .transpose()?;
            return Err(EngineError::MappingFailure {
                rule: index + 1,
                message,
            });
        }
    }
    Ok(())
}

fn source_extensions<'a>(
    context: &[&'a Instance],
    collection: &[String],
) -> Vec<WalkExtension<'a>> {
    context
        .iter()
        .rev()
        .find(|frame| match collection.first() {
            Some(first) => frame.field(first).is_some(),
            None => true,
        })
        .copied()
        .or_else(|| context.last().copied())
        .map_or_else(Vec::new, |base| walk(base, collection, &[], &[], &[]))
}

fn is_selected(
    program: EvalProgram<'_>,
    selection: FailureSelection,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<bool, EngineError> {
    let (predicate, expected) = match selection {
        FailureSelection::All => return Ok(true),
        FailureSelection::WhenTrue { predicate } => (predicate, true),
        FailureSelection::WhenFalse { predicate } => (predicate, false),
    };
    let mut in_progress = HashSet::new();
    match eval_expr(program, predicate, context, positions, &mut in_progress)? {
        Value::Bool(value) => Ok(value == expected),
        value => Err(EngineError::NotABool {
            node: predicate,
            found: value.type_name(),
        }),
    }
}

fn scalar_text(value: Value) -> String {
    match value {
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::String(value) => value,
    }
}
