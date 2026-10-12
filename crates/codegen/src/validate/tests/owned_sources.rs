use super::*;
use crate::{
    FailureIteration, FailureRule, FailureSelection, NamedSourceProgram,
    OwnedSourceCapabilityOrigin as I, OwnedSourceCapabilityReason as C, OwnedSourceDiagnostic as D,
    OwnedSourceDiagnosticLocation as L, OwnedSourceDiagnosticOwner as O,
    OwnedSourceDiagnosticRequest as Q, OwnedSourceNamespace as N, OwnedSourceSchemaShape as S,
    OwnedSourceStructureReason as R, UserFunctionProgram,
};
use mapping::{
    OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner, SourceScopeLocation,
    SourceScopeStep, SourceScopeTarget,
};
fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn basic() -> Program {
    let mut p = program();
    p.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Value", ScalarType::Int),
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
            SchemaNode::group(
                "Details",
                vec![SchemaNode::scalar("Value", ScalarType::Int)],
            ),
            SchemaNode::scalar("Values", ScalarType::Int).repeating(),
            scalar_union("Union"),
        ],
    );
    p
}
fn field(owner: SourceOwner, parts: &[&str], frame: Option<Vec<String>>) -> Expression {
    Expression::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame,
    })
}
fn primary_query(parts: &[&str], frame: Option<Vec<String>>) -> Q {
    Q::Field {
        owner: O::Primary,
        path: path(parts),
        frame,
    }
}
fn structure(query: Q, reason: R) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: D::Structure {
            location: L::Expression {
                namespace: N::Main,
                node: 9,
                scope: None,
            },
            request: query,
            reason,
        }
        .into(),
    }
}
fn append(p: &mut Program, expression: Expression) {
    p.expressions.push(ExpressionNode { id: 9, expression });
}
fn reached(p: &Program) -> Result<(), ProgramValidationError> {
    let result = validate_program(p);
    eprintln!("A516 complete input={p:#?}\ncomplete_result={result:#?}");
    result
}
#[test]
fn raw_empty_named_owner_payloads_are_structural_without_serde() {
    for expression in [
        field(
            SourceOwner::Named {
                name: String::new(),
            },
            &["Value"],
            None,
        ),
        Expression::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Named {
                name: String::new(),
            },
            collection: None,
        }),
    ] {
        let mut p = basic();
        let query = match &expression {
            Expression::OwnedSourceField(_) => Q::Field {
                owner: O::Named {
                    name: String::new(),
                },
                path: path(&["Value"]),
                frame: None,
            },
            _ => Q::Position {
                owner: O::Named {
                    name: String::new(),
                },
                collection: None,
            },
        };
        append(&mut p, expression);
        assert_eq!(reached(&p), Err(structure(query, R::InvalidNamedOwner)));
    }
}
#[test]
fn exact_named_owner_names_and_no_foreign_or_prefix_rescue() {
    let mut p = basic();
    p.extra_sources.push(NamedSourceProgram {
        name: "Ref".into(),
        source: p.source.clone(),
        dynamic: None,
    });
    for name in ["Missing", "ref", " Ref "] {
        let mut candidate = p.clone();
        append(
            &mut candidate,
            field(SourceOwner::Named { name: name.into() }, &["Value"], None),
        );
        assert_eq!(
            reached(&candidate),
            Err(structure(
                Q::Field {
                    owner: O::Named { name: name.into() },
                    path: path(&["Value"]),
                    frame: None
                },
                R::UnknownOwner
            ))
        );
    }
    let mut candidate = p.clone();
    append(
        &mut candidate,
        field(
            SourceOwner::Named { name: "Ref".into() },
            &["Ref", "Value"],
            None,
        ),
    );
    assert_eq!(
        reached(&candidate),
        Err(structure(
            Q::Field {
                owner: O::Named { name: "Ref".into() },
                path: path(&["Ref", "Value"]),
                frame: None
            },
            R::MissingOwnedPath { segment: 0 }
        ))
    );
    let mut candidate = p;
    candidate.source = SchemaNode::group("Input", vec![]);
    append(
        &mut candidate,
        field(SourceOwner::Primary, &["Value"], None),
    );
    assert_eq!(
        reached(&candidate),
        Err(structure(
            primary_query(&["Value"], None),
            R::MissingOwnedPath { segment: 0 }
        ))
    );
}
#[test]
fn old_declarations_graphs_and_cycles_keep_precedence_before_owned_shapes() {
    let mut base = basic();
    append(
        &mut base,
        field(
            SourceOwner::Named {
                name: String::new(),
            },
            &["Value"],
            None,
        ),
    );
    let mut p = base.clone();
    p.extra_sources.push(NamedSourceProgram {
        name: String::new(),
        source: p.source.clone(),
        dynamic: None,
    });
    assert_eq!(
        reached(&p),
        Err(ProgramValidationError::EmptyExtraSourceName { index: 0 })
    );
    let mut p = base.clone();
    for name in ["Ref", " Ref "] {
        p.extra_sources.push(NamedSourceProgram {
            name: name.into(),
            source: p.source.clone(),
            dynamic: None,
        });
    }
    assert_eq!(
        reached(&p),
        Err(ProgramValidationError::DuplicateExtraSourceName {
            name: "Ref".into(),
            first: 0,
            duplicate: 1
        })
    );
    let mut p = base.clone();
    p.expressions.push(ExpressionNode {
        id: 9,
        expression: Expression::Const {
            value: Value::Int(0),
        },
    });
    assert_eq!(
        reached(&p),
        Err(ProgramValidationError::DuplicateExpression { node: 9 })
    );
    let mut p = base.clone();
    p.expressions.push(ExpressionNode {
        id: 10,
        expression: Expression::Call {
            function: ScalarFunction::Add,
            args: vec![404, 1],
        },
    });
    assert_eq!(
        reached(&p),
        Err(ProgramValidationError::MissingDependency {
            node: 10,
            dependency: 404
        })
    );
    let mut p = base;
    p.expressions.push(ExpressionNode {
        id: 10,
        expression: Expression::Call {
            function: ScalarFunction::Add,
            args: vec![10, 1],
        },
    });
    assert_eq!(
        reached(&p),
        Err(ProgramValidationError::ExpressionCycle {
            cycle: vec![10, 10]
        })
    );
}
#[test]
fn owned_field_kind_and_missing_segment_payloads_are_exact() {
    for (parts, reason) in [
        (vec!["Rows", "Missing"], R::MissingOwnedPath { segment: 1 }),
        (
            vec!["Details"],
            R::InvalidOwnedField {
                found: S::Group { repeating: false },
            },
        ),
    ] {
        let mut p = basic();
        append(&mut p, field(SourceOwner::Primary, &parts, None));
        assert_eq!(
            reached(&p),
            Err(structure(primary_query(&parts, None), reason))
        );
    }
    for parts in [vec!["Values"], vec!["Union"], vec!["Rows", "Value"]] {
        let mut p = basic();
        append(&mut p, field(SourceOwner::Primary, &parts, None));
        assert_eq!(reached(&p), Ok(()));
    }
}
#[test]
fn owner_pin_compatibility_and_collection_occurrence_precedence_are_exact() {
    let mut p = basic();
    append(
        &mut p,
        field(SourceOwner::Primary, &["Value"], Some(path(&["Missing"]))),
    );
    assert_eq!(
        reached(&p),
        Err(structure(
            primary_query(&["Value"], Some(path(&["Missing"]))),
            R::IncompatibleFramePath
        ))
    );
    let mut p = basic();
    append(
        &mut p,
        field(
            SourceOwner::Named {
                name: "Missing".into(),
            },
            &["Value"],
            Some(path(&["Rows"])),
        ),
    );
    assert_eq!(
        reached(&p),
        Err(structure(
            Q::Field {
                owner: O::Named {
                    name: "Missing".into()
                },
                path: path(&["Value"]),
                frame: Some(path(&["Rows"]))
            },
            R::UnknownOwner
        ))
    );
    for (parts, found) in [
        (vec!["Value"], S::Scalar { repeating: false }),
        (vec!["Details"], S::Group { repeating: false }),
    ] {
        let mut p = basic();
        append(
            &mut p,
            Expression::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Primary,
                collection: Some(path(&parts)),
            }),
        );
        assert_eq!(
            reached(&p),
            Err(structure(
                Q::Position {
                    owner: O::Primary,
                    collection: Some(path(&parts))
                },
                R::InvalidOwnedCollection { found }
            ))
        );
    }
    for collection in [
        None,
        Some(vec![]),
        Some(path(&["Rows"])),
        Some(path(&["Values"])),
    ] {
        let mut p = basic();
        append(
            &mut p,
            Expression::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Primary,
                collection,
            }),
        );
        assert_eq!(reached(&p), Ok(()));
    }
}
#[test]
fn recursive_anchor_shape_cannot_replace_terminal_repetition() {
    for (occurrence_repeating, anchor_repeating) in [(true, false), (false, true)] {
        let mut p = basic();
        let mut anchor =
            SchemaNode::group("Anchor", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
        anchor.repeating = anchor_repeating;
        let mut occurrence = SchemaNode::recursive_group("Occurrence", "Anchor");
        occurrence.repeating = occurrence_repeating;
        p.source = SchemaNode::group("Input", vec![anchor, occurrence]);
        append(
            &mut p,
            Expression::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Primary,
                collection: Some(path(&["Occurrence"])),
            }),
        );
        let expected = if occurrence_repeating {
            Ok(())
        } else {
            Err(structure(
                Q::Position {
                    owner: O::Primary,
                    collection: Some(path(&["Occurrence"])),
                },
                R::InvalidOwnedCollection {
                    found: S::Group { repeating: false },
                },
            ))
        };
        assert_eq!(reached(&p), expected);
    }
    let mut p = basic();
    p.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::recursive_group("Occurrence", "Missing").repeating()],
    );
    append(
        &mut p,
        Expression::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: Some(path(&["Occurrence"])),
        }),
    );
    assert_eq!(
        reached(&p),
        Err(structure(
            Q::Position {
                owner: O::Primary,
                collection: Some(path(&["Occurrence"]))
            },
            R::UnresolvedRecursiveAnchor {
                anchor: "Missing".into()
            }
        ))
    );
    let mut p = basic();
    p.source = SchemaNode::scalar("Array", ScalarType::Int).repeating();
    append(
        &mut p,
        Expression::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: Some(vec![]),
        }),
    );
    assert_eq!(reached(&p), Ok(()));
}
#[test]
fn exact_structure_scope_routes_keep_all_child_dynamic_segment_and_named_indexes() {
    let invalid = OwnedSourceRef {
        owner: SourceOwner::Named {
            name: String::new(),
        },
        path: vec![],
    };
    let mut owned = empty_target_scope();
    owned.iteration = Some(IterationPlan::owned_source(invalid));
    for (step, target) in [
        (None, SourceScopeTarget::Primary),
        (Some(SourceScopeStep::Child(0)), SourceScopeTarget::Primary),
        (
            Some(SourceScopeStep::DynamicChild(0)),
            SourceScopeTarget::Primary,
        ),
        (
            Some(SourceScopeStep::Segment(0)),
            SourceScopeTarget::Primary,
        ),
        (
            Some(SourceScopeStep::Child(0)),
            SourceScopeTarget::Named {
                index: 0,
                name: "primary".into(),
            },
        ),
    ] {
        let mut p = basic();
        let mut parent = empty_target_scope();
        match &step {
            None => parent = owned.clone(),
            Some(SourceScopeStep::Child(_)) => parent.children.push(owned.clone()),
            Some(SourceScopeStep::DynamicChild(_)) => {
                parent.construction = TargetConstruction::DynamicGroup {
                    fixed_fields: vec![],
                    bindings: vec![],
                    children: vec![crate::DynamicTargetChild {
                        key: 1,
                        scope: owned.clone(),
                    }],
                    merge: false,
                }
            }
            Some(SourceScopeStep::Segment(_)) => {
                parent.iteration = Some(IterationPlan::concatenate(
                    owned.clone(),
                    vec![],
                    crate::IterationOutput::Repeated,
                ))
            }
        }
        match &target {
            SourceScopeTarget::Primary => p.root = parent,
            SourceScopeTarget::Named { name, .. } => p.extra_targets.push(NamedTargetProgram {
                name: name.clone(),
                target: p.target.clone(),
                root: parent,
            }),
        }
        // Isolate the new structure helper; old unrelated dynamic target preconditions are not changed.
        let result = super::super::owned_sources::validate_structure(&p);
        eprintln!("A-S07 exact route input={p:#?} result={result:#?}");
        assert_eq!(
            result,
            Err(ProgramValidationError::OwnedSource {
                diagnostic: D::Structure {
                    location: L::Iteration {
                        scope: SourceScopeLocation {
                            target,
                            path: step.into_iter().collect()
                        }
                    },
                    request: Q::Iteration {
                        owner: O::Named {
                            name: String::new()
                        },
                        path: vec![]
                    },
                    reason: R::InvalidNamedOwner
                }
                .into()
            })
        );
    }
}
fn function(expression: Expression) -> UserFunctionProgram {
    UserFunctionProgram {
        id: mapping::FunctionId::new(7),
        library: "tests".into(),
        name: "owned".into(),
        parameters: vec![],
        output_type: ScalarType::Int,
        expressions: vec![ExpressionNode { id: 9, expression }],
        output: 9,
    }
}
#[test]
fn retained_dead_function_structure_and_same_node_namespace_are_not_omitted() {
    let mut p = basic();
    p.user_functions.push(function(field(
        SourceOwner::Named {
            name: String::new(),
        },
        &["Value"],
        None,
    )));
    assert_eq!(
        reached(&p),
        Err(ProgramValidationError::OwnedSource {
            diagnostic: D::Structure {
                location: L::Expression {
                    namespace: N::Function(mapping::FunctionId::new(7)),
                    node: 9,
                    scope: None
                },
                request: Q::Field {
                    owner: O::Named {
                        name: String::new()
                    },
                    path: path(&["Value"]),
                    frame: None
                },
                reason: R::InvalidNamedOwner
            }
            .into()
        })
    );
    let mut p = basic();
    p.expressions.push(ExpressionNode {
        id: 0,
        expression: field(SourceOwner::Primary, &["Value"], None),
    });
    p.root.bindings[0].expression = 0;
    let mut body = function(field(SourceOwner::Primary, &["Value"], None));
    body.expressions[0].id = 0;
    body.output = 0;
    p.user_functions.push(body);
    assert_eq!(
        reached(&p),
        Err(ProgramValidationError::OwnedSource {
            diagnostic: D::Capability {
                location: L::Expression {
                    namespace: N::Function(mapping::FunctionId::new(7)),
                    node: 0,
                    scope: None
                },
                request: Some(primary_query(&["Value"], None)),
                reason: C::IsolatedFunction,
                origin: I::FunctionBody {
                    function: mapping::FunctionId::new(7)
                }
            }
            .into()
        })
    );
}
#[test]
fn basic_visit_cannot_suppress_failure_or_generated_potential_branch_visits() {
    let route = SourceScopeLocation::root(SourceScopeTarget::Primary);
    let mut p = basic();
    append(&mut p, field(SourceOwner::Primary, &["Value"], None));
    p.root.bindings[0].expression = 9;
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(vec![])),
        selection: FailureSelection::All,
        message: Some(9),
    });
    assert_eq!(
        reached(&p),
        Err(ProgramValidationError::OwnedSource {
            diagnostic: D::Capability {
                location: L::Expression {
                    namespace: N::Main,
                    node: 9,
                    scope: None
                },
                request: Some(primary_query(&["Value"], None)),
                reason: C::FailureRule,
                origin: I::Failure { index: 0 }
            }
            .into()
        })
    );
    let mut p = basic();
    append(&mut p, field(SourceOwner::Primary, &["Value"], None));
    p.expressions.extend([
        ExpressionNode {
            id: 8,
            expression: Expression::Const {
                value: Value::Bool(false),
            },
        },
        ExpressionNode {
            id: 10,
            expression: Expression::If {
                condition: 8,
                then: 9,
                else_: 2,
            },
        },
        ExpressionNode {
            id: 88,
            expression: Expression::SourceField {
                frame: None,
                path: vec![],
            },
        },
    ]);
    p.root.iteration = Some(IterationPlan::generated(GeneratedSequence::Range {
        from: None,
        to: 1,
        item: 88,
    }));
    p.root.bindings[0].expression = 10;
    assert_eq!(
        reached(&p),
        Err(ProgramValidationError::OwnedSource {
            diagnostic: D::Capability {
                location: L::Expression {
                    namespace: N::Main,
                    node: 9,
                    scope: Some(route.clone())
                },
                request: Some(primary_query(&["Value"], None)),
                reason: C::GeneratedItem,
                origin: I::Scope { scope: route }
            }
            .into()
        })
    );
}
#[test]
fn owned_ancestry_cannot_enter_an_ordinary_isolated_function() {
    let mut p = basic();
    p.root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: path(&["Rows"]),
    }));
    p.user_functions.push(function(Expression::Const {
        value: Value::Int(1),
    }));
    append(
        &mut p,
        Expression::UserFunctionCall {
            function: mapping::FunctionId::new(7),
            args: vec![],
        },
    );
    p.root.bindings[0].expression = 9;
    assert_eq!(
        reached(&p),
        Err(ProgramValidationError::OwnedSource {
            diagnostic: D::Capability {
                location: L::Expression {
                    namespace: N::Main,
                    node: 9,
                    scope: Some(SourceScopeLocation::root(SourceScopeTarget::Primary))
                },
                request: None,
                reason: C::IsolatedFunction,
                origin: I::FunctionCall {
                    caller_namespace: N::Main,
                    call: 9,
                    callee: mapping::FunctionId::new(7)
                }
            }
            .into()
        })
    );
}

