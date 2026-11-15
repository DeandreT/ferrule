//! Private, occurrence-budgeted tree copying without a recursive native visitor.
//! Source trees and the returned tree keep their existing ownership/Drop behavior.

use super::{ExpansionBudget, ScalarExpr, ScalarSequenceExpr};
use std::collections::BTreeMap;

#[derive(Clone, Copy)]
enum Mode<'a> {
    Clone,
    Substitute(&'a BTreeMap<u32, ScalarExpr>),
}

#[derive(Clone, Copy)]
enum Source<'a> {
    Scalar(&'a ScalarExpr),
    Sequence(&'a ScalarSequenceExpr),
}

enum Built {
    Scalar(ScalarExpr),
    Sequence(ScalarSequenceExpr),
}

impl Built {
    fn scalar(self) -> ScalarExpr {
        match self {
            Self::Scalar(value) => value,
            Self::Sequence(_) => unreachable!("scalar expansion child kind"),
        }
    }

    fn sequence(self) -> ScalarSequenceExpr {
        match self {
            Self::Sequence(value) => value,
            Self::Scalar(_) => unreachable!("sequence expansion child kind"),
        }
    }
}

struct Frame<'a> {
    source: Source<'a>,
    mode: Mode<'a>,
    depth: usize,
    next: usize,
    base: usize,
    // The old Call arm clones its name after the claim, before its arguments.
    function: Option<String>,
}

impl<'a> Frame<'a> {
    /// A cursor, not a pending task for every unvisited argument of a wide Call.
    fn child(&mut self) -> Option<(Source<'a>, usize)> {
        let position = self.next;
        self.next += 1;
        match self.source {
            Source::Scalar(value) => {
                let child = match value {
                    ScalarExpr::DefaultedParameter { default, .. } => {
                        (position == 0).then_some(Source::Scalar(default))
                    }
                    ScalarExpr::Call { args, .. } => args.get(position).map(Source::Scalar),
                    ScalarExpr::If {
                        condition,
                        then,
                        else_,
                    } => match position {
                        0 => Some(Source::Scalar(condition)),
                        1 => Some(Source::Scalar(then)),
                        2 => Some(Source::Scalar(else_)),
                        _ => None,
                    },
                    ScalarExpr::ValueMap { input, .. } => {
                        (position == 0).then_some(Source::Scalar(input))
                    }
                    ScalarExpr::SequenceItemAt { sequence, index } => match position {
                        0 => Some(Source::Sequence(sequence)),
                        1 => Some(Source::Scalar(index)),
                        _ => None,
                    },
                    ScalarExpr::SequenceExists {
                        sequence,
                        predicate,
                        ..
                    } => match position {
                        0 => Some(Source::Sequence(sequence)),
                        1 => Some(Source::Scalar(predicate)),
                        _ => None,
                    },
                    ScalarExpr::SequenceAggregate {
                        sequence,
                        predicate,
                        expression,
                        arg,
                        ..
                    } => {
                        if position == 0 {
                            Some(Source::Sequence(sequence))
                        } else {
                            // Optional operands retain their declaration order without holes.
                            [predicate.as_deref(), expression.as_deref(), arg.as_deref()]
                                .into_iter()
                                .flatten()
                                .nth(position - 1)
                                .map(Source::Scalar)
                        }
                    }
                    ScalarExpr::Parameter(_)
                    | ScalarExpr::Const(_)
                    | ScalarExpr::RuntimeValue(_)
                    | ScalarExpr::SequenceItem(_)
                    | ScalarExpr::SequencePosition(_) => None,
                };
                child.map(|child| (child, self.depth + 1))
            }
            Source::Sequence(value) => {
                let child = match value {
                    ScalarSequenceExpr::Tokenize { input, delimiter } => match position {
                        0 => Some(input.as_ref()),
                        1 => Some(delimiter.as_ref()),
                        _ => None,
                    },
                    ScalarSequenceExpr::TokenizeByLength { input, length } => match position {
                        0 => Some(input.as_ref()),
                        1 => Some(length.as_ref()),
                        _ => None,
                    },
                    ScalarSequenceExpr::TokenizeRegex {
                        input,
                        pattern,
                        flags,
                    } => match position {
                        0 => Some(input.as_ref()),
                        1 => Some(pattern.as_ref()),
                        2 => flags.as_deref(),
                        _ => None,
                    },
                    ScalarSequenceExpr::Generate { from, to } => {
                        match (from.as_deref(), position) {
                            (Some(from), 0) => Some(from),
                            (Some(_), 1) | (None, 0) => Some(to.as_ref()),
                            _ => None,
                        }
                    }
                };
                // Sequence wrappers neither claim a node nor add another level.
                child.map(|child| (Source::Scalar(child), self.depth))
            }
        }
    }
}

