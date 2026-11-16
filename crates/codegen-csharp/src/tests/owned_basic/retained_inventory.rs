use super::*;

pub(super) fn retained585_result(id: &str, p: &Program) -> Result<String, EmitError> {
    use std::error::Error as _;
    eprintln!("INPUT {id} complete Program={p:#?}");
    let actual = render_basic_owned_mapping(p);
    eprintln!("RAW {id} complete Result={actual:#?}");
    if let Err(error) = &actual {
        eprintln!(
            "RAW {id} complete cause={:?}",
            error.source().map(|cause| format!("{cause:#?}"))
        );
    }
    actual
}
pub(super) fn retained585_extract(actual: Result<String, EmitError>) -> ProgramValidationError {
    match actual {
        Err(EmitError::ProgramValidation(error)) => error,
        other => panic!("complete retained inventory refusal expected: {other:#?}"),
    }
}
pub(super) fn retained585_function(
    id: ::mapping::FunctionId,
    output: u32,
) -> codegen::UserFunctionProgram {
    codegen::UserFunctionProgram {
        id,
        library: "fixture".into(),
        name: format!("scalar_{}", id.get()),
        parameters: Vec::new(),
        output_type: ScalarType::String,
        expressions: vec![expression(
            output,
            Expression::Const {
                value: Value::String("function body".into()),
            },
        )],
        output,
    }
}
pub(super) fn retained585_function_expected(
    id: ::mapping::FunctionId,
    output: u32,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Function(id),
                node: output,
                scope: None,
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::IsolatedFunction,
            origin: codegen::OwnedSourceCapabilityOrigin::FunctionBody { function: id },
        }),
    }
}
pub(super) fn retained585_leaf_expected(
    node: u32,
    reason: codegen::OwnedSourceCapabilityReason,
) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node,
                scope: None,
            },
            request: None,
            reason,
            origin: codegen::OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node,
            },
        }),
    }
}

#[test]
fn private_retained_inventory_omits_dead_ordinary_nodes_and_unreferenced_functions() {
    let mut baseline = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    baseline.extra_sources.push(NamedSourceProgram {
        name: "D".into(),
        source: source_schema("Dynamic"),
        dynamic: Some(codegen::DynamicSourceProgram {
            path: 98,
            driver: codegen::SourceIteration::new(path(&["Rows"])).into(),
        }),
    });
    baseline.expressions.push(expression(
        98,
        Expression::Const {
            value: Value::String("unused.json".into()),
        },
    ));
    let original = retained585_result("S585-01 baseline", &baseline).unwrap();
    let mut p = baseline;
    p.expressions.extend([
        expression(90, Expression::SourceDocumentPath),
        expression(
            91,
            Expression::Call {
                function: ScalarFunction::Upper,
                args: vec![92],
            },
        ),
        expression(
            92,
            Expression::Const {
                value: Value::String("dead call input".into()),
            },
        ),
        expression(
            93,
            Expression::RuntimeParameterDefault {
                name: "Unused".into(),
                ty: ScalarType::String,
                default: 94,
            },
        ),
        expression(94, Expression::Raise { message: Some(95) }),
        expression(
            95,
            Expression::Const {
                value: Value::String("dead Raise message".into()),
            },
        ),
        expression(96, target_value_map(97, None, Vec::new(), None)),
        expression(
            97,
            Expression::SourceField {
                path: path(&["D", "Value"]),
                frame: None,
            },
        ),
        expression(99, field(named("Ref"), &["Value"], None)),
    ]);
    p.user_functions = vec![
        retained585_function(::mapping::FunctionId::new(7), 17),
        retained585_function(::mapping::FunctionId::new(9), 29),
    ];
    let before = p.clone();
    let actual = retained585_result("S585-02 dead valid nodes and functions", &p).unwrap();
    assert_eq!(actual, original);
    for id in 90..=99 {
        assert!(!actual.contains(&format!("FerruleValue Node_{id}(")));
        assert!(!actual.contains(&format!("FerruleValue NodeBody_{id}(")));
    }
    assert!(!actual.contains("UserFunction_"));
    assert!(!actual.contains("unused.json"));
    assert_eq!(p, before);
}