fn stage_program(expression: Expression) -> Program {
    use crate::{GeneratedFilterMapV1, GeneratedRange, UserFunctionParameter};
    let mut p = basic();
    p.target = SchemaNode::group(
        "Target",
        vec![
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    );
    p.expressions = vec![
        ExpressionNode {
            id: 1,
            expression: Expression::Const {
                value: Value::Int(1),
            },
        },
        ExpressionNode {
            id: 2,
            expression: Expression::Const {
                value: Value::Int(3),
            },
        },
        ExpressionNode {
            id: 10,
            expression: Expression::SourceField {
                path: Vec::new(),
                frame: None,
            },
        },
        ExpressionNode {
            id: 11,
            expression: Expression::SourceField {
                path: Vec::new(),
                frame: None,
            },
        },
    ];
    let parameters = || {
        (1..=2)
            .map(|id| UserFunctionParameter {
                id: mapping::FunctionParameterId::new(id),
                ty: ScalarType::Int,
            })
            .collect()
    };
    p.user_functions = vec![
        UserFunctionProgram {
            id: mapping::FunctionId::new(100),
            library: "owned-stage-controls".into(),
            name: "keep".into(),
            parameters: parameters(),
            output_type: ScalarType::Bool,
            expressions: vec![ExpressionNode {
                id: 1,
                expression: Expression::Const {
                    value: Value::Bool(true),
                },
            }],
            output: 1,
        },
        UserFunctionProgram {
            id: mapping::FunctionId::new(101),
            library: "owned-stage-controls".into(),
            name: "map".into(),
            parameters: parameters(),
            output_type: ScalarType::Int,
            expressions: vec![
                ExpressionNode {
                    id: 1,
                    expression: Expression::FunctionParameter {
                        parameter: mapping::FunctionParameterId::new(1),
                    },
                },
                ExpressionNode { id: 9, expression },
            ],
            output: 1,
        },
    ];
    p.root.bindings.clear();
    p.root.children = vec![TargetScope {
        target_field: "Rows".into(),
        repeating: true,
        iteration: Some(IterationPlan::generated(GeneratedSequence::FilterMapV1(
            GeneratedFilterMapV1 {
                source: GeneratedRange {
                    from: Some(1),
                    to: 2,
                    item: 10,
                },
                item: 11,
                predicate: mapping::FunctionId::new(100),
                mapper: mapping::FunctionId::new(101),
                output_type: ScalarType::Int,
                captures: Vec::new(),
            },
        ))),
        construction: TargetConstruction::Group,
        bindings: vec![Binding {
            target_field: "Value".into(),
            expression: 11,
            target_domain: crate::ScalarTargetDomain::Single(ScalarType::Int),
            repeating: false,
        }],
        children: Vec::new(),
    }];
    p
}
#[test]
fn new_stage_leaves_defer_without_skipping_later_legacy_builtin_checks() {
    use mapping::FilterMapAdmissionKind as K;
    for expression in [
        field(SourceOwner::Primary, &["Value"], None),
        Expression::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: None,
        }),
    ] {
        let mut p = stage_program(expression);
        p.user_functions[1].expressions.push(ExpressionNode {
            id: 10,
            expression: Expression::Call {
                function: ScalarFunction::Subtract,
                args: vec![1, 1],
            },
        });
        let expected = ProgramValidationError::FilterMapAdmission {
            item: 11,
            kind: K::UnsupportedBuiltin {
                function: mapping::FunctionId::new(101),
                node: 10,
                builtin: "subtract".into(),
            },
        };
        let result = reached(&p);
        eprintln!("A516 stage later-old-error expected={expected:#?}");
        assert_eq!(result, Err(expected));
    }
}
#[test]
fn valid_new_stage_leaves_remain_isolated_and_malformed_requests_stay_structural() {
    let expressions = [
        field(SourceOwner::Primary, &["Value"], None),
        Expression::OwnedPosition(OwnedPosition {
            owner: SourceOwner::Primary,
            collection: None,
        }),
    ];
    let requests = [
        primary_query(&["Value"], None),
        Q::Position {
            owner: O::Primary,
            collection: None,
        },
    ];
    for (expression, request) in expressions.into_iter().zip(requests) {
        let p = stage_program(expression);
        let expected = ProgramValidationError::OwnedSource {
            diagnostic: D::Capability {
                location: L::Expression {
                    namespace: N::Function(mapping::FunctionId::new(101)),
                    node: 9,
                    scope: None,
                },
                request: Some(request),
                reason: C::IsolatedFunction,
                origin: I::FunctionBody {
                    function: mapping::FunctionId::new(101),
                },
            }
            .into(),
        };
        let result = reached(&p);
        eprintln!("A516 valid-owned-stage expected={expected:#?}");
        assert_eq!(result, Err(expected));
    }
    let p = stage_program(field(
        SourceOwner::Named {
            name: String::new(),
        },
        &["Value"],
        None,
    ));
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: D::Structure {
            location: L::Expression {
                namespace: N::Function(mapping::FunctionId::new(101)),
                node: 9,
                scope: None,
            },
            request: Q::Field {
                owner: O::Named {
                    name: String::new(),
                },
                path: path(&["Value"]),
                frame: None,
            },
            reason: R::InvalidNamedOwner,
        }
        .into(),
    };
    let result = reached(&p);
    eprintln!("A516 malformed-owned-stage expected={expected:#?}");
    assert_eq!(result, Err(expected));
}
#[test]
fn old_stage_signature_and_private_capture_precede_malformed_owned_requests() {
    use mapping::FilterMapAdmissionKind as K;
    let malformed = || {
        field(
            SourceOwner::Named {
                name: String::new(),
            },
            &["Value"],
            None,
        )
    };
    let mut signature = stage_program(malformed());
    signature.user_functions[1].parameters.pop();
    let signature_expected = ProgramValidationError::FilterMapAdmission {
        item: 11,
        kind: K::StageSignature {
            stage: mapping::FilterMapStage::Mapper,
            function: mapping::FunctionId::new(101),
            expected_parameters: vec![ScalarType::Int, ScalarType::Int],
            found_parameters: vec![ScalarType::Int],
            expected_output: ScalarType::Int,
            found_output: ScalarType::Int,
        },
    };
    let signature_result = reached(&signature);
    let mut capture = stage_program(malformed());
    capture.root.children[0].iteration = Some(IterationPlan::generated(
        GeneratedSequence::FilterMapV1(crate::GeneratedFilterMapV1 {
            source: crate::GeneratedRange {
                from: Some(1),
                to: 2,
                item: 10,
            },
            item: 11,
            predicate: mapping::FunctionId::new(100),
            mapper: mapping::FunctionId::new(101),
            output_type: ScalarType::Int,
            captures: vec![crate::GeneratedFilterMapCapture {
                node: 10,
                ty: ScalarType::Int,
            }],
        }),
    ));
    let capture_expected = ProgramValidationError::FilterMapAdmission {
        item: 11,
        kind: K::PrivateCapture {
            capture: 0,
            node: 10,
            item: 10,
        },
    };
    let capture_result = reached(&capture);
    eprintln!(
        "A516 old-stage precedence expected-signature={signature_expected:#?} expected-capture={capture_expected:#?}"
    );
    assert_eq!(signature_result, Err(signature_expected));
    assert_eq!(capture_result, Err(capture_expected));
}
