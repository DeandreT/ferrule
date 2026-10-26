//! Owned grouping preserves candidate associations and exact frame provenance.
//!
//! Partition before retaining groups; materialize only each selected group's
//! owned member container, while borrowing its first candidate's ancestors.

use std::collections::HashSet;

use ir::{Instance, Value};
use mapping::{NamedSource, NodeId, Scope};

use super::construction::ItemEvaluator;
use super::passes_scope_filter;
use super::windows::{EvaluatedWindow, apply_basic_owned_sequence_windows};
use crate::eval_expr::{EvalProgram, eval_expr};
use crate::source_iteration::PositionFrame;
use crate::trace::{
    TraceEvent, TraceFilterPhase, TraceGrouping, TraceScope, TraceValue, record_owned_event,
    trace_positions,
};
use crate::{DynamicSourceLoader, EngineError};

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
pub(super) enum BasicGroupPartition {
    By(NodeId),
    AdjacentBy(NodeId),
    StartingWith(NodeId),
    EndingWith(NodeId),
    // The positive size was already evaluated in the receiving parent.
    IntoBlocks { node: NodeId, size: usize },
}

#[allow(clippy::too_many_arguments)]
pub(super) fn eval_basic_owned_groups<'a>(
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
