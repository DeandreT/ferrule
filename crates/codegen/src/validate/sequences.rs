use std::collections::{BTreeMap, BTreeSet};

use mapping::NodeId;

use crate::{Expression, GeneratedSequence};

use super::{ProgramValidationError, SequenceExpressionRole, SequenceOwner, graph_dependencies};

pub(super) fn collect_expression_items(
    expressions: &BTreeMap<NodeId, &Expression>,
    owners: &mut BTreeMap<NodeId, SequenceOwner>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        let sequence = match expression {
            Expression::SequenceExists { sequence, .. }
            | Expression::SequenceItemAt { sequence, .. } => sequence,
            _ => continue,
        };
        register_item(
            sequence,
            SequenceOwner::Expression(node),
            expressions,
            owners,
        )?;
    }
    Ok(())
}

pub(super) fn register_item(
    sequence: &GeneratedSequence,
    owner: SequenceOwner,
    expressions: &BTreeMap<NodeId, &Expression>,
    owners: &mut BTreeMap<NodeId, SequenceOwner>,
) -> Result<(), ProgramValidationError> {
    let item = sequence.item();
    if let Some(first_owner) = owners.insert(item, owner.clone()) {
        return Err(ProgramValidationError::DuplicateSequenceItem {
            owner,
            first_owner,
            expression: item,
        });
    }
    let Some(expression) = expressions.get(&item) else {
        return Err(ProgramValidationError::MissingSequenceExpression {
            owner,
            role: SequenceExpressionRole::Item,
            expression: item,
        });
    };
    if !matches!(
        expression,
        Expression::SourceField {
            frame: None,
            path
        } if path.is_empty()
    ) {
        return Err(ProgramValidationError::InvalidSequenceItem {
            owner,
            expression: item,
        });
    }
    Ok(())
}

pub(super) fn validate_context(
    expression: NodeId,
    expressions: &BTreeMap<NodeId, &Expression>,
    sequence_items: &BTreeSet<NodeId>,
    active_sequence_items: &[NodeId],
    owner: &SequenceOwner,
) -> Result<(), ProgramValidationError> {
    let mut visited = BTreeSet::new();
    visit_context(
        expression,
        expression,
        expressions,
        sequence_items,
        active_sequence_items,
        owner,
        &mut visited,
    )
}

#[allow(clippy::too_many_arguments)]
fn visit_context(
    node: NodeId,
    root: NodeId,
    expressions: &BTreeMap<NodeId, &Expression>,
    sequence_items: &BTreeSet<NodeId>,
    active_sequence_items: &[NodeId],
    owner: &SequenceOwner,
    visited: &mut BTreeSet<(NodeId, Vec<NodeId>)>,
) -> Result<(), ProgramValidationError> {
    if !visited.insert((node, active_sequence_items.to_vec())) {
        return Ok(());
    }
    if sequence_items.contains(&node) && !active_sequence_items.contains(&node) {
        return Err(ProgramValidationError::SequenceItemOutOfContext {
            owner: owner.clone(),
            expression: root,
            item: node,
        });
    }
    let Some(expression) = expressions.get(&node) else {
        return Ok(());
    };
    match expression {
        Expression::SequenceExists {
            sequence,
            predicate,
        } => {
            let reducer = SequenceOwner::Expression(node);
            for input in sequence.inputs() {
                visit_context(
                    input,
                    root,
                    expressions,
                    sequence_items,
                    active_sequence_items,
                    &reducer,
                    visited,
                )?;
            }
            // Empty-path generated items resolve innermost-first, so only the
            // reducer's private item is visible inside its predicate.
            let predicate_items = [sequence.item()];
            visit_context(
                *predicate,
                root,
                expressions,
                sequence_items,
                &predicate_items,
                &reducer,
                visited,
            )
        }
        Expression::SequenceItemAt { sequence, index } => {
            let reducer = SequenceOwner::Expression(node);
            for input in sequence.inputs().chain([*index]) {
                visit_context(
                    input,
                    root,
                    expressions,
                    sequence_items,
                    &[],
                    &reducer,
                    visited,
                )?;
            }
            Ok(())
        }
        _ => {
            for dependency in graph_dependencies::of(expression) {
                visit_context(
                    dependency,
                    root,
                    expressions,
                    sequence_items,
                    active_sequence_items,
                    owner,
                    visited,
                )?;
            }
            Ok(())
        }
    }
}
