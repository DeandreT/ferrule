//! Interprets a mapping graph against a source instance to produce a target
//! instance.

use std::collections::{BTreeMap, HashSet};

use ir::{Instance, Value};
use mapping::{Graph, Node, NodeId, Project, Scope};
use thiserror::Error;

mod validate;

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
    #[error("node {node}: value-map lookup missed and there's no default")]
    ValueMapMiss { node: NodeId },
    #[error("a scope with `filter` but no `source` filtered out its only item")]
    FilteredNonRepeatingScope,
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
    eval_scope(&project.graph, &project.root, &[&extras_frame, source], &[])
}

#[derive(Clone)]
struct PositionFrame {
    collection: Vec<String>,
    index: usize,
}

struct WalkExtension<'a> {
    instances: Vec<&'a Instance>,
    positions: Vec<PositionFrame>,
}

fn eval_scope(
    graph: &Graph,
    scope: &Scope,
    context: &[&Instance],
    positions: &[PositionFrame],
) -> Result<Instance, EngineError> {
    let mut extensions = match &scope.source {
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
    let mut produced = Vec::with_capacity(take.unwrap_or(extensions.len()).min(extensions.len()));
    if let (Some(key_node), Some(path)) = (scope.group_by, &scope.source) {
        // Partition the iterated items by their key, in first-seen order.
        let mut groups: Vec<(Value, Vec<Instance>, Vec<PositionFrame>)> = Vec::new();
        for extension in &extensions {
            let mut item_context = context.to_vec();
            item_context.extend(extension.instances.iter().copied());
            let mut item_positions = positions.to_vec();
            item_positions.extend(extension.positions.iter().cloned());
            let mut in_progress = HashSet::new();
            let key = eval_expr(
                graph,
                key_node,
                &item_context,
                &item_positions,
                &mut in_progress,
            )?;
            let member = (*extension
                .instances
                .last()
                .expect("extensions are never empty"))
            .clone();
            match groups.iter_mut().find(|(k, _, _)| *k == key) {
                Some((_, members, _)) => members.push(member),
                None => groups.push((key, vec![member], item_positions)),
            }
        }
        // Each group's context: a wrapper naming the members after the
        // collection's last segment (so collection paths shadow the
        // ungrouped data) plus the members themselves (bindings read the
        // first member, aggregates over `[]` reduce the members).
        let owned: Vec<(Option<Instance>, Instance, Vec<PositionFrame>)> = groups
            .into_iter()
            .map(|(_, members, positions)| {
                let repeated = Instance::Repeated(members);
                let wrapper = path
                    .last()
                    .map(|segment| Instance::Group(vec![(segment.clone(), repeated.clone())]));
                (wrapper, repeated, positions)
            })
            .collect();
        for (wrapper, members, candidate_positions) in &owned {
            if take.is_some_and(|limit| produced.len() >= limit) {
                break;
            }
            let mut next_context = context.to_vec();
            if let Some(wrapper) = wrapper {
                next_context.push(wrapper);
            }
            next_context.push(members);
            let mut output_positions = candidate_positions.clone();
            if let Some(position) = output_positions.last_mut() {
                position.index = produced.len() + 1;
            }
            if let Some(instance) = produce_item(
                graph,
                scope,
                &next_context,
                candidate_positions,
                &output_positions,
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
                &next_context,
                &candidate_positions,
                &output_positions,
            )? {
                if !extension.positions.is_empty() {
                    compact_positions.insert(parent_key, next_position);
                }
                produced.push(instance);
            }
        }
    }

    if scope.source.is_some() {
        Ok(Instance::Repeated(produced))
    } else {
        produced
            .into_iter()
            .next()
            .ok_or(EngineError::FilteredNonRepeatingScope)
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

/// Evaluates one iteration item: the filter (`None` when it drops the
/// item), then the scope's bindings and child scopes.
fn produce_item(
    graph: &Graph,
    scope: &Scope,
    context: &[&Instance],
    filter_positions: &[PositionFrame],
    output_positions: &[PositionFrame],
) -> Result<Option<Instance>, EngineError> {
    if let Some(filter_node) = scope.filter {
        let mut in_progress = HashSet::new();
        match eval_expr(
            graph,
            filter_node,
            context,
            filter_positions,
            &mut in_progress,
        )? {
            Value::Bool(true) => {}
            Value::Bool(false) => return Ok(None),
            other => {
                return Err(EngineError::NotABool {
                    node: filter_node,
                    found: other.type_name(),
                });
            }
        }
    }

    let mut fields = Vec::with_capacity(scope.bindings.len() + scope.children.len());
    for binding in &scope.bindings {
        let mut in_progress = HashSet::new();
        let value = eval_expr(
            graph,
            binding.node,
            context,
            output_positions,
            &mut in_progress,
        )?;
        fields.push((binding.target_field.clone(), Instance::Scalar(value)));
    }
    for child in &scope.children {
        let child_instance = eval_scope(graph, child, context, output_positions)?;
        fields.push((child.target_field.clone(), child_instance));
    }
    Ok(Some(Instance::Group(fields)))
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
            Ok(aggregate(*function, items.len(), &values, arg_value))
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
) -> Value {
    use mapping::AggregateOp;
    match function {
        AggregateOp::Count => Value::Int(item_count as i64),
        AggregateOp::Sum | AggregateOp::Avg => {
            let numbers: Vec<(f64, bool)> = values.iter().filter_map(numeric_value).collect();
            if function == AggregateOp::Avg {
                if numbers.is_empty() {
                    return Value::Null;
                }
                let sum: f64 = numbers.iter().map(|(f, _)| f).sum();
                return Value::Float(sum / numbers.len() as f64);
            }
            let sum: f64 = numbers.iter().map(|(f, _)| f).sum();
            if numbers.iter().all(|(_, is_int)| *is_int) {
                Value::Int(sum as i64)
            } else {
                Value::Float(sum)
            }
        }
        AggregateOp::Min | AggregateOp::Max => {
            let want = if function == AggregateOp::Min {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Greater
            };
            let mut best: Option<&Value> = None;
            for value in values.iter().filter(|v| !matches!(v, Value::Null)) {
                match best {
                    None => best = Some(value),
                    Some(current) => {
                        if value_ordering(value, current) == Some(want) {
                            best = Some(value);
                        }
                    }
                }
            }
            best.cloned().unwrap_or(Value::Null)
        }
        AggregateOp::Join => {
            let separator = arg.map(|v| value_text(&v)).unwrap_or_default();
            Value::String(
                values
                    .iter()
                    .filter(|v| !matches!(v, Value::Null))
                    .map(value_text)
                    .collect::<Vec<_>>()
                    .join(&separator),
            )
        }
        AggregateOp::ItemAt => {
            // 1-based, XPath style; anything out of range is Null.
            let index = arg.as_ref().and_then(|v| match v {
                Value::Int(i) => Some(*i),
                Value::Float(f) => Some(f.round() as i64),
                Value::String(s) => s.trim().parse().ok(),
                _ => None,
            });
            match index {
                Some(i) if i >= 1 => values.get(i as usize - 1).cloned().unwrap_or(Value::Null),
                _ => Value::Null,
            }
        }
    }
}

/// A value as a number, remembering whether it was integral (strings from
/// untyped sources parse; everything else doesn't aggregate).
fn numeric_value(value: &Value) -> Option<(f64, bool)> {
    match value {
        Value::Int(i) => Some((*i as f64, true)),
        Value::Float(f) => Some((*f, false)),
        Value::String(s) => {
            let s = s.trim();
            s.parse::<i64>()
                .map(|i| (i as f64, true))
                .ok()
                .or_else(|| s.parse::<f64>().map(|f| (f, false)).ok())
        }
        _ => None,
    }
}

fn value_ordering(a: &Value, b: &Value) -> Option<std::cmp::Ordering> {
    match (a, b) {
        (Value::Int(a), Value::Int(b)) => a.partial_cmp(b),
        (Value::Float(a), Value::Float(b)) => a.partial_cmp(b),
        (Value::Int(a), Value::Float(b)) => (*a as f64).partial_cmp(b),
        (Value::Float(a), Value::Int(b)) => a.partial_cmp(&(*b as f64)),
        (Value::String(a), Value::String(b)) => Some(a.cmp(b)),
        (Value::Bool(a), Value::Bool(b)) => Some(a.cmp(b)),
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
    let context_offset = context.len().checked_sub(positions.len())?;
    let instance = *context.get(context_offset + position_index)?;
    resolve_scalar(&[instance], path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ir::SchemaNode;
    use mapping::Binding;

    fn graph_from(nodes: Vec<(NodeId, Node)>) -> Graph {
        Graph {
            nodes: nodes.into_iter().collect(),
        }
    }

    fn dummy_schema() -> SchemaNode {
        SchemaNode::group("root", vec![])
    }

    #[test]
    fn evaluates_a_function_call_over_source_fields() {
        let graph = graph_from(vec![
            (
                0,
                Node::SourceField {
                    frame: None,
                    path: vec!["first".into()],
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String(" ".into()),
                },
            ),
            (
                2,
                Node::SourceField {
                    frame: None,
                    path: vec!["last".into()],
                },
            ),
            (
                3,
                Node::Call {
                    function: "concat".into(),
                    args: vec![0, 1, 2],
                },
            ),
        ]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: None,
                filter: None,
                group_by: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![Binding {
                    target_field: "full_name".into(),
                    node: 3,
                }],
                children: vec![],
            },
        };
        let source = Instance::Group(vec![
            (
                "first".into(),
                Instance::Scalar(Value::String("Jane".into())),
            ),
            ("last".into(), Instance::Scalar(Value::String("Doe".into()))),
        ]);

        let target = run(&project, &source).unwrap();
        assert_eq!(
            target.field("full_name").and_then(Instance::as_scalar),
            Some(&Value::String("Jane Doe".into()))
        );
    }

    #[test]
    fn missing_source_field_is_reported() {
        let graph = graph_from(vec![(
            0,
            Node::SourceField {
                frame: None,
                path: vec!["missing".into()],
            },
        )]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: None,
                filter: None,
                group_by: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![Binding {
                    target_field: "out".into(),
                    node: 0,
                }],
                children: vec![],
            },
        };
        let err = run(&project, &Instance::Group(vec![])).unwrap_err();
        assert_eq!(err, EngineError::MissingSourceField("missing".to_string()));
    }

    #[test]
    fn self_referential_node_is_a_cycle() {
        let graph = graph_from(vec![(
            0,
            Node::Call {
                function: "concat".into(),
                args: vec![0],
            },
        )]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: None,
                filter: None,
                group_by: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![Binding {
                    target_field: "out".into(),
                    node: 0,
                }],
                children: vec![],
            },
        };
        let err = run(&project, &Instance::Group(vec![])).unwrap_err();
        assert_eq!(err, EngineError::Cycle(0));
    }

    /// The "hard part" this milestone is about: a nested repeating source
    /// (Order -> Item) flattened into a single repeating target level, with
    /// an Order-level field ("cust") broadcast into every produced row --
    /// this is the shape of a real-world nested join.
    #[test]
    fn nested_repetition_flattens_with_broadcast_from_enclosing_scope() {
        let graph = graph_from(vec![
            (
                0,
                Node::SourceField {
                    frame: None,
                    path: vec!["cust".into()],
                },
            ),
            (
                1,
                Node::SourceField {
                    frame: None,
                    path: vec!["item_id".into()],
                },
            ),
            (
                2,
                Node::Position {
                    collection: vec!["orders".into()],
                },
            ),
            (
                3,
                Node::Position {
                    collection: vec!["items".into()],
                },
            ),
            (
                4,
                Node::SourceField {
                    frame: None,
                    path: vec!["keep".into()],
                },
            ),
        ]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: Some(vec!["orders".into(), "items".into()]),
                filter: Some(4),
                group_by: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![
                    Binding {
                        target_field: "cust".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "item_id".into(),
                        node: 1,
                    },
                    Binding {
                        target_field: "order_position".into(),
                        node: 2,
                    },
                    Binding {
                        target_field: "item_position".into(),
                        node: 3,
                    },
                ],
                children: vec![],
            },
        };

        let item = |id: &str, keep: bool| {
            Instance::Group(vec![
                ("item_id".into(), Instance::Scalar(Value::String(id.into()))),
                ("keep".into(), Instance::Scalar(Value::Bool(keep))),
            ])
        };
        let order = |cust: &str, items: Vec<Instance>| {
            Instance::Group(vec![
                ("cust".into(), Instance::Scalar(Value::String(cust.into()))),
                ("items".into(), Instance::Repeated(items)),
            ])
        };
        let source = Instance::Group(vec![(
            "orders".into(),
            Instance::Repeated(vec![
                order(
                    "Jane",
                    vec![item("A", false), item("B", true), item("C", true)],
                ),
                order("John", vec![item("D", false), item("E", true)]),
            ]),
        )]);

        let target = run(&project, &source).unwrap();
        let rows = target.as_repeated().unwrap();
        assert_eq!(rows.len(), 3);

        let row = |i: usize| &rows[i];
        let cust = |i: usize| row(i).field("cust").and_then(Instance::as_scalar).cloned();
        let item_id = |i: usize| {
            row(i)
                .field("item_id")
                .and_then(Instance::as_scalar)
                .cloned()
        };
        let position =
            |i: usize, field: &str| row(i).field(field).and_then(Instance::as_scalar).cloned();

        assert_eq!(cust(0), Some(Value::String("Jane".into())));
        assert_eq!(item_id(0), Some(Value::String("B".into())));
        assert_eq!(position(0, "order_position"), Some(Value::Int(1)));
        assert_eq!(position(0, "item_position"), Some(Value::Int(1)));
        assert_eq!(cust(1), Some(Value::String("Jane".into())));
        assert_eq!(item_id(1), Some(Value::String("C".into())));
        assert_eq!(position(1, "order_position"), Some(Value::Int(1)));
        assert_eq!(position(1, "item_position"), Some(Value::Int(2)));
        assert_eq!(cust(2), Some(Value::String("John".into())));
        assert_eq!(item_id(2), Some(Value::String("E".into())));
        assert_eq!(position(2, "order_position"), Some(Value::Int(2)));
        assert_eq!(position(2, "item_position"), Some(Value::Int(1)));
    }

    #[test]
    fn if_only_evaluates_the_taken_branch() {
        let graph = graph_from(vec![
            (
                0,
                Node::Const {
                    value: Value::Bool(true),
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String("then".into()),
                },
            ),
            // A self-referential "else" branch would cycle if it were ever
            // evaluated -- this proves `If` short-circuits.
            (
                2,
                Node::Call {
                    function: "concat".into(),
                    args: vec![2],
                },
            ),
            (
                3,
                Node::If {
                    condition: 0,
                    then: 1,
                    else_: 2,
                },
            ),
        ]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: None,
                filter: None,
                group_by: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![Binding {
                    target_field: "out".into(),
                    node: 3,
                }],
                children: vec![],
            },
        };
        let target = run(&project, &Instance::Group(vec![])).unwrap();
        assert_eq!(
            target.field("out").and_then(Instance::as_scalar),
            Some(&Value::String("then".into()))
        );
    }

    #[test]
    fn value_map_falls_back_to_default_on_miss() {
        let graph = graph_from(vec![
            (
                0,
                Node::Const {
                    value: Value::String("ZZ".into()),
                },
            ),
            (
                1,
                Node::ValueMap {
                    input: 0,
                    table: vec![(
                        Value::String("BD".into()),
                        Value::String("Balance Due".into()),
                    )],
                    default: Some(Value::String("Original".into())),
                },
            ),
        ]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: None,
                filter: None,
                group_by: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![Binding {
                    target_field: "out".into(),
                    node: 1,
                }],
                children: vec![],
            },
        };
        let target = run(&project, &Instance::Group(vec![])).unwrap();
        assert_eq!(
            target.field("out").and_then(Instance::as_scalar),
            Some(&Value::String("Original".into()))
        );
    }

    #[test]
    fn scope_filter_drops_items_that_fail_the_predicate() {
        let graph = graph_from(vec![
            (
                0,
                Node::SourceField {
                    frame: None,
                    path: vec!["age".into()],
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::Int(18),
                },
            ),
            (
                2,
                Node::Call {
                    function: "greater_or_equal".into(),
                    args: vec![0, 1],
                },
            ),
            (
                3,
                Node::Position {
                    collection: Vec::new(),
                },
            ),
        ]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: Some(vec![]),
                filter: Some(2),
                group_by: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![
                    Binding {
                        target_field: "age".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "position".into(),
                        node: 3,
                    },
                ],
                children: vec![],
            },
        };
        let person =
            |age: i64| Instance::Group(vec![("age".into(), Instance::Scalar(Value::Int(age)))]);
        let source = Instance::Repeated(vec![person(29), person(17), person(41)]);

        let target = run(&project, &source).unwrap();
        let ages: Vec<_> = target
            .as_repeated()
            .unwrap()
            .iter()
            .map(|row| row.field("age").and_then(Instance::as_scalar).cloned())
            .collect();
        assert_eq!(ages, vec![Some(Value::Int(29)), Some(Value::Int(41))]);
        let positions: Vec<_> = target
            .as_repeated()
            .unwrap()
            .iter()
            .map(|row| row.field("position").and_then(Instance::as_scalar).cloned())
            .collect();
        assert_eq!(positions, vec![Some(Value::Int(1)), Some(Value::Int(2))]);
    }

    #[test]
    fn scope_sort_and_take_are_stable_and_reindex_positions() {
        let graph = graph_from(vec![
            (
                0,
                Node::SourceField {
                    frame: None,
                    path: vec!["score".into()],
                },
            ),
            (
                1,
                Node::SourceField {
                    frame: None,
                    path: vec!["name".into()],
                },
            ),
            (
                2,
                Node::Const {
                    value: Value::Int(2),
                },
            ),
            (
                3,
                Node::Position {
                    collection: Vec::new(),
                },
            ),
        ]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                source: Some(Vec::new()),
                sort_by: Some(0),
                sort_descending: true,
                take: Some(2),
                bindings: vec![
                    Binding {
                        target_field: "name".into(),
                        node: 1,
                    },
                    Binding {
                        target_field: "position".into(),
                        node: 3,
                    },
                ],
                ..Scope::default()
            },
        };
        let row = |name: &str, score: i64| {
            Instance::Group(vec![
                ("name".into(), Instance::Scalar(Value::String(name.into()))),
                ("score".into(), Instance::Scalar(Value::Int(score))),
            ])
        };
        let source = Instance::Repeated(vec![
            row("low", 1),
            row("first-high", 5),
            row("second-high", 5),
            row("middle", 3),
        ]);

        let target = run(&project, &source).unwrap();
        let rows = target.as_repeated().unwrap();
        let values: Vec<_> = rows
            .iter()
            .map(|row| {
                (
                    row.field("name").and_then(Instance::as_scalar).cloned(),
                    row.field("position").and_then(Instance::as_scalar).cloned(),
                )
            })
            .collect();
        assert_eq!(
            values,
            vec![
                (
                    Some(Value::String("first-high".into())),
                    Some(Value::Int(1))
                ),
                (
                    Some(Value::String("second-high".into())),
                    Some(Value::Int(2))
                ),
            ]
        );
    }

    /// A field path crossing a repeating element that no scope iterates
    /// reads the first item (the visual-mapper convention for wiring a
    /// repeating source into a singular target).
    #[test]
    fn uniterated_repeating_elements_resolve_to_their_first_item() {
        let graph = graph_from(vec![(
            0,
            Node::SourceField {
                frame: None,
                path: vec!["Address".into(), "city".into()],
            },
        )]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: None,
                filter: None,
                group_by: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![Binding {
                    target_field: "City".into(),
                    node: 0,
                }],
                children: vec![],
            },
        };
        let address = |city: &str| {
            Instance::Group(vec![(
                "city".into(),
                Instance::Scalar(Value::String(city.into())),
            )])
        };
        let source = Instance::Group(vec![(
            "Address".into(),
            Instance::Repeated(vec![address("Vienna"), address("Boston")]),
        )]);

        let target = run(&project, &source).unwrap();
        assert_eq!(
            target.field("City").and_then(Instance::as_scalar),
            Some(&Value::String("Vienna".into()))
        );
    }

    /// A grouped scope produces one target item per distinct key (in
    /// first-seen order); inside it, bindings read the first member and
    /// aggregates reduce the group -- whether addressed as `[]` or by the
    /// collection's own name (the group shadows the ungrouped data).
    #[test]
    fn group_by_partitions_iterated_items() {
        use mapping::AggregateOp;
        let graph = graph_from(vec![
            (
                0,
                Node::Call {
                    function: "substring_before".into(),
                    args: vec![1, 2],
                },
            ),
            (
                1,
                Node::SourceField {
                    frame: None,
                    path: vec!["month".into()],
                },
            ),
            (
                2,
                Node::Const {
                    value: Value::String("-".into()),
                },
            ),
            (
                3,
                Node::Aggregate {
                    function: AggregateOp::Avg,
                    collection: vec!["Row".into()],
                    value: vec!["temp".into()],
                    expression: None,
                    arg: None,
                },
            ),
            (
                4,
                Node::Aggregate {
                    function: AggregateOp::Count,
                    collection: vec![],
                    value: vec![],
                    expression: None,
                    arg: None,
                },
            ),
        ]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: None,
                filter: None,
                group_by: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![],
                children: vec![Scope {
                    target_field: "Year".into(),
                    source: Some(vec!["Row".into()]),
                    filter: None,
                    group_by: Some(0),
                    sort_by: None,
                    sort_descending: false,
                    take: None,
                    bindings: vec![
                        Binding {
                            target_field: "Label".into(),
                            node: 0,
                        },
                        Binding {
                            target_field: "AvgTemp".into(),
                            node: 3,
                        },
                        Binding {
                            target_field: "Months".into(),
                            node: 4,
                        },
                    ],
                    children: vec![],
                }],
            },
        };
        let row = |month: &str, temp: f64| {
            Instance::Group(vec![
                (
                    "month".into(),
                    Instance::Scalar(Value::String(month.into())),
                ),
                ("temp".into(), Instance::Scalar(Value::Float(temp))),
            ])
        };
        let source = Instance::Group(vec![(
            "Row".into(),
            Instance::Repeated(vec![
                row("2024-01", 2.0),
                row("2024-07", 22.0),
                row("2025-01", 4.0),
            ]),
        )]);

        let target = run(&project, &source).unwrap();
        let years = target
            .field("Year")
            .and_then(Instance::as_repeated)
            .unwrap();
        assert_eq!(years.len(), 2);
        assert_eq!(
            years[0].field("Label").and_then(Instance::as_scalar),
            Some(&Value::String("2024".into()))
        );
        assert_eq!(
            years[0].field("AvgTemp").and_then(Instance::as_scalar),
            Some(&Value::Float(12.0))
        );
        assert_eq!(
            years[0].field("Months").and_then(Instance::as_scalar),
            Some(&Value::Int(2))
        );
        assert_eq!(
            years[1].field("Label").and_then(Instance::as_scalar),
            Some(&Value::String("2025".into()))
        );
        assert_eq!(
            years[1].field("Months").and_then(Instance::as_scalar),
            Some(&Value::Int(1))
        );
    }

    /// Aggregates reduce a repeating collection found by outward context
    /// fallback: count/sum inside an iterating scope see the current
    /// item's children, and join with a separator works over leaf values.
    #[test]
    fn aggregates_reduce_collections_in_context() {
        use mapping::AggregateOp;
        let graph = graph_from(vec![
            (
                0,
                Node::Aggregate {
                    function: AggregateOp::Count,
                    collection: vec!["Item".into()],
                    value: vec![],
                    expression: None,
                    arg: None,
                },
            ),
            (
                1,
                Node::Aggregate {
                    function: AggregateOp::Sum,
                    collection: vec!["Item".into()],
                    value: vec!["Price".into()],
                    expression: None,
                    arg: None,
                },
            ),
            (
                2,
                Node::Const {
                    value: Value::String(", ".into()),
                },
            ),
            (
                3,
                Node::Aggregate {
                    function: AggregateOp::Join,
                    collection: vec!["Order".into()],
                    value: vec!["Id".into()],
                    expression: None,
                    arg: Some(2),
                },
            ),
            (
                4,
                Node::SourceField {
                    frame: None,
                    path: vec!["Price".into()],
                },
            ),
            (
                5,
                Node::Const {
                    value: Value::Int(2),
                },
            ),
            (
                6,
                Node::Call {
                    function: "multiply".into(),
                    args: vec![4, 5],
                },
            ),
            (
                7,
                Node::Aggregate {
                    function: AggregateOp::Sum,
                    collection: vec!["Item".into()],
                    value: vec![],
                    expression: Some(6),
                    arg: None,
                },
            ),
            (
                8,
                Node::Position {
                    collection: vec!["Item".into()],
                },
            ),
            (
                9,
                Node::Aggregate {
                    function: AggregateOp::Sum,
                    collection: vec!["Item".into()],
                    value: vec![],
                    expression: Some(8),
                    arg: None,
                },
            ),
        ]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: None,
                filter: None,
                group_by: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![Binding {
                    target_field: "AllIds".into(),
                    node: 3,
                }],
                children: vec![Scope {
                    target_field: "Order".into(),
                    source: Some(vec!["Order".into()]),
                    filter: None,
                    group_by: None,
                    sort_by: None,
                    sort_descending: false,
                    take: None,
                    bindings: vec![
                        Binding {
                            target_field: "ItemCount".into(),
                            node: 0,
                        },
                        Binding {
                            target_field: "Total".into(),
                            node: 1,
                        },
                        Binding {
                            target_field: "ComputedTotal".into(),
                            node: 7,
                        },
                        Binding {
                            target_field: "PositionSum".into(),
                            node: 9,
                        },
                    ],
                    children: vec![],
                }],
            },
        };
        let item = |price: f64| {
            Instance::Group(vec![(
                "Price".into(),
                Instance::Scalar(Value::Float(price)),
            )])
        };
        let order = |id: &str, items: Vec<Instance>| {
            Instance::Group(vec![
                ("Id".into(), Instance::Scalar(Value::String(id.into()))),
                ("Item".into(), Instance::Repeated(items)),
            ])
        };
        let source = Instance::Group(vec![(
            "Order".into(),
            Instance::Repeated(vec![
                order("A", vec![item(1.5), item(2.5)]),
                order("B", vec![]),
            ]),
        )]);

        let target = run(&project, &source).unwrap();
        assert_eq!(
            target.field("AllIds").and_then(Instance::as_scalar),
            Some(&Value::String("A, B".into()))
        );
        let orders = target
            .field("Order")
            .and_then(Instance::as_repeated)
            .unwrap();
        assert_eq!(
            orders[0].field("ItemCount").and_then(Instance::as_scalar),
            Some(&Value::Int(2))
        );
        assert_eq!(
            orders[0].field("Total").and_then(Instance::as_scalar),
            Some(&Value::Float(4.0))
        );
        assert_eq!(
            orders[0]
                .field("ComputedTotal")
                .and_then(Instance::as_scalar),
            Some(&Value::Float(8.0))
        );
        assert_eq!(
            orders[0].field("PositionSum").and_then(Instance::as_scalar),
            Some(&Value::Int(3))
        );
        // An empty collection counts 0 and sums to 0.
        assert_eq!(
            orders[1].field("ItemCount").and_then(Instance::as_scalar),
            Some(&Value::Int(0))
        );
        assert_eq!(
            orders[1].field("Total").and_then(Instance::as_scalar),
            Some(&Value::Int(0))
        );
        assert_eq!(
            orders[1]
                .field("ComputedTotal")
                .and_then(Instance::as_scalar),
            Some(&Value::Int(0))
        );
        assert_eq!(
            orders[1].field("PositionSum").and_then(Instance::as_scalar),
            Some(&Value::Int(0))
        );
    }

    /// The enrichment pattern: iterate the primary source's rows while a
    /// `Lookup` node joins each row against a named extra source by key.
    /// A key with no match resolves to `Null` rather than erroring.
    #[test]
    fn lookup_joins_rows_against_an_extra_source() {
        let graph = graph_from(vec![
            (
                0,
                Node::SourceField {
                    frame: None,
                    path: vec!["customer_id".into()],
                },
            ),
            (
                1,
                Node::Lookup {
                    collection: vec!["customers".into()],
                    key: vec!["id".into()],
                    matches: 0,
                    value: vec!["name".into()],
                },
            ),
        ]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: Some(vec![]),
                filter: None,
                group_by: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![
                    Binding {
                        target_field: "customer_id".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "customer_name".into(),
                        node: 1,
                    },
                ],
                children: vec![],
            },
        };

        let order = |cid: i64| {
            Instance::Group(vec![(
                "customer_id".into(),
                Instance::Scalar(Value::Int(cid)),
            )])
        };
        let customer = |id: i64, name: &str| {
            Instance::Group(vec![
                ("id".into(), Instance::Scalar(Value::Int(id))),
                ("name".into(), Instance::Scalar(Value::String(name.into()))),
            ])
        };
        let source = Instance::Repeated(vec![order(2), order(1), order(99)]);
        let customers = Instance::Repeated(vec![customer(1, "Jane"), customer(2, "John")]);

        let target =
            run_with_sources(&project, &source, vec![("customers".into(), customers)]).unwrap();
        let names: Vec<_> = target
            .as_repeated()
            .unwrap()
            .iter()
            .map(|row| {
                row.field("customer_name")
                    .and_then(Instance::as_scalar)
                    .cloned()
            })
            .collect();
        assert_eq!(
            names,
            vec![
                Some(Value::String("John".into())),
                Some(Value::String("Jane".into())),
                Some(Value::Null),
            ]
        );
    }

    /// A scope can iterate a named extra source directly: its path falls
    /// back outward past the primary source to the extras frame.
    #[test]
    fn scope_source_path_reaches_an_extra_source() {
        let graph = graph_from(vec![(
            0,
            Node::SourceField {
                frame: None,
                path: vec!["name".into()],
            },
        )]);
        let project = Project {
            source: dummy_schema(),
            target: dummy_schema(),
            source_path: None,
            target_path: None,
            source_options: Default::default(),
            target_options: Default::default(),
            extra_sources: Vec::new(),
            graph,
            root: Scope {
                target_field: String::new(),
                source: Some(vec!["customers".into()]),
                filter: None,
                group_by: None,
                sort_by: None,
                sort_descending: false,
                take: None,
                bindings: vec![Binding {
                    target_field: "name".into(),
                    node: 0,
                }],
                children: vec![],
            },
        };

        let customers = Instance::Repeated(vec![Instance::Group(vec![(
            "name".into(),
            Instance::Scalar(Value::String("Jane".into())),
        )])]);
        let source = Instance::Group(vec![]);

        let target =
            run_with_sources(&project, &source, vec![("customers".into(), customers)]).unwrap();
        assert_eq!(target.as_repeated().map(<[Instance]>::len), Some(1));
    }
}
