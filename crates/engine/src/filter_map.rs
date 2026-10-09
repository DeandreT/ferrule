use std::cell::Cell;
use std::collections::HashSet;

use ir::{Instance, ScalarType, Value};
use mapping::{FilterMapV1, FunctionId, NodeId, SequenceExpr};
use thiserror::Error;

use crate::eval_expr::EvalProgram;
use crate::sequence::{MAX_GENERATED_SEQUENCE_ITEMS, eval_sequence_arg, sequence_integer};
use crate::source_iteration::PositionFrame;
use crate::{EngineError, user_function};

pub const MAX_FILTER_MAP_SOURCE_ITEMS: u128 = 1_000_000;
pub const MAX_FILTER_MAP_WORK: u128 = 10_000_000;

/// Run-wide ceilings for filter/map subevaluations only. These are not RAM or
/// wall-time limits; legacy sequence and ordinary UDF paths keep their policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterMapLimits {
    source_items: u128,
    work: u128,
}

#[derive(Debug, Error, PartialEq, Eq)]
#[error("filter/map {kind:?} limit {requested} exceeds ceiling {max}")]
pub struct FilterMapLimitError {
    pub kind: FilterMapBudgetKind,
    pub requested: u128,
    pub max: u128,
}

impl FilterMapLimits {
    /// A host may lower either ceiling, including to zero, but cannot raise it.
    pub fn new(source_items: u128, work: u128) -> Result<Self, FilterMapLimitError> {
        for (kind, requested, max) in [
            (
                FilterMapBudgetKind::SourceItems,
                source_items,
                MAX_FILTER_MAP_SOURCE_ITEMS,
            ),
            (FilterMapBudgetKind::Work, work, MAX_FILTER_MAP_WORK),
        ] {
            if requested > max {
                return Err(FilterMapLimitError {
                    kind,
                    requested,
                    max,
                });
            }
        }
        Ok(Self { source_items, work })
    }

    pub const fn source_items(self) -> u128 {
        self.source_items
    }
    pub const fn work(self) -> u128 {
        self.work
    }
}

