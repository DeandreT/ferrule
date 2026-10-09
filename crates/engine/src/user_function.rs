use std::cell::Cell;
use std::collections::{BTreeMap, HashSet};

use ir::{Instance, ScalarType, Value};
use mapping::{FunctionId, FunctionParameterId, Node, NodeId, UserFunction};

use crate::context::{parameter_value, runtime_field};
use crate::debug::{
    DebugHook, after_function_node_failure, after_function_node_input, after_function_node_value,
};
use crate::filter_map::FilterMapWork;
use crate::source_iteration::PositionFrame;
use crate::trace::{TraceSink, record_function_node_input_value, record_function_node_value};
use crate::{EngineError, ExecutionPurpose};

pub(super) const MAX_USER_FUNCTION_DEPTH: usize = 64;

#[derive(Clone, Copy)]
struct FunctionTrace<'a> {
    sink: Option<&'a dyn TraceSink>,
    debug_hook: Option<&'a dyn DebugHook>,
    first_failure_reported: &'a Cell<bool>,
    positions: &'a [PositionFrame],
    purpose: ExecutionPurpose,
    work: Option<FilterMapWork<'a>>,
}

#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate(
    functions: &BTreeMap<FunctionId, UserFunction>,
    function: FunctionId,
    arguments: Vec<Value>,
    runtime: Option<&Instance>,
    trace_sink: Option<&dyn TraceSink>,
    debug_hook: Option<&dyn DebugHook>,
    first_failure_reported: &Cell<bool>,
    positions: &[PositionFrame],
    purpose: ExecutionPurpose,
    work: Option<FilterMapWork<'_>>,
) -> Result<Value, EngineError> {
    evaluate_nested(
        functions,
        function,
        arguments,
        runtime,
        FunctionTrace {
            sink: trace_sink,
            debug_hook,
            first_failure_reported,
            positions,
            purpose,
            work,
        },
        &mut Vec::new(),
    )
}

fn evaluate_nested(
    functions: &BTreeMap<FunctionId, UserFunction>,
    function_id: FunctionId,
    arguments: Vec<Value>,
    runtime: Option<&Instance>,
    trace: FunctionTrace<'_>,
    call_stack: &mut Vec<FunctionId>,
) -> Result<Value, EngineError> {
    if let Some(work) = trace.work {
        work.call(function_id)?;
    }
    if call_stack.contains(&function_id) {
        return Err(EngineError::UserFunctionCycle {
            function: function_id,
        });
    }
    if call_stack.len() >= MAX_USER_FUNCTION_DEPTH {
        return Err(EngineError::UserFunctionDepth {
            limit: MAX_USER_FUNCTION_DEPTH,
        });
    }
    let function = functions
        .get(&function_id)
        .ok_or(EngineError::MissingUserFunction {
            function: function_id,
        })?;
    if arguments.len() != function.parameters.len() {
        return Err(EngineError::UserFunctionArity {
            function: function_id,
            expected: function.parameters.len(),
            found: arguments.len(),
        });
    }

    let mut parameters = Vec::with_capacity(arguments.len());
    for (parameter, argument) in function.parameters.iter().zip(arguments) {
        let found = argument.type_name();
        let value =
            adapt_scalar(argument, parameter.ty).ok_or(EngineError::UserFunctionParameterType {
                function: function_id,
                parameter: parameter.id,
                expected: parameter.ty,
                found,
            })?;
        parameters.push((parameter.id, value));
    }

    call_stack.push(function_id);
    let result = evaluate_body_node(
        functions,
        function_id,
        function,
        function.output,
        &parameters,
        runtime,
        trace,
        call_stack,
        &mut HashSet::new(),
    );
    call_stack.pop();
    let value = result?;
    let found = value.type_name();
    adapt_scalar(value, function.output_type).ok_or(EngineError::UserFunctionOutputType {
        function: function_id,
        expected: function.output_type,
        found,
    })
}

