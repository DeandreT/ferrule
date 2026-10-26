//! Owned candidate selection and controls, with the existing metadata ledger.
//!
//! Ordinary evaluation remains in the parent module. These candidates retain
//! their borrowed trees, owned provenance metadata, and optional First replay;
//! grouping is delegated without materializing another candidate cohort.

use std::collections::HashSet;

use ir::Instance;
use mapping::{IterationOutput, NamedSource, Scope, SortFilterOrder};

use super::construction::ItemEvaluator;
use super::owned_grouping::{BasicGroupPartition, eval_basic_owned_groups};
use super::windows::{
    EvaluatedWindow, apply_basic_owned_sequence_windows, eval_block_size, eval_sequence_window,
};
use super::{
    owned_access_error, owned_document_set, owned_dynamic_loading, owned_join_scope,
    passes_scope_filter, renumber_extension, retain_first_source_candidate,
};
use crate::aggregate::sort_value_ordering;
use crate::eval_expr::{EvalProgram, eval_expr};
use crate::frame_view::FrameView;
use crate::iteration_output::finalize_scope_output;
use crate::source_iteration::{PositionFrame, WalkExtension};
use crate::trace::{
    TraceEvent, TraceFilterPhase, TraceOutputKind, TraceScope, TraceSortKey, TraceSourceRow,
    TraceValue, basic_scope_iteration, record_owned_event, trace_positions,
};
use crate::{DynamicSourceLoader, EngineError};

#[allow(clippy::too_many_arguments)]
pub(super) fn record_basic_candidate<'tree>(
    program: EvalProgram<'_>,
    state: &crate::owned_context::OwnedContext<'tree>,
    positions: &[PositionFrame],
    trace_scope: &TraceScope,
    extension: &mut crate::source_iteration::ObservedExtension<'tree>,
    ordinal: usize,
    own: bool,
    has_source: bool,
) {
    let Some(sink) = program.trace_sink else {
        return;
    };
    let mut candidate_positions = positions.to_vec();
    candidate_positions.extend_from_slice(&extension.extension.positions);
    // Move the already-budgeted metadata into this transient borrowed view.
    // Restore it before traversal retains or drops the candidate; no tree is cloned.
    let candidate: crate::owned_context::OwnedContext<'tree> = crate::owned_context::OwnedContext {
        roots: state.roots,
        frames: std::mem::take(&mut extension.frames),
        origins: std::mem::take(&mut extension.origins),
        owned_ancestry: state.owned_ancestry || own,
    };
    record_owned_event(
        program
            .with_owned_context(&candidate)
            .with_owned_trace_phase(crate::TraceOwnedPhase::Structural),
        &candidate_positions,
        || TraceEvent::IterationCandidate {
            scope: trace_scope.clone(),
            ordinal,
            positions: trace_positions(&candidate_positions),
            source_row: if has_source && sink.wants_source_row() {
                extension
                    .extension
                    .instances
                    .last()
                    .map(|instance| TraceSourceRow::new(instance))
            } else {
                None
            },
        },
    );
    extension.frames = candidate.frames;
    extension.origins = candidate.origins;
}

#[allow(clippy::too_many_arguments)]
fn record_basic_sort_position<'tree>(
    program: EvalProgram<'_>,
    state: &crate::owned_context::OwnedContext<'tree>,
    positions: &[PositionFrame],
    trace_scope: &TraceScope,
    extension: &mut crate::source_iteration::ObservedExtension<'tree>,
    output_index: usize,
    own: bool,
) {
    if program.trace_sink.is_none() {
        return;
    }
    let mut input_positions = positions.to_vec();
    input_positions.extend_from_slice(&extension.extension.positions);
    // Ranking observes the original structural cursor before renumbering.
    // Borrow its already-budgeted metadata without retaining a second cohort.
    let original: crate::owned_context::OwnedContext<'tree> = crate::owned_context::OwnedContext {
        roots: state.roots,
        frames: std::mem::take(&mut extension.frames),
        origins: std::mem::take(&mut extension.origins),
        owned_ancestry: state.owned_ancestry || own,
    };
    record_owned_event(
        program
            .with_owned_context(&original)
            .with_owned_trace_phase(crate::TraceOwnedPhase::Structural),
        &input_positions,
        || TraceEvent::SortPosition {
            scope: trace_scope.clone(),
            positions: trace_positions(&input_positions),
            output_index,
        },
    );
    extension.frames = original.frames;
    extension.origins = original.origins;
}

