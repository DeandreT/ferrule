use std::collections::{BTreeMap, HashSet};

use ir::{Instance, Value};
use mapping::{
    Graph, IterationOutput, NamedSource, NodeId, Scope, ScopeConstruction, SortFilterOrder,
};

use crate::aggregate::value_ordering;
use crate::dynamic_target::{self, eval_dynamic_key, insert_target_field};
use crate::eval_expr::eval_expr;
use crate::grouping::GroupingMode;
use crate::iteration_output::finalize_scope_output;
use crate::join::{execute as execute_join, extensions as join_extensions};
use crate::recursive_filter;
use crate::resolve::context_for_position;
use crate::sequence::eval_sequence;
use crate::source_iteration::{PositionFrame, WalkExtension, walk};
use crate::{DynamicSourceLoader, EngineError};

struct GroupBucket {
    key: Option<Value>,
    members: Vec<Instance>,
    intermediate_frames: Vec<Instance>,
    positions: Vec<PositionFrame>,
}

struct OwnedGroup {
    wrapper: Option<Instance>,
    intermediate_frames: Vec<Instance>,
    members: Instance,
    positions: Vec<PositionFrame>,
}

struct ItemEvaluator<'a> {
    graph: &'a Graph,
    scope: &'a Scope,
    target: Option<&'a ir::SchemaNode>,
    extra_sources: &'a [NamedSource],
    source_loader: Option<&'a dyn DynamicSourceLoader>,
}

