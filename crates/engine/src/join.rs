use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{
    AggregateOp, CollectionJoinSource, JoinConditions, JoinId, JoinKeyLeft, JoinPlan, JoinSource,
    NodeId,
};

use super::EngineError;
use super::aggregate::aggregate;
use super::eval_expr::{EvalProgram, eval_node_input};
use super::frame_view::{FrameKind, FrameOrigin, FrameView};
use super::policy::BudgetMeter;
use super::resolve::field_scalar;
use super::source_iteration::{PositionFrame, WalkExtension, walk};

pub(super) struct JoinedRow<'a> {
    instances: Vec<&'a Instance>,
    frames: Vec<RowFrame>,
    /// Whether every source contributed exactly one instance per position.
    /// A walk can end at an unpositioned value or synthesize a position with
    /// no instance; such rows keep the legacy index pairing.
    exact: bool,
}

struct RowFrame {
    position: PositionFrame,
    origin: FrameOrigin,
}

impl<'a> JoinedRow<'a> {
    pub(super) fn view(&self) -> FrameView<'_, 'a> {
        FrameView::row(
            &self.instances,
            self.frames
                .iter()
                .map(|frame| (&frame.position, frame.origin)),
            self.exact,
        )
    }

    fn positions(&self) -> Vec<PositionFrame> {
        self.frames
            .iter()
            .map(|frame| frame.position.clone())
            .collect()
    }
}

pub(super) struct AggregateInput<'a> {
    pub(super) consumer: NodeId,
    pub(super) function: AggregateOp,
    pub(super) join: JoinId,
    pub(super) plan: &'a JoinPlan,
    pub(super) expression: Option<NodeId>,
    pub(super) arg: Option<NodeId>,
}

pub(super) fn execute<'a>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    join: JoinId,
    plan: &JoinPlan,
    budget: Option<&BudgetMeter>,
) -> Result<Vec<JoinedRow<'a>>, EngineError> {
    let sources = plan
        .sources()
        .map(|source| collection_source(join, source))
        .collect::<Result<Vec<_>, _>>()?;
    let Some((first, rights)) = sources.split_first() else {
        return Ok(Vec::new());
    };
    let receiving = FrameView::receiving(context, positions);
    let mut rows = source_rows(context, &receiving, join, first);
    for (right_source, (_, conditions)) in rights.iter().zip(plan.stages()) {
        let right_rows = source_rows(context, &receiving, join, right_source);
        let mut joined = Vec::new();
        for left in rows {
            let left_frames = left.view();
            for right in &right_rows {
                // Every examined pairing is one restricted join-row unit.
                if let Some(budget) = budget {
                    budget.charge_join_row()?;
                }
                if conditions_match(join, &left_frames, right, conditions)? {
                    let mut instances = left.instances.clone();
                    instances.extend(right.instances.iter().copied());
                    let frames = left
                        .frames
                        .iter()
                        .chain(&right.frames)
                        .map(|frame| RowFrame {
                            position: frame.position.clone(),
                            origin: frame.origin,
                        })
                        .collect();
                    joined.push(JoinedRow {
                        instances,
                        frames,
                        exact: left.exact && right.exact,
                    });
                }
            }
        }
        rows = joined;
    }
    for (index, row) in rows.iter_mut().enumerate() {
        if let Some(frame) = row.frames.last_mut() {
            frame.position.join_position = Some((join, index + 1));
        }
    }
    Ok(rows)
}

/// Computed sources are refused until the engine executes them.
fn collection_source(
    join: JoinId,
    source: &JoinSource,
) -> Result<&CollectionJoinSource, EngineError> {
    match source {
        JoinSource::Collection(source) => Ok(source),
        JoinSource::Computed(computed) => Err(EngineError::ComputedJoinSource {
            join,
            computed: computed.id(),
        }),
    }
}

pub(super) fn extensions<'a>(rows: &[JoinedRow<'a>]) -> Vec<WalkExtension<'a>> {
    rows.iter()
        .map(|row| WalkExtension {
            instances: row.instances.clone(),
            positions: row.positions(),
        })
        .collect()
}