#[allow(clippy::too_many_arguments)]
fn evaluate_body_node(
    functions: &BTreeMap<FunctionId, UserFunction>,
    function_id: FunctionId,
    function: &UserFunction,
    node_id: NodeId,
    parameters: &[(FunctionParameterId, Value)],
    runtime: Option<&Instance>,
    trace: FunctionTrace<'_>,
    call_stack: &mut Vec<FunctionId>,
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let work = trace
        .work
        .map(|work| work.at_node(Some(function_id), node_id));
    let result = work
        .map_or(Ok(()), FilterMapWork::charge)
        .and_then(|()| {
            evaluate_body_node_inner(
                functions,
                function_id,
                function,
                node_id,
                parameters,
                runtime,
                trace,
                call_stack,
                in_progress,
            )
        })
        .map_err(|error| match work {
            Some(work) => work.wrap(error),
            None => error,
        });
    if let Err(error) = &result {
        after_function_node_failure(
            trace.debug_hook,
            trace.first_failure_reported,
            function_id,
            node_id,
            error,
            trace.positions,
        )?;
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn evaluate_body_node_inner(
    functions: &BTreeMap<FunctionId, UserFunction>,
    function_id: FunctionId,
    function: &UserFunction,
    node_id: NodeId,
    parameters: &[(FunctionParameterId, Value)],
    runtime: Option<&Instance>,
    trace: FunctionTrace<'_>,
    call_stack: &mut Vec<FunctionId>,
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    if !in_progress.insert(node_id) {
        return Err(EngineError::UserFunctionNodeCycle {
            function: function_id,
            node: node_id,
        });
    }
    let node = function
        .body
        .nodes
        .get(&node_id)
        .ok_or(EngineError::MissingUserFunctionNode {
            function: function_id,
            node: node_id,
        })?;
    let result = match node {
        Node::Unconnected => Ok(Value::Null),
        Node::Const { value } => Ok(value.clone()),
        Node::FunctionParameter { parameter } => parameters
            .iter()
            .find(|(id, _)| id == parameter)
            .map(|(_, value)| value.clone())
            .ok_or(EngineError::MissingUserFunctionParameter {
                function: function_id,
                parameter: *parameter,
            }),
        Node::RuntimeValue { value } => runtime
            .and_then(|frame| frame.field(runtime_field(*value)))
            .and_then(Instance::as_scalar)
            .cloned()
            .ok_or(EngineError::MissingRuntimeValue(*value)),
        Node::RuntimeParameter { name, ty, preview } => {
            let value = parameter_value(runtime, name, preview.as_deref(), trace.purpose, node_id)?
                .ok_or_else(|| EngineError::MissingRuntimeParameter {
                    node: node_id,
                    name: name.clone(),
                })?;
            let found = value.type_name();
            adapt_scalar(value.into_owned(), *ty).ok_or_else(|| EngineError::RuntimeParameterType {
                node: node_id,
                name: name.clone(),
                expected: *ty,
                found,
            })
        }
        Node::RuntimeParameterDefault {
            name,
            ty,
            default,
            preview,
        } => {
            let value =
                match parameter_value(runtime, name, preview.as_deref(), trace.purpose, node_id)? {
                    Some(value) => value.into_owned(),
                    None => evaluate_body_input(
                        functions,
                        function_id,
                        function,
                        node_id,
                        *default,
                        0,
                        parameters,
                        runtime,
                        trace,
                        call_stack,
                        in_progress,
                    )?,
                };
            let found = value.type_name();
            adapt_scalar(value, *ty).ok_or_else(|| EngineError::RuntimeParameterType {
                node: node_id,
                name: name.clone(),
                expected: *ty,
                found,
            })
        }
        Node::Call {
            function: name,
            args,
        } => {
            let mut values = Vec::with_capacity(args.len());
            for (input_index, argument) in args.iter().enumerate() {
                values.push(evaluate_body_input(
                    functions,
                    function_id,
                    function,
                    node_id,
                    *argument,
                    input_index,
                    parameters,
                    runtime,
                    trace,
                    call_stack,
                    in_progress,
                )?);
            }
            functions::call(name, &values).map_err(|source| EngineError::UserFunctionBuiltin {
                function: function_id,
                node: node_id,
                source,
            })
        }
        Node::UserFunctionCall {
            function: callee,
            args,
        } => {
            let mut values = Vec::with_capacity(args.len());
            for (input_index, argument) in args.iter().enumerate() {
                values.push(evaluate_body_input(
                    functions,
                    function_id,
                    function,
                    node_id,
                    *argument,
                    input_index,
                    parameters,
                    runtime,
                    trace,
                    call_stack,
                    in_progress,
                )?);
            }
            evaluate_nested(functions, *callee, values, runtime, trace, call_stack)
        }
        Node::If {
            condition,
            then,
            else_,
        } => match evaluate_body_input(
            functions,
            function_id,
            function,
            node_id,
            *condition,
            0,
            parameters,
            runtime,
            trace,
            call_stack,
            in_progress,
        )? {
            Value::Bool(true) => evaluate_body_input(
                functions,
                function_id,
                function,
                node_id,
                *then,
                1,
                parameters,
                runtime,
                trace,
                call_stack,
                in_progress,
            ),
            Value::Bool(false) => evaluate_body_input(
                functions,
                function_id,
                function,
                node_id,
                *else_,
                2,
                parameters,
                runtime,
                trace,
                call_stack,
                in_progress,
            ),
            value => Err(EngineError::UserFunctionNotABool {
                function: function_id,
                node: *condition,
                found: value.type_name(),
            }),
        },
        Node::Raise { message } => {
            let message = message
                .map(|message| {
                    evaluate_body_input(
                        functions,
                        function_id,
                        function,
                        node_id,
                        message,
                        0,
                        parameters,
                        runtime,
                        trace,
                        call_stack,
                        in_progress,
                    )
                    .map(crate::failure::scalar_text)
                })
                .transpose()?;
            Err(EngineError::MappingException {
                node: node_id,
                message,
            })
        }
        Node::ValueMap {
            input,
            input_type,
            table,
            default,
        } => {
            let value = evaluate_body_input(
                functions,
                function_id,
                function,
                node_id,
                *input,
                0,
                parameters,
                runtime,
                trace,
                call_stack,
                in_progress,
            )?;
            let value = input_type
                .and_then(|ty| adapt_scalar(value.clone(), ty))
                .unwrap_or(value);
            Ok(table
                .iter()
                .find(|(from, _)| *from == value)
                .map(|(_, to)| to.clone())
                .or_else(|| default.clone())
                .unwrap_or(Value::Null))
        }
        _ => Err(EngineError::UnsupportedUserFunctionNode {
            function: function_id,
            node: node_id,
        }),
    };
    in_progress.remove(&node_id);
    if let Ok(value) = &result {
        record_function_node_value(trace.sink, function_id, node_id, trace.positions, value);
        after_function_node_value(
            trace.debug_hook,
            function_id,
            node_id,
            value,
            trace.positions,
        )?;
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn evaluate_body_input(
    functions: &BTreeMap<FunctionId, UserFunction>,
    function_id: FunctionId,
    function: &UserFunction,
    consumer: NodeId,
    input: NodeId,
    input_index: usize,
    parameters: &[(FunctionParameterId, Value)],
    runtime: Option<&Instance>,
    trace: FunctionTrace<'_>,
    call_stack: &mut Vec<FunctionId>,
    in_progress: &mut HashSet<NodeId>,
) -> Result<Value, EngineError> {
    let value = evaluate_body_node(
        functions,
        function_id,
        function,
        input,
        parameters,
        runtime,
        trace,
        call_stack,
        in_progress,
    )?;
    record_function_node_input_value(
        trace.sink,
        function_id,
        consumer,
        input,
        input_index,
        trace.positions,
        &value,
    );
    after_function_node_input(
        trace.debug_hook,
        function_id,
        consumer,
        input,
        input_index,
        &value,
        trace.positions,
    )?;
    Ok(value)
}

fn adapt_scalar(value: Value, expected: ScalarType) -> Option<Value> {
    match (expected, value) {
        (_, value @ (Value::Null | Value::JsonNull(_) | Value::XmlNil(_))) => Some(value),
        (ScalarType::String, value @ Value::String(_))
        | (ScalarType::Int, value @ Value::Int(_))
        | (ScalarType::Float, value @ Value::Float(_))
        | (ScalarType::Bool, value @ Value::Bool(_)) => Some(value),
        (ScalarType::String, Value::Bool(value)) => Some(Value::String(value.to_string())),
        (ScalarType::String, Value::Int(value)) => Some(Value::String(value.to_string())),
        (ScalarType::String, Value::Float(value)) if value.is_finite() => {
            Some(Value::String(value.to_string()))
        }
        (ScalarType::Int, Value::Float(value))
            if value.is_finite()
                && value.fract() == 0.0
                && value >= i64::MIN as f64
                && value < -(i64::MIN as f64) =>
        {
            Some(Value::Int(value as i64))
        }
        (ScalarType::Int, Value::String(value)) => value.trim().parse::<i64>().ok().map(Value::Int),
        (ScalarType::Float, Value::Int(value)) => {
            let converted = value as f64;
            ((converted as i128) == i128::from(value)).then_some(Value::Float(converted))
        }
        (ScalarType::Float, Value::String(value)) => value
            .trim()
            .parse::<f64>()
            .ok()
            .filter(|value| value.is_finite())
            .map(Value::Float),
        (ScalarType::Bool, Value::String(value)) => match value.trim() {
            "true" | "1" => Some(Value::Bool(true)),
            "false" | "0" => Some(Value::Bool(false)),
            _ => None,
        },
        (ScalarType::String, Value::Float(_))
        | (ScalarType::Int, _)
        | (ScalarType::Float, _)
        | (ScalarType::Bool, _) => None,
    }
}
