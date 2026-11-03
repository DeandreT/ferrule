#[path = "fixtures/closed_primary_source.rs"]
mod fixture;

use codegen::{
    Expression, ExpressionNode, IterationPlan, ProgramValidationError, TargetConstruction,
};
use mapping::{ExecutionContract, SourceContractReason};

fn reason(program: &codegen::Program) -> SourceContractReason {
    match codegen::validate_program(program) {
        Err(ProgramValidationError::SourceContractViolation(error)) => error.reason,
        other => panic!("expected typed source contract failure, got {other:?}"),
    }
}

#[test]
fn lower_preserves_contract_and_reproves_current_program() {
    let program = fixture::program();
    assert_eq!(
        program.execution_contract,
        Some(ExecutionContract::ClosedPrimarySourceV1)
    );
    codegen::validate_program(&program).unwrap();
    assert!(program.validate_execution_contract().unwrap().is_some());
    for companions in [
        codegen::CompanionSelection::Complete,
        codegen::CompanionSelection::TypedOnly,
    ] {
        codegen::prepare_boundary_emission(&program, companions).unwrap();
    }
}

#[test]
fn source_owner_proof_does_not_execute_scalar_bodies() {
    let mut program = fixture::program();
    program.user_functions[0].expressions[2].expression = Expression::Call {
        function: codegen::ScalarFunction::Upper,
        args: Vec::new(),
    };
    assert!(program.validate_execution_contract().unwrap().is_some());
}

#[test]
fn direct_program_mutations_cannot_retain_an_unchecked_contract() {
    let original = fixture::program();
    let mut path = original.clone();
    path.expressions[0].expression = Expression::SourceField {
        path: vec!["missing".into()],
        frame: None,
    };
    assert_eq!(reason(&path), SourceContractReason::UnprovedOwner);
    use std::error::Error;
    let error = codegen::validate_program(&path).unwrap_err();
    assert!(
        error
            .source()
            .unwrap()
            .is::<mapping::SourceContractViolation>()
    );
    let mut frame = original.clone();
    frame.expressions[1].expression = Expression::SourceField {
        path: vec!["Value".into()],
        frame: Some(vec!["Cell".into()]),
    };
    assert_eq!(reason(&frame), SourceContractReason::UnprovedOwner);
    let mut driver = original.clone();
    driver.root.children[0].iteration =
        Some(IterationPlan::source(vec!["Row".into(), "Cell".into()]));
    assert_eq!(reason(&driver), SourceContractReason::UnprovedOwner);
    let mut scope = original.clone();
    scope.extra_targets[0].root.construction = TargetConstruction::CopyCurrentSource;
    assert_eq!(reason(&scope), SourceContractReason::UnsupportedModel);
    let mut unused = original.clone();
    unused.expressions.push(ExpressionNode {
        id: 999,
        expression: Expression::RuntimeParameter {
            name: "secret".into(),
            ty: ir::ScalarType::String,
        },
    });
    assert_eq!(reason(&unused), SourceContractReason::UnsupportedModel);
    let mut body = original.clone();
    body.user_functions[0].expressions.push(ExpressionNode {
        id: 999,
        expression: Expression::SourceField {
            path: vec!["Text".into()],
            frame: None,
        },
    });
    assert_eq!(reason(&body), SourceContractReason::UnsupportedModel);
}

#[test]
fn guarded_target_metadata_is_bounded_before_recursive_validation() {
    let mut program = fixture::program();
    let mut target = ir::SchemaNode::scalar("Value", ir::ScalarType::String);
    for _ in 0..257 {
        target = ir::SchemaNode::group("Nested", vec![target]);
    }
    program.target = target;
    assert_eq!(reason(&program), SourceContractReason::ResourceLimit);
}

#[test]
fn model_proof_checks_schema_shadowing_and_nested_latest_frame() {
    let mut shadow = fixture::program();
    let ir::SchemaKind::Group { children, .. } = &mut shadow.source.kind else {
        panic!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[3].kind else {
        panic!()
    };
    children.push(ir::SchemaNode::scalar("Text", ir::ScalarType::String));
    assert_eq!(reason(&shadow), SourceContractReason::UnprovedOwner);
    let mut nested = fixture::program();
    let ir::SchemaKind::Group { children, .. } = &mut nested.source.kind else {
        panic!()
    };
    let ir::SchemaKind::Group { children, .. } = &mut children[3].kind else {
        panic!()
    };
    children[1].name = "Row".into();
    nested.root.children[0].children[0].iteration = Some(IterationPlan::source(vec!["Row".into()]));
    assert_eq!(reason(&nested), SourceContractReason::UnprovedOwner);
}

#[test]
fn owned_udf_gate_is_not_promoted_and_other_contracts_refuse() {
    let mut program = fixture::program();
    let error = codegen::validate_basic_owned_emission_with_capabilities(
        &program,
        codegen::OwnedPreset::ControlFree.capabilities(),
    )
    .unwrap_err();
    assert!(format!("{error:?}").contains("IsolatedFunction"));
    program.user_functions.clear();
    for node in &mut program.expressions {
        if let Expression::UserFunctionCall { args, .. } = &node.expression {
            node.expression = Expression::Call {
                function: codegen::ScalarFunction::String,
                args: args.clone(),
            };
        }
    }
    assert_eq!(
        codegen::validate_basic_owned_emission_with_capabilities(
            &program,
            codegen::OwnedPreset::ControlFree.capabilities()
        ),
        Err(ProgramValidationError::UnsupportedExecutionContract(
            ExecutionContract::ClosedPrimarySourceV1
        ))
    );
}
