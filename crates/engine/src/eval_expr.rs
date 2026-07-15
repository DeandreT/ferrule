use std::collections::HashSet;

use ir::{Instance, ScalarType, Value};
use mapping::{Graph, Node, NodeId};

use crate::EngineError;
use crate::aggregate::aggregate;
use crate::context::runtime_field;
use crate::join::{AggregateInput as JoinAggregateInput, eval_aggregate as eval_join_aggregate};
use crate::resolve::{field_scalar, join_scalar, repeated, scalar, scalar_in_frame};
use crate::sequence::eval_sequence_exists;
use crate::source_iteration::PositionFrame;

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
        Node::SourceField { path, frame } => {
            let value = match frame {
                Some(frame) => scalar_in_frame(context, positions, frame, path),
                None => scalar(context, path),
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
            let items = repeated(context, collection)
                .ok_or_else(|| EngineError::MissingSourceField(collection.join("/")))?;
            Ok(items
                .iter()
                .find(|item| field_scalar(item, key).is_some_and(|key| *key == needle))
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
            let items = repeated(context, collection).unwrap_or(&[]);
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
