use std::collections::{BTreeMap, BTreeSet};

use mapping::NodeId;

use crate::{Expression, FailureIteration, Program};

use super::sources::SourceCatalog;
use super::{
    ProgramValidationError, SequenceExpressionRole, SequenceOwner, joins, recursive_sequence,
    sequences,
};

pub(super) fn collect_sequence_items(
    program: &Program,
    expressions: &BTreeMap<NodeId, &Expression>,
    owners: &mut BTreeMap<NodeId, SequenceOwner>,
) -> Result<(), ProgramValidationError> {
    for (index, rule) in program.failure_rules.iter().enumerate() {
        let FailureIteration::Generated(sequence) = &rule.iteration else {
            continue;
        };
        sequences::register_item(
            sequence,
            SequenceOwner::FailureRule(index + 1),
            expressions,
            owners,
        )?;
    }
    Ok(())
}

pub(super) fn validate(
    program: &Program,
    expressions: &BTreeMap<NodeId, &Expression>,
    sequence_items: &BTreeSet<NodeId>,
) -> Result<(), ProgramValidationError> {
    let sources = SourceCatalog::new(&program.source, &program.extra_sources);
    for (index, rule) in program.failure_rules.iter().enumerate() {
        let number = index + 1;
        let owner = SequenceOwner::FailureRule(number);
        let active_item = match &rule.iteration {
            FailureIteration::Source(source) => {
                if !source.path().is_empty()
                    && !sources.path_matches(source.path(), |node| node.repeating)
                {
                    return Err(ProgramValidationError::InvalidFailureSourceIteration {
                        rule: number,
                        source_path: source.path().to_vec(),
                    });
                }
                None
            }
            FailureIteration::Generated(sequence) => {
                for (input, expression) in sequence.inputs().enumerate() {
                    if !expressions.contains_key(&expression) {
                        return Err(ProgramValidationError::MissingSequenceExpression {
                            owner: owner.clone(),
                            role: SequenceExpressionRole::Input(input),
                            expression,
                        });
                    }
                    sequences::validate_context(
                        expression,
                        expressions,
                        sequence_items,
                        &[],
                        &owner,
                    )?;
                    joins::validate_expression(expression, expressions, sources, &[])?;
                }
                recursive_sequence::validate(sources, sequence, &owner)?;
                Some(sequence.item())
            }
        };
        let active_items = active_item.as_slice();
        if let Some(predicate) = rule.selection.predicate() {
            if !expressions.contains_key(&predicate) {
                return Err(ProgramValidationError::MissingFailurePredicate {
                    rule: number,
                    expression: predicate,
                });
            }
            sequences::validate_context(
                predicate,
                expressions,
                sequence_items,
                active_items,
                &owner,
            )?;
            joins::validate_expression(predicate, expressions, sources, &[])?;
        }
        if let Some(message) = rule.message {
            if !expressions.contains_key(&message) {
                return Err(ProgramValidationError::MissingFailureMessage {
                    rule: number,
                    expression: message,
                });
            }
            sequences::validate_context(
                message,
                expressions,
                sequence_items,
                active_items,
                &owner,
            )?;
            joins::validate_expression(message, expressions, sources, &[])?;
        }
    }
    Ok(())
}
