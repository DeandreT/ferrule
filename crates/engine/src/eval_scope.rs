use std::collections::{BTreeMap, HashSet};

use ir::{
    DocumentMember, Instance, ScalarType, SchemaKind, Value, XML_MIXED_CONTENT_FIELD,
    XML_MIXED_CONTENT_VALUE_FIELD, XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
};
use mapping::{
    IterationOutput, NamedSource, NodeId, Scope, ScopeConstruction, SequenceWindow,
    SortFilterOrder, XmlMixedContentElement,
};

use crate::aggregate::sort_value_ordering;
use crate::debug::before_target_write;
use crate::dynamic_target::{self, eval_dynamic_key, insert_target_field};
use crate::eval_expr::{EvalProgram, eval_expr};
use crate::grouping::GroupingMode;
use crate::iteration_output::finalize_scope_output;
use crate::join::{execute as execute_join, extensions as join_extensions};
use crate::recursive_filter;
use crate::resolve::context_for_position;
use crate::sequence::eval_sequence;
use crate::source_iteration::{PositionFrame, WalkExtension, walk};
use crate::trace::{
    TraceEvent, TraceFilterPhase, TraceGrouping, TraceOutputKind, TraceScope, TraceSortKey,
    TraceSourceRow, TraceTargetFieldBinding, TraceValue, TraceWindow, basic_scope_iteration,
    bounded_text, record, record_owned_event, scope_iteration, trace_positions,
};
use crate::{DynamicSourceLoader, EngineError};

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

struct ProducedItem {
    instance: Instance,
    output_path: Option<String>,
}

