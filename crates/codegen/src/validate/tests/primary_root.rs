use super::*;

fn root_program() -> Program {
    let mut program = program();
    program.source = serde_json::from_str(r#"{"name":"Root","xml_type_alternatives":true,"xml_default_type":"Base","kind":{"kind":"group","children":[{"name":"Code","attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"NodeName","kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"Base","members":["Code","NodeName"]},{"name":"Derived","members":["Code","Extra","NodeName"]}]}}"#).unwrap();
    program.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Value", ScalarType::Bool)],
    );
    program.expressions = vec![ExpressionNode {
        id: 1,
        expression: Expression::SourceRootXmlTypeEquals {
            canonical_expanded_type: "Derived".into(),
        },
    }];
    program.root.bindings[0].expression = 1;
    program.root.bindings[0].target_domain = ScalarType::Bool.into();
    program
}

#[test]
fn primary_root_validates_known_owner_type_and_exact_physical_field() {
    let mut valid = root_program();
    assert_eq!(validate_program(&valid), Ok(()));
    valid.expressions[0].expression = Expression::SourceRootField {
        path: vec!["NodeName".into()],
    };
    valid.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    valid.root.bindings[0].target_domain = ScalarType::String.into();
    assert_eq!(validate_program(&valid), Ok(()));
    for path in [
        vec![],
        vec!["Missing".into()],
        vec!["element()".into()],
        vec![ir::XML_TYPE_ORIGIN_FIELD.into()],
    ] {
        valid.expressions[0].expression = Expression::SourceRootField { path: path.clone() };
        assert_eq!(
            validate_program(&valid),
            Err(ProgramValidationError::InvalidPrimaryRootField { node: 1, path })
        );
    }
}

#[test]
fn primary_root_rejects_unknown_type_and_non_xml_or_repeating_owner() {
    let valid = root_program();
    for identity in ["{}Derived", "p:Derived", "Unknown"] {
        let mut invalid = valid.clone();
        invalid.expressions[0].expression = Expression::SourceRootXmlTypeEquals {
            canonical_expanded_type: identity.into(),
        };
        assert_eq!(
            validate_program(&invalid),
            Err(ProgramValidationError::InvalidPrimaryRootType {
                node: 1,
                identity: identity.into()
            })
        );
    }
    let mut repeating = valid.clone();
    repeating.source.repeating = true;
    assert_eq!(
        validate_program(&repeating),
        Err(ProgramValidationError::InvalidPrimaryRootSchema { node: 1 })
    );
    let mut ordinary = valid;
    ordinary.source =
        SchemaNode::group("Root", vec![SchemaNode::scalar("Code", ScalarType::String)]);
    assert_eq!(
        validate_program(&ordinary),
        Err(ProgramValidationError::InvalidPrimaryRootSchema { node: 1 })
    );
}

#[test]
fn primary_root_rejects_nonflat_scope_and_shared_named_target_consumer() {
    let valid = root_program();
    let mut child = valid.clone();
    child.root.children.push(empty_target_scope());
    assert_eq!(
        validate_program(&child),
        Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 1 })
    );
    let mut repeated = valid.clone();
    repeated.root.iteration = Some(IterationPlan::source(vec![]));
    assert_eq!(
        validate_program(&repeated),
        Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 1 })
    );
    let mut scalar = valid.clone();
    scalar.root.construction = TargetConstruction::Scalar {
        expression: 1,
        target_domain: ScalarType::Bool.into(),
    };
    assert_eq!(
        validate_program(&scalar),
        Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 1 })
    );
    let mut named = valid.clone();
    named.extra_targets.push(NamedTargetProgram {
        name: "Named".into(),
        target: valid.target,
        root: valid.root,
    });
    assert_eq!(
        validate_program(&named),
        Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 1 })
    );
}

#[test]
fn primary_root_allows_static_named_inputs_without_granting_fallback() {
    let mut valid = root_program();
    valid.extra_sources.push(crate::NamedSourceProgram {
        name: "Named".into(),
        source: valid.source.clone(),
        dynamic: None,
    });
    assert_eq!(validate_program(&valid), Ok(()));
}

#[test]
fn primary_root_lazy_scalar_tree_is_valid_but_reducer_ancestor_is_not() {
    let mut valid = root_program();
    valid.expressions.push(ExpressionNode {
        id: 2,
        expression: Expression::Const {
            value: Value::Bool(false),
        },
    });
    valid.expressions.push(ExpressionNode {
        id: 3,
        expression: Expression::If {
            condition: 2,
            then: 1,
            else_: 2,
        },
    });
    valid.root.bindings[0].expression = 3;
    assert_eq!(validate_program(&valid), Ok(()));
    valid.expressions[2].expression = Expression::Aggregate {
        function: AggregateFunction::Count,
        collection: vec![],
        value: AggregateValue::Expression(1),
        arg: None,
    };
    assert_eq!(
        validate_program(&valid),
        Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 3 })
    );
}

#[test]
fn primary_root_rejects_failure_consumer_even_when_shared_with_valid_binding() {
    let mut invalid = root_program();
    invalid.failure_rules.push(crate::FailureRule {
        iteration: crate::FailureIteration::Source(SourceIteration::new(vec![])),
        selection: crate::FailureSelection::WhenTrue(1),
        message: None,
    });
    assert_eq!(
        validate_program(&invalid),
        Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 1 })
    );
}

#[test]
fn primary_root_unbound_neutral_primitive_has_no_static_usage_proof() {
    let mut invalid = root_program();
    invalid.expressions.push(ExpressionNode {
        id: 2,
        expression: Expression::Const {
            value: Value::Bool(true),
        },
    });
    invalid.root.bindings[0].expression = 2;
    assert_eq!(
        validate_program(&invalid),
        Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 1 })
    );
}

#[test]
fn primary_root_rejects_host_default_expression_ownership() {
    let mut invalid = root_program();
    invalid.expressions.push(ExpressionNode {
        id: 2,
        expression: Expression::RuntimeParameterDefault {
            name: "Fallback".into(),
            ty: ScalarType::Bool,
            default: 1,
        },
    });
    invalid.root.bindings[0].expression = 2;
    assert_eq!(
        validate_program(&invalid),
        Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 2 })
    );
}
