use std::collections::{BTreeMap, HashSet};

use ir::{ScalarType, Value};
use mapping::{FunctionId, FunctionParameterId, Node, NodeId, UserFunction};

use crate::EngineError;

pub(super) const MAX_USER_FUNCTION_DEPTH: usize = 64;

pub(super) fn evaluate(
    functions: &BTreeMap<FunctionId, UserFunction>,
    function: FunctionId,
    arguments: Vec<Value>,
) -> Result<Value, EngineError> {
    evaluate_nested(functions, function, arguments, &mut Vec::new())
}

fn evaluate_nested(
    functions: &BTreeMap<FunctionId, UserFunction>,
    function_id: FunctionId,
    arguments: Vec<Value>,
    call_stack: &mut Vec<FunctionId>,
) -> Result<Value, EngineError> {
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
        Node::Call {
            function: name,
            args,
        } => {
            let mut values = Vec::with_capacity(args.len());
            for argument in args {
                values.push(evaluate_body_node(
                    functions,
                    function_id,
                    function,
                    *argument,
                    parameters,
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
            for argument in args {
                values.push(evaluate_body_node(
                    functions,
                    function_id,
                    function,
                    *argument,
                    parameters,
                    call_stack,
                    in_progress,
                )?);
            }
            evaluate_nested(functions, *callee, values, call_stack)
        }
        Node::If {
            condition,
            then,
            else_,
        } => match evaluate_body_node(
            functions,
            function_id,
            function,
            *condition,
            parameters,
            call_stack,
            in_progress,
        )? {
            Value::Bool(true) => evaluate_body_node(
                functions,
                function_id,
                function,
                *then,
                parameters,
                call_stack,
                in_progress,
            ),
            Value::Bool(false) => evaluate_body_node(
                functions,
                function_id,
                function,
                *else_,
                parameters,
                call_stack,
                in_progress,
            ),
            value => Err(EngineError::UserFunctionNotABool {
                function: function_id,
                node: *condition,
                found: value.type_name(),
            }),
        },
        Node::ValueMap {
            input,
            input_type,
            table,
            default,
        } => {
            let value = evaluate_body_node(
                functions,
                function_id,
                function,
                *input,
                parameters,
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
    result
}

fn adapt_scalar(value: Value, expected: ScalarType) -> Option<Value> {
    match (expected, value) {
        (_, value @ (Value::Null | Value::XmlNil(_))) => Some(value),
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
