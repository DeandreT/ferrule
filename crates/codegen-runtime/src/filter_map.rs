//! Bounded eager filter/map support driven by emitted scalar callbacks.
//! This module does not interpret mapping graphs or alter legacy limits.
use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use crate::{RuntimeError, ScalarType, ScopeContext, Value};

pub const MAX_FILTER_MAP_SOURCE_ITEMS: u128 = 1_000_000;
pub const MAX_FILTER_MAP_WORK: u128 = 10_000_000;
pub const MAX_FILTER_MAP_FUNCTION_DEPTH: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterMapBudgetKind {
    SourceItems,
    Work,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterMapLimits {
    source_items: u128,
    work: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterMapLimitError {
    pub kind: FilterMapBudgetKind,
    pub requested: u128,
    pub max: u128,
}

impl FilterMapLimits {
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

impl fmt::Display for FilterMapLimitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "filter/map {:?} limit {} exceeds ceiling {}",
            self.kind, self.requested, self.max
        )
    }
}
impl std::error::Error for FilterMapLimitError {}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterMapBoundary {
    pub item: u32,
    pub phase: FilterMapPhase,
    pub capture_index: Option<usize>,
    pub source_position: Option<usize>,
    pub function: Option<u64>,
    pub node: Option<u32>,
    pub kind: FilterMapBoundaryKind,
}

pub trait FilterMapCancellation {
    fn is_cancelled(&self, boundary: &FilterMapBoundary) -> bool;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FilterMapCounters {
    pub source_items: u128,
    pub work: u128,
}

/// An additive host-control entry retains counters even when execution fails.
#[derive(Debug, PartialEq)]
pub struct FilterMapExecution<T, E = RuntimeError> {
    pub outcome: Result<T, E>,
    pub counters: FilterMapCounters,
}

/// One run owns these counters; cloned source/scope contexts share this state.
/// Ordinary generated entry points construct a fresh state for each call.
pub struct FilterMapRun<'a> {
    pub(crate) state: Rc<FilterMapState<'a>>,
}

impl<'a> FilterMapRun<'a> {
    pub fn new(
        limits: FilterMapLimits,
        cancellation: Option<&'a dyn FilterMapCancellation>,
    ) -> Self {
        Self {
            state: Rc::new(FilterMapState {
                limits,
                source_items: Cell::new(0),
                work: Cell::new(0),
                cancellation,
                functions: RefCell::new(Vec::new()),
            }),
        }
    }
    pub fn counters(&self) -> FilterMapCounters {
        self.state.counters()
    }
}

pub(crate) struct FilterMapState<'a> {
    limits: FilterMapLimits,
    source_items: Cell<u128>,
    work: Cell<u128>,
    cancellation: Option<&'a dyn FilterMapCancellation>,
    functions: RefCell<Vec<u64>>,
}