impl Default for FilterMapLimits {
    fn default() -> Self {
        Self {
            source_items: MAX_FILTER_MAP_SOURCE_ITEMS,
            work: MAX_FILTER_MAP_WORK,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMapBudgetKind {
    SourceItems,
    Work,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMapPhase {
    Source,
    Capture,
    Predicate,
    Mapper,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMapBoundaryKind {
    NodeEvaluation,
    CallEntry,
    SourceReservation,
    StageItem,
    Result,
}

/// Exact pre-work or failure boundary in one descriptor invocation. Function
/// and node IDs remain qualified, and source_position is never the dense index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterMapBoundary {
    pub item: NodeId,
    pub phase: FilterMapPhase,
    pub capture_index: Option<usize>,
    pub source_position: Option<usize>,
    pub function: Option<FunctionId>,
    pub node: Option<NodeId>,
    pub kind: FilterMapBoundaryKind,
}

/// A synchronous run-owned checker. It is consulted before charged work and
/// source reservation/allocation; it does not interrupt a builtin in progress.
pub trait FilterMapCancellation {
    fn is_cancelled(&self, boundary: &FilterMapBoundary) -> bool;
}

pub(crate) struct FilterMapRunState<'a> {
    limits: FilterMapLimits,
    source_items: Cell<u128>,
    work: Cell<u128>,
    cancellation: Option<&'a dyn FilterMapCancellation>,
}

impl<'a> FilterMapRunState<'a> {
    pub(crate) fn new(
        limits: FilterMapLimits,
        cancellation: Option<&'a dyn FilterMapCancellation>,
    ) -> Self {
        Self {
            limits,
            source_items: Cell::new(0),
            work: Cell::new(0),
            cancellation,
        }
    }

    #[cfg(test)]
    pub(crate) fn counters(&self) -> (u128, u128) {
        (self.source_items.get(), self.work.get())
    }

    fn check(&self, boundary: FilterMapBoundary) -> Result<(), EngineError> {
        if self
            .cancellation
            .is_some_and(|checker| checker.is_cancelled(&boundary))
        {
            return Err(wrap(boundary, EngineError::FilterMapCancelled));
        }
        Ok(())
    }

    fn charge(&self, boundary: FilterMapBoundary) -> Result<(), EngineError> {
        self.check(boundary)?;
        let used = self.work.get();
        if used >= self.limits.work {
            return Err(wrap(
                boundary,
                EngineError::FilterMapBudget {
                    kind: FilterMapBudgetKind::Work,
                    used,
                    requested: 1,
                    max: self.limits.work,
                },
            ));
        }
        self.work.set(used + 1);
        Ok(())
    }

    fn reserve(&self, boundary: FilterMapBoundary, requested: u128) -> Result<(), EngineError> {
        self.check(boundary)?;
        if requested > MAX_GENERATED_SEQUENCE_ITEMS {
            return Err(wrap(
                boundary,
                EngineError::GeneratedSequenceTooLarge {
                    requested,
                    max: MAX_GENERATED_SEQUENCE_ITEMS,
                },
            ));
        }
        let items_used = self.source_items.get();
        let work_used = self.work.get();
        for (kind, used, max) in [
            (
                FilterMapBudgetKind::SourceItems,
                items_used,
                self.limits.source_items,
            ),
            (FilterMapBudgetKind::Work, work_used, self.limits.work),
        ] {
            if requested > max - used {
                return Err(wrap(
                    boundary,
                    EngineError::FilterMapBudget {
                        kind,
                        used,
                        requested,
                        max,
                    },
                ));
            }
        }
        // Both limits passed: neither counter changes on a refused reservation.
        self.source_items.set(items_used + requested);
        self.work.set(work_used + requested);
        Ok(())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct FilterMapWork<'a> {
    pub(crate) state: &'a FilterMapRunState<'a>,
    pub(crate) boundary: FilterMapBoundary,
}

impl FilterMapWork<'_> {
    pub(crate) fn at_node(mut self, function: Option<FunctionId>, node: NodeId) -> Self {
        self.boundary.function = function;
        self.boundary.node = Some(node);
        self.boundary.kind = FilterMapBoundaryKind::NodeEvaluation;
        self
    }

    pub(crate) fn call(mut self, function: FunctionId) -> Result<(), EngineError> {
        self.boundary.function = Some(function);
        self.boundary.node = None;
        self.boundary.kind = FilterMapBoundaryKind::CallEntry;
        self.state.charge(self.boundary)
    }

    pub(crate) fn charge(self) -> Result<(), EngineError> {
        self.state.charge(self.boundary)
    }
    pub(crate) fn wrap(self, error: EngineError) -> EngineError {
        wrap(self.boundary, error)
    }
}

fn wrap(boundary: FilterMapBoundary, source: EngineError) -> EngineError {
    // Preserve the first, most specific envelope and its complete original cause.
    if matches!(source, EngineError::FilterMapRuntime { .. }) {
        source
    } else {
        EngineError::FilterMapRuntime {
            boundary,
            source: Box::new(source),
        }
    }
}

fn boundary(item: NodeId, phase: FilterMapPhase) -> FilterMapBoundary {
    FilterMapBoundary {
        item,
        phase,
        capture_index: None,
        source_position: None,
        function: None,
        node: None,
        kind: FilterMapBoundaryKind::Result,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate(
    program: EvalProgram<'_>,
    descriptor: &FilterMapV1,
    consumer: Option<NodeId>,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Vec<Value>, EngineError> {
    // Internal expression-only callers have no public run. Public runs always
    // install one state that survives all reached scopes and selected targets.
    if let Some(state) = program.filter_map_run_state {
        evaluate_with_state(
            program,
            descriptor,
            consumer,
            context,
            positions,
            in_progress,
            state,
        )
    } else {
        let state = FilterMapRunState::new(FilterMapLimits::default(), None);
        evaluate_with_state(
            program,
            descriptor,
            consumer,
            context,
            positions,
            in_progress,
            &state,
        )
    }
}

#[allow(clippy::too_many_arguments)]
fn evaluate_with_state<'a>(
    program: EvalProgram<'a>,
    descriptor: &FilterMapV1,
    consumer: Option<NodeId>,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
    state: &'a FilterMapRunState<'a>,
) -> Result<Vec<Value>, EngineError> {
    let program = program.with_filter_map_run_state(state);
    let source_work = FilterMapWork {
        state,
        boundary: boundary(descriptor.item, FilterMapPhase::Source),
    };
    let source = source_values(
        program.with_filter_map_work(source_work),
        descriptor,
        consumer,
        context,
        positions,
        in_progress,
        source_work,
    )?;
    let mut captures = Vec::with_capacity(descriptor.captures.len());
    for (index, capture) in descriptor.captures.iter().enumerate() {
        let mut at = boundary(descriptor.item, FilterMapPhase::Capture);
        at.capture_index = Some(index);
        at.node = Some(capture.node);
        let work = FilterMapWork {
            state,
            boundary: at,
        };
        let value = match consumer {
            Some(consumer) => crate::eval_expr::eval_node_input(
                program.with_filter_map_work(work),
                consumer,
                capture.node,
                descriptor.source.inputs().len() + index,
                context,
                positions,
                in_progress,
            ),
            None => crate::eval_expr::eval_expr(
                program.with_filter_map_work(work),
                capture.node,
                context,
                positions,
                in_progress,
            ),
        }?;
        require_value(work, &value, capture.ty)?;
        captures.push(value);
    }
    let mut output = Vec::new();
    for (index, item) in source.into_iter().enumerate() {
        let source_position = index + 1;
        let mut at = boundary(descriptor.item, FilterMapPhase::Predicate);
        at.source_position = Some(source_position);
        at.kind = FilterMapBoundaryKind::StageItem;
        state.check(at)?;
        let mut arguments = Vec::with_capacity(captures.len() + 2);
        arguments.push(item);
        arguments.push(Value::Int(source_position as i64));
        arguments.extend(captures.iter().cloned());
        let predicate = stage(
            program,
            descriptor.predicate,
            arguments.clone(),
            positions,
            FilterMapWork {
                state,
                boundary: at,
            },
        )?;
        if !matches!(predicate, Value::Bool(true)) {
            continue;
        }
        at.phase = FilterMapPhase::Mapper;
        let value = stage(
            program,
            descriptor.mapper,
            arguments,
            positions,
            FilterMapWork {
                state,
                boundary: at,
            },
        )?;
        output.push(value);
    }
    Ok(output)
}

#[allow(clippy::too_many_arguments)]
fn source_values(
    program: EvalProgram<'_>,
    descriptor: &FilterMapV1,
    consumer: Option<NodeId>,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
    work: FilterMapWork<'_>,
) -> Result<Vec<Value>, EngineError> {
    let SequenceExpr::Generate { from, to, .. } = descriptor.source.as_ref() else {
        return Err(work.wrap(EngineError::UnsupportedSequenceComposition {
            item: descriptor.item,
        }));
    };
    let from_value = match from {
        Some(node) => {
            match eval_sequence_arg(program, consumer, *node, 0, context, positions, in_progress)? {
                Some(value) => Some(value),
                None => {
                    reserve_empty(work)?;
                    return Ok(Vec::new());
                }
            }
        }
        None => None,
    };
    let Some(to_value) = eval_sequence_arg(
        program,
        consumer,
        *to,
        usize::from(from.is_some()),
        context,
        positions,
        in_progress,
    )?
    else {
        reserve_empty(work)?;
        return Ok(Vec::new());
    };
    // Preserve complete bound evaluation before lower/upper coercion.
    let lower = from_value
        .map_or(Ok(1), |value| sequence_integer(value, "generate-sequence"))
        .map_err(|error| {
            let mut at = work.boundary;
            at.node = *from;
            wrap(at, error)
        })?;
    let upper = sequence_integer(to_value, "generate-sequence").map_err(|error| {
        let mut at = work.boundary;
        at.node = Some(*to);
        wrap(at, error)
    })?;
    let requested = if lower > upper {
        0
    } else {
        (i128::from(upper) - i128::from(lower) + 1) as u128
    };
    let mut at = work.boundary;
    at.kind = FilterMapBoundaryKind::SourceReservation;
    work.state.reserve(at, requested)?;
    let mut values = Vec::with_capacity(requested as usize);
    if requested != 0 {
        values.extend((lower..=upper).map(Value::Int));
    }
    Ok(values)
}

fn reserve_empty(work: FilterMapWork<'_>) -> Result<(), EngineError> {
    let mut at = work.boundary;
    at.kind = FilterMapBoundaryKind::SourceReservation;
    work.state.reserve(at, 0)
}

fn stage(
    program: EvalProgram<'_>,
    function: FunctionId,
    arguments: Vec<Value>,
    positions: &[PositionFrame],
    mut work: FilterMapWork<'_>,
) -> Result<Value, EngineError> {
    work.boundary.function = Some(function);
    work.boundary.kind = FilterMapBoundaryKind::Result;
    let definition = program
        .user_functions
        .get(&function)
        .ok_or(EngineError::MissingUserFunction { function })
        .map_err(|error| work.wrap(error))?;
    work.boundary.node = Some(definition.output);
    let value = user_function::evaluate(
        program.user_functions,
        function,
        arguments,
        None,
        program.trace_sink,
        program.debug_hook,
        program.first_failure_reported,
        positions,
        program.purpose(),
        Some(work),
    )
    .map_err(|error| work.wrap(error))?;
    require_value(work, &value, definition.output_type)?;
    Ok(value)
}

fn require_value(
    work: FilterMapWork<'_>,
    value: &Value,
    expected: ScalarType,
) -> Result<(), EngineError> {
    let same_tag = matches!(
        (expected, value),
        (ScalarType::Bool, Value::Bool(_))
            | (ScalarType::Int, Value::Int(_))
            | (ScalarType::Float, Value::Float(_))
            | (ScalarType::String, Value::String(_))
    );
    if !same_tag {
        return Err(work.wrap(EngineError::FilterMapValueType {
            expected,
            found: value.clone(),
        }));
    }
    if let Value::Float(value) = value
        && !value.is_finite()
    {
        return Err(work.wrap(EngineError::FilterMapNonFinite {
            bits: value.to_bits(),
        }));
    }
    Ok(())
}
