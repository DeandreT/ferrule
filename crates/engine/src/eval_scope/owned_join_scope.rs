//! The owned join scope pipeline. Controls keep the legacy join phases: a
//! filter before sorting and every sort key see the original tuple position,
//! sorting renumbers it, a later filter sees the sorted position, window
//! bounds evaluate once in the receiving parent, and bindings and static
//! descendants see the compacted output position. Raw member positions never
//! change.

use mapping::{JoinId, OwnedJoinPlan};

use super::*;
use crate::owned_join_tuple::{self, OwnedJoinTuple};
use crate::trace::TraceIteration;

#[allow(clippy::too_many_arguments)]
pub(super) fn eval_owned_join_scope<'a>(
    program: EvalProgram<'a>,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&'a Instance],
    positions: &[PositionFrame],
    extra_sources: &[NamedSource],
    source_loader: Option<&dyn DynamicSourceLoader>,
    trace_scope: &TraceScope,
    join: JoinId,
    plan: &OwnedJoinPlan,
) -> Result<Instance, EngineError> {
    let receiving_program = program;
    let program = program.with_owned_trace_phase(crate::TraceOwnedPhase::Structural);
    let state = program
        .owned_context
        .expect("admitted owned joins run in the paired context");
    let access = |source| match source {
        crate::OwnedSourceError::UnsupportedSourceDomain { found } => {
            EngineError::UnsupportedOwnedContext {
                node: None,
                function: None,
                scope: Some(trace_scope.source_scope.clone()),
                context: if found == "a document set" {
                    crate::OwnedCapabilityContext::Document
                } else {
                    crate::OwnedCapabilityContext::GeneratedItem
                },
            }
        }
        source => EngineError::OwnedSourceAccess {
            node: None,
            scope: Some(Box::new(trace_scope.source_scope.clone())),
            source,
        },
    };
    record_owned_event(receiving_program, positions, || TraceEvent::ScopeStarted {
        scope: trace_scope.clone(),
        iteration: TraceIteration::Join { join },
        positions: trace_positions(positions),
    });
    let budget = state
        .roots
        .budget
        .work(None, &[], None, &state.frames, &state.origins)
        .map_err(access)?;
    let mut tuples = owned_join_tuple::execute(state, positions, join, plan, &budget, access)?;
    let candidates = tuples.len();
    for (ordinal, tuple) in tuples.iter().enumerate() {
        record_owned_event(program.with_owned_join(tuple), positions, || {
            TraceEvent::IterationCandidate {
                scope: trace_scope.clone(),
                ordinal: ordinal + 1,
                positions: trace_positions(positions),
                source_row: None,
            }
        });
    }
    let filter_before_sort = scope.filter.is_some()
        && scope.has_sort()
        && scope.sort_filter_order == SortFilterOrder::FilterThenSort;
    let selection_phase = if scope.has_sort() {
        TraceFilterPhase::AfterSort
    } else {
        TraceFilterPhase::Selection
    };
    let retain = |tuples: Vec<OwnedJoinTuple<'a>>, phase| {
        let mut retained = Vec::with_capacity(tuples.len());
        for tuple in tuples {
            if passes_scope_filter(
                program.with_owned_join(&tuple),
                scope.filter,
                phase,
                trace_scope,
                context,
                positions,
            )? {
                retained.push(tuple);
            }
        }
        Ok::<_, EngineError>(retained)
    };
    if filter_before_sort {
        tuples = retain(tuples, TraceFilterPhase::BeforeSort)?;
    }
    let sort_keys = scope.sort_keys().collect::<Vec<_>>();
    if !sort_keys.is_empty() {
        let mut keyed = Vec::with_capacity(tuples.len());
        for tuple in tuples {
            let tuple_program = program.with_owned_join(&tuple);
            let values = sort_keys
                .iter()
                .map(|key| {
                    eval_expr(
                        tuple_program,
                        key.node,
                        context,
                        positions,
                        &mut HashSet::new(),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            record_owned_event(tuple_program, positions, || TraceEvent::SortCandidate {
                scope: trace_scope.clone(),
                positions: trace_positions(positions),
                keys: sort_keys
                    .iter()
                    .zip(&values)
                    .map(|(key, value)| TraceSortKey {
                        node: key.node,
                        descending: key.descending,
                        value: TraceValue::new(value),
                    })
                    .collect(),
            });
            keyed.push((tuple, values));
        }
        keyed.sort_by(|(_, left), (_, right)| {
            for ((left, right), key) in left.iter().zip(right).zip(&sort_keys) {
                let order = sort_value_ordering(left, right);
                let order = if key.descending {
                    order.reverse()
                } else {
                    order
                };
                if order != std::cmp::Ordering::Equal {
                    return order;
                }
            }
            std::cmp::Ordering::Equal
        });
        tuples = keyed
            .into_iter()
            .enumerate()
            .map(|(index, (tuple, _))| {
                record_owned_event(program.with_owned_join(&tuple), positions, || {
                    TraceEvent::SortPosition {
                        scope: trace_scope.clone(),
                        positions: trace_positions(positions),
                        output_index: index + 1,
                    }
                });
                tuple.with_position(index + 1)
            })
            .collect();
    }
    let mut windows = scope
        .windows
        .iter()
        .copied()
        .map(|window| eval_sequence_window(program, window, context, positions))
        .collect::<Result<Vec<_>, _>>()?;
    if scope.iteration_output == IterationOutput::First {
        windows.push(EvaluatedWindow::First(1));
    }
    if !windows.is_empty() && !filter_before_sort {
        tuples = retain(tuples, selection_phase)?;
    }
    let tuples = apply_basic_owned_sequence_windows(
        tuples,
        &windows,
        receiving_program,
        positions,
        trace_scope,
        None,
    );
    let filter_in_output = !filter_before_sort && windows.is_empty();
    let mut produced = Vec::with_capacity(tuples.len());
    for tuple in &tuples {
        if filter_in_output
            && !passes_scope_filter(
                program.with_owned_join(tuple),
                scope.filter,
                selection_phase,
                trace_scope,
                context,
                positions,
            )?
        {
            continue;
        }
        let output = tuple.with_position(produced.len() + 1);
        let evaluator = ItemEvaluator {
            program: program
                .with_owned_join(&output)
                .with_owned_trace_phase(crate::TraceOwnedPhase::Output),
            scope,
            trace_scope,
            target,
            extra_sources,
            source_loader,
        };
        if let Some(item) = evaluator.produce(context, positions, positions, false)? {
            produced.push(item.instance);
        }
    }
    let produced_count = produced.len();
    let result = finalize_scope_output(
        scope,
        target.is_some_and(|target| target.repeating),
        produced,
    )?;
    record_owned_event(receiving_program, positions, || TraceEvent::ScopeFinished {
        scope: trace_scope.clone(),
        candidates,
        produced: produced_count,
        kind: TraceOutputKind::of(&result),
    });
    Ok(result)
}