pub(super) fn eval_aggregate(
    program: EvalProgram<'_>,
    input: AggregateInput<'_>,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let rows = execute(context, positions, input.join, input.plan, program.budget)?;
    let extensions = extensions(&rows);
    let mut values = Vec::with_capacity(extensions.len());
    for extension in &extensions {
        let value = match input.expression {
            Some(expression) => {
                let mut item_context = context.to_vec();
                item_context.extend(extension.instances.iter().copied());
                let mut item_positions = positions.to_vec();
                item_positions.extend(extension.positions.iter().cloned());
                eval_node_input(
                    program,
                    input.consumer,
                    expression,
                    0,
                    &item_context,
                    &item_positions,
                    in_progress,
                )?
            }
            None => Value::Null,
        };
        super::aggregate::collect_value(input.function, &mut values, value)?;
    }
    let arg = input
        .arg
        .map(|arg| {
            eval_node_input(
                program,
                input.consumer,
                arg,
                usize::from(input.expression.is_some()),
                context,
                positions,
                in_progress,
            )
        })
        .transpose()?;
    aggregate(input.function, extensions.len(), &values, arg)
}

/// Rows of one join source. The latest ungrouped receiving frame of the
/// source's exact collection that this join did not contribute correlates
/// the source to that one row. When that frame has no instance, or no frame
/// matches, the source is walked from the innermost context frame holding
/// its first segment, falling back to the innermost frame.
fn source_rows<'a>(
    context: &[&'a Instance],
    receiving: &FrameView<'_, 'a>,
    join: JoinId,
    source: &CollectionJoinSource,
) -> Vec<JoinedRow<'a>> {
    let collection = source.collection();
    if let Some(frame) = receiving.latest(|frame| {
        frame.kind != FrameKind::GroupMembers
            && frame.origin != FrameOrigin::Joined(join)
            && frame.collection == collection
    }) && let Some(instance) = receiving.instance(frame)
    {
        let mut correlated = frame.position.clone();
        correlated.join = Some(join);
        correlated.join_position = None;
        return vec![JoinedRow {
            instances: vec![instance],
            frames: vec![RowFrame {
                position: correlated,
                origin: FrameOrigin::Correlated,
            }],
            exact: true,
        }];
    }
    context
        .iter()
        .rev()
        .find(|frame| match collection.first() {
            Some(first) => frame.field(first).is_some(),
            None => true,
        })
        .copied()
        .or_else(|| context.last().copied())
        .map_or_else(Vec::new, |base| {
            walk(base, collection, &[], &[], &[])
                .into_iter()
                .enumerate()
                .map(|(index, extension)| {
                    let mut positions = extension.positions;
                    let origin = if positions.is_empty() {
                        positions.push(PositionFrame {
                            generated_origin: None,
                            collection: collection.to_vec(),
                            index: index + 1,
                            grouped: false,
                            join: Some(join),
                            join_position: None,
                            document_path: None,
                        });
                        FrameOrigin::Synthetic
                    } else {
                        for position in &mut positions {
                            position.join = Some(join);
                        }
                        FrameOrigin::Walked
                    };
                    JoinedRow {
                        exact: extension.instances.len() == positions.len(),
                        instances: extension.instances,
                        frames: positions
                            .into_iter()
                            .map(|position| RowFrame { position, origin })
                            .collect(),
                    }
                })
                .collect()
        })
}

fn conditions_match(
    join: JoinId,
    left: &FrameView<'_, '_>,
    right: &JoinedRow<'_>,
    conditions: &JoinConditions,
) -> Result<bool, EngineError> {
    for condition in conditions.iter() {
        let left_collection = match condition.left() {
            JoinKeyLeft::Collection { collection, .. } => collection,
            JoinKeyLeft::Computed(computed) => {
                return Err(EngineError::ComputedJoinSource {
                    join,
                    computed: *computed,
                });
            }
        };
        let Some(left_value) = row_value(left, left_collection, condition.left_path()) else {
            return Ok(false);
        };
        let Some(right_value) = right
            .instances
            .last()
            .and_then(|item| field_scalar(item, condition.right_path()))
        else {
            return Ok(false);
        };
        if is_null_like(left_value) || is_null_like(right_value) {
            return Ok(false);
        }
        if functions::call("equal", &[left_value.clone(), right_value.clone()])?
            != Value::Bool(true)
        {
            return Ok(false);
        }
    }
    Ok(true)
}

/// A left key read from the latest row frame of its exact collection.
fn row_value<'a>(
    row: &FrameView<'_, 'a>,
    collection: &[String],
    path: &[String],
) -> Option<&'a Value> {
    let frame = row.latest(|frame| frame.collection == collection)?;
    row.instance(frame)
        .and_then(|item| field_scalar(item, path))
}

fn is_null_like(value: &Value) -> bool {
    value.is_null_like()
}
