//! Scope orchestration and ordinary borrowed-context evaluation.
//!
//! The owned pipeline stays separate: selection and grouping retain its exact
//! frame/provenance ledger. Item construction and window operations expose only
//! private boundaries and never extend the public engine API.

use std::collections::{BTreeMap, HashSet};

use ir::{DocumentMember, Instance, Value};
use mapping::{IterationOutput, NamedSource, NodeId, Scope, ScopeConstruction, SortFilterOrder};

use crate::aggregate::sort_value_ordering;
use crate::eval_expr::{EvalProgram, eval_expr};
use crate::frame_view::FrameView;
use crate::grouping::GroupingMode;
use crate::iteration_output::finalize_scope_output;
use crate::join::{execute as execute_join, extensions as join_extensions};
use crate::sequence::eval_sequence;
use crate::source_iteration::{PositionFrame, WalkExtension, walk};
use crate::trace::{
    TraceEvent, TraceFilterPhase, TraceGrouping, TraceOutputKind, TraceScope, TraceSortKey,
    TraceSourceRow, TraceValue, record, record_owned_event, scope_iteration, trace_positions,
};
use crate::{DynamicSourceLoader, EngineError};

#[path = "eval_scope/construction.rs"]
mod construction;
mod document_collection;
#[path = "eval_scope/owned_document_set.rs"]
mod owned_document_set;
#[path = "eval_scope/owned_dynamic_loading.rs"]
mod owned_dynamic_loading;
#[path = "eval_scope/owned_grouping.rs"]
mod owned_grouping;
#[path = "eval_scope/owned_join_scope.rs"]
mod owned_join_scope;
#[path = "eval_scope/owned_selection.rs"]
mod owned_selection;
#[path = "eval_scope/windows.rs"]
mod windows;

use construction::ItemEvaluator;
use owned_selection::eval_basic_owned_scope;
use windows::{EvaluatedWindow, apply_sequence_windows, eval_block_size, eval_sequence_window};

// Only the synthetic member collection is owned. Intermediate ancestors are
// borrowed from the first member's candidate, so a group adds no copy of the
// repeated levels crossed above its members.
struct GroupBucket<'e, 'a> {
    key: Option<Value>,
    members: Vec<Instance>,
    intermediate_frames: &'e [&'a Instance],
    positions: Vec<PositionFrame>,
    post_filter_match: bool,
}

struct OwnedGroup<'e, 'a> {
    // Named groups own their member list inside a one-field wrapper;
    // anonymous groups own the repeated member frame directly.
    frame: Instance,
    intermediate_frames: &'e [&'a Instance],
    positions: Vec<PositionFrame>,
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn eval_scope(
    program: EvalProgram<'_>,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&Instance],
    positions: &[PositionFrame],
    extra_sources: &[NamedSource],
    source_loader: Option<&dyn DynamicSourceLoader>,
    trace_scope: &TraceScope,
) -> Result<Instance, EngineError> {
    eval_scope_with_scalar_output(
        program,
        scope,
        target,
        context,
        positions,
        extra_sources,
        source_loader,
        trace_scope,
        None,
    )
}

