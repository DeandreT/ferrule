use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{
    AggregateOp, CollectionJoinSource, ComputedJoinValue, JoinConditions, JoinId, JoinKeyLeft,
    JoinPlan, JoinSource, NodeId,
};

use super::EngineError;
use super::aggregate::aggregate;
use super::eval_expr::{EvalProgram, eval_node_input};
use super::frame_view::{FrameKind, FrameOrigin, FrameView};
use super::policy::BudgetMeter;
use super::resolve::field_scalar;
use super::sequence::eval_sequence_in_progress;
use super::source_iteration::{InstanceBinding, PositionFrame, WalkExtension, walk};

pub(super) struct JoinedRow<'a> {
    instances: Vec<&'a Instance>,
    frames: Vec<RowFrame>,
    /// Whether every source contributed exactly one instance per position.
    /// A walk can end at an unpositioned value or synthesize a position with
    /// no instance; such rows keep the legacy index pairing.
    exact: bool,
    /// The computed member this row holds, as an index into the members of
    /// the plan's computed source. It adds no instance or frame.
    computed: Option<usize>,
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

/// Executes `plan` in the receiving context. `key` evaluates the plan's
/// computed source there to the values of its members, once, before any
/// source is selected or compared; it is not called for a plan without a
/// computed source.
pub(super) fn execute<'a>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    join: JoinId,
    plan: &JoinPlan,
    budget: Option<&BudgetMeter>,
    key: impl FnOnce(&ComputedJoinValue) -> Result<Vec<Value>, EngineError>,
) -> Result<Vec<JoinedRow<'a>>, EngineError> {
    // Computed members live for this execution only: they add no instance or
    // frame to a row, and no read names them once the tuples are built.
    // Null-like values give no member; every other value, duplicates and
    // empty strings included, gives one member in evaluation order.
    let mut computed = plan
        .computed_source()
        .map(|computed| key(computed.value()))
        .transpose()?
        .unwrap_or_default();
    computed.retain(|value| !is_null_like(value));
    let computed = computed.as_slice();
    let mut sources = plan.sources();
    let Some(first) = sources.next() else {
        return Ok(Vec::new());
    };
    let receiving = FrameView::receiving(context, positions);
    let mut rows = member_rows(context, &receiving, join, first, computed);
    for (right_source, conditions) in plan.stages() {
        let right_rows = member_rows(context, &receiving, join, right_source, computed);
        let mut joined = Vec::new();
        for left in rows {
            let left_frames = left.view();
            for right in &right_rows {
                // Every examined pairing is one restricted join-row unit.
                if let Some(budget) = budget {
                    budget.charge_join_row()?;
                }
                let right_member = match right_source {
                    JoinSource::Collection(_) => RightMember::Row(right),
                    JoinSource::Computed(_) => {
                        RightMember::Computed(computed_value(computed, right))
                    }
                };
                let left_computed = computed_value(computed, &left);
                if conditions_match(&left_frames, left_computed, right_member, conditions)? {
                    let mut instances = left.instances.clone();
                    instances.extend(right.instances.iter().copied());
                    let exact = left.exact && right.exact;
                    let frames = left
                        .frames
                        .iter()
                        .map(|frame| (frame, 0))
                        .chain(
                            right
                                .frames
                                .iter()
                                .map(|frame| (frame, left.instances.len())),
                        )
                        .map(|(frame, offset)| {
                            let mut position = frame.position.clone();
                            position.binding = if exact {
                                position.binding.rebased(offset)
                            } else {
                                InstanceBinding::LegacyUnspecified
                            };
                            RowFrame {
                                position,
                                origin: frame.origin,
                            }
                        })
                        .collect();
                    joined.push(JoinedRow {
                        instances,
                        frames,
                        exact,
                        computed: left.computed.or(right.computed),
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

/// Rows of one plan member. A computed source gives one row without
/// instances or frames per present member value, in order.
fn member_rows<'a>(
    context: &[&'a Instance],
    receiving: &FrameView<'_, 'a>,
    join: JoinId,
    source: &JoinSource,
    computed: &[Value],
) -> Vec<JoinedRow<'a>> {
    match source {
        JoinSource::Collection(source) => source_rows(context, receiving, join, source),
        JoinSource::Computed(_) => (0..computed.len())
            .map(|member| JoinedRow {
                instances: Vec::new(),
                frames: Vec::new(),
                exact: true,
                computed: Some(member),
            })
            .collect(),
    }
}

/// The value of the computed member `row` holds, if it holds one.
fn computed_value<'v>(computed: &'v [Value], row: &JoinedRow<'_>) -> Option<&'v Value> {
    row.computed.and_then(|member| computed.get(member))
}

/// The right member of one examined pairing.
#[derive(Clone, Copy)]
enum RightMember<'r, 'a, 'v> {
    Row(&'r JoinedRow<'a>),
    /// A computed member, whose key is its value.
    Computed(Option<&'v Value>),
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
    // Inputs are indexed in visit order: the computed key or its sequence's
    // inputs, the per-tuple expression, then the argument.
    let key_inputs = input
        .plan
        .computed_source()
        .map_or(0, |computed| match computed.value() {
            ComputedJoinValue::Scalar(_) => 1,
            ComputedJoinValue::Sequence(sequence) => sequence.inputs().len(),
        });
    let rows = execute(
        context,
        positions,
        input.join,
        input.plan,
        program.budget,
        |key| match key {
            ComputedJoinValue::Scalar(node) => eval_node_input(
                program,
                input.consumer,
                *node,
                0,
                context,
                positions,
                in_progress,
            )
            .map(|value| vec![value]),
            ComputedJoinValue::Sequence(sequence) => eval_sequence_in_progress(
                program,
                sequence,
                Some(input.consumer),
                context,
                positions,
                in_progress,
            ),
        },
    )?;
    let extensions = extensions(&rows);
    let mut values = Vec::with_capacity(extensions.len());
    for extension in &extensions {
        let value = match input.expression {
            Some(expression) => {
                let mut item_context = context.to_vec();
                let mut item_positions = positions.to_vec();
                extension.append_to(&mut item_context, &mut item_positions);
                eval_node_input(
                    program,
                    input.consumer,
                    expression,
                    key_inputs,
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
                key_inputs + usize::from(input.expression.is_some()),
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
/// the source to that one row. An explicitly unbound frame yields no rows.
/// When no frame matches (or an unbound legacy frame matches), the source is
/// walked from the innermost context holding its first segment.
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
    }) {
        let Some(instance) = receiving.instance(frame) else {
            if frame.position.binding != InstanceBinding::LegacyUnspecified {
                return Vec::new();
            }
            return walked_source_rows(context, join, source);
        };
        let mut correlated = frame.position.clone();
        correlated.binding = InstanceBinding::ContextSlot(0);
        correlated.join = Some(join);
        correlated.join_position = None;
        return vec![JoinedRow {
            instances: vec![instance],
            frames: vec![RowFrame {
                position: correlated,
                origin: FrameOrigin::Correlated,
            }],
            exact: true,
            computed: None,
        }];
    }
    walked_source_rows(context, join, source)
}

fn walked_source_rows<'a>(
    context: &[&'a Instance],
    join: JoinId,
    source: &CollectionJoinSource,
) -> Vec<JoinedRow<'a>> {
    let collection = source.collection();
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
                            binding: extension.instances.len().checked_sub(1).map_or(
                                InstanceBinding::ExplicitUnbound,
                                InstanceBinding::ContextSlot,
                            ),
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
                    let exact = extension.instances.len() == positions.len();
                    // Malformed unvalidated rows retain their historical index
                    // pairing, including shifts caused by unpositioned members.
                    if !exact {
                        for position in &mut positions {
                            position.binding = InstanceBinding::LegacyUnspecified;
                        }
                    }
                    JoinedRow {
                        computed: None,
                        exact,
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

/// `left_computed` is the value of the computed member the left row holds,
/// if it holds one.
fn conditions_match(
    left: &FrameView<'_, '_>,
    left_computed: Option<&Value>,
    right: RightMember<'_, '_, '_>,
    conditions: &JoinConditions,
) -> Result<bool, EngineError> {
    for condition in conditions.iter() {
        let left_value = match condition.left() {
            JoinKeyLeft::Collection { collection, path } => row_value(left, collection, path),
            JoinKeyLeft::Computed(_) => left_computed,
        };
        let Some(left_value) = left_value else {
            return Ok(false);
        };
        let right_value = match right {
            RightMember::Row(row) => row
                .instances
                .last()
                .and_then(|item| field_scalar(item, condition.right_path())),
            RightMember::Computed(value) => value,
        };
        let Some(right_value) = right_value else {
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
