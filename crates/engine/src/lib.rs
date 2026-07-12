//! Interprets a mapping graph against a source instance to produce a target
//! instance.

use std::collections::{BTreeMap, HashSet};

use ir::{Instance, Value};
use mapping::{Graph, Node, NodeId, Project, Scope, SequenceExpr};
use thiserror::Error;

mod dynamic_target;
mod validate;

use dynamic_target::{eval_dynamic_key, insert_target_field, merge_dynamic_fragments};

pub use validate::{ValidationIssue, validate};

const MAX_GENERATED_SEQUENCE_ITEMS: u128 = 1_000_000;

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
    #[error("a scope cannot combine group-by with group-into-blocks")]
    ConflictingGroupingModes,
    #[error("node {node}: value-map lookup missed and there's no default")]
    ValueMapMiss { node: NodeId },
    #[error("a scope with `filter` but no `source` filtered out its only item")]
    FilteredNonRepeatingScope,
    #[error("node {node}: dynamic target property name must be a string, got {found}")]
    DynamicPropertyName { node: NodeId, found: &'static str },
    #[error("dynamic target object contains duplicate or fixed-colliding property `{0}`")]
    DuplicateDynamicProperty(String),
    #[error("a dynamic object merge can contain only object property fragments")]
    InvalidDynamicPropertyFragment,
    #[error("generate-sequence requested {requested} items; maximum is {max}")]
    GeneratedSequenceTooLarge { requested: u128, max: u128 },
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
    run_with_sources(project, source, Vec::new())
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
    let extras_frame = Instance::Group(extras);
    eval_scope(
        &project.graph,
        &project.root,
        Some(&project.target),
        &[&extras_frame, source],
        &[],
    )
}

#[derive(Clone)]
struct PositionFrame {
    collection: Vec<String>,
    index: usize,
    /// The matching context instance has a synthetic named collection
    /// wrapper immediately before it.
    grouped: bool,
}

struct WalkExtension<'a> {
    instances: Vec<&'a Instance>,
    positions: Vec<PositionFrame>,
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

#[derive(Clone, Copy)]
enum GroupingMode {
    By(NodeId),
    IntoBlocks(usize),
}

fn eval_scope(
    graph: &Graph,
    scope: &Scope,
    target: Option<&ir::SchemaNode>,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<Instance, EngineError> {
    let sequence_items = scope
        .sequence
        .as_ref()
        .map(|sequence| eval_sequence(graph, sequence, context, positions))
        .transpose()?
        .map(|values| {
            Instance::Repeated(values.into_iter().map(Instance::Scalar).collect::<Vec<_>>())
        });
    let mut extensions = if let Some(items) = &sequence_items {
        walk(items, &[], &[], &[], &[])
    } else {
        match &scope.source {
            None => vec![WalkExtension {
                instances: vec![*context.last().expect("context is never empty")],
                positions: Vec::new(),
            }],
            // The frame to iterate from is the innermost one that has the
            // path's first field -- so a nested scope can still iterate an
            // extra source (outermost frame) by name.
            Some(path) => {
                let base = context
                    .iter()
                    .rev()
                    .find(|frame| match path.first() {
                        Some(first) => frame.field(first).is_some(),
                        None => true,
                    })
                    .copied()
                    .unwrap_or_else(|| *context.last().expect("context is never empty"));
                walk(base, path, &[], &[], &[])
            }
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
                if let Some(position) = extension.positions.last_mut() {
                    position.index = index + 1;
                }
                extension
            })
            .collect();
    }

    let take = scope
        .take
        .map(|node| eval_item_count(graph, node, context, positions))
        .transpose()?;
    let grouping = match (scope.group_by, scope.group_into_blocks) {
        (Some(_), Some(_)) => return Err(EngineError::ConflictingGroupingModes),
        (Some(node), None) => Some(GroupingMode::By(node)),
        (None, Some(node)) => Some(GroupingMode::IntoBlocks(eval_block_size(
            graph, node, context, positions,
        )?)),
        (None, None) => None,
    };
    let mut produced = Vec::with_capacity(take.unwrap_or(extensions.len()).min(extensions.len()));
    if let Some(grouping) = grouping {
        // Partition key groups in first-seen order and block groups in
        // contiguous source order. Both become the same grouped frame below.
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
                GroupingMode::IntoBlocks(_) => None,
            };
            let existing = match grouping {
                GroupingMode::By(_) => groups.iter_mut().find(|group| group.key == key),
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
                    .source
                    .as_ref()
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
            let next_position = compact_positions.get(&parent_key).copied().unwrap_or(0) + 1;
            let mut output_positions = candidate_positions.clone();
            if !extension.positions.is_empty()
                && let Some(position) = output_positions.last_mut()
            {
                position.index = next_position;
            }
            if let Some(instance) = produce_item(
                graph,
                scope,
                target,
                &next_context,
                &candidate_positions,
                &output_positions,
                true,
            )? {
                if !extension.positions.is_empty() {
                    compact_positions.insert(parent_key, next_position);
                }
                produced.push(instance);
            }
        }
    }

    if scope.source.is_some() || scope.sequence.is_some() {
        if scope.merge_dynamic_fields {
            merge_dynamic_fragments(produced)
        } else {
            Ok(Instance::Repeated(produced))
        }
    } else {
        produced
            .into_iter()
            .next()
            .ok_or(EngineError::FilteredNonRepeatingScope)
    }
}

