use std::collections::HashSet;

use ir::{
    Instance, ScalarType, Value, XML_MIXED_CONTENT_FIELD, XML_MIXED_CONTENT_VALUE_FIELD,
    XML_NODE_NAME_FIELD, XML_TEXT_FIELD,
};
use mapping::{Graph, Node, NodeId};

use crate::EngineError;
use crate::aggregate::aggregate;
use crate::context::runtime_field;
use crate::join::{AggregateInput as JoinAggregateInput, eval_aggregate as eval_join_aggregate};
use crate::resolve::{
    dynamic_scalar, field_scalar, instance_in_active_collection, instance_in_frame, join_scalar,
    repeated, scalar_in_active_collection, scalar_in_frame, source_document_path,
};
use crate::sequence::{eval_sequence_exists, eval_sequence_item_at};
use crate::source_iteration::{PositionFrame, WalkExtension, walk};

pub(crate) fn eval_expr(
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
        Node::SourceField { path, frame } => match frame {
            Some(frame) => {
                Ok(scalar_in_frame(context, positions, frame, path).unwrap_or(Value::Null))
            }
            None => scalar_in_active_collection(context, positions, path)
                .ok_or_else(|| EngineError::MissingSourceField(path.join("/"))),
        },
        Node::SourceDocumentPath => source_document_path(context, positions)
            .map(|path| Value::String(path.to_string()))
            .ok_or_else(|| EngineError::MissingSourceField("<document-path>".into())),
        Node::Position { collection } => Ok(Value::Int(position(positions, collection) as i64)),
        Node::JoinField {
            join,
            collection,
            path,
        } => join_scalar(context, positions, *join, collection, path).ok_or_else(|| {
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
            Ok(table
                .iter()
                .find(|(from, _)| *from == value)
                .map(|(_, to)| to.clone())
                .or_else(|| default.clone())
                .unwrap_or(Value::Null))
        }
        Node::Lookup {
            collection,
            key,
            matches,
            value,
        } => {
            let needle = eval_expr(graph, *matches, context, positions, in_progress)?;
            let items = repeated(context, collection)
                .ok_or_else(|| EngineError::MissingSourceField(collection.join("/")))?;
            Ok(items
                .iter()
                .find(|item| field_scalar(item, key).is_some_and(|key| *key == needle))
                .and_then(|item| field_scalar(item, value).cloned())
                .unwrap_or(Value::Null))
        }
        Node::DynamicSourceField { object, frame, key } => {
            let key = eval_expr(graph, *key, context, positions, in_progress)?;
            let Value::String(key) = key else {
                return Ok(Value::Null);
            };
            Ok(
                dynamic_scalar(context, positions, frame.as_deref(), object, &key)
                    .unwrap_or(Value::Null),
            )
        }
        Node::XmlMixedContent {
            path,
            frame,
            replacements,
        } => eval_xml_mixed_content(
            graph,
            path,
            frame.as_deref(),
            replacements,
            context,
            positions,
            in_progress,
        ),
        Node::XmlSerialize {
            path,
            frame,
            schema,
            declaration,
            indent,
            namespace,
        } => {
            let instance = match frame {
                Some(frame) => instance_in_frame(context, positions, frame, path),
                None => instance_in_active_collection(context, positions, path),
            }
            .ok_or_else(|| {
                let mut display = frame.clone().unwrap_or_default();
                display.extend(path.iter().cloned());
                EngineError::MissingSourceField(display.join("/"))
            })?;
            let options = format_xml::XmlWriteOptions {
                declaration: *declaration,
                indent: *indent,
                default_namespace: namespace.clone(),
            };
            format_xml::to_string_with_options(schema, instance, &options)
                .map(Value::String)
                .map_err(|error| EngineError::XmlSerialization {
                    node: node_id,
                    message: error.to_string(),
                })
        }
        Node::CollectionFind {
            collection,
            predicate,
            value,
        } => eval_collection_find(
            graph,
            collection,
            *predicate,
            *value,
            context,
            positions,
            in_progress,
        ),
        Node::SequenceExists {
            sequence,
            predicate,
        } => eval_sequence_exists(graph, sequence, *predicate, context, positions, in_progress),
        Node::SequenceItemAt { sequence, index } => {
            eval_sequence_item_at(graph, sequence, *index, context, positions, in_progress)
        }
        Node::Aggregate {
            function,
            collection,
            value,
            expression,
            arg,
        } => {
            // Absent repeating data aggregates as an empty collection.
            let items = aggregate_items(context, collection);
            let mut values = Vec::with_capacity(items.len());
            for item in &items {
                let terminal = item.instances.last().copied();
                let item_value = if let Some(expression) = expression {
                    let mut item_context = context.to_vec();
                    item_context.extend(item.instances.iter().copied());
                    let mut item_positions = positions.to_vec();
                    item_positions.extend(item.positions.iter().cloned());
                    eval_expr(
                        graph,
                        *expression,
                        &item_context,
                        &item_positions,
                        in_progress,
                    )?
                } else if value.is_empty() {
                    terminal
                        .and_then(Instance::as_scalar)
                        .cloned()
                        .unwrap_or(Value::Null)
                } else {
                    terminal
                        .and_then(|item| field_scalar(item, value))
                        .cloned()
                        .unwrap_or(Value::Null)
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

#[allow(clippy::too_many_arguments)]
fn eval_xml_mixed_content(
    graph: &Graph,
    path: &[String],
    frame: Option<&[String]>,
    replacements: &[mapping::XmlMixedContentReplacement],
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let group = match frame {
        Some(frame) => instance_in_frame(context, positions, frame, path),
        None => instance_in_active_collection(context, positions, path),
    }
    .ok_or_else(|| EngineError::MissingSourceField(path.join("/")))?;
    let Some(items) = group
        .field(XML_MIXED_CONTENT_FIELD)
        .and_then(Instance::as_repeated)
    else {
        return Ok(group
            .field(XML_TEXT_FIELD)
            .and_then(Instance::as_scalar)
            .cloned()
            .unwrap_or(Value::Null));
    };
    let mut output = String::new();
    let mut occurrences = std::collections::BTreeMap::<&str, usize>::new();
    for item in items {
        let name = item
            .field(XML_NODE_NAME_FIELD)
            .and_then(Instance::as_scalar)
            .and_then(|value| match value {
                Value::String(name) => Some(name.as_str()),
                _ => None,
            })
            .unwrap_or_default();
        let text = item
            .field(XML_TEXT_FIELD)
            .and_then(Instance::as_scalar)
            .and_then(|value| match value {
                Value::String(text) => Some(text.as_str()),
                _ => None,
            })
            .unwrap_or_default();
        let Some(replacement) = replacements.iter().find(|rule| rule.element == name) else {
            output.push_str(text);
            continue;
        };
        let mut item_context = context.to_vec();
        let mut item_positions = positions.to_vec();
        if !replacement.collection.is_empty()
            && let Some(value) = item.field(XML_MIXED_CONTENT_VALUE_FIELD)
        {
            let index = occurrences.entry(name).or_default();
            *index += 1;
            item_context.push(value);
            item_positions.push(PositionFrame {
                collection: replacement.collection.clone(),
                index: *index,
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            });
        }
        let value = eval_expr(
            graph,
            replacement.expression,
            &item_context,
            &item_positions,
            in_progress,
        )?;
        match value {
            Value::Null | Value::XmlNil(_) => {}
            Value::String(value) => output.push_str(&value),
            Value::Bool(value) => output.push_str(if value { "true" } else { "false" }),
            Value::Int(value) => output.push_str(&value.to_string()),
            Value::Float(value) => output.push_str(&value.to_string()),
        }
    }
    Ok(Value::String(output))
}

fn aggregate_items<'a>(context: &[&'a Instance], collection: &[String]) -> Vec<WalkExtension<'a>> {
    let base = if let Some(first) = collection.first() {
        context
            .iter()
            .rev()
            .copied()
            .find(|item| item.field(first).is_some())
    } else {
        context
            .iter()
            .rev()
            .copied()
            .find(|item| matches!(item, Instance::Repeated(_) | Instance::DocumentSet(_)))
    };
    base.map_or_else(Vec::new, |base| walk(base, collection, &[], &[], &[]))
}

fn eval_collection_find(
    graph: &Graph,
    collection: &[String],
    predicate: NodeId,
    value: NodeId,
    context: &[&Instance],
    positions: &[PositionFrame],
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let root = if let Some(first) = collection.first() {
        context
            .iter()
            .rev()
            .copied()
            .find(|item| item.field(first).is_some())
    } else {
        context
            .iter()
            .rev()
            .copied()
            .find(|item| item.as_repeated().is_some())
    }
    .ok_or_else(|| EngineError::MissingSourceField(collection.join("/")))?;
    let mut item_context = context.to_vec();
    let mut item_positions = positions.to_vec();
    visit_collection_find(
        graph,
        root,
        collection,
        0,
        predicate,
        value,
        &mut item_context,
        &mut item_positions,
        in_progress,
    )
    .map(|found| found.unwrap_or(Value::Null))
}

#[allow(clippy::too_many_arguments)]
fn visit_collection_find<'a>(
    graph: &Graph,
    current: &'a Instance,
    collection: &[String],
    consumed: usize,
    predicate: NodeId,
    value: NodeId,
    context: &mut Vec<&'a Instance>,
    positions: &mut Vec<PositionFrame>,
    in_progress: &mut HashSet<NodeId>,
) -> Result<Option<Value>, EngineError> {
    if let Instance::Repeated(items) = current {
        for (item_index, item) in items.iter().enumerate() {
            context.push(item);
            positions.push(PositionFrame {
                collection: collection[..consumed].to_vec(),
                index: item_index + 1,
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            });
            let found = visit_collection_find(
                graph,
                item,
                collection,
                consumed,
                predicate,
                value,
                context,
                positions,
                in_progress,
            )?;
            positions.pop();
            context.pop();
            if found.is_some() {
                return Ok(found);
            }
        }
        return Ok(None);
    }
    if consumed < collection.len() {
        return match current.field(&collection[consumed]) {
            Some(next) => visit_collection_find(
                graph,
                next,
                collection,
                consumed + 1,
                predicate,
                value,
                context,
                positions,
                in_progress,
            ),
            None => Ok(None),
        };
    }
    match eval_expr(graph, predicate, context, positions, in_progress)? {
        Value::Bool(true) => eval_expr(graph, value, context, positions, in_progress).map(Some),
        Value::Bool(false) | Value::Null | Value::XmlNil(_) => Ok(None),
        other => Err(EngineError::NotABool {
            node: predicate,
            found: other.type_name(),
        }),
    }
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
