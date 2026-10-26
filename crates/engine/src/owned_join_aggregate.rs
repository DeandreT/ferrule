//! Owned joined aggregates.
//!
//! The plan executes once in the receiving owned context with the owned join
//! tuple rules: the latest active frame of each source's owner anchors its
//! walk, so a source naming an active owned row correlates to that one row, a
//! source inside an owned group or block walks only its members, and another
//! endpoint with the same path never supplies members. The optional
//! expression then evaluates once per tuple with only this join's members and
//! flattened position active, and `arg` evaluates once in the receiving
//! context after every tuple value, exactly as for the legacy joined
//! aggregate. Ordinary reducers retain their empty, null and type policies;
//! DecimalSum folds each projection before evaluating the next tuple.

use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{NodeId, OwnedJoinAggregate};

use crate::EngineError;
use crate::aggregate::{aggregate, collect_value};
use crate::eval_expr::{EvalProgram, eval_node_input};
use crate::owned_join_tuple;
use crate::owned_search::{access, owned_state};
use crate::source_iteration::PositionFrame;

/// Without an expression every tuple contributes Null, which Count counts and
/// the value reducers treat as the legacy joined aggregate does.
pub(crate) fn eval_owned_join_aggregate(
    program: EvalProgram<'_>,
    node: NodeId,
    owned: &OwnedJoinAggregate,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    if !matches!(
        owned.function,
        mapping::AggregateOp::Count
            | mapping::AggregateOp::Sum
            | mapping::AggregateOp::DecimalSum
            | mapping::AggregateOp::Avg
            | mapping::AggregateOp::Min
            | mapping::AggregateOp::Max
            | mapping::AggregateOp::Join
            | mapping::AggregateOp::ItemAt
    ) {
        return Err(functions::FunctionError::InvalidArgument {
            function: "owned_join_aggregate",
            message: "this reducer is not supported for owned joined collections",
        }
        .into());
    }
    let state = owned_state(program, node, "owned_join_aggregate")?;
    let access = access(node);
    let budget = state
        .roots
        .budget
        .work(None, &[], None, &state.frames, &state.origins)
        .map_err(&access)?;
    let tuples =
        owned_join_tuple::execute(state, positions, owned.join, &owned.plan, &budget, &access)?;
    let mut values = Vec::with_capacity(tuples.len());
    for tuple in &tuples {
        let value = match owned.expression {
            Some(expression) => eval_node_input(
                program.with_owned_join(tuple),
                node,
                expression,
                0,
                context,
                positions,
                in_progress,
            )?,
            None => Value::Null,
        };
        collect_value(owned.function, &mut values, value)?;
    }
    let arg = match owned.arg {
        Some(arg) => Some(eval_node_input(
            program,
            node,
            arg,
            usize::from(owned.expression.is_some()),
            context,
            positions,
            in_progress,
        )?),
        None => None,
    };
    aggregate(owned.function, tuples.len(), &values, arg)
}