/// All completed children live here, not hidden in the borrowed frames. On Err,
/// Drop empties this storage by destructively walking owned children iteratively.
#[derive(Default)]
struct Partials(Vec<Built>);

impl Partials {
    fn pop_scalar(&mut self) -> ScalarExpr {
        self.0.pop().expect("completed scalar child").scalar()
    }

    fn pop_sequence(&mut self) -> ScalarSequenceExpr {
        self.0.pop().expect("completed sequence child").sequence()
    }

    fn optional(&mut self, present: bool) -> Option<Box<ScalarExpr>> {
        present.then(|| Box::new(self.pop_scalar()))
    }
}

impl Drop for Partials {
    fn drop(&mut self) {
        let mut work = Vec::new();
        while let Some(value) = self.0.pop() {
            work.push(Drain::Value(value));
            drain(&mut work);
        }
    }
}

enum Drain {
    Value(Built),
    Arguments(std::vec::IntoIter<ScalarExpr>),
}

fn drain(work: &mut Vec<Drain>) {
    while let Some(value) = work.pop() {
        match value {
            Drain::Arguments(mut arguments) => {
                if let Some(argument) = arguments.next() {
                    work.push(Drain::Arguments(arguments));
                    work.push(Drain::Value(Built::Scalar(argument)));
                }
            }
            Drain::Value(Built::Scalar(value)) => {
                #[cfg(test)]
                observe(|stats| stats.cleaned_scalars += 1);
                let mut scalar = |value| work.push(Drain::Value(Built::Scalar(value)));
                match value {
                    ScalarExpr::DefaultedParameter { default, .. } => scalar(*default),
                    ScalarExpr::Call { args, .. } => work.push(Drain::Arguments(args.into_iter())),
                    ScalarExpr::If {
                        condition,
                        then,
                        else_,
                    } => {
                        scalar(*else_);
                        scalar(*then);
                        scalar(*condition);
                    }
                    ScalarExpr::ValueMap { input, .. } => scalar(*input),
                    ScalarExpr::SequenceItemAt { sequence, index } => {
                        scalar(*index);
                        work.push(Drain::Value(Built::Sequence(sequence)));
                    }
                    ScalarExpr::SequenceExists {
                        sequence,
                        predicate,
                        ..
                    } => {
                        scalar(*predicate);
                        work.push(Drain::Value(Built::Sequence(sequence)));
                    }
                    ScalarExpr::SequenceAggregate {
                        sequence,
                        predicate,
                        expression,
                        arg,
                        ..
                    } => {
                        if let Some(arg) = arg {
                            scalar(*arg);
                        }
                        if let Some(expression) = expression {
                            scalar(*expression);
                        }
                        if let Some(predicate) = predicate {
                            scalar(*predicate);
                        }
                        work.push(Drain::Value(Built::Sequence(sequence)));
                    }
                    ScalarExpr::Parameter(_)
                    | ScalarExpr::Const(_)
                    | ScalarExpr::RuntimeValue(_)
                    | ScalarExpr::SequenceItem(_)
                    | ScalarExpr::SequencePosition(_) => {}
                }
            }
            Drain::Value(Built::Sequence(value)) => {
                #[cfg(test)]
                observe(|stats| stats.cleaned_sequences += 1);
                let mut scalar = |value| work.push(Drain::Value(Built::Scalar(value)));
                match value {
                    ScalarSequenceExpr::Tokenize { input, delimiter } => {
                        scalar(*delimiter);
                        scalar(*input);
                    }
                    ScalarSequenceExpr::TokenizeByLength { input, length } => {
                        scalar(*length);
                        scalar(*input);
                    }
                    ScalarSequenceExpr::TokenizeRegex {
                        input,
                        pattern,
                        flags,
                    } => {
                        if let Some(flags) = flags {
                            scalar(*flags);
                        }
                        scalar(*pattern);
                        scalar(*input);
                    }
                    ScalarSequenceExpr::Generate { from, to } => {
                        scalar(*to);
                        if let Some(from) = from {
                            scalar(*from);
                        }
                    }
                }
            }
        }
        #[cfg(test)]
        observe(|stats| stats.max_cleanup_work = stats.max_cleanup_work.max(work.len()));
    }
}