#[test]
fn private_retained_inventory_keeps_target_failure_and_potential_scalar_dependencies() {
    let mut root = group(vec![
        binding("Own", 1, ScalarType::String),
        binding("Value", 9, ScalarType::String),
    ]);
    root.children.push(TargetScope {
        target_field: "Child".into(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Scalar {
            expression: 13,
            target_domain: ScalarType::String.into(),
        },
        bindings: Vec::new(),
        children: Vec::new(),
    });
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                2,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(
                3,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: None,
                },
            ),
            expression(
                4,
                Expression::Call {
                    function: ScalarFunction::Upper,
                    args: vec![3],
                },
            ),
            expression(
                5,
                Expression::If {
                    condition: 2,
                    then: 4,
                    else_: 6,
                },
            ),
            expression(
                6,
                Expression::RuntimeParameterDefault {
                    name: "Potential".into(),
                    ty: ScalarType::String,
                    default: 7,
                },
            ),
            expression(7, Expression::Raise { message: Some(8) }),
            expression(
                8,
                Expression::Const {
                    value: Value::String("potential error".into()),
                },
            ),
            expression(9, target_value_map(5, None, Vec::new(), Some(Value::Null))),
            expression(
                12,
                Expression::Const {
                    value: Value::String("named".into()),
                },
            ),
            expression(
                13,
                Expression::RuntimeValue {
                    value: RuntimeValue::MappingFilePath,
                },
            ),
            expression(
                21,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ),
            expression(
                22,
                Expression::Call {
                    function: ScalarFunction::Upper,
                    args: vec![23],
                },
            ),
            expression(
                23,
                Expression::Const {
                    value: Value::String("lazy failure message".into()),
                },
            ),
            expression(24, Expression::SourceDocumentPath),
        ],
        root,
    );
    add_target(
        &mut p,
        "named",
        group(vec![binding("Value", 12, ScalarType::String)]),
    );
    p.failure_rules.push(source_failure(
        &[],
        codegen::FailureSelection::WhenTrue(21),
        Some(22),
    ));
    let before = p.clone();
    let actual =
        retained585_result("S585-03 complete roots and potential dependencies", &p).unwrap();
    for id in [1, 2, 3, 4, 5, 6, 7, 8, 9, 12, 13, 21, 22, 23] {
        assert!(
            actual.contains(&format!("FerruleValue Node_{id}(")),
            "retained node {id}"
        );
        assert!(
            actual.contains(&format!("FerruleValue NodeBody_{id}(")),
            "retained node body {id}"
        );
    }
    assert!(!actual.contains("FerruleValue Node_24("));
    assert!(actual.contains("case 1: result = new SelectedTargetOutput.Named("));
    assert!(actual.contains("EvaluateBasicFailureRules(context);"));
    assert_eq!(p, before);
}

#[test]
fn private_retained_inventory_rejects_exact_called_function_in_stored_call_order() {
    for target_call in [false, true] {
        for definitions_reversed in [false, true] {
            for calls_reversed in [false, true] {
                let mut p = basic(
                    vec![
                        expression(
                            1,
                            Expression::Const {
                                value: Value::String("primary".into()),
                            },
                        ),
                        expression(
                            2,
                            Expression::Const {
                                value: Value::Bool(true),
                            },
                        ),
                        expression(
                            40,
                            Expression::If {
                                condition: 2,
                                then: 1,
                                else_: 50,
                            },
                        ),
                    ],
                    group(vec![binding(
                        "Value",
                        if target_call { 40 } else { 1 },
                        ScalarType::String,
                    )]),
                );
                let first = retained585_function(::mapping::FunctionId::new(7), 17);
                let second = retained585_function(::mapping::FunctionId::new(9), 29);
                p.user_functions = if definitions_reversed {
                    vec![second, first]
                } else {
                    vec![first, second]
                };
                let first_call = expression(
                    50,
                    Expression::UserFunctionCall {
                        function: ::mapping::FunctionId::new(9),
                        args: Vec::new(),
                    },
                );
                let second_call = expression(
                    60,
                    Expression::UserFunctionCall {
                        function: ::mapping::FunctionId::new(7),
                        args: Vec::new(),
                    },
                );
                p.expressions.extend(if calls_reversed {
                    [second_call, first_call]
                } else {
                    [first_call, second_call]
                });
                let expected = if calls_reversed {
                    retained585_function_expected(::mapping::FunctionId::new(7), 17)
                } else {
                    retained585_function_expected(::mapping::FunctionId::new(9), 29)
                };
                let actual =
                    retained585_result("S585-04 exact referenced body and stored call policy", &p);
                eprintln!("EXPECTED S585-04 complete literal={expected:#?}");
                assert_eq!(retained585_extract(actual), expected);
            }
        }
    }
}