pub(crate) fn eval_scope(
    graph: &Graph,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&Instance],
    positions: &[PositionFrame],
    extra_sources: &[NamedSource],
    source_loader: Option<&dyn DynamicSourceLoader>,
) -> Result<Instance, EngineError> {
    if let Some(segments) = scope.concatenated() {
        let mut output = Vec::new();
        for segment in segments.iter() {
            match eval_scope(
                graph,
                segment,
                target,
                context,
                positions,
                extra_sources,
                source_loader,
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
        return Ok(match scope.iteration_output {
            IterationOutput::Repeated => Instance::Repeated(output),
            IterationOutput::MappedSequence => Instance::MappedSequence(output),
            IterationOutput::First => {
                return Err(EngineError::InvalidConcatenatedScopeItem {
                    found: "a first-item wrapper",
                });
            }
        });
    }
    let sequence_items = scope
        .sequence()
        .map(|sequence| eval_sequence(graph, sequence, context, positions))
        .transpose()?
        .map(|values| {
            Instance::Repeated(values.into_iter().map(Instance::Scalar).collect::<Vec<_>>())
        });
    let join_rows = scope
        .join()
        .map(|(join, plan)| execute_join(context, join, plan))
        .transpose()?;
    let dynamic_source = scope.source().and_then(|path| {
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
                graph,
                dynamic.node,
                &item_context,
                &item_positions,
                &mut in_progress,
            )?;
            let Value::String(path) = path else {
                if path == Value::Null {
                    continue;
                }
                return Err(EngineError::DynamicSourcePath {
                    source_name: source.name.clone(),
                    found: path.type_name(),
                });
            };
            let instance = loader.load(&source.name, &path).map_err(|message| {
                EngineError::DynamicSourceLoad {
                    source_name: source.name.clone(),
                    path,
                    message,
                }
            })?;
            loaded_dynamic.push((driver_index, instance));
        }
    }
    let mut extensions = if let Some(rows) = &join_rows {
        join_extensions(rows)
    } else if let Some(items) = &sequence_items {
        walk(items, &[], &[], &[], &[])
    } else if let Some(source) = dynamic_source {
        let path = scope.source().unwrap_or_default();
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
        match scope.source() {
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

    let filter_before_sort = scope.filter.is_some()
        && scope.sort_by.is_some()
        && scope.sort_filter_order == SortFilterOrder::FilterThenSort;
    if filter_before_sort {
        let mut filtered = Vec::with_capacity(extensions.len());
        for extension in extensions {
            let mut item_context = context.to_vec();
            item_context.extend(extension.instances.iter().copied());
            let mut item_positions = positions.to_vec();
            item_positions.extend(extension.positions.iter().cloned());
            if passes_filter(graph, scope.filter, &item_context, &item_positions)? {
                filtered.push(extension);
            }
        }
        extensions = filtered;
    }

    if let Some(sort_node) = scope.sort_by {
        let mut keyed = Vec::with_capacity(extensions.len());
        for extension in extensions {
            let mut item_context = context.to_vec();
            item_context.extend(extension.instances.iter().copied());
            let mut item_positions = positions.to_vec();
            item_positions.extend(extension.positions.iter().cloned());
            let mut in_progress = HashSet::new();
            let key = eval_expr(
                graph,
                sort_node,
                &item_context,
                &item_positions,
                &mut in_progress,
            )?;
            keyed.push((extension, key));
        }
        keyed.sort_by(|(_, left), (_, right)| {
            let ordering = value_ordering(left, right).unwrap_or(std::cmp::Ordering::Equal);
            if scope.sort_descending {
                ordering.reverse()
            } else {
                ordering
            }
        });
        extensions = keyed
            .into_iter()
            .enumerate()
            .map(|(index, (mut extension, _))| {
                renumber_extension(&mut extension.positions, index + 1);
                extension
            })
            .collect();
    }

    let take = scope
        .take
        .map(|node| eval_item_count(graph, node, context, positions))
        .transpose()?;
    let take = match scope.iteration_output {
        IterationOutput::Repeated | IterationOutput::MappedSequence => take,
        IterationOutput::First => Some(take.unwrap_or(1).min(1)),
    };
    let grouping_count = [
        scope.group_by,
        scope.group_starting_with,
        scope.group_into_blocks,
    ]
    .into_iter()
    .flatten()
    .count();
    if scope.join().is_some() && grouping_count != 0 {
        return Err(EngineError::JoinGroupingUnsupported);
    }
    if grouping_count > 1 {
        return Err(EngineError::ConflictingGroupingModes);
    }
    let grouping = if let Some(node) = scope.group_by {
        Some(GroupingMode::By(node))
    } else if let Some(node) = scope.group_starting_with {
        Some(GroupingMode::StartingWith(node))
    } else if let Some(node) = scope.group_into_blocks {
        Some(GroupingMode::IntoBlocks(eval_block_size(
            graph, node, context, positions,
        )?))
    } else {
        None
    };
    let mut produced = Vec::with_capacity(take.unwrap_or(extensions.len()).min(extensions.len()));
    let item_evaluator = ItemEvaluator {
        graph,
        scope,
        target,
        extra_sources,
        source_loader,
    };
    if let Some(grouping) = grouping {
        let mut groups: Vec<GroupBucket> = Vec::new();
        for extension in &extensions {
            let mut item_context = context.to_vec();
            item_context.extend(extension.instances.iter().copied());
            let mut item_positions = positions.to_vec();
            item_positions.extend(extension.positions.iter().cloned());
            if !filter_before_sort
                && !passes_filter(graph, scope.filter, &item_context, &item_positions)?
            {
                continue;
            }
            let member = (*extension
                .instances
                .last()
                .expect("extensions are never empty"))
            .clone();
            let key = match grouping {
                GroupingMode::By(key_node) => {
                    let mut in_progress = HashSet::new();
                    Some(eval_expr(
                        graph,
                        key_node,
                        &item_context,
                        &item_positions,
                        &mut in_progress,
                    )?)
                }
                GroupingMode::StartingWith(_) | GroupingMode::IntoBlocks(_) => None,
            };
            let starts_group = match grouping {
                GroupingMode::StartingWith(predicate) => {
                    passes_filter(graph, Some(predicate), &item_context, &item_positions)?
                }
                GroupingMode::By(_) | GroupingMode::IntoBlocks(_) => false,
            };
            let existing = match grouping {
                GroupingMode::By(_) => groups.iter_mut().find(|group| group.key == key),
                GroupingMode::StartingWith(_) => {
                    if starts_group {
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
                Some(group) => group.members.push(member),
                None => groups.push(GroupBucket {
                    key,
                    members: vec![member],
                    intermediate_frames: extension.instances[..extension.instances.len() - 1]
                        .iter()
                        .map(|instance| (**instance).clone())
                        .collect(),
                    positions: item_positions,
                }),
            }
        }
        // Position frames stay in order, with the named collection wrapper
        // immediately before the grouped members.
        let owned: Vec<OwnedGroup> = groups
            .into_iter()
            .map(|group| {
                let members = Instance::Repeated(group.members);
                let wrapper = scope
                    .source()
                    .and_then(|path| path.last())
                    .map(|segment| Instance::Group(vec![(segment.clone(), members.clone())]));
                OwnedGroup {
                    wrapper,
                    intermediate_frames: group.intermediate_frames,
                    members,
                    positions: group.positions,
                }
            })
            .collect();
        for group in &owned {
            if take.is_some_and(|limit| produced.len() >= limit) {
                break;
            }
            let parent_wrappers = positions.iter().filter(|position| position.grouped).count();
            let parent_frame_start = context
                .len()
                .checked_sub(positions.len() + parent_wrappers)
                .expect("iteration positions have matching context frames");
            let mut next_context = context[..parent_frame_start].to_vec();
            next_context.extend_from_slice(&context[parent_frame_start..]);
            next_context.extend(group.intermediate_frames.iter());
            if let Some(wrapper) = &group.wrapper {
                next_context.push(wrapper);
            }
            next_context.push(&group.members);
            let mut output_positions = group.positions.clone();
            if let Some(position) = output_positions.last_mut() {
                position.index = produced.len() + 1;
                position.grouped = group.wrapper.is_some();
            }
            if let Some(instance) =
                item_evaluator.produce(&next_context, &group.positions, &output_positions, false)?
            {
                produced.push(instance);
            }
        }
    } else {
        let mut compact_positions: BTreeMap<Vec<usize>, usize> = BTreeMap::new();
        for extension in &extensions {
            if take.is_some_and(|limit| produced.len() >= limit) {
                break;
            }
            let mut next_context = context.to_vec();
            next_context.extend(extension.instances.iter().copied());
            let mut candidate_positions = positions.to_vec();
            candidate_positions.extend(extension.positions.iter().cloned());

            let parent_key: Vec<usize> = extension
                .positions
                .iter()
                .take(extension.positions.len().saturating_sub(1))
                .map(|position| position.index)
                .collect();
            let joined = extension
                .positions
                .last()
                .is_some_and(|position| position.join_position.is_some());
            let next_position = if joined {
                produced.len() + 1
            } else {
                compact_positions.get(&parent_key).copied().unwrap_or(0) + 1
            };
            let mut output_positions = candidate_positions.clone();
            renumber_extension(&mut output_positions, next_position);
            if let Some(instance) = item_evaluator.produce(
                &next_context,
                &candidate_positions,
                &output_positions,
                !filter_before_sort,
            )? {
                if !joined && !extension.positions.is_empty() {
                    compact_positions.insert(parent_key, next_position);
                }
                produced.push(instance);
            }
        }
    }

    finalize_scope_output(
        scope,
        target.is_some_and(|target| target.repeating),
        produced,
    )
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
    graph: &Graph,
    node: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<usize, EngineError> {
    let mut in_progress = HashSet::new();
    let value = eval_expr(graph, node, context, positions, &mut in_progress)?;
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
    graph: &Graph,
    node: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<usize, EngineError> {
    let size = eval_item_count(graph, node, context, positions)?;
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
    ) -> Result<Option<Instance>, EngineError> {
        let Self {
            graph,
            scope,
            target,
            extra_sources,
            source_loader,
        } = *self;
        if apply_filter && !passes_filter(graph, scope.filter, context, filter_positions)? {
            return Ok(None);
        }

        if let ScopeConstruction::Scalar { value } = &scope.construction {
            let mut in_progress = HashSet::new();
            return eval_expr(graph, *value, context, output_positions, &mut in_progress)
                .map(Instance::Scalar)
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
            return recursive_filter::execute(graph, plan, current, context, output_positions)
                .map(Some);
        }

        if let ScopeConstruction::PathHierarchy { plan } = &scope.construction {
            return crate::path_hierarchy::build(plan, context).map(Some);
        }

        if let ScopeConstruction::AdjacencyTree { plan } = &scope.construction {
            return crate::adjacency_tree::construct(graph, plan, context, output_positions)
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
                graph,
                binding.node,
                context,
                output_positions,
                &mut in_progress,
            )?;
            let repeating = target
                .and_then(|schema| schema.child(&binding.target_field))
                .is_some_and(|field| field.repeating);
            let value = match repeating {
                true => match value {
                    Value::Null => Instance::Repeated(Vec::new()),
                    value => Instance::Repeated(vec![Instance::Scalar(value)]),
                },
                false => Instance::Scalar(value),
            };
            insert_static_binding(&mut fields, binding.target_field.clone(), value, repeating)?;
        }
        for binding in &scope.dynamic_bindings {
            let key = eval_dynamic_key(graph, binding.key, context, output_positions)?;
            let mut in_progress = HashSet::new();
            let value = eval_expr(
                graph,
                binding.value,
                context,
                output_positions,
                &mut in_progress,
            )?;
            dynamic_target::insert_dynamic_target_field(
                &mut fields,
                key,
                Instance::Scalar(value),
                target,
            )?;
        }
        for child in &scope.children {
            let child_target = target.and_then(|schema| schema.child(&child.target_field));
            let child_instance = eval_scope(
                graph,
                child,
                child_target,
                context,
                output_positions,
                extra_sources,
                source_loader,
            )?;
            insert_target_field(&mut fields, child.target_field.clone(), child_instance)?;
        }
        for child in &scope.dynamic_children {
            if child.scope.iteration_output == IterationOutput::MappedSequence {
                return Err(EngineError::MappedSequenceDynamicTarget);
            }
            let key = eval_dynamic_key(graph, child.key, context, output_positions)?;
            let child_target = target.and_then(ir::SchemaNode::dynamic_fields);
            let child_instance = eval_scope(
                graph,
                &child.scope,
                child_target,
                context,
                output_positions,
                extra_sources,
                source_loader,
            )?;
            dynamic_target::insert_dynamic_target_field(&mut fields, key, child_instance, target)?;
        }
        Ok(Some(Instance::Group(fields)))
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
    graph: &Graph,
    filter: Option<NodeId>,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<bool, EngineError> {
    let Some(filter_node) = filter else {
        return Ok(true);
    };
    let mut in_progress = HashSet::new();
    match eval_expr(graph, filter_node, context, positions, &mut in_progress)? {
        Value::Bool(value) => Ok(value),
        other => Err(EngineError::NotABool {
            node: filter_node,
            found: other.type_name(),
        }),
    }
}