fn enter<'a>(
    mut source: Source<'a>,
    mut mode: Mode<'a>,
    depth: usize,
    budget: &mut ExpansionBudget,
    frames: &mut Vec<Frame<'a>>,
    values: &mut Partials,
) -> Result<(), String> {
    loop {
        let scalar = match source {
            Source::Scalar(value) => {
                #[cfg(test)]
                observe(|stats| stats.scalar_entries += 1);
                value
            }
            Source::Sequence(_) => {
                #[cfg(test)]
                observe(|stats| stats.sequence_entries += 1);
                frames.push(Frame {
                    source,
                    mode,
                    depth,
                    next: 0,
                    base: values.0.len(),
                    function: None,
                });
                #[cfg(test)]
                observe(|stats| stats.max_frames = stats.max_frames.max(frames.len()));
                return Ok(());
            }
        };
        if let Mode::Substitute(parameters) = mode {
            match scalar {
                ScalarExpr::Parameter(id)
                | ScalarExpr::DefaultedParameter {
                    component_id: id, ..
                } if parameters.contains_key(id) => {
                    source = Source::Scalar(&parameters[id]);
                    mode = Mode::Clone;
                    continue;
                }
                ScalarExpr::DefaultedParameter { default, .. } => {
                    // Erase the wrapper, retaining both the depth and substitution mode.
                    source = Source::Scalar(default);
                    continue;
                }
                _ => {}
            }
        }
        #[cfg(test)]
        observe(|stats| stats.claim_attempts += 1);
        budget.claim(depth)?;
        let function = match scalar {
            ScalarExpr::Call { function, .. } => Some(function.clone()),
            _ => None,
        };
        frames.push(Frame {
            source,
            mode,
            depth,
            next: 0,
            base: values.0.len(),
            function,
        });
        #[cfg(test)]
        observe(|stats| stats.max_frames = stats.max_frames.max(frames.len()));
        return Ok(());
    }
}

fn finish(frame: Frame<'_>, values: &mut Partials) {
    let built = match frame.source {
        Source::Scalar(value) => Built::Scalar(match value {
            ScalarExpr::Parameter(id) => match frame.mode {
                Mode::Clone => ScalarExpr::Parameter(*id),
                Mode::Substitute(_) => ScalarExpr::Const(ir::Value::Null),
            },
            ScalarExpr::DefaultedParameter { component_id, .. } => ScalarExpr::DefaultedParameter {
                component_id: *component_id,
                default: Box::new(values.pop_scalar()),
            },
            ScalarExpr::Const(value) => ScalarExpr::Const(value.clone()),
            ScalarExpr::RuntimeValue(value) => ScalarExpr::RuntimeValue(*value),
            ScalarExpr::Call { .. } => ScalarExpr::Call {
                function: frame.function.expect("prepared Call name"),
                // Only successfully claimed/completed children can be reserved here.
                args: values.0.drain(frame.base..).map(Built::scalar).collect(),
            },
            ScalarExpr::If { .. } => {
                let else_ = Box::new(values.pop_scalar());
                let then = Box::new(values.pop_scalar());
                let condition = Box::new(values.pop_scalar());
                ScalarExpr::If {
                    condition,
                    then,
                    else_,
                }
            }
            ScalarExpr::ValueMap {
                input_type,
                table,
                default,
                ..
            } => ScalarExpr::ValueMap {
                input: Box::new(values.pop_scalar()),
                input_type: *input_type,
                table: table.clone(),
                default: default.clone(),
            },
            ScalarExpr::SequenceItemAt { .. } => {
                let index = Box::new(values.pop_scalar());
                let sequence = values.pop_sequence();
                ScalarExpr::SequenceItemAt { sequence, index }
            }
            ScalarExpr::SequenceExists { item_feed, .. } => {
                let predicate = Box::new(values.pop_scalar());
                let sequence = values.pop_sequence();
                ScalarExpr::SequenceExists {
                    sequence,
                    item_feed: *item_feed,
                    predicate,
                }
            }
            ScalarExpr::SequenceItem(feed) => ScalarExpr::SequenceItem(*feed),
            ScalarExpr::SequencePosition(feed) => ScalarExpr::SequencePosition(*feed),
            ScalarExpr::SequenceAggregate {
                function,
                item_feed,
                predicate,
                expression,
                arg,
                ..
            } => {
                let arg = values.optional(arg.is_some());
                let expression = values.optional(expression.is_some());
                let predicate = values.optional(predicate.is_some());
                let sequence = values.pop_sequence();
                ScalarExpr::SequenceAggregate {
                    function: *function,
                    sequence,
                    item_feed: *item_feed,
                    predicate,
                    expression,
                    arg,
                }
            }
        }),
        Source::Sequence(value) => Built::Sequence(match value {
            ScalarSequenceExpr::Tokenize { .. } => {
                let delimiter = Box::new(values.pop_scalar());
                let input = Box::new(values.pop_scalar());
                ScalarSequenceExpr::Tokenize { input, delimiter }
            }
            ScalarSequenceExpr::TokenizeByLength { .. } => {
                let length = Box::new(values.pop_scalar());
                let input = Box::new(values.pop_scalar());
                ScalarSequenceExpr::TokenizeByLength { input, length }
            }
            ScalarSequenceExpr::TokenizeRegex { flags, .. } => {
                let flags = values.optional(flags.is_some());
                let pattern = Box::new(values.pop_scalar());
                let input = Box::new(values.pop_scalar());
                ScalarSequenceExpr::TokenizeRegex {
                    input,
                    pattern,
                    flags,
                }
            }
            ScalarSequenceExpr::Generate { from, .. } => {
                let to = Box::new(values.pop_scalar());
                let from = values.optional(from.is_some());
                ScalarSequenceExpr::Generate { from, to }
            }
        }),
    };
    debug_assert_eq!(values.0.len(), frame.base);
    values.0.push(built);
    #[cfg(test)]
    observe(|stats| stats.finished += 1);
}

