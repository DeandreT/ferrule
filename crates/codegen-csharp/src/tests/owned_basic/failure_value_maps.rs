use super::*;

fn failure594_expression(program: &mut Program, id: u32, value: Expression) {
    program
        .expressions
        .iter_mut()
        .find(|node| node.id == id)
        .unwrap()
        .expression = value;
}

fn failure594_program() -> Program {
    let mut program = failure_program(vec![
        expression(
            10,
            Expression::Const {
                value: mapped_string("static"),
            },
        ),
        expression(
            20,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(
            30,
            target_value_map(
                20,
                None,
                vec![(Value::Bool(false), Value::Bool(false))],
                Some(Value::Bool(true)),
            ),
        ),
        expression(
            40,
            target_value_map(
                10,
                None,
                vec![(mapped_string("static"), mapped_string("mapped"))],
                None,
            ),
        ),
        expression(
            99,
            Expression::Const {
                value: mapped_string("unused.json"),
            },
        ),
    ]);
    program.failure_rules.push(source_failure(
        &["Rows"],
        codegen::FailureSelection::WhenTrue(30),
        Some(40),
    ));
    program
}

fn failure594_admit(label: &str, program: &Program) {
    let actual = retained585_result(label, program);
    assert!(
        actual.is_ok(),
        "{label}: complete source result/cause retained before admission comparison"
    );
}

fn failure594_refuse(label: &str, program: &Program, expected: ProgramValidationError) {
    let actual = retained585_result(label, program);
    eprintln!("EXPECTED {label} complete independent typed diagnostic={expected:#?}");
    assert_eq!(retained585_extract(actual), expected, "{label}");
}

// Generated consumers use these Programs to exercise predicate, message,
// default, lazy-input, and typed-error behavior.
fn failure594_matched_program_one() -> Program {
    let mut program = failure_program(vec![
        failure_legacy(10, &["Score"], None),
        failure_legacy(11, &["Value"], None),
        expression(
            20,
            target_value_map(
                10,
                Some(ScalarType::Int),
                vec![
                    (Value::Int(1), Value::Bool(true)),
                    (Value::Int(0), Value::Bool(false)),
                ],
                None,
            ),
        ),
        expression(
            21,
            target_value_map(
                11,
                Some(ScalarType::String),
                vec![
                    (mapped_string("hit"), mapped_string("mapped")),
                    (mapped_string("hit"), mapped_string("later duplicate")),
                ],
                Some(mapped_string("fallback")),
            ),
        ),
        expression(
            60,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(
            61,
            Expression::RuntimeParameterDefault {
                name: "later".into(),
                ty: ScalarType::Bool,
                default: 60,
            },
        ),
        expression(
            62,
            Expression::Const {
                value: mapped_string("later"),
            },
        ),
    ]);
    program.failure_rules.extend([
        source_failure(&["Rows"], codegen::FailureSelection::WhenTrue(20), Some(21)),
        source_failure(&["Rows"], codegen::FailureSelection::WhenTrue(61), Some(62)),
    ]);
    program
}

fn failure594_matched_program_two() -> Program {
    let mut program = failure_program(vec![
        expression(
            10,
            Expression::RuntimeParameterDefault {
                name: "mode".into(),
                ty: ScalarType::String,
                default: 11,
            },
        ),
        expression(11, Expression::Raise { message: Some(12) }),
        expression(
            12,
            Expression::Const {
                value: mapped_string("input default"),
            },
        ),
        expression(
            20,
            target_value_map(
                10,
                Some(ScalarType::String),
                vec![
                    (mapped_string("skip"), Value::Bool(false)),
                    (mapped_string("raise"), Value::Bool(true)),
                ],
                Some(Value::Bool(true)),
            ),
        ),
        expression(
            30,
            Expression::RuntimeParameterDefault {
                name: "message".into(),
                ty: ScalarType::String,
                default: 31,
            },
        ),
        expression(
            31,
            Expression::Const {
                value: mapped_string("hit"),
            },
        ),
        expression(
            40,
            target_value_map(
                30,
                None,
                vec![(mapped_string("hit"), mapped_string("mapped"))],
                None,
            ),
        ),
        expression(
            41,
            Expression::RuntimeParameterDefault {
                name: "explode".into(),
                ty: ScalarType::Bool,
                default: 44,
            },
        ),
        expression(
            42,
            Expression::If {
                condition: 41,
                then: 43,
                else_: 40,
            },
        ),
        expression(43, Expression::Raise { message: Some(45) }),
        expression(
            44,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(
            45,
            Expression::Const {
                value: mapped_string("message raise"),
            },
        ),
    ]);
    program.failure_rules.push(source_failure(
        &["Rows"],
        codegen::FailureSelection::WhenTrue(20),
        Some(42),
    ));
    program
}

#[test]
fn private_failure_value_maps_admit_predicate_message_hits_misses_and_defaults() {
    let base = failure594_program();
    for (label, input, table, default) in [
        (
            "S594-01 predicate false hit",
            Value::Bool(false),
            vec![(Value::Bool(false), Value::Bool(false))],
            None,
        ),
        (
            "S594-02 predicate true hit",
            Value::Bool(true),
            vec![(Value::Bool(true), Value::Bool(true))],
            None,
        ),
        (
            "S594-03 predicate miss literal default",
            Value::Bool(false),
            vec![],
            Some(Value::Bool(true)),
        ),
        (
            "S594-04 predicate miss absent default",
            Value::Bool(false),
            vec![],
            None,
        ),
    ] {
        let mut program = base.clone();
        failure594_expression(&mut program, 20, Expression::Const { value: input });
        failure594_expression(&mut program, 30, target_value_map(20, None, table, default));
        failure594_admit(label, &program);
    }
    for (label, table, default) in [
        (
            "S594-05 message hit",
            vec![(mapped_string("static"), mapped_string("mapped"))],
            None,
        ),
        (
            "S594-06 message miss literal default",
            vec![],
            Some(mapped_string("fallback")),
        ),
        ("S594-07 message miss absent default", vec![], None),
    ] {
        let mut program = base.clone();
        failure594_expression(&mut program, 40, target_value_map(10, None, table, default));
        failure594_admit(label, &program);
    }
}

#[test]
fn private_failure_value_maps_keep_potential_inputs_and_retained_dynamic_envelopes() {
    use codegen::OwnedSourceCapabilityReason as Reason;
    let mut base = failure594_program();
    base.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(vec![]).into(),
    });
    for (label, input) in [
        (
            "S594-08 dynamic map input path",
            Expression::SourceField {
                path: path(&["Unused", "Value"]),
                frame: None,
            },
        ),
        (
            "S594-09 dynamic map input frame",
            Expression::SourceField {
                path: path(&["Value"]),
                frame: Some(path(&["Unused", "Rows"])),
            },
        ),
    ] {
        let mut program = base.clone();
        failure594_expression(&mut program, 10, input);
        failure594_refuse(
            label,
            &program,
            retained585_leaf_expected(10, Reason::UnsupportedBasicContext),
        );
    }
    let mut empty = base.clone();
    failure594_expression(
        &mut empty,
        10,
        Expression::SourceField {
            path: path(&["Unused", "Value"]),
            frame: Some(vec![]),
        },
    );
    failure594_admit("S594-10 empty frame replaces dynamic path", &empty);
    let mut static_source = base.clone();
    static_source.extra_sources[0].dynamic = None;
    failure594_expression(
        &mut static_source,
        10,
        Expression::SourceField {
            path: path(&["Unused", "Value"]),
            frame: None,
        },
    );
    failure594_admit("S594-11 same name static metadata", &static_source);
    let mut different = base.clone();
    failure594_expression(
        &mut different,
        10,
        Expression::SourceField {
            path: path(&["Ref", "Value"]),
            frame: None,
        },
    );
    failure594_admit("S594-12 distinct exact static source name", &different);
    for (label, input) in [
        (
            "S594-13 potential parameter default map input",
            Expression::RuntimeParameterDefault {
                name: "supplied".into(),
                ty: ScalarType::String,
                default: 10,
            },
        ),
        (
            "S594-14 potential unchosen If map input",
            Expression::If {
                condition: 20,
                then: 10,
                else_: 11,
            },
        ),
        (
            "S594-15 potential ordinary call map input",
            Expression::Call {
                function: codegen::ScalarFunction::Trim,
                args: vec![10],
            },
        ),
        (
            "S594-16 potential Raise message map input",
            Expression::Raise { message: Some(10) },
        ),
    ] {
        let mut program = base.clone();
        failure594_expression(
            &mut program,
            10,
            Expression::SourceField {
                path: path(&["Unused", "Value"]),
                frame: None,
            },
        );
        program.expressions.extend([
            expression(
                11,
                Expression::Const {
                    value: mapped_string("static"),
                },
            ),
            expression(50, input),
        ]);
        failure594_expression(
            &mut program,
            40,
            target_value_map(50, None, vec![], Some(mapped_string("fallback"))),
        );
        failure594_refuse(
            label,
            &program,
            retained585_leaf_expected(10, Reason::UnsupportedBasicContext),
        );
    }
}

#[test]
fn private_failure_value_maps_keep_owned_structure_and_actual_called_body_priority() {
    use codegen::OwnedSourceCapabilityReason as Reason;
    let base = failure594_program();
    let mut unknown = base.clone();
    failure594_expression(&mut unknown, 10, field(named("Absent"), &["Value"], None));
    failure594_refuse(
        "S594-17 unknown map input owner remains structural",
        &unknown,
        ProgramValidationError::OwnedSource {
            diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Structure {
                location: codegen::OwnedSourceDiagnosticLocation::Expression {
                    namespace: codegen::OwnedSourceNamespace::Main,
                    node: 10,
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
        },
    );
    let mut owned = base.clone();
    failure594_expression(
        &mut owned,
        10,
        field(SourceOwner::Primary, &["Value"], None),
    );
    failure594_refuse(
        "S594-18 owned map input keeps failure request and origin",
        &owned,
        ProgramValidationError::OwnedSource {
            diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
                location: codegen::OwnedSourceDiagnosticLocation::Expression {
                    namespace: codegen::OwnedSourceNamespace::Main,
                    node: 10,
                    scope: None,
                },
                request: Some(codegen::OwnedSourceDiagnosticRequest::Field {
                    owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                    path: path(&["Value"]),
                    frame: None,
                }),
                reason: Reason::FailureRule,
                origin: codegen::OwnedSourceCapabilityOrigin::Failure { index: 0 },
            }),
        },
    );
    let mut call = base.clone();
    call.user_functions.extend([
        retained585_function(::mapping::FunctionId::new(7), 17),
        retained585_function(::mapping::FunctionId::new(9), 29),
    ]);
    failure594_expression(
        &mut call,
        10,
        Expression::UserFunctionCall {
            function: ::mapping::FunctionId::new(9),
            args: vec![],
        },
    );
    failure594_refuse(
        "S594-19 actual map input called body",
        &call,
        retained585_function_expected(::mapping::FunctionId::new(9), 29),
    );
    call.user_functions[1]
        .parameters
        .push(codegen::UserFunctionParameter {
            id: ::mapping::FunctionParameterId::new(1),
            ty: ScalarType::String,
        });
    failure594_expression(
        &mut call,
        10,
        Expression::UserFunctionCall {
            function: ::mapping::FunctionId::new(9),
            args: vec![11],
        },
    );
    call.expressions.push(expression(
        11,
        Expression::SourceField {
            path: path(&["Unused", "Value"]),
            frame: None,
        },
    ));
    call.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(vec![]).into(),
    });
    failure594_refuse(
        "S594-20 stored call before dynamic argument",
        &call,
        retained585_function_expected(::mapping::FunctionId::new(9), 29),
    );
    let call_index = call
        .expressions
        .iter()
        .position(|node| node.id == 10)
        .unwrap();
    let argument_index = call
        .expressions
        .iter()
        .position(|node| node.id == 11)
        .unwrap();
    call.expressions.swap(call_index, argument_index);
    failure594_refuse(
        "S594-21 stored dynamic argument before call",
        &call,
        retained585_leaf_expected(11, Reason::UnsupportedBasicContext),
    );
    let mut inert = base;
    inert
        .user_functions
        .push(retained585_function(::mapping::FunctionId::new(7), 17));
    inert
        .expressions
        .push(expression(98, Expression::SourceDocumentPath));
    failure594_admit(
        "S594-22 dead ordinary document and unreferenced function stay inert",
        &inert,
    );
}

