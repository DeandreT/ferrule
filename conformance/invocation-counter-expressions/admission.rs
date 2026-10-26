//! Public emitter admission fixtures. No generated or reference execution.
use codegen::{Expression, Program, ProgramValidationError, UserFunctionProgram};
use ir::{ScalarType, Value};
use mapping::FunctionId;

pub fn owned_programs() -> [Program; 2] {
    let program = codegen::lower(&crate::fixture::project()).unwrap();
    let mut unused = program.clone();
    for binding in &mut unused.root.bindings {
        binding.expression = 2;
    }
    for target in &mut unused.extra_targets {
        for binding in &mut target.root.bindings {
            binding.expression = 2;
        }
    }
    [program, unused]
}

pub fn function_program() -> (Program, ProgramValidationError) {
    let mut program = codegen::lower(&crate::fixture::project()).unwrap();
    let output = program
        .expressions
        .iter()
        .find(|node| matches!(node.expression, Expression::InvocationCounter { .. }))
        .unwrap()
        .id;
    program.user_functions.push(UserFunctionProgram {
        id: FunctionId::new(7),
        library: "fixture".into(),
        name: "closed".into(),
        parameters: vec![],
        output_type: ScalarType::Int,
        output,
        expressions: program.expressions.clone(),
    });
    (
        program,
        ProgramValidationError::UnsupportedUserFunctionExpression {
            function: FunctionId::new(7),
            node: output,
        },
    )
}

pub fn make_function_valid(program: &mut Program) {
    let function = &mut program.user_functions[0];
    function.expressions = vec![codegen::ExpressionNode {
        id: function.output,
        expression: Expression::Const {
            value: Value::Int(1),
        },
    }];
}