fn eval_sequence(
    graph: &Graph,
    sequence: &SequenceExpr,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<Vec<Value>, EngineError> {
    match sequence {
        SequenceExpr::Tokenize {
            input, delimiter, ..
        } => {
            let Some(input) = eval_sequence_arg(graph, *input, context, positions)? else {
                return Ok(Vec::new());
            };
            let Some(delimiter) = eval_sequence_arg(graph, *delimiter, context, positions)? else {
                return Ok(Vec::new());
            };
            tokenize(input, delimiter)
        }
        SequenceExpr::TokenizeByLength { input, length, .. } => {
            let Some(input) = eval_sequence_arg(graph, *input, context, positions)? else {
                return Ok(Vec::new());
            };
            let Some(length) = eval_sequence_arg(graph, *length, context, positions)? else {
                return Ok(Vec::new());
            };
            tokenize_by_length(input, length)
        }
        SequenceExpr::Generate { from, to, .. } => {
            let from = match from {
                Some(node) => {
                    let Some(value) = eval_sequence_arg(graph, *node, context, positions)? else {
                        return Ok(Vec::new());
                    };
                    Some(value)
                }
                None => None,
            };
            let Some(to) = eval_sequence_arg(graph, *to, context, positions)? else {
                return Ok(Vec::new());
            };
            generate_sequence(from, to)
        }
    }
}

fn eval_sequence_arg(
    graph: &Graph,
    node: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<Option<Value>, EngineError> {
    let mut in_progress = HashSet::new();
    let value = eval_expr(graph, node, context, positions, &mut in_progress)?;
    Ok((value != Value::Null).then_some(value))
}

fn generate_sequence(from: Option<Value>, to: Value) -> Result<Vec<Value>, EngineError> {
    let from = from.map_or(Ok(1), |value| sequence_integer(value, "generate-sequence"))?;
    let to = sequence_integer(to, "generate-sequence")?;
    if from > to {
        return Ok(Vec::new());
    }
    let requested = (i128::from(to) - i128::from(from) + 1) as u128;
    if requested > MAX_GENERATED_SEQUENCE_ITEMS {
        return Err(EngineError::GeneratedSequenceTooLarge {
            requested,
            max: MAX_GENERATED_SEQUENCE_ITEMS,
        });
    }
    let mut values = Vec::with_capacity(requested as usize);
    values.extend((from..=to).map(Value::Int));
    Ok(values)
}

fn sequence_integer(value: Value, function: &'static str) -> Result<i64, EngineError> {
    match value {
        Value::Int(value) => Ok(value),
        other => Err(functions::FunctionError::TypeMismatch {
            function,
            got: other.type_name(),
        }
        .into()),
    }
}

fn tokenize(input: Value, delimiter: Value) -> Result<Vec<Value>, EngineError> {
    let input = sequence_string(input, "tokenize")?;
    let delimiter = sequence_string(delimiter, "tokenize")?;
    if delimiter.is_empty() {
        return Err(functions::FunctionError::InvalidArgument {
            function: "tokenize",
            message: "requires a non-empty delimiter",
        }
        .into());
    }
    Ok(input
        .split(&delimiter)
        .map(|value| Value::String(value.to_string()))
        .collect())
}

fn tokenize_by_length(input: Value, length: Value) -> Result<Vec<Value>, EngineError> {
    let input = sequence_string(input, "tokenize-by-length")?;
    let length = match length {
        Value::Int(value) => Some(value),
        Value::Float(value) if value.is_finite() => Some(value.trunc() as i64),
        Value::String(value) => value.trim().parse().ok(),
        _ => None,
    }
    .filter(|length| *length > 0)
    .ok_or(functions::FunctionError::InvalidArgument {
        function: "tokenize-by-length",
        message: "requires a positive integer length",
    })? as usize;

    let chars: Vec<char> = input.chars().collect();
    Ok(chars
        .chunks(length)
        .map(|chunk| Value::String(chunk.iter().collect()))
        .collect())
}

fn sequence_string(value: Value, function: &'static str) -> Result<String, EngineError> {
    match value {
        Value::String(value) => Ok(value),
        other => Err(functions::FunctionError::TypeMismatch {
            function,
            got: other.type_name(),
        }
        .into()),
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
        insert_target_field(
            &mut fields,
            binding.target_field.clone(),
            Instance::Scalar(value),
        )?;
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

/// Walks `path` from `base`, branching (and pushing one context frame) each
/// time it crosses a repeating element -- whether mid-path or, if `path` is
/// exhausted and the final value is itself repeating (e.g. `path` is empty
/// and `base` is a CSV file's rows), at the very end. Returns one extension
/// (the new frames to push, innermost last) per produced item. Repeating
/// frames also retain their collection path and 1-based source position.
fn walk<'a>(
    base: &'a Instance,
    path: &[String],
    prefix: &[String],
    acc: &[&'a Instance],
    positions: &[PositionFrame],
) -> Vec<WalkExtension<'a>> {
    match path.split_first() {
        None => match base {
            Instance::Repeated(items) => items
                .iter()
                .enumerate()
                .map(|(index, item)| {
                    let mut next_instances = acc.to_vec();
                    next_instances.push(item);
                    let mut next_positions = positions.to_vec();
                    next_positions.push(PositionFrame {
                        collection: prefix.to_vec(),
                        index: index + 1,
                        grouped: false,
                    });
                    WalkExtension {
                        instances: next_instances,
                        positions: next_positions,
                    }
                })
                .collect(),
            _ => {
                let mut next_instances = acc.to_vec();
                next_instances.push(base);
                vec![WalkExtension {
                    instances: next_instances,
                    positions: positions.to_vec(),
                }]
            }
        },
        Some((segment, rest)) => {
            let mut collection_path = prefix.to_vec();
            collection_path.push(segment.clone());
            match base.field(segment) {
                None => Vec::new(),
                Some(Instance::Repeated(items)) => items
                    .iter()
                    .enumerate()
                    .flat_map(|(index, item)| {
                        let mut next_instances = acc.to_vec();
                        next_instances.push(item);
                        let mut next_positions = positions.to_vec();
                        next_positions.push(PositionFrame {
                            collection: collection_path.clone(),
                            index: index + 1,
                            grouped: false,
                        });
                        if rest.is_empty() {
                            vec![WalkExtension {
                                instances: next_instances,
                                positions: next_positions,
                            }]
                        } else {
                            walk(
                                item,
                                rest,
                                &collection_path,
                                &next_instances,
                                &next_positions,
                            )
                        }
                    })
                    .collect(),
                Some(other) => walk(other, rest, &collection_path, acc, positions),
            }
        }
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
        Node::Const { value } => Ok(value.clone()),
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
            table,
            default,
        } => {
            let value = eval_expr(graph, *input, context, positions, in_progress)?;
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
        Node::Aggregate {
            function,
            collection,
            value,
            expression,
            arg,
        } => {
            // An unresolvable collection aggregates as empty rather than
            // erroring -- absent repeating data is normal instance data.
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
    };

    in_progress.remove(&node_id);
    result
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
        Value::Null => String::new(),
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
    let wrapper_count = positions.iter().filter(|position| position.grouped).count();
    let context_offset = context.len().checked_sub(positions.len() + wrapper_count)?;
    let preceding_wrappers = positions[..=position_index]
        .iter()
        .filter(|position| position.grouped)
        .count();
    let instance = *context.get(context_offset + position_index + preceding_wrappers)?;
    resolve_scalar(&[instance], path)
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
