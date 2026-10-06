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
        required: false,
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
        valid.expressions[0].expression = Expression::SourceRootField {
            path: path.clone(),
            required: false,
        };
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

fn document_boundary_program() -> Program {
    let mut program = root_program();
    program.source = serde_json::from_str(r#"{"name":"Root","xml_namespace":{"kind":"unqualified"},"xml_type_alternatives":true,"xml_default_type":"Base","kind":{"kind":"group","children":[{"name":"Code","xml_namespace":{"kind":"unqualified"},"attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","xml_namespace":{"kind":"unqualified"},"attribute":true,"xml_attribute_required":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"Base","members":["Code"]},{"name":"Derived","members":["Code","Extra"]}]}}"#).unwrap();
    program.xml_boundary = Some(crate::XmlBoundaryProgram {
        input: crate::XmlInputPolicy {
            allow_inactive_root_type_members: true,
            root_view_policy: true,
        },
        output: crate::XmlOutputPolicy::default(),
        extra_inputs: Vec::new(),
        extra_outputs: Vec::new(),
    });
    program
}

#[test]
fn xml_document_boundary_requires_explicit_primary_flat_string_reader() {
    let valid = document_boundary_program();
    assert_eq!(validate_program(&valid), Ok(()));
    let mut flags = valid.clone();
    flags.xml_boundary.as_mut().unwrap().input.root_view_policy = false;
    let mut numeric = valid.clone();
    if let ir::SchemaKind::Group { children, .. } = &mut numeric.source.kind {
        children[0].kind = ir::SchemaKind::Scalar {
            ty: ScalarType::Int,
        };
    }
    let mut element = valid.clone();
    if let ir::SchemaKind::Group { children, .. } = &mut element.source.kind {
        children[0].attribute = false;
    }
    for invalid in [flags, numeric, element] {
        assert_eq!(
            validate_program(&invalid),
            Err(ProgramValidationError::InvalidXmlBoundary {
                reason: "requires the closed observed primary XML root input policy".into(),
            })
        );
    }
}

#[test]
fn xml_document_boundary_rejects_named_outputs_and_invalid_literal_policy() {
    let valid = document_boundary_program();
    let mut named = valid.clone();
    named.extra_targets.push(NamedTargetProgram {
        name: "other".into(),
        target: valid.target.clone(),
        root: valid.root.clone(),
    });
    assert_eq!(
        validate_program(&named),
        Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "named XML document inputs and observed root-view outputs require separate adapter support".into(),
        })
    );
    for namespace in [String::new(), "x".repeat(4097)] {
        let mut invalid = valid.clone();
        invalid
            .xml_boundary
            .as_mut()
            .unwrap()
            .output
            .default_namespace = Some(namespace);
        assert_eq!(
            validate_program(&invalid),
            Err(ProgramValidationError::InvalidXmlBoundary {
                reason: "default XML namespace must be nonempty and at most 4096 UTF-8 bytes"
                    .into(),
            })
        );
    }
    let mut hints = valid;
    hints.xml_boundary.as_mut().unwrap().output.schema_hints = Some(ir::XmlSchemaHints::default());
    assert_eq!(
        validate_program(&hints),
        Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "invalid literal XML schema hints".into(),
        })
    );
}

#[test]
fn xml_document_boundary_rejects_forbidden_default_namespace_characters() {
    let mut program = document_boundary_program();
    program
        .xml_boundary
        .as_mut()
        .unwrap()
        .output
        .default_namespace = Some("\0".into());
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::InvalidXmlBoundary {
            reason: "default XML namespace must contain XML 1.0 characters".into(),
        })
    );
}

#[test]
fn xml_document_boundary_rejects_reserved_default_namespace_with_or_without_hints() {
    for namespace in [
        "http://www.w3.org/XML/1998/namespace",
        "http://www.w3.org/2000/xmlns/",
    ] {
        for with_hints in [false, true] {
            let mut program = document_boundary_program();
            let output = &mut program.xml_boundary.as_mut().unwrap().output;
            output.default_namespace = Some(namespace.into());
            if with_hints {
                output.schema_hints = Some(ir::XmlSchemaHints {
                    no_namespace_location: Some("literal.xsd".into()),
                    locations: vec![],
                });
            }
            assert_eq!(
                validate_program(&program),
                Err(ProgramValidationError::InvalidXmlBoundary {
                    reason: "reserved XML namespace cannot be the default namespace".into()
                })
            );
        }
    }
}

#[test]
fn xml_document_boundary_rejects_reserved_schema_root_namespace() {
    for namespace in [
        "http://www.w3.org/XML/1998/namespace",
        "http://www.w3.org/2000/xmlns/",
    ] {
        for with_hints in [false, true] {
            let mut program = document_boundary_program();
            program.target.xml_namespace = Some(ir::XmlNamespace::Qualified(
                ir::XmlNamespaceUri::new(namespace).unwrap(),
            ));
            let output = &mut program.xml_boundary.as_mut().unwrap().output;
            output.default_namespace = None;
            if with_hints {
                output.schema_hints = Some(ir::XmlSchemaHints {
                    no_namespace_location: Some("literal.xsd".into()),
                    locations: vec![],
                });
            }
            assert_eq!(
                validate_program(&program),
                Err(ProgramValidationError::InvalidXmlBoundary {
                    reason: "reserved XML namespace cannot be the default namespace".into()
                })
            );
        }
    }
}

