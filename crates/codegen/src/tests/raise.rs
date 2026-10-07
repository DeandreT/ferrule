use super::*;

#[test]
fn raise_message_dependencies_are_reachable_and_checked_before_emission() {
    let mut project = supported_project();
    project.root.bindings[0].node = 40;
    project
        .graph
        .nodes
        .insert(40, Node::Raise { message: Some(41) });
    project.graph.nodes.insert(
        41,
        Node::Const {
            value: Value::String("message".into()),
        },
    );
    let program = lower(&project).expect("a reached Raise and its message lower");
    assert!(program.expressions.iter().any(|node| node.id == 41));

    let mut missing = program.clone();
    missing.expressions.retain(|node| node.id != 41);
    assert_eq!(
        validate_program(&missing),
        Err(ProgramValidationError::MissingDependency {
            node: 40,
            dependency: 41
        })
    );

    let mut cyclic = program;
    cyclic
        .expressions
        .iter_mut()
        .find(|node| node.id == 41)
        .unwrap()
        .expression = Expression::Raise { message: Some(40) };
    assert!(matches!(
        validate_program(&cyclic),
        Err(ProgramValidationError::ExpressionCycle { .. })
    ));
}

#[test]
fn unselected_raise_keeps_its_message_function_diagnostic_precise() {
    let mut project = supported_project();
    project.graph.nodes.extend([
        (40, Node::Raise { message: Some(41) }),
        (
            41,
            Node::Call {
                function: "not_a_supported_message_function".into(),
                args: Vec::new(),
            },
        ),
        (
            42,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
        (
            43,
            Node::If {
                condition: 42,
                then: 40,
                else_: 10,
            },
        ),
    ]);
    project.root.bindings[0].node = 43;
    // Lowering checks all potentially selected branches without eagerly running them.
    let diagnostic = lower(&project).expect_err("unsupported message cannot be emitted");
    println!("original lowering diagnostic: {diagnostic:#?}");
    assert!(diagnostic.diagnostics().iter().any(|item| matches!(item,
        Diagnostic::Validation { message, .. } if message == "unknown function `not_a_supported_message_function`"
    )));
}