impl FilterMapState<'_> {
    fn counters(&self) -> FilterMapCounters {
        FilterMapCounters {
            source_items: self.source_items.get(),
            work: self.work.get(),
        }
    }
    fn check(&self, at: FilterMapBoundary) -> Result<(), RuntimeError> {
        if self
            .cancellation
            .is_some_and(|checker| checker.is_cancelled(&at))
        {
            return Err(wrap(at, RuntimeError::FilterMapCancelled));
        }
        Ok(())
    }
    fn charge(&self, at: FilterMapBoundary) -> Result<(), RuntimeError> {
        self.check(at)?;
        let used = self.work.get();
        if used >= self.limits.work {
            return Err(wrap(
                at,
                RuntimeError::FilterMapBudget {
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
    fn reserve(&self, at: FilterMapBoundary, requested: u128) -> Result<(), RuntimeError> {
        self.check(at)?;
        if requested > crate::MAX_GENERATED_SEQUENCE_ITEMS {
            return Err(wrap(
                at,
                RuntimeError::GeneratedSequenceTooLarge {
                    requested,
                    max: crate::MAX_GENERATED_SEQUENCE_ITEMS,
                },
            ));
        }
        let items = self.source_items.get();
        let work = self.work.get();
        for (kind, used, max) in [
            (
                FilterMapBudgetKind::SourceItems,
                items,
                self.limits.source_items,
            ),
            (FilterMapBudgetKind::Work, work, self.limits.work),
        ] {
            if requested > max - used {
                return Err(wrap(
                    at,
                    RuntimeError::FilterMapBudget {
                        kind,
                        used,
                        requested,
                        max,
                    },
                ));
            }
        }
        self.source_items.set(items + requested);
        self.work.set(work + requested);
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct FilterMapContext<'a> {
    pub(crate) state: Rc<FilterMapState<'a>>,
    pub(crate) boundary: Option<FilterMapBoundary>,
}

/// Restores function depth even when parameter/body/return adaptation fails.
pub struct FilterMapCallGuard<'a> {
    state: Rc<FilterMapState<'a>>,
}
impl Drop for FilterMapCallGuard<'_> {
    fn drop(&mut self) {
        self.state.functions.borrow_mut().pop();
    }
}

impl<'a> ScopeContext<'a> {
    pub fn with_filter_map_run(mut self, run: &FilterMapRun<'a>) -> Self {
        self.filter_map = Some(FilterMapContext {
            state: Rc::clone(&run.state),
            boundary: None,
        });
        self
    }
    pub fn with_filter_map_defaults(&self) -> Self {
        if self.filter_map.is_some() {
            self.clone()
        } else {
            self.clone()
                .with_filter_map_run(&FilterMapRun::new(FilterMapLimits::default(), None))
        }
    }
    fn filter_map_at(&self, at: FilterMapBoundary) -> Self {
        let mut context = self.with_filter_map_defaults();
        context
            .filter_map
            .as_mut()
            .expect("default state installed")
            .boundary = Some(at);
        context
    }
    /// Called once before every emitted reached node, only charging active
    /// descriptor work. Repeated references are separate node evaluations.
    pub fn filter_map_node(&self, function: Option<u64>, node: u32) -> Result<Self, RuntimeError> {
        let Some(work) = &self.filter_map else {
            return Ok(self.clone());
        };
        let Some(mut at) = work.boundary else {
            return Ok(self.clone());
        };
        at.function = function;
        at.node = Some(node);
        at.kind = FilterMapBoundaryKind::NodeEvaluation;
        work.state.charge(at)?;
        Ok(self.filter_map_at(at))
    }
    pub fn filter_map_wrap(&self, error: RuntimeError) -> RuntimeError {
        if let Some(at) = self.filter_map.as_ref().and_then(|work| work.boundary) {
            wrap(at, error)
        } else {
            error
        }
    }
    /// All argument expressions have completed before this entry. It runs
    /// before ordered declared-parameter adaptation, cycle/depth and body.
    pub fn filter_map_call_entry(
        &self,
        function: u64,
    ) -> Result<Option<FilterMapCallGuard<'a>>, RuntimeError> {
        let Some(work) = &self.filter_map else {
            return Ok(None);
        };
        let Some(mut at) = work.boundary else {
            return Ok(None);
        };
        at.function = Some(function);
        at.node = None;
        at.kind = FilterMapBoundaryKind::CallEntry;
        work.state.charge(at)?;
        let mut functions = work.state.functions.borrow_mut();
        if functions.contains(&function) {
            return Err(wrap(at, RuntimeError::UserFunctionCycle { function }));
        }
        if functions.len() >= MAX_FILTER_MAP_FUNCTION_DEPTH {
            return Err(wrap(
                at,
                RuntimeError::UserFunctionDepth {
                    limit: MAX_FILTER_MAP_FUNCTION_DEPTH,
                },
            ));
        }
        functions.push(function);
        drop(functions);
        Ok(Some(FilterMapCallGuard {
            state: Rc::clone(&work.state),
        }))
    }
}

fn wrap(at: FilterMapBoundary, source: RuntimeError) -> RuntimeError {
    if matches!(source, RuntimeError::FilterMapRuntime { .. }) {
        source
    } else {
        RuntimeError::FilterMapRuntime {
            boundary: at,
            source: Box::new(source),
        }
    }
}

pub struct FilterMapInput<'f, 's> {
    pub node: u32,
    pub evaluate: &'f dyn Fn(&ScopeContext<'s>) -> Result<Value, RuntimeError>,
}
pub struct FilterMapCapture<'f, 's> {
    pub input: FilterMapInput<'f, 's>,
    pub ty: ScalarType,
}
type FilterMapFunctionEvaluator<'f, 's> =
    &'f dyn Fn(&ScopeContext<'s>, &[Value]) -> Result<Value, RuntimeError>;

pub struct FilterMapFunction<'f, 's> {
    pub id: u64,
    pub output: u32,
    pub evaluate: FilterMapFunctionEvaluator<'f, 's>,
}
pub struct FilterMapDescriptor<'f, 's> {
    pub item: u32,
    pub from: Option<FilterMapInput<'f, 's>>,
    pub to: FilterMapInput<'f, 's>,
    pub captures: &'f [FilterMapCapture<'f, 's>],
    pub predicate: FilterMapFunction<'f, 's>,
    pub mapper: FilterMapFunction<'f, 's>,
    pub output_type: ScalarType,
}