#[test]
fn private_failure_value_maps_keep_per_rule_priority_and_shared_public_refusals() {
    use codegen::OwnedSourceCapabilityReason as Reason;
    let mut base = failure594_program();
    base.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(vec![]).into(),
    });
    let mut later = base.clone();
    failure594_expression(
        &mut later,
        10,
        Expression::SourceField {
            path: path(&["Unused", "Value"]),
            frame: None,
        },
    );
    later.failure_rules.push(source_failure(
        &["Unused", "Rows"],
        codegen::FailureSelection::All,
        None,
    ));
    failure594_refuse(
        "S594-23 later dynamic iteration precedes retained map input phase",
        &later,
        expected_failure_diagnostic(1, None, Reason::DynamicLoader),
    );
    let mut predicate = base.clone();
    predicate
        .expressions
        .push(failure_legacy(50, &["Unused", "Value"], None));
    predicate.failure_rules[0].selection = codegen::FailureSelection::WhenTrue(50);
    failure594_refuse(
        "S594-24 ordinary dynamic predicate keeps exact failure origin",
        &predicate,
        expected_failure_diagnostic(0, Some(50), Reason::DynamicLoader),
    );
    let mut message = base.clone();
    message
        .expressions
        .push(failure_legacy(50, &["Unused", "Value"], None));
    message.failure_rules[0].message = Some(50);
    failure594_refuse(
        "S594-25 ordinary dynamic message remains potential",
        &message,
        expected_failure_diagnostic(0, Some(50), Reason::DynamicLoader),
    );
    let mut specialized = base.clone();
    specialized
        .expressions
        .push(expression(50, Expression::SourceDocumentPath));
    specialized.failure_rules.push(source_failure(
        &["Rows"],
        codegen::FailureSelection::All,
        Some(50),
    ));
    failure594_refuse(
        "S594-26 later specialized failure remains closed",
        &specialized,
        expected_failure_diagnostic(1, None, Reason::FailureRule),
    );
    let expected = expected_failure_diagnostic(0, None, Reason::FailureRule);
    let shared = codegen::validate_basic_owned_emission(&base);
    eprintln!("INPUT S594-27 full Program={base:#?} RAW shared={shared:#?} EXPECTED={expected:#?}");
    assert_eq!(shared, Err(expected.clone()));
    let public = public_basic_result("S594-27 unchanged public failure ValueMap gate", &base);
    match public {
        Err(EmitError::ProgramValidation(actual)) => assert_eq!(actual, expected),
        other => panic!("S594-27 complete public failure ValueMap refusal differs: {other:#?}"),
    }
}

#[test]
fn private_failure_value_maps_admit_lazy_inputs_and_frozen_matched_host_programs() {
    let mut lazy = failure594_program();
    lazy.expressions.extend([
        expression(
            11,
            Expression::Const {
                value: mapped_string("unchosen raise"),
            },
        ),
        expression(50, Expression::Raise { message: Some(11) }),
        expression(
            51,
            Expression::Const {
                value: mapped_string("static"),
            },
        ),
        expression(
            52,
            Expression::If {
                condition: 20,
                then: 50,
                else_: 51,
            },
        ),
    ]);
    failure594_expression(
        &mut lazy,
        40,
        target_value_map(
            52,
            None,
            vec![(mapped_string("static"), mapped_string("mapped"))],
            None,
        ),
    );
    failure594_admit(
        "S594-28 ordinary lazy If and Raise input source admission",
        &lazy,
    );
    failure594_admit(
        "S594-29 frozen matched predicate/message/default host Program",
        &failure594_matched_program_one(),
    );
    failure594_admit(
        "S594-30 frozen matched runtime-default/lazy/error host Program",
        &failure594_matched_program_two(),
    );
}