struct ItemEvaluator<'a> {
    program: EvalProgram<'a>,
    scope: &'a Scope,
    trace_scope: &'a TraceScope,
    target: Option<&'a ir::SchemaNode>,
    extra_sources: &'a [NamedSource],
    source_loader: Option<&'a dyn DynamicSourceLoader>,
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
    if matches!(
        scope.source_selector(),
        Some(mapping::SourceSelectorRef::Owned(_))
    ) || matches!(
        scope.construction,
        ScopeConstruction::OwnedPathHierarchy { .. } | ScopeConstruction::OwnedAdjacencyTree { .. }
    ) {
        return Err(EngineError::UnsupportedSourceIteration {
            scope: trace_scope.source_scope.clone(),
        });
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
        .map(|(join, plan)| execute_join(context, positions, join, plan, program.budget))
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
    let dynamic_drivers = dynamic_source
        .and_then(|source| source.dynamic_path.as_ref())
        .map(|dynamic| {
            context
                .iter()
                .rev()
                .find(|frame| match dynamic.iteration.first() {
                    Some(first) => frame.field(first).is_some(),
                    None => true,
                })
                .copied()
                .or_else(|| context.last().copied())
                .map_or_else(Vec::new, |base| {
                    walk(base, &dynamic.iteration, &[], &[], &[])
                })
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
            let loaded = match program.restricted {
                None => loader
                    .load(&source.name, &path)
                    .map_err(crate::DynamicSourceLoadError::Failed),
                Some(policy) => loader.load_restricted(&source.name, &path, policy),
            };
            let instance = loaded.map_err(|error| match error {
                crate::DynamicSourceLoadError::Refused(refusal) => {
                    EngineError::DynamicSourceRefused {
                        source_name: source.name.clone(),
                        refusal,
                    }
                }
                crate::DynamicSourceLoadError::Failed(message) => EngineError::DynamicSourceLoad {
                    source_name: source.name.clone(),
                    path,
                    message,
                },
            })?;
            loaded_dynamic.push((driver_index, instance));
        }
    }
    let mut extensions = if let Some(rows) = &join_rows {
        join_extensions(rows)
    } else if let Some(items) = &sequence_items {
        walk(items, &[], &[], &[], &[])
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
                    let prefix = if path.is_empty()
                        && positions.last().is_some_and(|position| {
                            position.grouped
                                && context_for_position(
                                    context,
                                    positions,
                                    positions.len().saturating_sub(1),
                                ) == Some(base)
                        }) {
                        positions
                            .last()
                            .map(|position| position.collection.as_slice())
                            .unwrap_or_default()
                    } else {
                        &[]
                    };
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
            let parent_wrappers = positions.iter().filter(|position| position.grouped).count();
            let parent_frame_start = context
                .len()
                .checked_sub(positions.len() + parent_wrappers)
                .expect("iteration positions have matching context frames");
            let mut next_context = context[..parent_frame_start].to_vec();
            next_context.extend_from_slice(&context[parent_frame_start..]);
            next_context.extend_from_slice(group.intermediate_frames);
            if let Some(wrapper) = wrapper {
                next_context.push(wrapper);
            }
            next_context.push(members);
            let mut output_positions = group.positions.clone();
            if let Some(position) = output_positions.last_mut() {
                position.index = produced.len() + 1;
                position.grouped = wrapper.is_some();
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
    if matches!(
        scope.source_selector(),
        Some(mapping::SourceSelectorRef::Owned(_))
    ) || matches!(
        scope.construction,
        ScopeConstruction::OwnedPathHierarchy { .. } | ScopeConstruction::OwnedAdjacencyTree { .. }
    ) {
        return true;
    }
    let mut pending: Vec<_> = scope
        .bindings
        .iter()
        .map(|binding| binding.node)
        .chain(match &scope.construction {
            ScopeConstruction::Scalar { value } => Some(*value),
            _ => None,
        })
        .chain(scope.filter)
        .chain(scope.grouping_nodes())
        .chain(scope.post_group_filter)
        .chain(scope.sort_keys().map(|key| key.node))
        .chain(
            scope
                .windows
                .iter()
                .copied()
                .flat_map(|window| window.nodes()),
        )
        .collect();
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

#[allow(clippy::too_many_arguments)]
fn record_basic_candidate<'tree>(
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
fn eval_basic_owned_scope<'a>(
    program: EvalProgram<'a>,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&'a Instance],
    positions: &[PositionFrame],
    extra_sources: &[NamedSource],
    source_loader: Option<&dyn DynamicSourceLoader>,
    trace_scope: &TraceScope,
) -> Result<Instance, EngineError> {
    use crate::owned_context::OwnedContext;
    use crate::source_iteration::{
        ObservedExtension, ObservedWalkRequest, reserve_observed_copy, select_observed,
        select_observed_with_candidate_observer,
    };
    // Scope boundaries observe the incoming receiving context and phase.
    // Traversal and item producers retain their existing explicit phase programs.
    let receiving_program = program;
    let program = program.with_owned_trace_phase(crate::TraceOwnedPhase::Structural);
    let state = program
        .owned_context
        .expect("basic owned route has a context");
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
    let (mut extensions, budget, own, candidates, replay) = match scope.source_selector() {
        Some(mapping::SourceSelectorRef::Owned(source)) => {
            let (owner, base, prefix, tail, budget) = state.select(source).map_err(access)?;
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
                        raw_prefix: if path.is_empty()
                            && positions.last().is_some_and(|position| {
                                position.grouped
                                    && context_for_position(
                                        context,
                                        positions,
                                        positions.len().saturating_sub(1),
                                    ) == Some(base)
                            }) {
                            &positions
                                .last()
                                .expect("checked grouped position")
                                .collection
                        } else {
                            &path[..consumed]
                        },
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

/// A bucket retains candidate associations, not a second occurrence identity.
/// The first association supplies both representative and intermediate prefix.
struct BasicGroupBucket {
    // Preserve the existing Value cell. Starting, Ending and IntoBlocks leave it
    // Null and never treat a predicate or size as a key or charge key text.
    key: Value,
    first: usize,
    members: Vec<usize>,
    retained: bool,
}

/// This private partition choice does not admit other grouping families or
/// change the shared native metadata layout and WorkBudget.
#[derive(Clone, Copy)]
enum BasicGroupPartition {
    By(NodeId),
    AdjacentBy(NodeId),
    StartingWith(NodeId),
    EndingWith(NodeId),
    // The positive size was already evaluated in the receiving parent.
    IntoBlocks { node: NodeId, size: usize },
}

#[allow(clippy::too_many_arguments)]
fn eval_basic_owned_groups<'a>(
    program: EvalProgram<'a>,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&'a Instance],
    positions: &[PositionFrame],
    extra_sources: &[NamedSource],
    source_loader: Option<&dyn DynamicSourceLoader>,
    trace_scope: &TraceScope,
    extensions: &[crate::source_iteration::ObservedExtension<'a>],
    budget: &crate::owned_context::WorkBudget,
    windows: &[EvaluatedWindow],
    partition: BasicGroupPartition,
    ancestry: bool,
    filter_before_sort: bool,
    selection_phase: TraceFilterPhase,
) -> Result<Vec<Instance>, EngineError> {
    use crate::owned_context::{
        OccurrenceKind, OwnedContext, SourceFrame, SourceOrigin, SourceOriginKind,
    };
    use crate::source_iteration::reserve_observed_copy;

    let state = program
        .owned_context
        .expect("owned grouping receiving context");
    let access = |source| EngineError::OwnedSourceAccess {
        node: None,
        scope: Some(Box::new(trace_scope.source_scope.clone())),
        source,
    };
    // Keep the native explicit metadata ledger. Source-tree copies below remain
    // payload ownership, not a claim of a process-memory/allocation-total bound.
    budget.reserve(extensions.len()).map_err(access)?;
    let mut groups: Vec<BasicGroupBucket> = Vec::new();
    // Ending: the previous selected candidate's true boundary closed its group.
    // Before any candidate, the next selected candidate opens the first group.
    let mut ending_group_closed = true;
    for (index, extension) in extensions.iter().enumerate() {
        reserve_observed_copy(
            budget,
            &extension.frames,
            &extension.origins,
            positions,
            context.len(),
        )
        .map_err(access)?;
        reserve_observed_copy(
            budget,
            &[],
            &[],
            &extension.extension.positions,
            extension.extension.instances.len(),
        )
        .map_err(access)?;
        let candidate = OwnedContext {
            roots: state.roots,
            frames: extension.frames.clone(),
            origins: extension.origins.clone(),
            owned_ancestry: ancestry,
        };
        let candidate_program = program
            .with_owned_context(&candidate)
            .with_owned_trace_phase(crate::TraceOwnedPhase::Selection);
        let mut item_context = context.to_vec();
        item_context.extend(extension.extension.instances.iter().copied());
        let mut item_positions = positions.to_vec();
        item_positions.extend_from_slice(&extension.extension.positions);
        // Unlike ordinary window selection, this is deliberately fused: the
        // current candidate's key/boundary and post precede the next filter.
        if !filter_before_sort
            && !passes_scope_filter(
                candidate_program,
                scope.filter,
                selection_phase,
                trace_scope,
                &item_context,
                &item_positions,
            )?
        {
            continue;
        }
        let value = match partition {
            BasicGroupPartition::By(key) | BasicGroupPartition::AdjacentBy(key) => eval_expr(
                candidate_program,
                key,
                &item_context,
                &item_positions,
                &mut HashSet::new(),
            )?,
            BasicGroupPartition::StartingWith(_)
            | BasicGroupPartition::EndingWith(_)
            | BasicGroupPartition::IntoBlocks { .. } => Value::Null,
        };
        // Evaluate every boundary, including the first and last, and require
        // its exact Bool before post. A leading false Starting prefix and a
        // trailing false Ending suffix still form real nonempty groups.
        let boundary = match partition {
            BasicGroupPartition::StartingWith(predicate) => passes_scope_filter(
                candidate_program,
                Some(predicate),
                TraceFilterPhase::GroupStarting,
                trace_scope,
                &item_context,
                &item_positions,
            )?,
            BasicGroupPartition::EndingWith(predicate) => passes_scope_filter(
                candidate_program,
                Some(predicate),
                TraceFilterPhase::GroupEnding,
                trace_scope,
                &item_context,
                &item_positions,
            )?,
            BasicGroupPartition::By(_)
            | BasicGroupPartition::AdjacentBy(_)
            | BasicGroupPartition::IntoBlocks { .. } => false,
        };
        let retained = passes_scope_filter(
            candidate_program,
            scope.post_group_filter,
            TraceFilterPhase::PostGroupMember,
            trace_scope,
            &item_context,
            &item_positions,
        )?;
        budget.reserve(1).map_err(access)?;
        // Value's tagged PartialEq is intentional: NaNs are distinct, signed
        // zeros equal, and Int/Float or Null/JsonNull never collapse.
        // AdjacentBy compares only the last constructed run, including runs
        // whose post predicate has not retained them. Retention is applied
        // after partitioning, so dropping a separator can never join runs.
        let existing = match partition {
            BasicGroupPartition::By(_) => groups.iter_mut().find(|group| group.key == value),
            BasicGroupPartition::AdjacentBy(_) => {
                groups.last_mut().filter(|group| group.key == value)
            }
            BasicGroupPartition::StartingWith(_) => {
                if boundary {
                    None
                } else {
                    groups.last_mut()
                }
            }
            // A true boundary closes the group containing it; the next selected
            // candidate opens a new one. Retention never reopens a closed group.
            BasicGroupPartition::EndingWith(_) => {
                if ending_group_closed {
                    None
                } else {
                    groups.last_mut()
                }
            }
            // Selected candidates fill each block in order; the last block may
            // be partial. Retention never moves members between blocks.
            BasicGroupPartition::IntoBlocks { size, .. } => {
                groups.last_mut().filter(|group| group.members.len() < size)
            }
        };
        ending_group_closed = boundary;
        if let Some(group) = existing {
            group.members.push(index);
            group.retained |= retained;
        } else {
            if let Value::String(text) = &value {
                budget.charge(text.len()).map_err(access)?;
            }
            groups.push(BasicGroupBucket {
                key: value,
                first: index,
                members: vec![index],
                retained,
            });
        }
    }
    for (index, group) in groups.iter().enumerate() {
        let first = &extensions[group.first];
        reserve_observed_copy(budget, &first.frames, &first.origins, positions, 0)
            .map_err(access)?;
        reserve_observed_copy(budget, &[], &[], &first.extension.positions, 0).map_err(access)?;
        let candidate = OwnedContext {
            roots: state.roots,
            frames: first.frames.clone(),
            origins: first.origins.clone(),
            owned_ancestry: ancestry,
        };
        let mut item_positions = positions.to_vec();
        item_positions.extend_from_slice(&first.extension.positions);
        record_owned_event(
            program.with_owned_context(&candidate),
            &item_positions,
            || TraceEvent::GroupProduced {
                scope: trace_scope.clone(),
                grouping: match partition {
                    BasicGroupPartition::By(node) => TraceGrouping::By { node },
                    BasicGroupPartition::AdjacentBy(node) => TraceGrouping::AdjacentBy { node },
                    BasicGroupPartition::StartingWith(node) => TraceGrouping::StartingWith { node },
                    BasicGroupPartition::EndingWith(node) => TraceGrouping::EndingWith { node },
                    BasicGroupPartition::IntoBlocks { node, size } => {
                        TraceGrouping::IntoBlocks { node, size }
                    }
                },
                group_index: index + 1,
                member_count: group.members.len(),
                key: match partition {
                    BasicGroupPartition::By(_) | BasicGroupPartition::AdjacentBy(_) => {
                        Some(TraceValue::new(&group.key))
                    }
                    BasicGroupPartition::StartingWith(_)
                    | BasicGroupPartition::EndingWith(_)
                    | BasicGroupPartition::IntoBlocks { .. } => None,
                },
                retained: group.retained,
                positions: trace_positions(&item_positions),
            },
        );
    }
    groups.retain(|group| group.retained);
    for _ in windows {
        budget.reserve(groups.len()).map_err(access)?;
    }
    groups =
        apply_basic_owned_sequence_windows(groups, windows, program, positions, trace_scope, None);
    budget.reserve(groups.len()).map_err(access)?;
    let mut produced = Vec::new();
    for (ordinal, group) in groups.iter().enumerate() {
        let first = &extensions[group.first];
        // Root [] and an already-active ordinary item are identity candidates:
        // their selected value is authentic, but they introduce no source frame.
        let identity_base = match scope.source_selector() {
            Some(mapping::SourceSelectorRef::Owned(source)) => {
                Some(state.select(source).map_err(access)?.1)
            }
            _ => context.last().copied(),
        };
        let member = first
            .extension
            .instances
            .last()
            .copied()
            .or(identity_base)
            .expect("source grouping has a selected value");
        let (owner, collection) = match scope.source_selector() {
            Some(mapping::SourceSelectorRef::Owned(source)) => (
                Some(state.roots.key(&source.owner).map_err(access)?),
                source.path.as_slice(),
            ),
            _ => first
                .origins
                .iter()
                .rev()
                .find(|origin| std::ptr::eq(origin.item, member))
                .map(|origin| (Some(origin.owner), origin.path.as_slice()))
                .or_else(|| {
                    first
                        .frames
                        .iter()
                        .rev()
                        .find(|frame| std::ptr::eq(frame.item, member))
                        .map(|frame| (Some(frame.owner), frame.collection.as_slice()))
                })
                .or_else(|| {
                    state
                        .legacy_origin(member, &[])
                        .map(|(owner, _, path, _)| (Some(owner), path))
                })
                .unwrap_or((None, &[])),
        };
        let terminal = first
            .frames
            .iter()
            .enumerate()
            .rev()
            .find(|(index, frame)| {
                *index >= state.frames.len()
                    && Some(frame.owner) == owner
                    && frame.collection == collection
                    && std::ptr::eq(frame.item, member)
            })
            .map(|(index, frame)| (index, frame.position_slot));
        let slot = terminal.map_or_else(
            || {
                positions
                    .len()
                    .checked_add(first.extension.positions.len())
                    .ok_or_else(|| access(budget.slot_overflow()))
            },
            |(_, slot)| Ok(slot),
        )?;
        // Wrapper spelling and raw legacy identity are separate from owner/C.
        // In particular [Alias] over a named repeated root has C=[] but still
        // calls for the observed Alias wrapper used by legacy child iteration.
        let (wrapper_name, raw_collection) = match scope.source_selector() {
            Some(mapping::SourceSelectorRef::Legacy(path)) => {
                let raw = terminal
                    .and_then(|(_, slot)| slot.checked_sub(positions.len()))
                    .and_then(|index| first.extension.positions.get(index))
                    .map_or(path, |position| position.collection.as_slice());
                (path.last().and_then(|_| raw.last()), raw)
            }
            _ => (collection.last(), collection),
        };
        reserve_observed_copy(
            budget,
            &first.frames,
            &first.origins,
            positions,
            context.len(),
        )
        .map_err(access)?;
        reserve_observed_copy(
            budget,
            &[],
            &[],
            &first.extension.positions,
            first.extension.instances.len(),
        )
        .map_err(access)?;
        let frame_count = first
            .frames
            .len()
            .checked_add(usize::from(terminal.is_none()))
            .ok_or_else(|| access(budget.slot_overflow()))?;
        let replace_origin = first
            .origins
            .iter()
            .enumerate()
            .rposition(|(index, origin)| {
                index >= state.origins.len()
                    && Some(origin.owner) == owner
                    && origin.path == collection
                    && std::ptr::eq(origin.item, member)
            });
        let origin_count = first
            .origins
            .len()
            .saturating_sub(usize::from(replace_origin.is_some()))
            .checked_add(1 + usize::from(wrapper_name.is_some()))
            .ok_or_else(|| access(budget.slot_overflow()))?;
        if owner.is_some()
            && (frame_count > budget.limits.depth || origin_count > budget.limits.depth)
        {
            return Err(access(crate::OwnedSourceError::ResourceLimit {
                resource: "frame depth",
                limit: budget.limits.depth,
            }));
        }
        // Each projected frame, member container, wrapper and new position owns
        // its own checked path/name metadata. The same ledger spans all buckets.
        budget.path(collection).map_err(access)?;
        budget.path(raw_collection).map_err(access)?;
        budget
            .reserve(
                collection
                    .len()
                    .checked_mul(4)
                    .and_then(|n| n.checked_add(8))
                    .and_then(|n| {
                        n.checked_add(if terminal.is_none() {
                            raw_collection.len()
                        } else {
                            0
                        })
                    })
                    .and_then(|n| n.checked_add(group.members.len()))
                    .ok_or_else(|| access(budget.slot_overflow()))?,
            )
            .map_err(access)?;
        let bytes = collection
            .iter()
            .try_fold(0usize, |sum, part| sum.checked_add(part.len()))
            .and_then(|n| n.checked_mul(4))
            .and_then(|n| n.checked_add(wrapper_name.map_or(0, String::len)))
            .and_then(|n| {
                if terminal.is_none() {
                    raw_collection
                        .iter()
                        .try_fold(n, |sum, part| sum.checked_add(part.len()))
                } else {
                    Some(n)
                }
            })
            .ok_or_else(|| access(budget.overflow()))?;
        budget.charge(bytes).map_err(access)?;
        let members = Instance::Repeated(
            group
                .members
                .iter()
                .map(|index| {
                    extensions[*index]
                        .extension
                        .instances
                        .last()
                        .copied()
                        .or(identity_base)
                        .expect("group member is an admitted candidate")
                        .clone()
                })
                .collect(),
        );
        let frame = match wrapper_name {
            Some(name) => Instance::Group(vec![(name.clone(), members)].into()),
            None => members,
        };
        // Borrow the container actually stored in the legacy wrapper, not an
        // equal-but-unrelated copy: pointer provenance must stay exact.
        let (wrapper, members) = match &frame {
            Instance::Group(fields) => (Some(&frame), &fields[0].1),
            members => (None, members),
        };
        let mut frames = first.frames.clone();
        if let Some(owner) = owner {
            let frame = SourceFrame {
                owner,
                collection: collection.to_vec(),
                item: members,
                position_slot: slot,
                kind: OccurrenceKind::GroupMembers,
            };
            if let Some((index, _)) = terminal {
                frames[index] = frame;
            } else {
                frames.push(frame);
            }
        }
        let mut origins = first.origins.clone();
        if let Some(owner) = owner {
            if let Some(index) = replace_origin {
                origins.remove(index);
            }
            if let Some(wrapper) = wrapper {
                origins.push(SourceOrigin {
                    owner,
                    path: collection.to_vec(),
                    item: wrapper,
                    kind: SourceOriginKind::GroupWrapper,
                });
            }
            origins.push(SourceOrigin {
                owner,
                path: collection.to_vec(),
                item: members,
                kind: SourceOriginKind::GroupMembers,
            });
        }
        let grouped = OwnedContext {
            roots: state.roots,
            frames,
            origins,
            owned_ancestry: ancestry,
        };
        let mut item_context = context.to_vec();
        item_context.extend(
            first
                .extension
                .instances
                .iter()
                .take(first.extension.instances.len().saturating_sub(1))
                .copied(),
        );
        item_context.extend(wrapper);
        item_context.push(members);
        let mut output_positions = positions.to_vec();
        output_positions.extend_from_slice(&first.extension.positions);
        if terminal.is_none() {
            output_positions.push(PositionFrame {
                collection: raw_collection.to_vec(),
                index: ordinal + 1,
                grouped: wrapper.is_some(),
                join: None,
                join_position: None,
                document_path: None,
            });
        } else {
            let position = &mut output_positions[slot];
            position.index = ordinal + 1;
            position.grouped = wrapper.is_some();
        }
        let evaluator = ItemEvaluator {
            program: program
                .with_owned_context(&grouped)
                .with_owned_trace_phase(crate::TraceOwnedPhase::Output),
            scope,
            trace_scope,
            target,
            extra_sources,
            source_loader,
        };
        if let Some(item) =
            evaluator.produce(&item_context, &output_positions, &output_positions, false)?
        {
            produced.push(item.instance);
        }
    }
    Ok(produced)
}

#[derive(Debug, Clone, Copy)]
enum EvaluatedWindow {
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

fn eval_sequence_window(
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

fn apply_basic_owned_sequence_windows<T>(
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

fn apply_sequence_windows<T>(
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

fn eval_block_size(
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

/// Evaluates one iteration item: the filter (`None` when it drops the item),
/// then the scope's bindings and child scopes.
impl ItemEvaluator<'_> {
    fn produce(
        &self,
        context: &[&Instance],
        filter_positions: &[PositionFrame],
        output_positions: &[PositionFrame],
        apply_filter: bool,
    ) -> Result<Option<ProducedItem>, EngineError> {
        let Some(instance) =
            self.produce_instance(context, filter_positions, output_positions, apply_filter)?
        else {
            return Ok(None);
        };
        let output_path = self
            .scope
            .output_path()
            .map(|node| {
                let mut in_progress = HashSet::new();
                match eval_expr(
                    self.program,
                    node,
                    context,
                    output_positions,
                    &mut in_progress,
                )? {
                    Value::String(path) if !path.trim().is_empty() => Ok(path),
                    Value::String(_) => Err(EngineError::EmptyDynamicTargetPath { node }),
                    value => Err(EngineError::DynamicTargetPath {
                        node,
                        found: value.type_name(),
                    }),
                }
            })
            .transpose()?;
        record_owned_event(self.program, output_positions, || {
            TraceEvent::TargetProduced {
                scope: self.trace_scope.clone(),
                positions: trace_positions(output_positions),
                output_path: output_path.as_deref().map(bounded_text),
                kind: TraceOutputKind::of(&instance),
            }
        });
        Ok(Some(ProducedItem {
            instance,
            output_path,
        }))
    }

    fn produce_instance(
        &self,
        context: &[&Instance],
        filter_positions: &[PositionFrame],
        output_positions: &[PositionFrame],
        apply_filter: bool,
    ) -> Result<Option<Instance>, EngineError> {
        let Self {
            program,
            scope,
            trace_scope,
            target,
            extra_sources,
            source_loader,
        } = *self;
        let filter_phase = if scope.has_sort() {
            TraceFilterPhase::AfterSort
        } else {
            TraceFilterPhase::Selection
        };
        if apply_filter
            && !passes_scope_filter(
                program,
                scope.filter,
                filter_phase,
                trace_scope,
                context,
                filter_positions,
            )?
        {
            return Ok(None);
        }

        if let ScopeConstruction::Scalar { value } = &scope.construction {
            let mut in_progress = HashSet::new();
            let value = eval_expr(program, *value, context, output_positions, &mut in_progress)?;
            return Ok(Some(Instance::Scalar(adapt_target_scalar(value, target))));
        }

        if let Some((source, plan)) = scope.owned_recursive_filter() {
            // Admission routes every owned recursive filter through the paired context.
            let state =
                program
                    .owned_context
                    .ok_or_else(|| EngineError::UnsupportedSourceIteration {
                        scope: trace_scope.source_scope.clone(),
                    })?;
            return recursive_filter::execute_owned(
                program,
                plan,
                source,
                state,
                context,
                output_positions,
            )
            .map(Some);
        }

        if let ScopeConstruction::RecursiveFilter { plan } = &scope.construction {
            let current =
                context
                    .last()
                    .copied()
                    .ok_or(EngineError::RecursiveFilterRequiresGroup {
                        found: "missing context",
                    })?;
            return recursive_filter::execute(program, plan, current, context, output_positions)
                .map(Some);
        }

        if let ScopeConstruction::PathHierarchy { plan } = &scope.construction {
            return crate::path_hierarchy::build(plan, context).map(Some);
        }

        if let ScopeConstruction::OwnedPathHierarchy { plan } = &scope.construction {
            // Admission routes every owned hierarchy through the paired context.
            let state =
                program
                    .owned_context
                    .ok_or_else(|| EngineError::UnsupportedSourceIteration {
                        scope: trace_scope.source_scope.clone(),
                    })?;
            return crate::path_hierarchy::build_owned(
                plan,
                state,
                output_positions.len(),
                &trace_scope.source_scope,
            )
            .map(Some);
        }

        if let ScopeConstruction::AdjacencyTree { plan } = &scope.construction {
            return crate::adjacency_tree::construct(program, plan, context, output_positions)
                .map(Some);
        }

        if let ScopeConstruction::OwnedAdjacencyTree { plan } = &scope.construction {
            // Admission routes every owned adjacency tree through the paired context.
            let state =
                program
                    .owned_context
                    .ok_or_else(|| EngineError::UnsupportedSourceIteration {
                        scope: trace_scope.source_scope.clone(),
                    })?;
            return crate::adjacency_tree::construct_owned(
                program,
                plan,
                state,
                context,
                output_positions,
                &trace_scope.source_scope,
            )
            .map(Some);
        }

        if matches!(&scope.construction, ScopeConstruction::CopyCurrentSource) {
            return match context.last().copied() {
                Some(current @ Instance::Group(_)) => Ok(Some((*current).clone())),
                Some(Instance::Scalar(_)) => {
                    Err(EngineError::CopyCurrentSourceRequiresGroup { found: "scalar" })
                }
                Some(Instance::Repeated(_)) => Err(EngineError::CopyCurrentSourceRequiresGroup {
                    found: "repeated collection",
                }),
                Some(Instance::MappedSequence(_)) => {
                    Err(EngineError::CopyCurrentSourceRequiresGroup {
                        found: "mapped sequence",
                    })
                }
                Some(Instance::DocumentSet(_)) => {
                    Err(EngineError::CopyCurrentSourceRequiresGroup {
                        found: "document set",
                    })
                }
                None => Err(EngineError::CopyCurrentSourceRequiresGroup {
                    found: "missing context",
                }),
            };
        }

        let mut fields = Vec::with_capacity(
            scope.bindings.len()
                + scope.dynamic_bindings.len()
                + scope.children.len()
                + scope.dynamic_children.len(),
        );
        for binding in &scope.bindings {
            let mut in_progress = HashSet::new();
            let value = eval_expr(
                program,
                binding.node,
                context,
                output_positions,
                &mut in_progress,
            )?;
            let target_field = target.and_then(|schema| schema.child(&binding.target_field));
            let value = adapt_target_scalar(value, target_field);
            let repeating = target_field.is_some_and(|field| field.repeating);
            let value = match repeating {
                true => match value {
                    Value::Null => Instance::Repeated(Vec::new()),
                    value => Instance::Repeated(vec![Instance::Scalar(value)]),
                },
                false => Instance::Scalar(value),
            };
            let traced_value = trace_field_value(&value);
            before_target_write(
                program.debug_hook,
                trace_scope,
                TraceTargetFieldBinding::StaticBinding {
                    value: binding.node,
                },
                output_positions,
                context,
                &binding.target_field,
                &value,
                &fields,
                program.primary_source(),
            )?;
            insert_static_binding(&mut fields, binding.target_field.clone(), value, repeating)?;
            record_target_field(
                program,
                trace_scope,
                bounded_text(&binding.target_field),
                TraceTargetFieldBinding::StaticBinding {
                    value: binding.node,
                },
                output_positions,
                traced_value,
            );
        }
        for binding in &scope.dynamic_bindings {
            let key = eval_dynamic_key(program, binding.key, context, output_positions)?;
            let mut in_progress = HashSet::new();
            let value = eval_expr(
                program,
                binding.value,
                context,
                output_positions,
                &mut in_progress,
            )?;
            let value = adapt_target_scalar(value, target.and_then(ir::SchemaNode::dynamic_fields));
            let value = Instance::Scalar(value);
            let traced_key = bounded_text(&key);
            let traced_value = trace_field_value(&value);
            before_target_write(
                program.debug_hook,
                trace_scope,
                TraceTargetFieldBinding::DynamicBinding {
                    key: binding.key,
                    value: binding.value,
                },
                output_positions,
                context,
                &key,
                &value,
                &fields,
                program.primary_source(),
            )?;
            dynamic_target::insert_dynamic_target_field(&mut fields, key, value, target)?;
            record_target_field(
                program,
                trace_scope,
                traced_key,
                TraceTargetFieldBinding::DynamicBinding {
                    key: binding.key,
                    value: binding.value,
                },
                output_positions,
                traced_value,
            );
        }
        for (index, child) in scope.children.iter().enumerate() {
            let child_target = target.and_then(|schema| schema.child(&child.target_field));
            let child_instance = eval_scope(
                program,
                child,
                child_target,
                context,
                output_positions,
                extra_sources,
                source_loader,
                &trace_scope.child(&child.target_field, index),
            )?;
            let traced_value = trace_field_value(&child_instance);
            before_target_write(
                program.debug_hook,
                trace_scope,
                TraceTargetFieldBinding::StaticChild,
                output_positions,
                context,
                &child.target_field,
                &child_instance,
                &fields,
                program.primary_source(),
            )?;
            insert_target_field(&mut fields, child.target_field.clone(), child_instance)?;
            record_target_field(
                program,
                trace_scope,
                bounded_text(&child.target_field),
                TraceTargetFieldBinding::StaticChild,
                output_positions,
                traced_value,
            );
        }
        for (index, child) in scope.dynamic_children.iter().enumerate() {
            if child.scope.iteration_output == IterationOutput::MappedSequence {
                return Err(EngineError::MappedSequenceDynamicTarget);
            }
            let key = eval_dynamic_key(program, child.key, context, output_positions)?;
            let child_target = target.and_then(ir::SchemaNode::dynamic_fields);
            let child_instance = eval_scope(
                program,
                &child.scope,
                child_target,
                context,
                output_positions,
                extra_sources,
                source_loader,
                &trace_scope.dynamic_child(index, scope.children.len().saturating_add(index)),
            )?;
            let traced_key = bounded_text(&key);
            let traced_value = trace_field_value(&child_instance);
            before_target_write(
                program.debug_hook,
                trace_scope,
                TraceTargetFieldBinding::DynamicChild { key: child.key },
                output_positions,
                context,
                &key,
                &child_instance,
                &fields,
                program.primary_source(),
            )?;
            dynamic_target::insert_dynamic_target_field(&mut fields, key, child_instance, target)?;
            record_target_field(
                program,
                trace_scope,
                traced_key,
                TraceTargetFieldBinding::DynamicChild { key: child.key },
                output_positions,
                traced_value,
            );
        }
        if let ScopeConstruction::XmlMixedContent { elements } = &scope.construction {
            attach_xml_mixed_content(&mut fields, context.last().copied(), elements);
        }
        Ok(Some(Instance::Group((fields).into())))
    }
}

fn trace_field_value(value: &Instance) -> (TraceOutputKind, Option<TraceValue>) {
    let preview = match value {
        Instance::Scalar(value) => Some(TraceValue::new(value)),
        Instance::Repeated(values) if values.len() == 1 => values
            .first()
            .and_then(Instance::as_scalar)
            .map(TraceValue::new),
        Instance::Group(_)
        | Instance::Repeated(_)
        | Instance::MappedSequence(_)
        | Instance::DocumentSet(_) => None,
    };
    (TraceOutputKind::of(value), preview)
}

fn record_target_field(
    program: EvalProgram<'_>,
    trace_scope: &TraceScope,
    field: String,
    binding: TraceTargetFieldBinding,
    positions: &[PositionFrame],
    (kind, value): (TraceOutputKind, Option<TraceValue>),
) {
    record_owned_event(program, positions, || TraceEvent::TargetFieldWritten {
        scope: trace_scope.clone(),
        field,
        binding,
        positions: trace_positions(positions),
        kind,
        value,
    });
}

fn attach_xml_mixed_content(
    fields: &mut Vec<(String, Instance)>,
    source: Option<&Instance>,
    elements: &[XmlMixedContentElement],
) {
    let Some(source_items) = source
        .and_then(|source| source.field(XML_MIXED_CONTENT_FIELD))
        .and_then(Instance::as_repeated)
    else {
        return;
    };
    let mut occurrences = BTreeMap::<&str, usize>::new();
    let items = source_items
        .iter()
        .filter_map(|item| {
            let name = item
                .field(XML_NODE_NAME_FIELD)
                .and_then(Instance::as_scalar)
                .and_then(|value| match value {
                    Value::String(name) => Some(name.as_str()),
                    _ => None,
                })?;
            if name.is_empty() {
                return Some(item.clone());
            }
            let element = elements.iter().find(|element| element.source == name)?;
            let index = occurrences.entry(&element.target).or_default();
            let value = fields
                .iter()
                .find(|(field, _)| field == &element.target)
                .and_then(|(_, value)| value.as_repeated())?
                .get(*index)?
                .clone();
            *index += 1;
            let text = value
                .as_scalar()
                .map(mixed_content_text)
                .unwrap_or_default();
            Some(Instance::Group(
                (vec![
                    (
                        XML_NODE_NAME_FIELD.to_string(),
                        Instance::Scalar(Value::String(element.target.clone())),
                    ),
                    (
                        XML_TEXT_FIELD.to_string(),
                        Instance::Scalar(Value::String(text)),
                    ),
                    (XML_MIXED_CONTENT_VALUE_FIELD.to_string(), value),
                ])
                .into(),
            ))
        })
        .collect::<Vec<_>>();
    if !items.is_empty() {
        fields.push((
            XML_MIXED_CONTENT_FIELD.to_string(),
            Instance::Repeated(items),
        ));
    }
}

fn mixed_content_text(value: &Value) -> String {
    match value {
        Value::Null | Value::JsonNull(_) | Value::XmlNil(_) => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Int(value) => value.to_string(),
        Value::Float(value) => value.to_string(),
        Value::Decimal(value) => value.to_string(),
        Value::String(value) => value.clone(),
    }
}

fn adapt_numeric_target(value: Value, expected: ScalarType) -> Value {
    match (expected, value) {
        (ScalarType::Int, Value::Float(value))
            if value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value < -(i64::MIN as f64) =>
        {
            Value::Int(value as i64)
        }
        (ScalarType::Float, Value::Int(value)) => {
            let converted = value as f64;
            if (converted as i128) == i128::from(value) {
                Value::Float(converted)
            } else {
                Value::Int(value)
            }
        }
        (_, value) => value,
    }
}

fn adapt_target_scalar(value: Value, target: Option<&ir::SchemaNode>) -> Value {
    let Some(target) = target else {
        return value;
    };
    match target.kind {
        SchemaKind::Scalar { ty } => adapt_numeric_target(value, ty),
        SchemaKind::ScalarUnion { types } => {
            let actual = match value {
                Value::String(_) => Some(ScalarType::String),
                Value::Int(_) => Some(ScalarType::Int),
                Value::Float(_) => Some(ScalarType::Float),
                Value::Bool(_) => Some(ScalarType::Bool),
                Value::Null | Value::JsonNull(_) | Value::XmlNil(_) | Value::Decimal(_) => None,
            };
            if actual.is_some_and(|ty| types.contains(ty)) {
                return value;
            }
            match value {
                value @ Value::Int(_) if types.contains(ScalarType::Float) => {
                    adapt_numeric_target(value, ScalarType::Float)
                }
                value @ Value::Float(_) if types.contains(ScalarType::Int) => {
                    adapt_numeric_target(value, ScalarType::Int)
                }
                value => value,
            }
        }
        SchemaKind::Group { .. } => value,
    }
}

fn insert_static_binding(
    fields: &mut Vec<(String, Instance)>,
    name: String,
    value: Instance,
    repeating: bool,
) -> Result<(), EngineError> {
    if repeating {
        let Instance::Repeated(mut additions) = value else {
            return insert_target_field(fields, name, value);
        };
        if let Some((_, Instance::Repeated(existing))) =
            fields.iter_mut().find(|(field, _)| field == &name)
        {
            existing.append(&mut additions);
            return Ok(());
        }
        return insert_target_field(fields, name, Instance::Repeated(additions));
    }
    insert_target_field(fields, name, value)
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