fn observed_one_static_named_root_program() -> Program {
    let mut program = document_boundary_program();
    program.extra_targets.push(NamedTargetProgram {
        name: "audit".into(),
        target: program.target.clone(),
        root: program.root.clone(),
    });
    program
        .xml_boundary
        .as_mut()
        .unwrap()
        .extra_outputs
        .push(crate::NamedXmlOutputPolicy {
            name: "audit".into(),
            output: crate::XmlOutputPolicy::default(),
        });
    program
}

#[test]
fn observed_one_static_named_root_program_admits_shared_and_named_only_readers() {
    let mut program = observed_one_static_named_root_program();
    assert_eq!(validate_program(&program), Ok(()));
    program.expressions.push(ExpressionNode {
        id: 2,
        expression: Expression::SourceRootField {
            path: vec!["Extra".into()],
            required: true,
        },
    });
    program.extra_targets[0].target = SchemaNode::group(
        "Audit",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    program.extra_targets[0].root.bindings[0].expression = 2;
    program.extra_targets[0].root.bindings[0].target_domain = ScalarType::String.into();
    assert_eq!(validate_program(&program), Ok(()));
}

#[test]
fn observed_static_named_root_program_retains_own_schema_names_from_lowering() {
    let mut program = observed_one_static_named_root_program();
    program.root.target_field = program.target.name.clone();
    program.extra_targets[0].target.name = "Audit".into();
    program.extra_targets[0].root.target_field = "Audit".into();
    assert_eq!(validate_program(&program), Ok(()));
    for (primary, name) in [
        (true, "descendant"),
        (false, "descendant"),
        (true, "Audit"),
        (false, "Target"),
    ] {
        let mut invalid = program.clone();
        if primary {
            invalid.root.target_field = name.into();
        } else {
            invalid.extra_targets[0].root.target_field = name.into();
        }
        assert_eq!(
            validate_program(&invalid),
            Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 1 })
        );
    }
    program.extra_targets.clear();
    program.xml_boundary.as_mut().unwrap().extra_outputs.clear();
    assert_eq!(
        validate_program(&program),
        Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 1 })
    );
}

#[test]
fn observed_one_static_named_root_program_keeps_unproved_compound_roots_closed() {
    let valid = observed_one_static_named_root_program();
    for mutation in 0..16 {
        let mut invalid = valid.clone();
        match mutation {
            0 => invalid.xml_boundary = None,
            1 => {
                invalid.xml_boundary.as_mut().unwrap().input = crate::XmlInputPolicy {
                    allow_inactive_root_type_members: false,
                    root_view_policy: false,
                }
            }
            2 => invalid.extra_sources.push(crate::NamedSourceProgram {
                name: "reference".into(),
                source: valid.source.clone(),
                dynamic: None,
            }),
            3 => {
                let mut other = invalid.extra_targets[0].clone();
                other.name = "other".into();
                invalid.extra_targets.push(other);
            }
            4 => invalid.extra_targets[0].root.iteration = Some(IterationPlan::source(vec![])),
            5 => invalid.root.iteration = Some(IterationPlan::source(vec![])),
            6 => invalid.extra_targets[0].root.repeating = true,
            7 => invalid.extra_targets[0].target.repeating = true,
            8 => invalid.extra_targets[0]
                .root
                .children
                .push(empty_target_scope()),
            9 => invalid.extra_targets[0].root.target_field = "descendant".into(),
            10 => {
                invalid.extra_targets[0].root.construction = TargetConstruction::Scalar {
                    expression: 1,
                    target_domain: ScalarType::Bool.into(),
                }
            }
            11 => invalid.target.repeating = true,
            12 => invalid.extra_targets[0].target = SchemaNode::scalar("Audit", ScalarType::Bool),
            13 => invalid.xml_boundary.as_mut().unwrap().extra_outputs[0].name = "wrong".into(),
            14 => invalid.xml_boundary.as_mut().unwrap().extra_inputs.push(
                crate::NamedXmlInputPolicy {
                    name: "reference".into(),
                    input: crate::XmlInputPolicy {
                        allow_inactive_root_type_members: false,
                        root_view_policy: false,
                    },
                },
            ),
            15 => {
                invalid
                    .xml_boundary
                    .as_mut()
                    .unwrap()
                    .input
                    .root_view_policy = false
            }
            _ => unreachable!(),
        }
        let expressions = invalid
            .expressions
            .iter()
            .map(|node| (node.id, &node.expression))
            .collect();
        // Inspect this ownership proof directly so an earlier XML-policy error
        // cannot hide a newly granted root-reader permission.
        assert_eq!(
            super::super::primary_root::validate(&invalid, &expressions),
            Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 1 }),
            "mutation {mutation}"
        );
        assert!(
            validate_program(&invalid).is_err(),
            "public mutation {mutation}"
        );
    }
}

#[test]
fn observed_one_static_named_root_program_still_rejects_private_and_failure_consumers() {
    let valid = observed_one_static_named_root_program();
    let mut failure = valid.clone();
    failure.failure_rules.push(crate::FailureRule {
        iteration: crate::FailureIteration::Source(SourceIteration::new(vec![])),
        selection: crate::FailureSelection::WhenTrue(1),
        message: None,
    });
    assert_eq!(
        validate_program(&failure),
        Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 1 })
    );
    let mut reducer = valid;
    reducer.expressions.push(ExpressionNode {
        id: 2,
        expression: Expression::Aggregate {
            function: AggregateFunction::Count,
            collection: vec![],
            value: AggregateValue::Expression(1),
            arg: None,
        },
    });
    reducer.extra_targets[0].root.bindings[0].expression = 2;
    assert_eq!(
        validate_program(&reducer),
        Err(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 2 })
    );
}