#[test]
fn private_retained_inventory_keeps_scope_failure_and_stored_main_priority() {
    use codegen::OwnedSourceCapabilityReason as Reason;
    for failure in [false, true] {
        for argument_first in [false, true] {
            let mut p = basic(
                vec![expression(
                    1,
                    Expression::Const {
                        value: Value::String("primary".into()),
                    },
                )],
                group(vec![binding("Value", 1, ScalarType::String)]),
            );
            let mut function = retained585_function(::mapping::FunctionId::new(9), 29);
            function.parameters.push(codegen::UserFunctionParameter {
                id: ::mapping::FunctionParameterId::new(1),
                ty: ScalarType::String,
            });
            p.user_functions.push(function);
            let argument = expression(90, Expression::SourceDocumentPath);
            let call = expression(
                50,
                Expression::UserFunctionCall {
                    function: ::mapping::FunctionId::new(9),
                    args: vec![90],
                },
            );
            p.expressions.extend(if argument_first {
                [argument, call]
            } else {
                [call, argument]
            });
            if failure {
                p.failure_rules.push(source_failure(
                    &[],
                    codegen::FailureSelection::All,
                    Some(90),
                ));
            }
            let expected = if failure {
                expected_failure_diagnostic(0, None, Reason::FailureRule)
            } else if argument_first {
                retained585_leaf_expected(90, Reason::Document)
            } else {
                retained585_function_expected(::mapping::FunctionId::new(9), 29)
            };
            let actual = retained585_result(
                "S585-05 failure before stored call and retained argument order",
                &p,
            );
            eprintln!("EXPECTED S585-05 complete literal={expected:#?}");
            assert_eq!(retained585_extract(actual), expected);
        }
    }
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::Const {
                    value: Value::String("primary".into()),
                },
            ),
            expression(
                50,
                Expression::UserFunctionCall {
                    function: ::mapping::FunctionId::new(9),
                    args: Vec::new(),
                },
            ),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.user_functions
        .push(retained585_function(::mapping::FunctionId::new(9), 29));
    p.root.iteration = Some(IterationPlan::new(
        codegen::SourceIteration::new(path(&["Rows"])),
        Some(50),
        None,
        Vec::new(),
        codegen::IterationOutput::Repeated,
    ));
    p.root.repeating = true;
    p.target = target_schema(&p.root, "Output");
    let route = ::mapping::SourceScopeLocation::root(::mapping::SourceScopeTarget::Primary);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Iteration {
                scope: route.clone(),
            },
            request: None,
            reason: Reason::ScopeControl,
            origin: codegen::OwnedSourceCapabilityOrigin::Scope { scope: route },
        }),
    };
    let actual = retained585_result("S585-06 scope before stored function call", &p);
    eprintln!("EXPECTED S585-06 complete literal={expected:#?}");
    assert_eq!(retained585_extract(actual), expected);
}

#[test]
fn private_retained_inventory_keeps_unconditional_owned_structure_and_public_gates() {
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(99, field(named("Absent"), &["Value"], None)),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Structure {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 99,
                scope: None,
            },
            request: codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Named {
                    name: "Absent".into(),
                },
                path: path(&["Value"]),
                frame: None,
            },
            reason: codegen::OwnedSourceStructureReason::UnknownOwner,
        }),
    };
    let actual = retained585_result("S585-07 dead owned field remains structural", &p);
    assert_eq!(retained585_extract(actual), expected);
    p.expressions.pop();
    let mut function = retained585_function(::mapping::FunctionId::new(7), 17);
    function.expressions[0].expression = field(named("Absent"), &["Value"], None);
    p.user_functions.push(function);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Structure {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Function(::mapping::FunctionId::new(7)),
                node: 17,
                scope: None,
            },
            request: codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Named {
                    name: "Absent".into(),
                },
                path: path(&["Value"]),
                frame: None,
            },
            reason: codegen::OwnedSourceStructureReason::UnknownOwner,
        }),
    };
    let actual = retained585_result(
        "S585-08 unreferenced owned function body remains structural",
        &p,
    );
    assert_eq!(retained585_extract(actual), expected);
    p.user_functions[0] = retained585_function(::mapping::FunctionId::new(7), 17);
    p.user_functions[0].output = 18;
    let actual = retained585_result(
        "S585-09 malformed unreferenced ordinary function remains structural",
        &p,
    );
    assert_eq!(
        retained585_extract(actual),
        ProgramValidationError::MissingUserFunctionOutput {
            function: ::mapping::FunctionId::new(7),
            output: 18
        }
    );
    p.user_functions[0].output = 17;
    assert!(retained585_result("S585-10 valid unreferenced function is private inert", &p).is_ok());
    let public = public_basic_result("S585-11 unchanged public stored function gate", &p);
    eprintln!(
        "EXPECTED S585-11 complete literal={:#?}",
        retained585_function_expected(::mapping::FunctionId::new(7), 17)
    );
    match public {
        Err(EmitError::ProgramValidation(error)) => assert_eq!(
            error,
            retained585_function_expected(::mapping::FunctionId::new(7), 17)
        ),
        other => panic!("complete public stored function refusal expected: {other:#?}"),
    }
    p.user_functions.clear();
    p.expressions
        .push(expression(99, Expression::SourceDocumentPath));
    assert!(retained585_result("S585-12 dead ordinary document private inert", &p).is_ok());
    let public = public_basic_result("S585-13 unchanged public stored document gate", &p);
    let expected = retained585_leaf_expected(99, codegen::OwnedSourceCapabilityReason::Document);
    eprintln!("EXPECTED S585-13 complete literal={expected:#?}");
    match public {
        Err(EmitError::ProgramValidation(error)) => assert_eq!(error, expected),
        other => panic!("complete public stored document refusal expected: {other:#?}"),
    }
}