fn boundary(item: u32, phase: FilterMapPhase) -> FilterMapBoundary {
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

fn require_value(
    at: FilterMapBoundary,
    value: &Value,
    expected: ScalarType,
) -> Result<(), RuntimeError> {
    if !matches!(
        (expected, value),
        (ScalarType::Int, Value::Int(_))
            | (ScalarType::Float, Value::Float(_))
            | (ScalarType::Bool, Value::Bool(_))
            | (ScalarType::String, Value::String(_))
    ) {
        return Err(wrap(
            at,
            RuntimeError::FilterMapValueType {
                expected,
                found: value.clone(),
            },
        ));
    }
    if let Value::Float(value) = value
        && !value.is_finite()
    {
        return Err(wrap(
            at,
            RuntimeError::FilterMapNonFinite {
                bits: value.to_bits(),
            },
        ));
    }
    Ok(())
}

/// Fully evaluates a bounded range and captures, then predicate/mapper stages
/// in source order. No successful prefix is returned after a later error.
pub fn filter_map_sequence<'s>(
    context: &ScopeContext<'s>,
    descriptor: FilterMapDescriptor<'_, 's>,
) -> Result<Vec<Value>, RuntimeError> {
    let context = context.with_filter_map_defaults();
    let source_at = boundary(descriptor.item, FilterMapPhase::Source);
    let mut source_context = context.filter_map_at(source_at);
    let from = match &descriptor.from {
        Some(input) => {
            let mut at = source_at;
            at.node = Some(input.node);
            let input_context = context.filter_map_at(at);
            let value = (input.evaluate)(&input_context).map_err(|error| wrap(at, error))?;
            if value == Value::Null || value.is_json_null() {
                None
            } else {
                Some(value)
            }
        }
        None => Some(Value::Int(1)),
    };
    let to = if from.is_some() {
        let mut at = source_at;
        at.node = Some(descriptor.to.node);
        let input_context = context.filter_map_at(at);
        let value = (descriptor.to.evaluate)(&input_context).map_err(|error| wrap(at, error))?;
        if value == Value::Null || value.is_json_null() {
            None
        } else {
            Some(value)
        }
    } else {
        None
    };
    let range = match (from, to) {
        (Some(from), Some(to)) => {
            let lower = crate::generated_sequence::sequence_integer(from).map_err(|error| {
                let mut at = source_at;
                at.node = descriptor.from.as_ref().map(|input| input.node);
                wrap(at, error)
            })?;
            let upper = crate::generated_sequence::sequence_integer(to).map_err(|error| {
                let mut at = source_at;
                at.node = Some(descriptor.to.node);
                wrap(at, error)
            })?;
            Some((lower, upper))
        }
        _ => None,
    };
    let requested = range.map_or(0, |(from, to)| {
        if from > to {
            0
        } else {
            (i128::from(to) - i128::from(from) + 1) as u128
        }
    });
    let mut reservation = source_at;
    reservation.kind = FilterMapBoundaryKind::SourceReservation;
    let state = &source_context
        .filter_map
        .as_ref()
        .expect("state installed")
        .state;
    state.reserve(reservation, requested)?;
    let mut source = Vec::with_capacity(requested as usize);
    if let Some((from, to)) = range
        && requested != 0
    {
        source.extend((from..=to).map(Value::Int));
    }
    let mut captures = Vec::with_capacity(descriptor.captures.len());
    for (index, capture) in descriptor.captures.iter().enumerate() {
        let mut at = boundary(descriptor.item, FilterMapPhase::Capture);
        at.capture_index = Some(index);
        at.node = Some(capture.input.node);
        source_context = context.filter_map_at(at);
        let value = (capture.input.evaluate)(&source_context).map_err(|error| wrap(at, error))?;
        require_value(at, &value, capture.ty)?;
        captures.push(value);
    }
    let mut output = Vec::new();
    for (index, item) in source.into_iter().enumerate() {
        let mut at = boundary(descriptor.item, FilterMapPhase::Predicate);
        at.source_position = Some(index + 1);
        at.kind = FilterMapBoundaryKind::StageItem;
        context
            .filter_map
            .as_ref()
            .expect("state installed")
            .state
            .check(at)?;
        let mut arguments = Vec::with_capacity(captures.len() + 2);
        arguments.push(item);
        arguments.push(Value::Int((index + 1) as i64));
        arguments.extend(captures.iter().cloned());
        at.function = Some(descriptor.predicate.id);
        at.node = Some(descriptor.predicate.output);
        at.kind = FilterMapBoundaryKind::Result;
        let predicate = (descriptor.predicate.evaluate)(&context.filter_map_at(at), &arguments)
            .map_err(|error| wrap(at, error))?;
        require_value(at, &predicate, ScalarType::Bool)?;
        if !matches!(predicate, Value::Bool(true)) {
            continue;
        }
        at.phase = FilterMapPhase::Mapper;
        at.function = Some(descriptor.mapper.id);
        at.node = Some(descriptor.mapper.output);
        let value = (descriptor.mapper.evaluate)(&context.filter_map_at(at), &arguments)
            .map_err(|error| wrap(at, error))?;
        require_value(at, &value, descriptor.output_type)?;
        output.push(value);
    }
    Ok(output)
}