#[allow(clippy::too_many_arguments)]
pub(super) fn eval_basic_owned_scope<'a>(
    program: EvalProgram<'a>,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&'a Instance],
    positions: &[PositionFrame],
    extra_sources: &[NamedSource],
    source_loader: Option<&dyn DynamicSourceLoader>,
    trace_scope: &TraceScope,
) -> Result<Instance, EngineError> {
    use crate::source_iteration::{
        ObservedExtension, ObservedWalkRequest, reserve_observed_copy, select_observed,
        select_observed_with_candidate_observer,
    };
    if let Some((join, plan)) = scope.iteration.owned_join() {
        return owned_join_scope::eval_owned_join_scope(
            program,
            scope,
            target,
            context,
            positions,
            extra_sources,
            source_loader,
            trace_scope,
            join,
            plan,
        );
    }
    // Scope boundaries observe the incoming receiving context and phase.
    // Traversal and item producers retain their existing explicit phase programs.
    let receiving_program = program;
    let program = program.with_owned_trace_phase(crate::TraceOwnedPhase::Structural);
    let state = program
        .owned_context
        .expect("basic owned route has a context");
    let access = |source| owned_access_error(trace_scope, source);
    let domain = crate::source_ownership::scope_context(scope);
    if domain != crate::OwnedCapabilityContext::Basic {
        return Err(EngineError::UnsupportedOwnedContext {
            node: None,
            function: None,
            scope: Some(trace_scope.source_scope.clone()),
            context: domain,
        });
    }
    record_owned_event(receiving_program, positions, || TraceEvent::ScopeStarted {
        scope: trace_scope.clone(),
        iteration: basic_scope_iteration(&scope.iteration)
            .expect("Basic scope domain admitted before lifecycle metadata"),
        positions: trace_positions(positions),
    });
    let retain_first = retain_first_source_candidate(scope);
    let has_source = scope.source_selector().is_some();
    let own_source = matches!(
        scope.source_selector(),
        Some(mapping::SourceSelectorRef::Owned(_))
    );
    let mut observe_candidate = |extension: &mut ObservedExtension<'a>, ordinal| {
        record_basic_candidate(
            program,
            state,
            positions,
            trace_scope,
            extension,
            ordinal,
            own_source,
            has_source,
        );
    };
    // An owned dynamic endpoint without an active document loads one per driver.
    if let Some(mapping::SourceSelectorRef::Owned(source)) = scope.source_selector()
        && let Some(driver) = owned_dynamic_loading::driver(state, source).map_err(access)?
    {
        return owned_dynamic_loading::eval_loading_scope(
            receiving_program,
            scope,
            target,
            context,
            positions,
            extra_sources,
            source_loader,
            trace_scope,
            source,
            driver,
        );
    }
    let (extensions, budget, own, candidates, replay) = match scope.source_selector() {
        Some(mapping::SourceSelectorRef::Owned(source)) => {
            let (owner, base, prefix, tail, budget) = state.select(source).map_err(access)?;
            // Only the endpoint root of an owner with no active document can
            // be a set; grouped scopes keep refusing it in the walk below.
            if let Instance::DocumentSet(members) = base
                && prefix.is_empty()
                && !state.roots.loaded(owner)
                && !scope.has_grouping()
                && scope.post_group_filter.is_none()
            {
                return owned_document_set::eval_document_set_scope(
                    receiving_program,
                    scope,
                    target,
                    context,
                    positions,
                    extra_sources,
                    source_loader,
                    trace_scope,
                    source,
                    owned_document_set::Members {
                        key: owner,
                        members,
                        budget,
                    },
                );
            }
            let request = ObservedWalkRequest {
                base,
                path: tail,
                raw_prefix: prefix,
                canonical_prefix: prefix,
                owner: Some(owner),
                context: state,
                position_offset: positions.len(),
                strict_collection: true,
            };
            let selected = if program.trace_sink.is_some() {
                select_observed_with_candidate_observer(
                    request,
                    &budget,
                    retain_first,
                    &mut observe_candidate,
                )
            } else {
                select_observed(request, &budget, retain_first)
            }
            .map_err(access)?;
            (
                selected.extensions,
                budget,
                true,
                selected.candidates,
                selected.replay,
            )
        }
        Some(mapping::SourceSelectorRef::Legacy(path)) => {
            let budget = state
                .roots
                .budget
                .work(None, path, None, &state.frames, &state.origins)
                .map_err(access)?;
            let base = context
                .iter()
                .rev()
                .find(|frame| match path.first() {
                    Some(first) => frame.field(first).is_some(),
                    None => true,
                })
                .copied()
                .or_else(|| context.last().copied());
            let selected = if let Some(base) = base {
                let request = if let Some((owner, actual, canonical, tail)) =
                    state.legacy_origin(base, path)
                {
                    let consumed = path.len() - tail.len();
                    // Only the actually consumed extra-source wrapper segment enters the raw legacy path.
                    ObservedWalkRequest {
                        base: actual,
                        path: tail,
                        raw_prefix: path
                            .is_empty()
                            .then(|| {
                                FrameView::grouped_members_collection(context, positions, base)
                            })
                            .flatten()
                            .unwrap_or(&path[..consumed]),
                        canonical_prefix: canonical,
                        owner: Some(owner),
                        context: state,
                        position_offset: positions.len(),
                        strict_collection: false,
                    }
                } else {
                    ObservedWalkRequest {
                        base,
                        path,
                        raw_prefix: &[],
                        canonical_prefix: &[],
                        owner: None,
                        context: state,
                        position_offset: positions.len(),
                        strict_collection: false,
                    }
                };
                Some(
                    if program.trace_sink.is_some() {
                        select_observed_with_candidate_observer(
                            request,
                            &budget,
                            retain_first,
                            &mut observe_candidate,
                        )
                    } else {
                        select_observed(request, &budget, retain_first)
                    }
                    .map_err(access)?,
                )
            } else {
                None
            };
            match selected {
                Some(selected) => (
                    selected.extensions,
                    budget,
                    false,
                    selected.candidates,
                    selected.replay,
                ),
                None => (Vec::new(), budget, false, 0, None),
            }
        }
        None => {
            let budget = state
                .roots
                .budget
                .work(None, &[], None, &state.frames, &state.origins)
                .map_err(access)?;
            budget.emit().map_err(access)?;
            budget.reserve(1).map_err(access)?;
            reserve_observed_copy(&budget, &state.frames, &state.origins, &[], 0)
                .map_err(access)?;
            let mut extension = ObservedExtension {
                extension: WalkExtension {
                    instances: Vec::new(),
                    positions: Vec::new(),
                },
                frames: state.frames.clone(),
                origins: state.origins.clone(),
            };
            observe_candidate(&mut extension, 1);
            (vec![extension], budget, false, 1, None)
        }
    };
    eval_basic_owned_selected(
        receiving_program,
        scope,
        target,
        context,
        positions,
        extra_sources,
        source_loader,
        trace_scope,
        BasicSelection {
            extensions,
            budget,
            own,
            candidates,
            replay,
            retain_first,
        },
    )
}

