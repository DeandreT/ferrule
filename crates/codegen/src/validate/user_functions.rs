use std::collections::{BTreeMap, BTreeSet};

use mapping::{FunctionId, NodeId};

use super::{ProgramValidationError, validate_cycles, validate_dependencies};
use crate::{Expression, Program, UserFunctionProgram};

const MAX_CALL_DEPTH: usize = 64;

pub(super) fn validate(
    program: &Program,
    main: &BTreeMap<NodeId, &Expression>,
) -> Result<(), ProgramValidationError> {
    let functions = collect_functions(program)?;
    validate_main_calls(main, &functions)?;
    for function in &program.user_functions {
        validate_definition(function, &functions)?;
    }
    validate_call_graph(&functions)
}

fn collect_functions(
    program: &Program,
) -> Result<BTreeMap<FunctionId, &UserFunctionProgram>, ProgramValidationError> {
    let mut functions = BTreeMap::new();
    let mut indices = BTreeMap::new();
    for (index, function) in program.user_functions.iter().enumerate() {
        if let Some(first) = indices.insert(function.id, index) {
            return Err(ProgramValidationError::DuplicateUserFunction {
                function: function.id,
                first,
                duplicate: index,
            });
        }
        functions.insert(function.id, function);
    }
    Ok(functions)
}

fn validate_main_calls(
    expressions: &BTreeMap<NodeId, &Expression>,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
) -> Result<(), ProgramValidationError> {
    for (&node, expression) in expressions {
        match expression {
            Expression::FunctionParameter { parameter } => {
                return Err(ProgramValidationError::FunctionParameterInMain {
                    node,
                    parameter: *parameter,
                });
            }
            Expression::UserFunctionCall { function, args } => {
                validate_call(None, node, *function, args.len(), functions)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn validate_definition(
    function: &UserFunctionProgram,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
) -> Result<(), ProgramValidationError> {
    let mut parameters = BTreeSet::new();
    for parameter in &function.parameters {
        if !parameters.insert(parameter.id) {
            return Err(ProgramValidationError::DuplicateUserFunctionParameter {
                function: function.id,
                parameter: parameter.id,
            });
        }
    }

    let expressions = collect_expressions_for(function)?;
    if !expressions.contains_key(&function.output) {
        return Err(ProgramValidationError::MissingUserFunctionOutput {
            function: function.id,
            output: function.output,
        });
    }
    validate_dependencies(&expressions).map_err(|error| ProgramValidationError::UserFunction {
        function: function.id,
        error: Box::new(error),
    })?;
    validate_cycles(&expressions).map_err(|error| ProgramValidationError::UserFunction {
        function: function.id,
        error: Box::new(error),
    })?;

    for (&node, expression) in &expressions {
        match expression {
            Expression::FunctionParameter { parameter } => {
                if !parameters.contains(parameter) {
                    return Err(ProgramValidationError::UnknownFunctionParameter {
                        function: function.id,
                        node,
                        parameter: *parameter,
                    });
                }
            }
            Expression::Const { .. }
            | Expression::Call { .. }
            | Expression::If { .. }
            | Expression::ValueMap { .. } => {}
            Expression::UserFunctionCall {
                function: called,
                args,
            } => validate_call(Some(function.id), node, *called, args.len(), functions)?,
            _ => {
                return Err(ProgramValidationError::UnsupportedUserFunctionExpression {
                    function: function.id,
                    node,
                });
            }
        }
    }
    Ok(())
}

fn collect_expressions_for(
    function: &UserFunctionProgram,
) -> Result<BTreeMap<NodeId, &Expression>, ProgramValidationError> {
    let mut expressions = BTreeMap::new();
    for expression in &function.expressions {
        if expressions
            .insert(expression.id, &expression.expression)
            .is_some()
        {
            return Err(ProgramValidationError::UserFunction {
                function: function.id,
                error: Box::new(ProgramValidationError::DuplicateExpression {
                    node: expression.id,
                }),
            });
        }
    }
    Ok(expressions)
}

fn validate_call(
    owner: Option<FunctionId>,
    node: NodeId,
    function: FunctionId,
    actual: usize,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
) -> Result<(), ProgramValidationError> {
    let Some(definition) = functions.get(&function) else {
        return Err(ProgramValidationError::MissingUserFunction {
            owner,
            node,
            function,
        });
    };
    let expected = definition.parameters.len();
    if actual != expected {
        return Err(ProgramValidationError::UserFunctionArity {
            owner,
            node,
            function,
            expected,
            actual,
        });
    }
    Ok(())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Visit {
    Active(usize),
    Complete,
}

fn validate_call_graph(
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
) -> Result<(), ProgramValidationError> {
    let mut visits = BTreeMap::new();
    let mut stack = Vec::new();
    for function in functions.keys().copied() {
        visit(function, functions, &mut visits, &mut stack)?;
    }
    Ok(())
}

fn visit(
    function: FunctionId,
    functions: &BTreeMap<FunctionId, &UserFunctionProgram>,
    visits: &mut BTreeMap<FunctionId, Visit>,
    stack: &mut Vec<FunctionId>,
) -> Result<(), ProgramValidationError> {
    match visits.get(&function) {
        Some(Visit::Complete) => return Ok(()),
        Some(Visit::Active(start)) => {
            let mut cycle = stack[*start..].to_vec();
            cycle.push(function);
            return Err(ProgramValidationError::UserFunctionCycle { cycle });
        }
        None => {}
    }
    if stack.len() == MAX_CALL_DEPTH {
        return Err(ProgramValidationError::UserFunctionDepth {
            function,
            limit: MAX_CALL_DEPTH,
        });
    }
    visits.insert(function, Visit::Active(stack.len()));
    stack.push(function);
    if let Some(definition) = functions.get(&function) {
        let mut calls = BTreeSet::new();
        for expression in &definition.expressions {
            if let Expression::UserFunctionCall { function, .. } = expression.expression {
                calls.insert(function);
            }
        }
        for called in calls {
            visit(called, functions, visits, stack)?;
        }
    }
    stack.pop();
    visits.insert(function, Visit::Complete);
    Ok(())
}
