use mapping::{FunctionId, FunctionParameterId};

use super::*;
use crate::{UserFunctionParameter, UserFunctionProgram};

fn definition(
    id: u64,
    parameter: u64,
    expressions: Vec<ExpressionNode>,
    output: u32,
) -> UserFunctionProgram {
    UserFunctionProgram {
        id: FunctionId::new(id),
        library: "tests".into(),
        name: format!("function_{id}"),
        parameters: vec![UserFunctionParameter {
            id: FunctionParameterId::new(parameter),
            ty: ScalarType::Int,
        }],
        output_type: ScalarType::Int,
        expressions,
        output,
    }
}

#[test]
fn accepts_isolated_nested_scalar_functions() {
    let mut program = program();
    program.user_functions = vec![
        definition(
            1,
            11,
            vec![ExpressionNode {
                id: 1,
                expression: Expression::FunctionParameter {
                    parameter: FunctionParameterId::new(11),
                },
            }],
            1,
        ),
        definition(
            2,
            21,
            vec![
                ExpressionNode {
                    id: 1,
                    expression: Expression::FunctionParameter {
                        parameter: FunctionParameterId::new(21),
                    },
                },
                ExpressionNode {
                    id: 2,
                    expression: Expression::UserFunctionCall {
                        function: FunctionId::new(1),
                        args: vec![1],
                    },
                },
            ],
            2,
        ),
    ];
    program.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::UserFunctionCall {
            function: FunctionId::new(2),
            args: vec![1],
        },
    });
    program.root.bindings[0].expression = 3;

    assert_eq!(validate_program(&program), Ok(()));
}

#[test]
fn rejects_parameter_leakage_and_context_dependent_function_bodies() {
    let mut leaked = program();
    leaked.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::FunctionParameter {
            parameter: FunctionParameterId::new(11),
        },
    });
    assert_eq!(
        validate_program(&leaked),
        Err(ProgramValidationError::FunctionParameterInMain {
            node: 3,
            parameter: FunctionParameterId::new(11),
        })
    );

    let mut contextual = program();
    contextual.user_functions.push(definition(
        1,
        11,
        vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: Vec::new(),
            },
        }],
        1,
    ));
    assert_eq!(
        validate_program(&contextual),
        Err(ProgramValidationError::UnsupportedUserFunctionExpression {
            function: FunctionId::new(1),
            node: 1,
        })
    );
}

#[test]
fn rejects_bad_arity_and_recursive_call_graphs() {
    let mut arity = program();
    arity.user_functions.push(definition(
        1,
        11,
        vec![ExpressionNode {
            id: 1,
            expression: Expression::FunctionParameter {
                parameter: FunctionParameterId::new(11),
            },
        }],
        1,
    ));
    arity.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::UserFunctionCall {
            function: FunctionId::new(1),
            args: Vec::new(),
        },
    });
    assert_eq!(
        validate_program(&arity),
        Err(ProgramValidationError::UserFunctionArity {
            owner: None,
            node: 3,
            function: FunctionId::new(1),
            expected: 1,
            actual: 0,
        })
    );

    let mut recursive = program();
    recursive.user_functions.push(definition(
        1,
        11,
        vec![
            ExpressionNode {
                id: 1,
                expression: Expression::FunctionParameter {
                    parameter: FunctionParameterId::new(11),
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::UserFunctionCall {
                    function: FunctionId::new(1),
                    args: vec![1],
                },
            },
        ],
        2,
    ));
    assert_eq!(
        validate_program(&recursive),
        Err(ProgramValidationError::UserFunctionCycle {
            cycle: vec![FunctionId::new(1), FunctionId::new(1)],
        })
    );
}