/// One owned scope's complete selected candidates before controls run.
pub(super) struct BasicSelection<'a, 'query> {
    pub(super) extensions: Vec<crate::source_iteration::ObservedExtension<'a>>,
    pub(super) budget: crate::owned_context::WorkBudget,
    /// The scope's own selector names an owner.
    pub(super) own: bool,
    pub(super) candidates: usize,
    pub(super) replay: Option<crate::source_iteration::ObservedReplay<'a, 'query>>,
    /// Only the original walk's implicit First retains one candidate.
    pub(super) retain_first: bool,
}

/// Runs the controls, windows, grouping and output of selected candidates in
/// their receiving context.
#[allow(clippy::too_many_arguments)]
pub(super) fn eval_basic_owned_selected<'a>(
    receiving_program: EvalProgram<'a>,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&'a Instance],
    positions: &[PositionFrame],
    extra_sources: &[NamedSource],
    source_loader: Option<&dyn DynamicSourceLoader>,
    trace_scope: &TraceScope,
    selected: BasicSelection<'a, '_>,
) -> Result<Instance, EngineError> {
    use crate::owned_context::OwnedContext;
    use crate::source_iteration::reserve_observed_copy;
    let BasicSelection {
        mut extensions,
        budget,
        own,
        candidates,
        replay,
        retain_first,
    } = selected;
    let program = receiving_program.with_owned_trace_phase(crate::TraceOwnedPhase::Structural);
    let state = program
        .owned_context
        .expect("basic owned route has a context");
    let access = |source| owned_access_error(trace_scope, source);
    let ancestry = state.owned_ancestry
        || own
        || scope.group_by.is_some()
        || scope.group_adjacent_by.is_some()
        || scope.group_starting_with.is_some()
        || scope.group_ending_with.is_some()
        || scope.group_into_blocks.is_some();
    let filter_before_sort = scope.filter.is_some()
        && scope.has_sort()
        && scope.sort_filter_order == SortFilterOrder::FilterThenSort;
    let selection_phase = if scope.has_sort() {
        TraceFilterPhase::AfterSort
    } else {
        TraceFilterPhase::Selection
    };
    if filter_before_sort {
        budget.reserve(extensions.len()).map_err(access)?;
        let mut filtered = Vec::new();
        for extension in extensions {
            reserve_observed_copy(
                &budget,
                &extension.frames,
                &extension.origins,
                positions,
                context.len(),
            )
            .map_err(access)?;
            reserve_observed_copy(
                &budget,
                &[],
                &[],
                &extension.extension.positions,
                extension.extension.instances.len(),
            )
            .map_err(access)?;
            let item_state = OwnedContext {
                roots: state.roots,
                frames: extension.frames.clone(),
                origins: extension.origins.clone(),
                owned_ancestry: ancestry,
            };
            let mut item_context = context.to_vec();
            item_context.extend(extension.extension.instances.iter().copied());
            let mut item_positions = positions.to_vec();
            item_positions.extend(extension.extension.positions.iter().cloned());
            if passes_scope_filter(
                program.with_owned_context(&item_state),
                scope.filter,
                TraceFilterPhase::BeforeSort,
                trace_scope,
                &item_context,
                &item_positions,
            )? {
                filtered.push(extension);
            }
        }
        extensions = filtered;
    }
    budget.reserve(scope.sort_keys().count()).map_err(access)?;
    let sort_keys = scope.sort_keys().collect::<Vec<_>>();
    if !sort_keys.is_empty() {
        budget.reserve(extensions.len()).map_err(access)?;
        let mut keyed = Vec::new();
        for extension in extensions {
            reserve_observed_copy(
                &budget,
                &extension.frames,
                &extension.origins,
                positions,
                context.len(),
            )
            .map_err(access)?;
            reserve_observed_copy(
                &budget,
                &[],
                &[],
                &extension.extension.positions,
                extension.extension.instances.len(),
            )
            .map_err(access)?;
            budget.reserve(sort_keys.len()).map_err(access)?;
            let item_state = OwnedContext {
                roots: state.roots,
                frames: extension.frames.clone(),
                origins: extension.origins.clone(),
                owned_ancestry: ancestry,
            };
            let mut item_context = context.to_vec();
            item_context.extend(extension.extension.instances.iter().copied());
            let mut item_positions = positions.to_vec();
            item_positions.extend(extension.extension.positions.iter().cloned());
            let values = sort_keys
                .iter()
                .map(|key| {
                    eval_expr(
                        program.with_owned_context(&item_state),
                        key.node,
                        &item_context,
                        &item_positions,
                        &mut HashSet::new(),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?;
            record_owned_event(
                program.with_owned_context(&item_state),
                &item_positions,
                || TraceEvent::SortCandidate {
                    scope: trace_scope.clone(),
                    positions: trace_positions(&item_positions),
                    keys: sort_keys
                        .iter()
                        .zip(&values)
                        .map(|(key, value)| TraceSortKey {
                            node: key.node,
                            descending: key.descending,
                            value: TraceValue::new(value),
                        })
                        .collect(),
                },
            );
            keyed.push((extension, values));
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
        budget.reserve(keyed.len()).map_err(access)?;
        extensions = keyed
            .into_iter()
            .enumerate()
            .map(|(index, (mut extension, _))| {
                record_basic_sort_position(
                    program,
                    state,
                    positions,
                    trace_scope,
                    &mut extension,
                    index + 1,
                    own,
                );
                renumber_extension(&mut extension.extension.positions, index + 1);
                extension
            })
            .collect();
    }
    budget
        .reserve(
            scope
                .windows
                .len()
                .checked_add(1)
                .ok_or_else(|| access(budget.slot_overflow()))?,
        )
        .map_err(access)?;
    let mut windows = scope
        .windows
        .iter()
        .copied()
        .map(|window| eval_sequence_window(program, window, context, positions))
        .collect::<Result<Vec<_>, _>>()?;
    if scope.iteration_output == IterationOutput::First {
        windows.push(EvaluatedWindow::First(1));
    }
    let grouping = scope
        .group_by
        .map(BasicGroupPartition::By)
        .or_else(|| scope.group_adjacent_by.map(BasicGroupPartition::AdjacentBy))
        .or_else(|| {
            scope
                .group_starting_with
                .map(BasicGroupPartition::StartingWith)
        })
        .or_else(|| scope.group_ending_with.map(BasicGroupPartition::EndingWith));
    // Like legacy fixed blocks, the size is evaluated once in the receiving
    // parent after window bounds and before any candidate control, even for an
    // empty cohort.
    let grouping = match (grouping, scope.group_into_blocks) {
        (None, Some(node)) => Some(BasicGroupPartition::IntoBlocks {
            node,
            size: eval_block_size(program, node, context, positions)?,
        }),
        (grouping, _) => grouping,
    };
    if let Some(partition) = grouping {
        let produced = eval_basic_owned_groups(
            program,
            scope,
            target,
            context,
            positions,
            extra_sources,
            source_loader,
            trace_scope,
            &extensions,
            &budget,
            &windows,
            partition,
            ancestry,
            filter_before_sort,
            selection_phase,
        )?;
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
        return Ok(result);
    }
    if retain_first {
        budget.reserve(candidates).map_err(access)?;
        if let Some(replay) = replay {
            replay
                .request
                .visit(&replay.budget, &mut |extension| {
                    reserve_observed_copy(
                        &budget,
                        &extension.frames,
                        &extension.origins,
                        positions,
                        context.len(),
                    )?;
                    reserve_observed_copy(
                        &budget,
                        &[],
                        &[],
                        &extension.extension.positions,
                        extension.extension.instances.len(),
                    )
                })
                .map_err(access)?;
        }
        // One implicit First(1) window; it originally reserved every candidate.
        budget.reserve(candidates).map_err(access)?;
    } else {
        if !windows.is_empty() && !filter_before_sort {
            budget.reserve(extensions.len()).map_err(access)?;
            let mut filtered = Vec::new();
            for extension in extensions {
                reserve_observed_copy(
                    &budget,
                    &extension.frames,
                    &extension.origins,
                    positions,
                    context.len(),
                )
                .map_err(access)?;
                reserve_observed_copy(
                    &budget,
                    &[],
                    &[],
                    &extension.extension.positions,
                    extension.extension.instances.len(),
                )
                .map_err(access)?;
                let item_state = OwnedContext {
                    roots: state.roots,
                    frames: extension.frames.clone(),
                    origins: extension.origins.clone(),
                    owned_ancestry: ancestry,
                };
                let mut item_context = context.to_vec();
                item_context.extend(extension.extension.instances.iter().copied());
                let mut item_positions = positions.to_vec();
                item_positions.extend(extension.extension.positions.iter().cloned());
                if passes_scope_filter(
                    program.with_owned_context(&item_state),
                    scope.filter,
                    selection_phase,
                    trace_scope,
                    &item_context,
                    &item_positions,
                )? {
                    filtered.push(extension);
                }
            }
            extensions = filtered;
        }
        for _ in &windows {
            budget.reserve(extensions.len()).map_err(access)?;
        }
    }
    extensions = apply_basic_owned_sequence_windows(
        extensions,
        &windows,
        receiving_program,
        positions,
        trace_scope,
        retain_first.then_some(candidates),
    );
    budget.reserve(extensions.len()).map_err(access)?;
    let mut produced = Vec::new();
    let renumber_output = scope.filter.is_some() || scope.has_sort() || !windows.is_empty();
    let mut compact = 0usize;
    for extension in &extensions {
        reserve_observed_copy(
            &budget,
            &extension.frames,
            &extension.origins,
            positions,
            context.len(),
        )
        .map_err(access)?;
        reserve_observed_copy(
            &budget,
            &[],
            &[],
            &extension.extension.positions,
            extension.extension.instances.len(),
        )
        .map_err(access)?;
        let item_state = OwnedContext {
            roots: state.roots,
            frames: extension.frames.clone(),
            origins: extension.origins.clone(),
            owned_ancestry: ancestry,
        };
        let mut item_context = context.to_vec();
        item_context.extend(extension.extension.instances.iter().copied());
        let mut candidate_positions = positions.to_vec();
        candidate_positions.extend(extension.extension.positions.iter().cloned());
        reserve_observed_copy(&budget, &[], &[], &candidate_positions, 0).map_err(access)?;
        let mut output_positions = candidate_positions.clone();
        let next = compact
            .checked_add(1)
            .ok_or_else(|| access(budget.slot_overflow()))?;
        // An identity walk retains every enclosing occurrence and its phase position.
        if renumber_output && !extension.extension.positions.is_empty() {
            renumber_extension(&mut output_positions, next);
        }
        let evaluator = ItemEvaluator {
            program: program
                .with_owned_context(&item_state)
                .with_owned_trace_phase(crate::TraceOwnedPhase::Output),
            scope,
            trace_scope,
            target,
            extra_sources,
            source_loader,
        };
        if let Some(item) = evaluator.produce(
            &item_context,
            &candidate_positions,
            &output_positions,
            !filter_before_sort && windows.is_empty(),
        )? {
            if !extension.extension.positions.is_empty() {
                compact = next;
            }
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
