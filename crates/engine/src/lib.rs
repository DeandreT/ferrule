//! Interprets a mapping graph against a source instance to produce a target instance.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use ir::{Instance, ScalarType, Value};
use mapping::{
    Graph, IterationOutput, JoinId, Node, NodeId, Project, RuntimeValue, Scope, ScopeConstruction,
};
use thiserror::Error;

mod context;
mod dynamic_target;
mod grouping;
mod iteration_output;
mod join;
mod sequence;
mod source_iteration;
mod validate;
mod validate_join;

use context::runtime_field;
use dynamic_target::{eval_dynamic_key, insert_target_field};
use grouping::GroupingMode;
use iteration_output::finalize_scope_output;
use join::{
    AggregateInput as JoinAggregateInput, eval_aggregate as eval_join_aggregate,
    execute as execute_join, extensions as join_extensions,
};
use sequence::{eval_sequence, eval_sequence_exists};
use source_iteration::{PositionFrame, WalkExtension, walk};

pub use validate::{ValidationIssue, validate};

#[derive(Debug, Error, PartialEq)]
pub enum EngineError {
    #[error("mapping graph has no node with id {0}")]
    MissingNode(NodeId),
    #[error("cycle detected while evaluating node {0}")]
    Cycle(NodeId),
    #[error("no source field found at path `{0}`")]
    MissingSourceField(String),
    #[error("node {node}: expected a bool, got {found}")]
    NotABool { node: NodeId, found: &'static str },
    #[error("node {node}: expected an item count, got {found}")]
    NotAnItemCount { node: NodeId, found: &'static str },
    #[error("node {node}: group block size must be greater than zero")]
    InvalidBlockSize { node: NodeId },
    #[error("a scope cannot combine multiple grouping modes")]
    ConflictingGroupingModes,
    #[error("node {node}: value-map lookup missed and there's no default")]
    ValueMapMiss { node: NodeId },
    #[error("execution context does not provide {0:?}")]
    MissingRuntimeValue(RuntimeValue),
    #[error("a scope with `filter` but no `source` filtered out its only item")]
    FilteredNonRepeatingScope,
    #[error("node {node}: dynamic target property name must be a string, got {found}")]
    DynamicPropertyName { node: NodeId, found: &'static str },
    #[error("dynamic target object contains duplicate or fixed-colliding property `{0}`")]
    DuplicateDynamicProperty(String),
    #[error("a dynamic object merge can contain only object property fragments")]
    InvalidDynamicPropertyFragment,
    #[error("first-item output requires an iterating scope")]
    FirstOutputWithoutIteration,
    #[error("dynamic object merging requires repeated iteration output")]
    ConflictingIterationOutput,
    #[error("mapped-sequence output cannot populate a computed target property")]
    MappedSequenceDynamicTarget,
    #[error("copy-current-source construction requires a group item, got {found}")]
    CopyCurrentSourceRequiresGroup { found: &'static str },
    #[error("generate-sequence requested {requested} items; maximum is {max}")]
    GeneratedSequenceTooLarge { requested: u128, max: u128 },
    #[error("join {} is not active in the current scope", .join.get())]
    MissingJoinContext { join: JoinId },
    #[error("inner-join iteration cannot be combined with grouping controls")]
    JoinGroupingUnsupported,
    #[error("{function:?} aggregate overflowed the integer range")]
    AggregateIntegerOverflow { function: mapping::AggregateOp },
    #[error("{function:?} aggregate encountered or produced a non-finite number")]
    AggregateNonFinite { function: mapping::AggregateOp },
    #[error(transparent)]
    Function(#[from] functions::FunctionError),
}

/// Runs `project`'s scope tree against `source`, producing one target
/// instance.
pub fn run(project: &Project, source: &Instance) -> Result<Instance, EngineError> {
    run_internal(project, source, Vec::new(), None)
}

/// Host values available to runtime graph nodes.
#[derive(Debug, Clone, Copy)]
pub struct ExecutionContext<'a> {
    mapping_file_path: &'a Path,
    main_mapping_file_path: &'a Path,
    current_datetime: Option<&'a str>,
}

impl<'a> ExecutionContext<'a> {
    /// Uses one path for both the active and top-level mapping.
    pub fn new(mapping_file_path: &'a Path) -> Self {
        Self {
            mapping_file_path,
            main_mapping_file_path: mapping_file_path,
            current_datetime: None,
        }
    }

    /// Distinguishes a reusable mapping's path from its top-level caller.
    pub fn with_main_mapping_file_path(
        mapping_file_path: &'a Path,
        main_mapping_file_path: &'a Path,
    ) -> Self {
        Self {
            mapping_file_path,
            main_mapping_file_path,
            current_datetime: None,
        }
    }

    /// Supplies one stable XML `dateTime` lexical value for the run.
    pub fn with_current_datetime(mut self, current_datetime: &'a str) -> Self {
        self.current_datetime = Some(current_datetime);
        self
    }

    fn value(self, value: RuntimeValue) -> Option<Value> {
        match value {
            RuntimeValue::MappingFilePath => Some(Value::String(
                self.mapping_file_path.to_string_lossy().into_owned(),
            )),
            RuntimeValue::MainMappingFilePath => Some(Value::String(
                self.main_mapping_file_path.to_string_lossy().into_owned(),
            )),
            RuntimeValue::CurrentDateTime => self
                .current_datetime
                .map(|value| Value::String(value.to_string())),
        }
    }
}

/// Like [`run`], with host-provided runtime values.
pub fn run_with_context(
    project: &Project,
    source: &Instance,
    execution: &ExecutionContext<'_>,
) -> Result<Instance, EngineError> {
    run_internal(project, source, Vec::new(), Some(execution))
}

/// Like [`run`], with named secondary sources. They form the outermost
/// context frame, so scope source paths and field paths reach them by name
/// through the usual outward fallback -- while anything the primary source
/// (or an inner scope item) defines still wins.
pub fn run_with_sources(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
) -> Result<Instance, EngineError> {
    run_internal(project, source, extras, None)
}

/// Like [`run_with_sources`], with host-provided runtime values.
pub fn run_with_sources_and_context(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
    execution: &ExecutionContext<'_>,
) -> Result<Instance, EngineError> {
    run_internal(project, source, extras, Some(execution))
}

fn run_internal(
    project: &Project,
    source: &Instance,
    extras: Vec<(String, Instance)>,
    execution: Option<&ExecutionContext<'_>>,
) -> Result<Instance, EngineError> {
    let runtime_frame = Instance::Group(
        execution
            .into_iter()
            .flat_map(|execution| {
                [
                    RuntimeValue::MappingFilePath,
                    RuntimeValue::MainMappingFilePath,
                    RuntimeValue::CurrentDateTime,
                ]
                .into_iter()
                .filter_map(|value| {
                    execution.value(value).map(|instance| {
                        (runtime_field(value).to_string(), Instance::Scalar(instance))
                    })
                })
            })
            .collect(),
    );
    let extras_frame = Instance::Group(extras);
    eval_scope(
        &project.graph,
        &project.root,
        Some(&project.target),
        &[&runtime_frame, &extras_frame, source],
        &[],
    )
}

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

fn eval_scope(
    graph: &Graph,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<Instance, EngineError> {
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
    let mut extensions = if let Some(rows) = &join_rows {
        join_extensions(rows)
    } else if let Some(items) = &sequence_items {
        walk(items, &[], &[], &[], &[])
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
                    // that identity when an empty-path child iterates the
                    // members, so frame-pinned fields select the current
                    // member instead of the group's first member.
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
    if let Some(grouping) = grouping {
        // Key and block groups both become the same grouped frame below.
        let mut groups: Vec<GroupBucket> = Vec::new();
        for extension in &extensions {
            let mut item_context = context.to_vec();
            item_context.extend(extension.instances.iter().copied());
            let mut item_positions = positions.to_vec();
            item_positions.extend(extension.positions.iter().cloned());
            if !passes_filter(graph, scope.filter, &item_context, &item_positions)? {
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
        // immediately before the grouped members. Reverse collection lookup
        // therefore finds this group before same-named outer collections;
        // frame-pinned lookup accounts for wrappers via PositionFrame::grouped.
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
            if let Some(instance) = produce_item(
                graph,
                scope,
                target,
                &next_context,
                &group.positions,
                &output_positions,
                false,
            )? {
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
            if let Some(instance) = produce_item(
                graph,
                scope,
                target,
                &next_context,
                &candidate_positions,
                &output_positions,
                true,
            )? {
                if !joined && !extension.positions.is_empty() {
                    compact_positions.insert(parent_key, next_position);
                }
                produced.push(instance);
            }
        }
    }

    finalize_scope_output(scope, produced)
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

/// Evaluates one iteration item: the filter (`None` when it drops the
/// item), then the scope's bindings and child scopes.
fn produce_item(
    graph: &Graph,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&Instance],
    filter_positions: &[PositionFrame],
    output_positions: &[PositionFrame],
    apply_filter: bool,
) -> Result<Option<Instance>, EngineError> {
    if apply_filter && !passes_filter(graph, scope.filter, context, filter_positions)? {
        return Ok(None);
    }

    if scope.construction == ScopeConstruction::CopyCurrentSource {
        return match context.last().copied() {
            Some(current @ Instance::Group(_)) => Ok(Some((*current).clone())),
            Some(Instance::Scalar(_)) => {
                Err(EngineError::CopyCurrentSourceRequiresGroup { found: "scalar" })
            }
            Some(Instance::Repeated(_)) => Err(EngineError::CopyCurrentSourceRequiresGroup {
                found: "repeated collection",
            }),
            Some(Instance::MappedSequence(_)) => Err(EngineError::CopyCurrentSourceRequiresGroup {
                found: "mapped sequence",
            }),
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
        let value = match target.and_then(|schema| schema.child(&binding.target_field)) {
            Some(field) if field.repeating => match value {
                Value::Null => Instance::Repeated(Vec::new()),
                value => Instance::Repeated(vec![Instance::Scalar(value)]),
            },
            _ => Instance::Scalar(value),
        };
        insert_target_field(&mut fields, binding.target_field.clone(), value)?;
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
        let child_instance = eval_scope(graph, child, child_target, context, output_positions)?;
        insert_target_field(&mut fields, child.target_field.clone(), child_instance)?;
    }
    for child in &scope.dynamic_children {
        if child.scope.iteration_output == IterationOutput::MappedSequence {
            return Err(EngineError::MappedSequenceDynamicTarget);
        }
        let key = eval_dynamic_key(graph, child.key, context, output_positions)?;
        let child_target = target.and_then(ir::SchemaNode::dynamic_fields);
        let child_instance =
            eval_scope(graph, &child.scope, child_target, context, output_positions)?;
        dynamic_target::insert_dynamic_target_field(&mut fields, key, child_instance, target)?;
    }
    Ok(Some(Instance::Group(fields)))
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

fn eval_expr(
    graph: &Graph,
    node_id: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    if !in_progress.insert(node_id) {
        return Err(EngineError::Cycle(node_id));
    }

    let node = graph
        .nodes
        .get(&node_id)
        .ok_or(EngineError::MissingNode(node_id))?;

    let result = match node {
        Node::SourceField { path, frame } => {
            let value = match frame {
                Some(frame) => resolve_scalar_in_frame(context, positions, frame, path),
                None => resolve_scalar(context, path),
            };
            value.ok_or_else(|| {
                let mut display = frame.clone().unwrap_or_default();
                display.extend(path.iter().cloned());
                EngineError::MissingSourceField(display.join("/"))
            })
        }
        Node::Position { collection } => Ok(Value::Int(position(positions, collection) as i64)),
        Node::JoinField {
            join,
            collection,
            path,
        } => resolve_join_scalar(context, positions, *join, collection, path).ok_or_else(|| {
            EngineError::MissingSourceField(format!(
                "join {}:{}/{}",
                join.get(),
                collection.join("/"),
                path.join("/")
            ))
        }),
        Node::JoinPosition { join } => positions
            .iter()
            .rev()
            .find_map(|position| {
                position
                    .join_position
                    .filter(|(owner, _)| owner == join)
                    .map(|(_, index)| Value::Int(index as i64))
            })
            .ok_or(EngineError::MissingJoinContext { join: *join }),
        Node::Const { value } => Ok(value.clone()),
        Node::RuntimeValue { value } => context
            .first()
            .and_then(|frame| frame.field(runtime_field(*value)))
            .and_then(Instance::as_scalar)
            .cloned()
            .ok_or(EngineError::MissingRuntimeValue(*value)),
        Node::Call { function, args } => {
            let mut values = Vec::with_capacity(args.len());
            for arg in args {
                values.push(eval_expr(graph, *arg, context, positions, in_progress)?);
            }
            functions::call(function, &values).map_err(EngineError::from)
        }
        Node::If {
            condition,
            then,
            else_,
        } => match eval_expr(graph, *condition, context, positions, in_progress)? {
            Value::Bool(true) => eval_expr(graph, *then, context, positions, in_progress),
            Value::Bool(false) => eval_expr(graph, *else_, context, positions, in_progress),
            other => Err(EngineError::NotABool {
                node: *condition,
                found: other.type_name(),
            }),
        },
        Node::ValueMap {
            input,
            input_type,
            table,
            default,
        } => {
            let value = eval_expr(graph, *input, context, positions, in_progress)?;
            let value = input_type
                .and_then(|ty| coerce_value_map_input(&value, ty))
                .unwrap_or(value);
            table
                .iter()
                .find(|(from, _)| *from == value)
                .map(|(_, to)| to.clone())
                .or_else(|| default.clone())
                .ok_or(EngineError::ValueMapMiss { node: node_id })
        }
        Node::Lookup {
            collection,
            key,
            matches,
            value,
        } => {
            let needle = eval_expr(graph, *matches, context, positions, in_progress)?;
            let items = resolve_repeated(context, collection)
                .ok_or_else(|| EngineError::MissingSourceField(collection.join("/")))?;
            Ok(items
                .iter()
                .find(|item| field_scalar(item, key).is_some_and(|k| *k == needle))
                .and_then(|item| field_scalar(item, value).cloned())
                .unwrap_or(Value::Null))
        }
        Node::SequenceExists {
            sequence,
            predicate,
        } => eval_sequence_exists(graph, sequence, *predicate, context, positions, in_progress),
        Node::Aggregate {
            function,
            collection,
            value,
            expression,
            arg,
        } => {
            // Absent repeating data aggregates as an empty collection.
            let items = resolve_repeated(context, collection).unwrap_or(&[]);
            let mut values = Vec::with_capacity(items.len());
            for (item_index, item) in items.iter().enumerate() {
                let item_value = if let Some(expression) = expression {
                    let mut item_context = context.to_vec();
                    item_context.push(item);
                    let mut item_positions = positions.to_vec();
                    item_positions.push(PositionFrame {
                        collection: collection.clone(),
                        index: item_index + 1,
                        grouped: false,
                        join: None,
                        join_position: None,
                    });
                    eval_expr(
                        graph,
                        *expression,
                        &item_context,
                        &item_positions,
                        in_progress,
                    )?
                } else if value.is_empty() {
                    item.as_scalar().cloned().unwrap_or(Value::Null)
                } else {
                    field_scalar(item, value).cloned().unwrap_or(Value::Null)
                };
                values.push(item_value);
            }
            let arg_value = match arg {
                Some(id) => Some(eval_expr(graph, *id, context, positions, in_progress)?),
                None => None,
            };
            aggregate(*function, items.len(), &values, arg_value)
        }
        Node::JoinAggregate {
            function,
            join,
            plan,
            expression,
            arg,
        } => eval_join_aggregate(
            graph,
            JoinAggregateInput {
                function: *function,
                join: *join,
                plan,
                expression: *expression,
                arg: *arg,
            },
            context,
            positions,
            in_progress,
        ),
    };

    in_progress.remove(&node_id);
    result
}

fn coerce_value_map_input(value: &Value, ty: ScalarType) -> Option<Value> {
    match (ty, value) {
        (_, Value::Null) => Some(Value::Null),
        (_, Value::XmlNil(value)) => Some(Value::XmlNil(*value)),
        (ScalarType::String, Value::String(value)) => Some(Value::String(value.clone())),
        (ScalarType::String, Value::Bool(value)) => Some(Value::String(value.to_string())),
        (ScalarType::String, Value::Int(value)) => Some(Value::String(value.to_string())),
        (ScalarType::String, Value::Float(value)) if value.is_finite() => {
            Some(Value::String(value.to_string()))
        }
        (ScalarType::String, Value::Float(_)) => None,
        (ScalarType::Int, Value::Int(value)) => Some(Value::Int(*value)),
        (ScalarType::Int, Value::Float(value))
            if value.is_finite()
                && value.fract() == 0.0
                && *value >= i64::MIN as f64
                && *value < -(i64::MIN as f64) =>
        {
            Some(Value::Int(*value as i64))
        }
        (ScalarType::Int, Value::String(value)) => value.trim().parse().ok().map(Value::Int),
        (ScalarType::Float, Value::Float(value)) if value.is_finite() => Some(Value::Float(*value)),
        (ScalarType::Float, Value::Int(value)) => Some(Value::Float(*value as f64)),
        (ScalarType::Float, Value::String(value)) => value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Value::Float),
        (ScalarType::Bool, Value::Bool(value)) => Some(Value::Bool(*value)),
        (ScalarType::Bool, Value::String(value)) => match value.trim() {
            "true" | "1" => Some(Value::Bool(true)),
            "false" | "0" => Some(Value::Bool(false)),
            _ => None,
        },
        (ScalarType::Int | ScalarType::Float | ScalarType::Bool, _) => None,
    }
}

fn position(positions: &[PositionFrame], collection: &[String]) -> usize {
    positions
        .iter()
        .rev()
        .find(|position| {
            collection.is_empty()
                || position.collection.len() >= collection.len()
                    && position.collection[position.collection.len() - collection.len()..]
                        == *collection
        })
        .map(|position| position.index)
        .unwrap_or(1)
}

/// Resolves `path` to a repeating collection, with the same outward
/// fallback as [`resolve_scalar`].
/// Applies one [`AggregateOp`] over the per-item `values` of a collection
/// (`item_count` counts items, not non-null values).
fn aggregate(
    function: mapping::AggregateOp,
    item_count: usize,
    values: &[Value],
    arg: Option<Value>,
) -> Result<Value, EngineError> {
    use mapping::AggregateOp;
    match function {
        AggregateOp::Count => Ok(Value::Int(item_count as i64)),
        AggregateOp::Sum => {
            let numbers = numeric_values(function, values)?;
            if numbers.iter().all(|number| number.is_int()) {
                numbers
                    .iter()
                    .try_fold(0_i64, |sum, number| {
                        let NumericValue::Int(value) = number else {
                            return Ok(sum);
                        };
                        sum.checked_add(*value)
                            .ok_or(EngineError::AggregateIntegerOverflow { function })
                    })
                    .map(Value::Int)
            } else {
                finite_float(function, compensated_sum(&numbers)?).map(Value::Float)
            }
        }
        AggregateOp::Avg => {
            let numbers = numeric_values(function, values)?;
            if numbers.is_empty() {
                return Ok(Value::Null);
            }
            finite_float(function, incremental_average(&numbers)?).map(Value::Float)
        }
        AggregateOp::Min | AggregateOp::Max => {
            let numbers = numeric_values(function, values)?;
            let want = if function == AggregateOp::Min {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
            let mut best: Option<NumericValue> = None;
            for value in numbers {
                match best {
                    None => best = Some(value),
                    Some(current) => {
                        if value.cmp(current) == want {
                            best = Some(value);
                        }
                    }
                }
            }
            Ok(best.map_or(Value::Null, NumericValue::into_value))
        }
        AggregateOp::Join => {
            let separator = arg.map(|v| value_text(&v)).unwrap_or_default();
            Ok(Value::String(
                values
                    .iter()
                    .filter(|v| !matches!(v, Value::Null))
                    .map(value_text)
                    .collect::<Vec<_>>()
                    .join(&separator),
            ))
        }
        AggregateOp::ItemAt => {
            // 1-based, XPath style; anything out of range is Null.
            let index = arg.as_ref().and_then(|v| match v {
                Value::Int(i) => Some(*i),
                Value::Float(f) => Some(f.round() as i64),
                Value::String(s) => s.trim().parse().ok(),
                _ => None,
            });
            Ok(match index {
                Some(i) if i >= 1 => values.get(i as usize - 1).cloned().unwrap_or(Value::Null),
                _ => Value::Null,
            })
        }
    }
}

#[derive(Clone, Copy)]
enum NumericValue {
    Int(i64),
    Float(f64),
}

impl NumericValue {
    fn is_int(self) -> bool {
        matches!(self, Self::Int(_))
    }

    fn as_float(self) -> f64 {
        match self {
            Self::Int(value) => value as f64,
            Self::Float(value) => value,
        }
    }

    fn into_value(self) -> Value {
        match self {
            Self::Int(value) => Value::Int(value),
            Self::Float(value) => Value::Float(value),
        }
    }

    fn cmp(self, other: Self) -> std::cmp::Ordering {
        match (self, other) {
            (Self::Int(left), Self::Int(right)) => left.cmp(&right),
            (Self::Float(left), Self::Float(right)) => left
                .partial_cmp(&right)
                .unwrap_or(std::cmp::Ordering::Equal),
            (Self::Int(left), Self::Float(right)) => compare_int_float(left, right),
            (Self::Float(left), Self::Int(right)) => compare_int_float(right, left).reverse(),
        }
    }
}

/// Parses numeric values without routing integers through `f64`. Strings from
/// untyped sources parse; everything else is omitted from numeric reductions.
fn numeric_value(value: &Value) -> Result<Option<NumericValue>, ()> {
    match value {
        Value::Int(value) => Ok(Some(NumericValue::Int(*value))),
        Value::Float(value) if value.is_finite() => Ok(Some(NumericValue::Float(*value))),
        Value::Float(_) => Err(()),
        Value::String(s) => {
            let s = s.trim();
            if let Ok(value) = s.parse::<i64>() {
                return Ok(Some(NumericValue::Int(value)));
            }
            match s.parse::<f64>() {
                Ok(value) if value.is_finite() => Ok(Some(NumericValue::Float(value))),
                Ok(_) => Err(()),
                Err(_) => Ok(None),
            }
        }
        _ => Ok(None),
    }
}

fn numeric_values(
    function: mapping::AggregateOp,
    values: &[Value],
) -> Result<Vec<NumericValue>, EngineError> {
    values
        .iter()
        .filter_map(|value| numeric_value(value).transpose())
        .collect::<Result<_, _>>()
        .map_err(|()| EngineError::AggregateNonFinite { function })
}

fn finite_float(function: mapping::AggregateOp, value: f64) -> Result<f64, EngineError> {
    if value.is_finite() {
        Ok(value)
    } else {
        Err(EngineError::AggregateNonFinite { function })
    }
}

fn compensated_sum(values: &[NumericValue]) -> Result<f64, EngineError> {
    let scale = values
        .iter()
        .map(|value| value.as_float().abs())
        .fold(0.0_f64, f64::max);
    if scale == 0.0 {
        return Ok(0.0);
    }

    let mut sum = 0.0;
    let mut correction = 0.0;
    for value in values {
        let value = value.as_float() / scale;
        let next = finite_float(mapping::AggregateOp::Sum, sum + value)?;
        correction += if sum.abs() >= value.abs() {
            (sum - next) + value
        } else {
            (value - next) + sum
        };
        correction = finite_float(mapping::AggregateOp::Sum, correction)?;
        sum = next;
    }
    let normalized = finite_float(mapping::AggregateOp::Sum, sum + correction)?;
    finite_float(mapping::AggregateOp::Sum, normalized * scale)
}

fn incremental_average(values: &[NumericValue]) -> Result<f64, EngineError> {
    let mut average = 0.0;
    for (index, value) in values.iter().enumerate() {
        let count = (index + 1) as f64;
        let retained_weight = index as f64 / count;
        average = finite_float(
            mapping::AggregateOp::Avg,
            average * retained_weight + value.as_float() / count,
        )?;
    }
    Ok(average)
}

/// Compares an integer and a finite float without rounding the integer first.
fn compare_int_float(integer: i64, float: f64) -> std::cmp::Ordering {
    if float >= i64::MAX as f64 {
        return std::cmp::Ordering::Less;
    }
    if float < i64::MIN as f64 {
        return std::cmp::Ordering::Greater;
    }

    let truncated = float.trunc() as i64;
    match integer.cmp(&truncated) {
        std::cmp::Ordering::Equal if float.fract().is_sign_positive() && float.fract() != 0.0 => {
            std::cmp::Ordering::Less
        }
        std::cmp::Ordering::Equal if float.fract().is_sign_negative() && float.fract() != 0.0 => {
            std::cmp::Ordering::Greater
        }
        ordering => ordering,
    }
}

fn value_ordering(left: &Value, right: &Value) -> Option<std::cmp::Ordering> {
    match (left, right) {
        (Value::Null, Value::Null) => Some(std::cmp::Ordering::Equal),
        (Value::Null, _) => Some(std::cmp::Ordering::Less),
        (_, Value::Null) => Some(std::cmp::Ordering::Greater),
        (Value::Int(left), Value::Int(right)) => Some(left.cmp(right)),
        (Value::Float(left), Value::Float(right)) => left.partial_cmp(right),
        (Value::Int(left), Value::Float(right)) if right.is_finite() => {
            Some(compare_int_float(*left, *right))
        }
        (Value::Float(left), Value::Int(right)) if left.is_finite() => {
            Some(compare_int_float(*right, *left).reverse())
        }
        (Value::String(left), Value::String(right)) => Some(left.cmp(right)),
        (Value::Bool(left), Value::Bool(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

fn value_text(value: &Value) -> String {
    match value {
        Value::Null | Value::XmlNil(_) => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::String(s) => s.clone(),
    }
}

fn resolve_repeated<'a>(context: &[&'a Instance], path: &[String]) -> Option<&'a [Instance]> {
    for item in context.iter().rev() {
        let mut current = *item;
        let mut found = true;
        for segment in path {
            match current.field(segment) {
                Some(next) => current = next,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if found && let Some(items) = current.as_repeated() {
            return Some(items);
        }
    }
    None
}

/// Follows a plain field path inside one instance (no fallback).
fn field_scalar<'a>(item: &'a Instance, path: &[String]) -> Option<&'a Value> {
    let mut current = item;
    for segment in path {
        current = current.field(segment)?;
    }
    current.as_scalar()
}

/// Resolves `path` against the innermost context item, falling back to
/// enclosing items if not found there (nearest enclosing wins). Crossing a
/// repeating element no scope iterates reads its first item -- the visual-
/// mapper convention for wiring a repeating source into a singular target.
fn resolve_scalar(context: &[&Instance], path: &[String]) -> Option<Value> {
    for item in context.iter().rev() {
        let mut current = *item;
        let mut found = true;
        for segment in path {
            if let Instance::Repeated(items) = current {
                match items.first() {
                    Some(first) => current = first,
                    None => {
                        found = false;
                        break;
                    }
                }
            }
            match current.field(segment) {
                Some(next) => current = next,
                None => {
                    found = false;
                    break;
                }
            }
        }
        if !found {
            continue;
        }
        if let Instance::Repeated(items) = current {
            match items.first() {
                Some(first) => current = first,
                None => continue,
            }
        }
        if let Some(value) = current.as_scalar() {
            return Some(value.clone());
        }
    }
    None
}

fn resolve_scalar_in_frame(
    context: &[&Instance],
    positions: &[PositionFrame],
    frame: &[String],
    path: &[String],
) -> Option<Value> {
    let position_index = positions.iter().rposition(|position| {
        position.collection == frame
            || !position.collection.is_empty() && frame.ends_with(position.collection.as_slice())
    })?;
    let instance = context_for_position(context, positions, position_index)?;
    resolve_scalar(&[instance], path)
}

fn resolve_join_scalar(
    context: &[&Instance],
    positions: &[PositionFrame],
    join: JoinId,
    collection: &[String],
    path: &[String],
) -> Option<Value> {
    let position_index = positions
        .iter()
        .rposition(|position| position.join == Some(join) && position.collection == collection)?;
    let instance = context_for_position(context, positions, position_index)?;
    field_scalar(instance, path).cloned()
}

fn context_for_position<'a>(
    context: &[&'a Instance],
    positions: &[PositionFrame],
    position_index: usize,
) -> Option<&'a Instance> {
    let wrapper_count = positions.iter().filter(|position| position.grouped).count();
    let context_offset = context.len().checked_sub(positions.len() + wrapper_count)?;
    let preceding_wrappers = positions[..=position_index]
        .iter()
        .filter(|position| position.grouped)
        .count();
    context
        .get(context_offset + position_index + preceding_wrappers)
        .copied()
}

#[cfg(test)]
mod aggregate_tests;
#[cfg(test)]
mod collection_tests;
#[cfg(test)]
mod core_tests;
#[cfg(test)]
mod dynamic_target_tests;
#[cfg(test)]
mod group_blocks_tests;
#[cfg(test)]
mod group_starting_tests;
#[cfg(test)]
mod iteration_output_tests;
#[cfg(test)]
mod join_tests;
#[cfg(test)]
mod sequence_exists_tests;
