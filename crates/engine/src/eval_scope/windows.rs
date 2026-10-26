//! Parent-context window bounds and ordered application to selected cohorts.
//!
//! Owned windows keep their receiving-context observations and logical original
//! cohort size when implicit First retains only a bounded prefix.

use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{NodeId, SequenceWindow};

use crate::EngineError;
use crate::eval_expr::{EvalProgram, eval_expr};
use crate::source_iteration::PositionFrame;
use crate::trace::{TraceEvent, TraceScope, TraceWindow, record, record_owned_event};

#[derive(Debug, Clone, Copy)]
pub(super) enum EvaluatedWindow {
    SkipFirst(usize),
    First(usize),
    From(usize),
    FromTo { first: usize, last: usize },
    Last(usize),
}

impl From<EvaluatedWindow> for TraceWindow {
    fn from(window: EvaluatedWindow) -> Self {
        match window {
            EvaluatedWindow::SkipFirst(count) => Self::SkipFirst(count),
            EvaluatedWindow::First(count) => Self::First(count),
            EvaluatedWindow::From(position) => Self::From(position),
            EvaluatedWindow::FromTo { first, last } => Self::FromTo { first, last },
            EvaluatedWindow::Last(count) => Self::Last(count),
        }
    }
}

pub(super) fn eval_sequence_window(
    program: EvalProgram<'_>,
    window: SequenceWindow,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<EvaluatedWindow, EngineError> {
    Ok(match window {
        SequenceWindow::SkipFirst { count } => {
            EvaluatedWindow::SkipFirst(eval_item_count(program, count, context, positions)?)
        }
        SequenceWindow::First { count } => {
            EvaluatedWindow::First(eval_item_count(program, count, context, positions)?)
        }
        SequenceWindow::From { position } => {
            EvaluatedWindow::From(eval_item_count(program, position, context, positions)?)
        }
        SequenceWindow::FromTo { first, last } => EvaluatedWindow::FromTo {
            first: eval_item_count(program, first, context, positions)?,
            last: eval_item_count(program, last, context, positions)?,
        },
        SequenceWindow::Last { count } => {
            EvaluatedWindow::Last(eval_item_count(program, count, context, positions)?)
        }
    })
}

pub(super) fn apply_basic_owned_sequence_windows<T>(
    mut items: Vec<T>,
    windows: &[EvaluatedWindow],
    receiving_program: EvalProgram<'_>,
    positions: &[PositionFrame],
    trace_scope: &TraceScope,
    bounded_first_before: Option<usize>,
) -> Vec<T> {
    for (index, window) in windows.iter().enumerate() {
        // Only eligible implicit First retains a prefix before window application.
        // Its original cohort was fully traversed and accounted without retaining it.
        let before = if index == 0 {
            bounded_first_before.unwrap_or(items.len())
        } else {
            items.len()
        };
        items = apply_sequence_windows(items, std::slice::from_ref(window), None, trace_scope);
        record_owned_event(receiving_program, positions, || TraceEvent::WindowApplied {
            scope: trace_scope.clone(),
            window_index: index + 1,
            window: (*window).into(),
            before,
            after: items.len(),
        });
    }
    items
}

pub(super) fn apply_sequence_windows<T>(
    mut items: Vec<T>,
    windows: &[EvaluatedWindow],
    trace_sink: Option<&dyn crate::TraceSink>,
    trace_scope: &TraceScope,
) -> Vec<T> {
    for (index, window) in windows.iter().enumerate() {
        let before = items.len();
        items = match *window {
            EvaluatedWindow::SkipFirst(count) => items.into_iter().skip(count).collect(),
            EvaluatedWindow::First(count) => items.into_iter().take(count).collect(),
            EvaluatedWindow::From(position) => {
                items.into_iter().skip(position.saturating_sub(1)).collect()
            }
            EvaluatedWindow::FromTo { first, last } => {
                let skip = first.saturating_sub(1);
                let count = last.saturating_sub(skip);
                items.into_iter().skip(skip).take(count).collect()
            }
            EvaluatedWindow::Last(count) => {
                let skip = items.len().saturating_sub(count);
                items.into_iter().skip(skip).collect()
            }
        };
        record(trace_sink, || TraceEvent::WindowApplied {
            scope: trace_scope.clone(),
            window_index: index + 1,
            window: (*window).into(),
            before,
            after: items.len(),
        });
    }
    items
}

fn eval_item_count(
    program: EvalProgram<'_>,
    node: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<usize, EngineError> {
    let mut in_progress = HashSet::new();
    let value = eval_expr(program, node, context, positions, &mut in_progress)?;
    let count = match &value {
        Value::Int(value) => Some(*value),
        Value::Float(value) if value.is_finite() => Some(value.trunc() as i64),
        Value::String(value) => value.trim().parse::<i64>().ok(),
        _ => None,
    };
    count
        .map(|count| count.max(0) as usize)
        .ok_or(EngineError::NotAnItemCount {
            node,
            found: value.type_name(),
        })
}

pub(super) fn eval_block_size(
    program: EvalProgram<'_>,
    node: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<usize, EngineError> {
    let size = eval_item_count(program, node, context, positions)?;
    if size == 0 {
        return Err(EngineError::InvalidBlockSize { node });
    }
    Ok(size)
}
