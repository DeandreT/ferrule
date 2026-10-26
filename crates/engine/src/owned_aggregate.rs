//! Explicitly owned collection aggregates.
//!
//! The collection is qualified once with the owned iteration rules shared by
//! owned search: the latest active same-owner frame anchors the walk, an absent,
//! null or empty collection yields no items, and another endpoint with the same
//! path is never consulted. Each item's value is then read or computed in that
//! item's owned context, and `arg` evaluates once in the parent context after
//! every item value, exactly as for the legacy collection aggregate. The
//! ordinary reducers retain their empty, null and type policies. DecimalSum
//! folds each projection immediately so arithmetic errors precede later reads.

use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{AggregateOp, NodeId, OwnedAggregate, OwnedAggregateValue};

use crate::EngineError;
use crate::aggregate::{aggregate, collect_value};
use crate::eval_expr::{EvalProgram, eval_node_input};
use crate::owned_context::OwnedContext;
use crate::owned_search::{access, candidates, item_field, item_frames, owned_state};
use crate::source_iteration::PositionFrame;

/// Count counts selected items and never reads a value path; a computed value
/// still evaluates once per item, as for the legacy aggregate.
pub(crate) fn eval_owned_aggregate(
    program: EvalProgram<'_>,
    node: NodeId,
    owned: &OwnedAggregate,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let state = owned_state(program, node, "owned_aggregate")?;
    let access = access(node);
    let (rows, work) = candidates(state, &owned.collection, positions.len()).map_err(&access)?;
    let item_count = rows.len();
    let mut values = Vec::new();
    match &owned.value {
        OwnedAggregateValue::Path(_) if owned.function == AggregateOp::Count => {}
        OwnedAggregateValue::Path(relative) => {
            let field = item_field(&owned.collection, relative);
            values.reserve(item_count);
            for row in rows {
                let item = OwnedContext {
                    roots: state.roots,
                    frames: row.frames,
                    origins: row.origins,
                    owned_ancestry: state.owned_ancestry,
                };
                collect_value(
                    owned.function,
                    &mut values,
                    item.field(&field).map_err(&access)?,
                )?;
            }
        }
        OwnedAggregateValue::Expression(expression) => {
            values.reserve(item_count);
            for row in rows {
                let (item_context, item_positions) =
                    item_frames(context, positions, &row, &work).map_err(&access)?;
                let item = OwnedContext {
                    roots: state.roots,
                    frames: row.frames,
                    origins: row.origins,
                    owned_ancestry: state.owned_ancestry,
                };
                let value = eval_node_input(
                    program.with_owned_context(&item),
                    node,
                    *expression,
                    0,
                    &item_context,
                    &item_positions,
                    in_progress,
                )?;
                collect_value(owned.function, &mut values, value)?;
            }
        }
    }
    let arg = match owned.arg {
        Some(arg) => Some(eval_node_input(
            program,
            node,
            arg,
            usize::from(owned.expression().is_some()),
            context,
            positions,
            in_progress,
        )?),
        None => None,
    };
    aggregate(owned.function, item_count, &values, arg)
}