/// The target dispatcher alone supplies this validated, non-inherited policy.
#[allow(clippy::too_many_arguments)]
pub(crate) fn eval_scope_with_scalar_output(
    program: EvalProgram<'_>,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&Instance],
    positions: &[PositionFrame],
    extra_sources: &[NamedSource],
    source_loader: Option<&dyn DynamicSourceLoader>,
    trace_scope: &TraceScope,
    preserve_scalars: Option<ir::XmlScalarPreservation<'_>>,
) -> Result<Instance, EngineError> {
    if let Some(owned) = program.owned_context {
        if owned.owned_ancestry || scope_needs_owned(program, scope) {
            return eval_basic_owned_scope(
                program,
                scope,
                target,
                context,
                positions,
                extra_sources,
                source_loader,
                trace_scope,
            );
        }
        // An unrelated legacy subtree keeps its complete original evaluator.
        return eval_scope(
            program.without_owned_context(),
            scope,
            target,
            context,
            positions,
            extra_sources,
            source_loader,
            trace_scope,
        );
    }
    if scope.iteration.owned_join().is_some()
        || matches!(
            scope.source_selector(),
            Some(mapping::SourceSelectorRef::Owned(_))
        )
        || matches!(
            scope.construction,
            ScopeConstruction::OwnedPathHierarchy { .. }
                | ScopeConstruction::OwnedAdjacencyTree { .. }
        )
    {
        return Err(EngineError::UnsupportedSourceIteration {
            scope: trace_scope.source_scope.clone(),
        });
    }
    if matches!(scope.construction, ScopeConstruction::CopyCurrentRows)
        && !matches!(context.last().copied(), Some(Instance::Repeated(rows)) if rows.iter().all(ir::is_flat_row))
    {
        return Err(EngineError::InvalidCurrentRows);
    }
    record(program.trace_sink, || TraceEvent::ScopeStarted {
        scope: trace_scope.clone(),
        iteration: scope_iteration(&scope.iteration).expect("owned iteration refused before trace"),
        positions: trace_positions(positions),
    });
    if let Some(segments) = scope.concatenated() {
        let mut output = Vec::new();
        for (index, segment) in segments.iter().enumerate() {
            match eval_scope(
                program,
                segment,
                target,
                context,
                positions,
                extra_sources,
                source_loader,
                &trace_scope.segment(index),
            )? {
                item @ Instance::Group(_) => output.push(item),
                Instance::Repeated(items) | Instance::MappedSequence(items) => output.extend(items),
                Instance::Scalar(_) => {
                    return Err(EngineError::InvalidConcatenatedScopeItem { found: "a scalar" });
                }
                Instance::DocumentSet(_) => {
                    return Err(EngineError::InvalidConcatenatedScopeItem {
                        found: "a document set",
                    });
                }
            }
        }
        let result = match scope.iteration_output {
            IterationOutput::Repeated => Instance::Repeated(output),
            IterationOutput::MappedSequence => Instance::MappedSequence(output),
            IterationOutput::First => {
                return Err(EngineError::InvalidConcatenatedScopeItem {
                    found: "a first-item wrapper",
                });
            }
        };
        record(program.trace_sink, || TraceEvent::ScopeFinished {
            scope: trace_scope.clone(),
            candidates: segments.len(),
            produced: output_len(&result),
            kind: TraceOutputKind::of(&result),
        });
        return Ok(result);
    }
    let sequence_items = scope
        .sequence()
        .map(|sequence| eval_sequence(program, sequence, context, positions))
        .transpose()?
        .map(|values| {
            Instance::Repeated(values.into_iter().map(Instance::Scalar).collect::<Vec<_>>())
        });
    let join_rows = scope
        .join()
        .map(|(join, plan)| {
            execute_join(context, positions, join, plan, program.budget, |key| {
                eval_expr(program, key, context, positions, &mut HashSet::new())
            })
        })
        .transpose()?;
    let dynamic_source = scope
        .source()
        .expect("owned iteration refused before evaluation")
        .and_then(|path| {
            let name = path.first()?;
            extra_sources.iter().find_map(|source| {
                (source.name == *name)
                    .then_some(source)
                    .filter(|source| source.dynamic_path.is_some())
            })
        });
    // An owned driver is reachable only through an owned selector; a legacy
    // alias never repairs its missing ownership.
    let legacy_driver = match dynamic_source.and_then(|source| source.dynamic_path.as_ref()) {
        Some(dynamic) => Some(dynamic.legacy_driver().ok_or_else(|| {
            EngineError::UnsupportedSourceIteration {
                scope: trace_scope.source_scope.clone(),
            }
        })?),
        None => None,
    };
    let dynamic_drivers = legacy_driver
        .map(|iteration| {
            context
                .iter()
                .rev()
                .find(|frame| match iteration.first() {
                    Some(first) => frame.field(first).is_some(),
                    None => true,
                })
                .copied()
                .or_else(|| context.last().copied())
                .map_or_else(Vec::new, |base| walk(base, iteration, &[], &[], &[]))
        })
        .unwrap_or_default();
    let mut loaded_dynamic = Vec::new();
    if let Some(source) = dynamic_source
        && let Some(dynamic) = &source.dynamic_path
    {
        let loader = source_loader.ok_or_else(|| EngineError::MissingDynamicSourceLoader {
            source_name: source.name.clone(),
        })?;
        for (driver_index, driver) in dynamic_drivers.iter().enumerate() {
            let mut item_context = context.to_vec();
            item_context.extend(driver.instances.iter().copied());
            let mut item_positions = positions.to_vec();
            item_positions.extend(driver.positions.iter().cloned());
            let mut in_progress = HashSet::new();
            let path = eval_expr(
                program,
                dynamic.node,
                &item_context,
                &item_positions,
                &mut in_progress,
            )?;
            let Value::String(path) = path else {
                if matches!(path, Value::Null | Value::JsonNull(_)) {
                    continue;
                }
                return Err(EngineError::DynamicSourcePath {
                    source_name: source.name.clone(),
                    found: path.type_name(),
                });
            };
            let instance = crate::dynamic_source::load_document(
                loader,
                program.restricted,
                &source.name,
                path,
            )?;
            loaded_dynamic.push((driver_index, instance));
        }
    }
    let mut extensions = if let Some(rows) = &join_rows {
        join_extensions(rows)
    } else if let Some(items) = &sequence_items {
        let mut generated = walk(items, &[], &[], &[], &[]);
        if let Some(sequence) = scope.sequence() {
            for extension in &mut generated {
                if let Some(frame) = extension.positions.last_mut() {
                    frame.generated_origin = Some((sequence.item(), frame.index));
                }
            }
        }
        generated
    } else if let Some(source) = dynamic_source {
        let path = scope
            .source()
            .expect("owned iteration refused before evaluation")
            .unwrap_or_default();
        let tail = path.get(1..).unwrap_or_default();
        let prefix = [source.name.clone()];
        loaded_dynamic
            .iter()
            .flat_map(|(driver_index, instance)| {
                let driver = &dynamic_drivers[*driver_index];
                walk(instance, tail, &prefix, &[], &[])
                    .into_iter()
                    .map(|loaded| {
                        let mut instances = driver.instances.clone();
                        instances.extend(loaded.instances);
                        let mut positions = driver.positions.clone();
                        positions.extend(loaded.positions);
                        WalkExtension {
                            instances,
                            positions,
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    } else if let Some(condition) = scope.presence() {
        // The parent item is the only candidate; no frame or position is added.
        if passes_filter(program, Some(condition), context, positions)? {
            vec![WalkExtension {
                instances: Vec::new(),
                positions: Vec::new(),
            }]
        } else {
            Vec::new()
        }
    } else {
        match scope
            .source()
            .expect("owned iteration refused before evaluation")
        {
            None => vec![WalkExtension {
                instances: Vec::new(),
                positions: Vec::new(),
            }],
            // Use the innermost frame with the path's first field, while
            // allowing nested scopes to iterate an extra source by name.
            Some(path) => context
                .iter()
                .rev()
                .find(|frame| match path.first() {
                    Some(first) => frame.field(first).is_some(),
                    None => true,
                })
                .copied()
                .or_else(|| context.last().copied())
                .map_or_else(Vec::new, |base| {
                    // A grouped scope stores its member collection in the
                    // context under the original collection frame. Preserve
                    // that identity when an empty-path child iterates members.
                    let prefix = path
                        .is_empty()
                        .then(|| FrameView::grouped_members_collection(context, positions, base))
                        .flatten()
                        .unwrap_or_default();
                    walk(base, path, prefix, &[], &[])
                }),
        }
    };
    let candidate_count = extensions.len();
    for (ordinal, extension) in extensions.iter().enumerate() {
        let mut candidate_positions = positions.to_vec();
        candidate_positions.extend(extension.positions.iter().cloned());
        let wants_source_row = scope
            .source()
            .expect("owned iteration refused before evaluation")
            .is_some()
            && program
                .trace_sink
                .is_some_and(|sink| sink.wants_source_row());
        record(program.trace_sink, || TraceEvent::IterationCandidate {
            scope: trace_scope.clone(),
            ordinal: ordinal + 1,
            positions: trace_positions(&candidate_positions),
            source_row: wants_source_row
                .then(|| extension.instances.last())
                .flatten()
                .map(|instance| TraceSourceRow::new(instance)),
        });
    }

    let filter_before_sort = scope.filter.is_some()
        && scope.has_sort()
        && scope.sort_filter_order == SortFilterOrder::FilterThenSort;
    let selection_filter_phase = if scope.has_sort() {
        TraceFilterPhase::AfterSort
    } else {
        TraceFilterPhase::Selection
    };
    if filter_before_sort {
        let mut filtered = Vec::with_capacity(extensions.len());
        for extension in extensions {
            let mut item_context = context.to_vec();
            item_context.extend(extension.instances.iter().copied());
            let mut item_positions = positions.to_vec();
            item_positions.extend(extension.positions.iter().cloned());
            if passes_scope_filter(
                program,
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

    let sort_keys = scope.sort_keys().collect::<Vec<_>>();
    if !sort_keys.is_empty() {
        let mut keyed = Vec::with_capacity(extensions.len());
        for extension in extensions {
            let mut item_context = context.to_vec();
            item_context.extend(extension.instances.iter().copied());
            let mut item_positions = positions.to_vec();
            item_positions.extend(extension.positions.iter().cloned());
            let mut values = Vec::with_capacity(sort_keys.len());
            for key in &sort_keys {
                let mut in_progress = HashSet::new();
                values.push(eval_expr(
                    program,
                    key.node,
                    &item_context,
                    &item_positions,
                    &mut in_progress,
                )?);
            }
            record(program.trace_sink, || TraceEvent::SortCandidate {
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
            });
            keyed.push((extension, values));
        }
        keyed.sort_by(|(_, left), (_, right)| {
            for ((left, right), key) in left.iter().zip(right).zip(&sort_keys) {
                let ordering = sort_value_ordering(left, right);
                let ordering = if key.descending {
                    ordering.reverse()
                } else {
                    ordering
                };
                if ordering != std::cmp::Ordering::Equal {
                    return ordering;
                }
            }
            std::cmp::Ordering::Equal
        });
        extensions = keyed
            .into_iter()
            .enumerate()
            .map(|(index, (mut extension, _))| {
                let mut input_positions = positions.to_vec();
                input_positions.extend(extension.positions.iter().cloned());
                renumber_extension(&mut extension.positions, index + 1);
                record(program.trace_sink, || TraceEvent::SortPosition {
                    scope: trace_scope.clone(),
                    positions: trace_positions(&input_positions),
                    output_index: index + 1,
                });
                extension
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
    if scope.join().is_some() && scope.has_grouping() {
        return Err(EngineError::JoinGroupingUnsupported);
    }
    if scope.has_conflicting_grouping() {
        return Err(EngineError::ConflictingGroupingModes);
    }
    let (grouping, trace_grouping) = if let Some(node) = scope.group_by {
        (
            Some(GroupingMode::By(node)),
            Some(TraceGrouping::By { node }),
        )
    } else if let Some(node) = scope.group_adjacent_by {
        (
            Some(GroupingMode::AdjacentBy(node)),
            Some(TraceGrouping::AdjacentBy { node }),
        )
    } else if let Some(node) = scope.group_starting_with {
        (
            Some(GroupingMode::StartingWith(node)),
            Some(TraceGrouping::StartingWith { node }),
        )
    } else if let Some(node) = scope.group_ending_with {
        (
            Some(GroupingMode::EndingWith(node)),
            Some(TraceGrouping::EndingWith { node }),
        )
    } else if let Some(node) = scope.group_into_blocks {
        let size = eval_block_size(program, node, context, positions)?;
        (
            Some(GroupingMode::IntoBlocks(size)),
            Some(TraceGrouping::IntoBlocks { node, size }),
        )
    } else {
        (None, None)
    };
    let mut produced = Vec::new();
    let item_evaluator = ItemEvaluator {
        preserve_scalars,
        program,
        scope,
        trace_scope,
        target,
        extra_sources,
        source_loader,
    };
    if let Some(grouping) = grouping {
        let mut groups: Vec<GroupBucket> = Vec::new();
        let mut ending_group_closed = true;
        for extension in &extensions {
            let mut item_context = context.to_vec();
            item_context.extend(extension.instances.iter().copied());
            let mut item_positions = positions.to_vec();
            item_positions.extend(extension.positions.iter().cloned());
            if !filter_before_sort
                && !passes_scope_filter(
                    program,
                    scope.filter,
                    selection_filter_phase,
                    trace_scope,
                    &item_context,
                    &item_positions,
                )?
            {
                continue;
            }
            let member = (*extension
                .instances
                .last()
                .expect("extensions are never empty"))
            .clone();
            let key = match grouping {
                GroupingMode::By(key_node) | GroupingMode::AdjacentBy(key_node) => {
                    let mut in_progress = HashSet::new();
                    Some(eval_expr(
                        program,
                        key_node,
                        &item_context,
                        &item_positions,
                        &mut in_progress,
                    )?)
                }
                GroupingMode::StartingWith(_)
                | GroupingMode::EndingWith(_)
                | GroupingMode::IntoBlocks(_) => None,
            };
            let starts_group = match grouping {
                GroupingMode::StartingWith(predicate) => passes_scope_filter(
                    program,
                    Some(predicate),
                    TraceFilterPhase::GroupStarting,
                    trace_scope,
                    &item_context,
                    &item_positions,
                )?,
                GroupingMode::By(_)
                | GroupingMode::AdjacentBy(_)
                | GroupingMode::EndingWith(_)
                | GroupingMode::IntoBlocks(_) => false,
            };
            let ends_group = match grouping {
                GroupingMode::EndingWith(predicate) => passes_scope_filter(
                    program,
                    Some(predicate),
                    TraceFilterPhase::GroupEnding,
                    trace_scope,
                    &item_context,
                    &item_positions,
                )?,
                GroupingMode::By(_)
                | GroupingMode::AdjacentBy(_)
                | GroupingMode::StartingWith(_)
                | GroupingMode::IntoBlocks(_) => false,
            };
            let post_filter_match = passes_scope_filter(
                program,
                scope.post_group_filter,
                TraceFilterPhase::PostGroupMember,
                trace_scope,
                &item_context,
                &item_positions,
            )?;
            let existing = match grouping {
                GroupingMode::By(_) => groups.iter_mut().find(|group| group.key == key),
                GroupingMode::AdjacentBy(_) => groups.last_mut().filter(|group| group.key == key),
                GroupingMode::StartingWith(_) => {
                    if starts_group {
                        None
                    } else {
                        groups.last_mut()
                    }
                }
                GroupingMode::EndingWith(_) => {
                    if ending_group_closed {
                        None
                    } else {
                        groups.last_mut()
                    }
                }
                GroupingMode::IntoBlocks(size) => {
                    groups.last_mut().filter(|group| group.members.len() < size)
                }
            };
            match existing {
                Some(group) => {
                    group.members.push(member);
                    group.post_filter_match |= post_filter_match;
                }
                None => groups.push(GroupBucket {
                    key,
                    members: vec![member],
                    intermediate_frames: &extension.instances[..extension.instances.len() - 1],
                    positions: item_positions,
                    post_filter_match,
                }),
            }
            ending_group_closed = ends_group;
        }
        if let Some(trace_grouping) = trace_grouping {
            for (index, group) in groups.iter().enumerate() {
                record(program.trace_sink, || TraceEvent::GroupProduced {
                    scope: trace_scope.clone(),
                    grouping: trace_grouping,
                    group_index: index + 1,
                    member_count: group.members.len(),
                    key: group.key.as_ref().map(TraceValue::new),
                    retained: group.post_filter_match,
                    positions: trace_positions(&group.positions),
                });
            }
        }
        // Position frames stay in order, with the named collection wrapper
        // immediately before the grouped members.
        let owned: Vec<OwnedGroup> = groups
            .into_iter()
            .filter(|group| group.post_filter_match)
            .map(|group| {
                let members = Instance::Repeated(group.members);
                let frame = match scope
                    .source()
                    .expect("owned iteration refused before evaluation")
                    .and_then(|path| path.last())
                {
                    Some(segment) => Instance::Group((vec![(segment.clone(), members)]).into()),
                    None => members,
                };
                OwnedGroup {
                    frame,
                    intermediate_frames: group.intermediate_frames,
                    positions: group.positions,
                }
            })
            .collect();
        let owned = apply_sequence_windows(owned, &windows, program.trace_sink, trace_scope);
        produced.reserve(owned.len());
        for group in &owned {
            let (wrapper, members) = match &group.frame {
                // Only the constructor above creates a group frame, with one field.
                Instance::Group(fields) => (Some(&group.frame), &fields[0].1),
                members => (None, members),
            };
            assert!(
                FrameView::receiving_start(context, positions).is_some(),
                "iteration positions have matching context frames"
            );
            let mut next_context = context.to_vec();
            next_context.extend_from_slice(group.intermediate_frames);
            if let Some(wrapper) = wrapper {
                next_context.push(wrapper);
            }
            next_context.push(members);
            let mut output_positions = group.positions.clone();
            if let Some(position) = output_positions.last_mut() {
                position.index = produced.len() + 1;
                position.grouped = wrapper.is_some();
                // A group has no single original generated item. Its members
                // retain their own origins when traversed separately.
                if scope.sequence().is_some_and(|sequence| {
                    position
                        .generated_origin
                        .is_some_and(|(item, _)| item == sequence.item())
                }) {
                    position.generated_origin = None;
                }
            }
            if let Some(instance) =
                item_evaluator.produce(&next_context, &group.positions, &output_positions, false)?
            {
                produced.push(instance);
            }
        }
    } else {
        if !windows.is_empty() && !filter_before_sort {
            let mut filtered = Vec::with_capacity(extensions.len());
            for extension in extensions {
                let mut item_context = context.to_vec();
                item_context.extend(extension.instances.iter().copied());
                let mut item_positions = positions.to_vec();
                item_positions.extend(extension.positions.iter().cloned());
                if passes_scope_filter(
                    program,
                    scope.filter,
                    selection_filter_phase,
                    trace_scope,
                    &item_context,
                    &item_positions,
                )? {
                    filtered.push(extension);
                }
            }
            extensions = filtered;
        }
        extensions = apply_sequence_windows(extensions, &windows, program.trace_sink, trace_scope);
        produced.reserve(extensions.len());
        let mut compact_positions: BTreeMap<Vec<usize>, usize> = BTreeMap::new();
        let renumber_output = scope.filter.is_some() || scope.has_sort() || !windows.is_empty();
        for extension in &extensions {
            let mut next_context = context.to_vec();
            next_context.extend(extension.instances.iter().copied());
            let mut candidate_positions = positions.to_vec();
            candidate_positions.extend(extension.positions.iter().cloned());

            // Intermediate repeating levels crossed by this scope belong to
            // one flattened candidate sequence. Only already-active outer
            // scopes identify distinct target parents for compact positions.
            let parent_key: Vec<usize> = positions.iter().map(|position| position.index).collect();
            let joined = extension
                .positions
                .last()
                .is_some_and(|position| position.join_position.is_some());
            let next_position = if joined {
                produced.len() + 1
            } else if !renumber_output {
                extension
                    .positions
                    .last()
                    .map_or(1, |position| position.index)
            } else {
                compact_positions.get(&parent_key).copied().unwrap_or(0) + 1
            };
            let mut output_positions = candidate_positions.clone();
            if joined || renumber_output {
                renumber_extension(&mut output_positions, next_position);
            }
            if let Some(instance) = item_evaluator.produce(
                &next_context,
                &candidate_positions,
                &output_positions,
                !filter_before_sort && windows.is_empty(),
            )? {
                if !joined && renumber_output && !extension.positions.is_empty() {
                    compact_positions.insert(parent_key, next_position);
                }
                produced.push(instance);
            }
        }
    }

    let produced_count = produced.len();
    let result = if let Some(node) = scope.output_path() {
        let documents = produced
            .into_iter()
            .map(|produced| {
                let path = produced
                    .output_path
                    .ok_or(EngineError::EmptyDynamicTargetPath { node })?;
                DocumentMember::new(path, produced.instance)
                    .ok_or(EngineError::EmptyDynamicTargetPath { node })
            })
            .collect::<Result<Vec<_>, _>>()?;
        Instance::DocumentSet(documents)
    } else if matches!(scope.construction, ScopeConstruction::CollectDocuments) {
        let mut documents = Vec::new();
        for item in produced {
            document_collection::extend(&mut documents, item.instance)?;
        }
        Instance::DocumentSet(documents)
    } else {
        finalize_scope_output(
            scope,
            target.is_some_and(|target| target.repeating),
            produced
                .into_iter()
                .map(|produced| produced.instance)
                .collect(),
        )?
    };
    record(program.trace_sink, || TraceEvent::ScopeFinished {
        scope: trace_scope.clone(),
        candidates: candidate_count,
        produced: produced_count,
        kind: TraceOutputKind::of(&result),
    });
    Ok(result)
}

fn scope_needs_owned(program: EvalProgram<'_>, scope: &Scope) -> bool {
    if scope.iteration.owned_join().is_some()
        || matches!(
            scope.source_selector(),
            Some(mapping::SourceSelectorRef::Owned(_))
        )
        || matches!(
            scope.construction,
            ScopeConstruction::OwnedPathHierarchy { .. }
                | ScopeConstruction::OwnedAdjacencyTree { .. }
        )
    {
        return true;
    }
    let mut pending = crate::scope_slots::roots(scope, crate::scope_slots::owned_probe);
    let mut visited = HashSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        if let Some(node) = program.graph.nodes.get(&id) {
            if node.owned_source_kind().is_some() {
                return true;
            }
            pending.extend(node.dependencies());
        }
    }
    scope
        .children
        .iter()
        .any(|child| scope_needs_owned(program, child))
}

/// Only the implicit First window may use bounded candidate retention.
/// Explicit sequence controls keep their complete original evaluator.
pub(crate) fn retain_first_source_candidate(scope: &Scope) -> bool {
    scope.iteration_output == IterationOutput::First
        && matches!(
            &scope.iteration,
            mapping::ScopeIteration::Source(_) | mapping::ScopeIteration::OwnedSource { .. }
        )
        && matches!(&scope.construction, ScopeConstruction::Constructed)
        && scope.filter.is_none()
        && !scope.has_sort()
        && scope.windows.is_empty()
        && !scope.has_grouping()
        && scope.post_group_filter.is_none()
        && scope.output_path().is_none()
        && !scope.merge_dynamic_fields
        && scope.dynamic_bindings.is_empty()
        && scope.dynamic_children.is_empty()
}

/// Maps an owned scope's metadata failure to its scope-located engine error.
pub(super) fn owned_access_error(
    trace_scope: &TraceScope,
    source: crate::OwnedSourceError,
) -> EngineError {
    match source {
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
    }
}

fn renumber_extension(positions: &mut [PositionFrame], index: usize) {
    let Some(position) = positions.last_mut() else {
        return;
    };
    if let Some((_, join_index)) = &mut position.join_position {
        *join_index = index;
    } else {
        position.index = index;
    }
}

fn passes_filter(
    program: EvalProgram<'_>,
    filter: Option<NodeId>,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<bool, EngineError> {
    let Some(filter_node) = filter else {
        return Ok(true);
    };
    let mut in_progress = HashSet::new();
    match eval_expr(program, filter_node, context, positions, &mut in_progress)? {
        Value::Bool(value) => Ok(value),
        other => Err(EngineError::NotABool {
            node: filter_node,
            found: other.type_name(),
        }),
    }
}

fn passes_scope_filter(
    program: EvalProgram<'_>,
    filter: Option<NodeId>,
    phase: TraceFilterPhase,
    trace_scope: &TraceScope,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<bool, EngineError> {
    let Some(node) = filter else {
        return Ok(true);
    };
    let program = program.with_owned_trace_phase(match phase {
        TraceFilterPhase::BeforeSort => crate::TraceOwnedPhase::Structural,
        _ => crate::TraceOwnedPhase::Selection,
    });
    let passed = passes_filter(program, Some(node), context, positions)?;
    record_owned_event(program, positions, || TraceEvent::FilterDecision {
        scope: trace_scope.clone(),
        node,
        phase,
        positions: trace_positions(positions),
        passed,
    });
    Ok(passed)
}

fn output_len(instance: &Instance) -> usize {
    match instance {
        Instance::Repeated(items) | Instance::MappedSequence(items) => items.len(),
        Instance::DocumentSet(documents) => documents.len(),
        Instance::Scalar(_) | Instance::Group(_) => 1,
    }
}
