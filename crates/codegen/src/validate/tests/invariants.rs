use super::*;

#[test]
fn rejects_self_and_multi_expression_cycles() {
    let mut self_cycle = program();
    self_cycle.expressions[1].expression = Expression::Call {
        function: ScalarFunction::Add,
        args: vec![2, 1],
    };
    assert_eq!(
        validate_program(&self_cycle),
        Err(ProgramValidationError::ExpressionCycle { cycle: vec![2, 2] })
    );

    let mut multi_cycle = program();
    multi_cycle.expressions[0].expression = Expression::If {
        condition: 2,
        then: 2,
        else_: 2,
    };
    assert_eq!(
        validate_program(&multi_cycle),
        Err(ProgramValidationError::ExpressionCycle {
            cycle: vec![1, 2, 1],
        })
    );
}

#[test]
fn rejects_invalid_target_scope_states() {
    let mut missing = program();
    missing.root.bindings[0].expression = 99;
    assert!(matches!(
        validate_program(&missing),
        Err(ProgramValidationError::MissingBindingExpression { expression: 99, .. })
    ));

    let mut duplicate_binding = program();
    duplicate_binding.root.bindings.push(Binding {
        target_field: "Value".into(),
        expression: 1,
        target_type: ScalarType::Int,
        repeating: false,
    });
    assert!(matches!(
        validate_program(&duplicate_binding),
        Err(ProgramValidationError::InvalidDuplicateBinding {
            first_binding: 0,
            duplicate_binding: 1,
            ..
        })
    ));

    let child = TargetScope {
        target_field: "Child".into(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings: Vec::new(),
        children: Vec::new(),
    };
    let mut duplicate_child = program();
    duplicate_child.root.children = vec![child.clone(), child.clone()];
    assert!(matches!(
        validate_program(&duplicate_child),
        Err(ProgramValidationError::DuplicateChildTarget {
            first_child: 0,
            duplicate_child: 1,
            ..
        })
    ));

    let mut collision = program();
    collision.root.bindings[0].target_field = "Child".into();
    collision.root.children.push(child);
    assert!(matches!(
        validate_program(&collision),
        Err(ProgramValidationError::BindingChildCollision {
            binding: 0,
            child: 0,
            ..
        })
    ));
}