fn run<'a>(
    root: Source<'a>,
    mode: Mode<'a>,
    budget: &mut ExpansionBudget,
    depth: usize,
) -> Result<Built, String> {
    let mut values = Partials::default();
    let mut frames = Vec::new();
    enter(root, mode, depth, budget, &mut frames, &mut values)?;
    while let Some(frame) = frames.last_mut() {
        if let Some((child, depth)) = frame.child() {
            let mode = frame.mode;
            enter(child, mode, depth, budget, &mut frames, &mut values)?;
        } else {
            finish(frames.pop().expect("active expansion frame"), &mut values);
        }
    }
    let result = values.0.pop().expect("one completed expansion root");
    debug_assert!(values.0.is_empty());
    Ok(result)
}

pub(super) fn scalar(
    expression: &ScalarExpr,
    parameters: Option<&BTreeMap<u32, ScalarExpr>>,
    budget: &mut ExpansionBudget,
    depth: usize,
) -> Result<ScalarExpr, String> {
    run(
        Source::Scalar(expression),
        parameters.map_or(Mode::Clone, Mode::Substitute),
        budget,
        depth,
    )
    .map(Built::scalar)
}

pub(super) fn sequence(
    expression: &ScalarSequenceExpr,
    parameters: Option<&BTreeMap<u32, ScalarExpr>>,
    budget: &mut ExpansionBudget,
    depth: usize,
) -> Result<ScalarSequenceExpr, String> {
    run(
        Source::Sequence(expression),
        parameters.map_or(Mode::Clone, Mode::Substitute),
        budget,
        depth,
    )
    .map(Built::sequence)
}

#[cfg(test)]
#[derive(Clone, Copy, Default)]
pub(super) struct Observation {
    pub scalar_entries: usize,
    pub sequence_entries: usize,
    pub claim_attempts: usize,
    pub finished: usize,
    pub max_frames: usize,
    pub cleaned_scalars: usize,
    pub cleaned_sequences: usize,
    pub max_cleanup_work: usize,
}

#[cfg(test)]
std::thread_local! {
    static OBSERVATION: std::cell::Cell<Observation> = std::cell::Cell::new(Observation::default());
}

#[cfg(test)]
fn observe(update: impl FnOnce(&mut Observation)) {
    OBSERVATION.with(|cell| {
        let mut stats = cell.get();
        update(&mut stats);
        cell.set(stats);
    });
}

#[cfg(test)]
pub(super) fn take_observation() -> Observation {
    OBSERVATION.with(|cell| cell.replace(Observation::default()))
}

#[cfg(test)]
pub(super) fn drain_for_test(value: ScalarExpr) {
    let mut values = Partials::default();
    values.0.push(Built::Scalar(value));
}
