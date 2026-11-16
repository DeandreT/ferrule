use super::*;

// #586 source controls: full typed refusals are retained before comparisons.
pub(super) fn failure586_program() -> Program {
    let mut program = failure_program(
        vec![
            node(
                10,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ),
            node(
                11,
                Expression::Const {
                    value: Value::String("static".into()),
                },
            ),
            node(
                12,
                Expression::SourceField {
                    path: path(&["D", "Value"]),
                    frame: None,
                },
            ),
            node(
                13,
                Expression::SourceField {
                    path: path(&["D", "Rows", "Flag"]),
                    frame: None,
                },
            ),
            node(
                90,
                Expression::Const {
                    value: Value::String("d.json".into()),
                },
            ),
        ],
        vec![failure_rule(
            &["Rows"],
            codegen::FailureSelection::WhenTrue(10),
            Some(11),
        )],
    );
    program.extra_sources.push(NamedSourceProgram {
        name: "D".into(),
        source: source_schema(),
        dynamic: Some(codegen::DynamicSourceProgram {
            path: 90,
            driver: codegen::SourceIteration::new(path(&["Rows"])).into(),
        }),
    });
    program
}

pub(super) fn failure586_expression(program: &mut Program, id: u32, expression: Expression) {
    program
        .expressions
        .iter_mut()
        .find(|node| node.id == id)
        .unwrap()
        .expression = expression;
}

pub(super) fn failure586_dynamic(index: usize, node: Option<u32>) -> ProgramValidationError {
    match node {
        None => {
            expected_failure_capability(index, codegen::OwnedSourceCapabilityReason::DynamicLoader)
        }
        Some(node) => ProgramValidationError::OwnedSource {
            diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
                location: codegen::OwnedSourceDiagnosticLocation::Expression {
                    namespace: codegen::OwnedSourceNamespace::Main,
                    node,
                    scope: None,
                },
                request: None,
                reason: codegen::OwnedSourceCapabilityReason::DynamicLoader,
                origin: codegen::OwnedSourceCapabilityOrigin::Failure { index },
            }),
        },
    }
}

pub(super) fn failure586_retained(node: u32) -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node,
                scope: None,
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::UnsupportedBasicContext,
            origin: codegen::OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node,
            },
        }),
    }
}

fn failure586_accept(label: &str, program: &Program) {
    let actual = failure_source_result(label, program);
    assert!(
        actual.is_ok(),
        "{label}: complete input/result/cause retained"
    );
}

#[test]
fn private_dynamic_failure_diagnostics_keep_rule_index_and_per_rule_priority() {
    use codegen::FailureSelection::{All, WhenTrue};
    let base = failure586_program();
    let mut cases = Vec::new();
    let mut iteration = base.clone();
    iteration.failure_rules[0] = failure_rule(&["D", "Rows"], WhenTrue(13), Some(12));
    cases.push((
        "D586-01 iteration before predicate/message",
        iteration,
        failure586_dynamic(0, None),
    ));
    let mut second_iteration = base.clone();
    second_iteration
        .failure_rules
        .push(failure_rule(&["D", "Rows"], All, None));
    cases.push((
        "D586-02 exact second iteration index",
        second_iteration,
        failure586_dynamic(1, None),
    ));
    let mut early_predicate = base.clone();
    early_predicate.failure_rules[0].selection = WhenTrue(13);
    early_predicate
        .failure_rules
        .push(failure_rule(&["D", "Rows"], All, None));
    cases.push((
        "D586-03 earlier predicate before later iteration",
        early_predicate,
        failure586_dynamic(0, Some(13)),
    ));
    let mut early_message = base.clone();
    early_message.failure_rules[0].message = Some(12);
    early_message
        .failure_rules
        .push(failure_rule(&["D", "Rows"], All, None));
    cases.push((
        "D586-04 earlier message before later iteration",
        early_message,
        failure586_dynamic(0, Some(12)),
    ));
    let mut predicate_first = base.clone();
    predicate_first.failure_rules[0] = failure_rule(&["Rows"], WhenTrue(13), Some(12));
    cases.push((
        "D586-05 predicate before earlier-stored message node",
        predicate_first,
        failure586_dynamic(0, Some(13)),
    ));
    let mut false_message = base.clone();
    false_message.failure_rules[0].message = Some(12);
    cases.push((
        "D586-06 potentially reached message despite false predicate",
        false_message,
        failure586_dynamic(0, Some(12)),
    ));
    let mut second_predicate = base.clone();
    second_predicate
        .failure_rules
        .push(failure_rule(&["Rows"], WhenTrue(13), None));
    cases.push((
        "D586-07 exact second predicate index",
        second_predicate,
        failure586_dynamic(1, Some(13)),
    ));
    let mut third_message = base.clone();
    third_message.failure_rules.extend([
        failure_rule(&["Rows"], All, None),
        failure_rule(&["Rows"], All, Some(12)),
    ]);
    cases.push((
        "D586-08 exact third message index",
        third_message,
        failure586_dynamic(2, Some(12)),
    ));
    let mut dynamic_operator = base.clone();
    dynamic_operator.expressions.push(node(
        15,
        Expression::DynamicSourceField {
            object: path(&["D"]),
            frame: None,
            key: 11,
        },
    ));
    dynamic_operator.failure_rules[0].message = Some(15);
    cases.push((
        "D586-09 dynamic field exact node",
        dynamic_operator,
        failure586_dynamic(0, Some(15)),
    ));
    let generated = |program: &mut Program| {
        program.expressions.extend([
            node(
                30,
                Expression::Const {
                    value: Value::Int(2),
                },
            ),
            node(
                31,
                Expression::SourceField {
                    path: vec![],
                    frame: None,
                },
            ),
        ]);
        codegen::FailureRule {
            iteration: codegen::FailureIteration::Generated(codegen::GeneratedSequence::Range {
                from: None,
                to: 30,
                item: 31,
            }),
            selection: All,
            message: None,
        }
    };
    let mut early_dynamic = base.clone();
    early_dynamic.failure_rules[0].selection = WhenTrue(13);
    let later_generated = generated(&mut early_dynamic);
    early_dynamic.failure_rules.push(later_generated);
    cases.push((
        "D586-10 earlier dynamic before later unsupported iteration",
        early_dynamic,
        failure586_dynamic(0, Some(13)),
    ));
    let mut early_generated = base;
    early_generated.failure_rules[0] = generated(&mut early_generated);
    early_generated
        .failure_rules
        .push(failure_rule(&["Rows"], WhenTrue(13), None));
    cases.push((
        "D586-11 earlier unsupported iteration remains first",
        early_generated,
        expected_failure_capability(0, codegen::OwnedSourceCapabilityReason::FailureRule),
    ));
    for (label, program, expected) in cases {
        check_failure_source_error(label, &program, expected);
    }
}

#[test]
fn private_dynamic_failure_frames_exact_names_and_dead_leaves_keep_admission_identity() {
    let base = failure586_program();
    failure586_accept("D586-12 dead dynamic leaf and unused metadata", &base);
    let mut message = base.clone();
    message.failure_rules[0].message = Some(12);
    check_failure_source_error(
        "D586-13 dynamic path",
        &message,
        failure586_dynamic(0, Some(12)),
    );
    let mut frame = message.clone();
    failure586_expression(
        &mut frame,
        12,
        Expression::SourceField {
            path: path(&["Value"]),
            frame: Some(path(&["D", "Rows"])),
        },
    );
    check_failure_source_error(
        "D586-14 dynamic explicit frame",
        &frame,
        failure586_dynamic(0, Some(12)),
    );
    let mut empty = message.clone();
    failure586_expression(
        &mut empty,
        12,
        Expression::SourceField {
            path: path(&["D", "Value"]),
            frame: Some(vec![]),
        },
    );
    failure586_accept("D586-15 present empty frame replaces path", &empty);
    let mut static_frame = message.clone();
    failure586_expression(
        &mut static_frame,
        12,
        Expression::SourceField {
            path: path(&["D", "Value"]),
            frame: Some(path(&["Rows"])),
        },
    );
    failure586_accept(
        "D586-16 static explicit frame replaces dynamic path",
        &static_frame,
    );
    let mut static_source = message.clone();
    static_source.extra_sources.last_mut().unwrap().dynamic = None;
    failure586_accept("D586-17 exact same-name static metadata", &static_source);
    let mut case = message.clone();
    case.extra_sources.push(NamedSourceProgram {
        name: "d".into(),
        source: source_schema(),
        dynamic: None,
    });
    failure586_expression(
        &mut case,
        12,
        Expression::SourceField {
            path: path(&["d", "Value"]),
            frame: None,
        },
    );
    failure586_accept("D586-18 names preserve case", &case);
    let mut unicode = message.clone();
    unicode.extra_sources.last_mut().unwrap().name = " D雪 ".into();
    failure586_expression(
        &mut unicode,
        12,
        Expression::SourceField {
            path: path(&[" D雪 ", "Value"]),
            frame: None,
        },
    );
    check_failure_source_error(
        "D586-19 exact whitespace Unicode name",
        &unicode,
        failure586_dynamic(0, Some(12)),
    );
    let mut untrimmed = unicode;
    failure586_expression(
        &mut untrimmed,
        12,
        Expression::SourceField {
            path: path(&["D雪", "Value"]),
            frame: None,
        },
    );
    failure586_accept("D586-20 names are not trimmed", &untrimmed);
    let mut primary_named = message.clone();
    primary_named
        .extra_sources
        .retain(|source| source.name != "primary");
    primary_named.extra_sources.last_mut().unwrap().name = "primary".into();
    failure586_expression(
        &mut primary_named,
        12,
        Expression::SourceField {
            path: path(&["primary", "Value"]),
            frame: None,
        },
    );
    check_failure_source_error(
        "D586-21 legacy named primary remains exact metadata",
        &primary_named,
        failure586_dynamic(0, Some(12)),
    );
}

#[test]
fn private_dynamic_failure_potential_dependencies_keep_stored_node_priority() {
    let base = failure586_program();
    let branches = |program: &mut Program| {
        program.expressions.extend([
            node(
                25,
                Expression::SourceField {
                    path: path(&["D", "Value"]),
                    frame: None,
                },
            ),
            node(
                20,
                Expression::If {
                    condition: 10,
                    then: 12,
                    else_: 25,
                },
            ),
        ]);
        program.failure_rules[0].message = Some(20);
    };
    let mut forward = base.clone();
    branches(&mut forward);
    check_failure_source_error(
        "D586-22 potential If branches stored order",
        &forward,
        failure586_dynamic(0, Some(12)),
    );
    let mut reversed = forward;
    reversed.expressions.sort_by_key(|node| {
        if node.id == 25 {
            0
        } else if node.id == 12 {
            1
        } else {
            2
        }
    });
    check_failure_source_error(
        "D586-23 stored vector wins over IDs and branch order",
        &reversed,
        failure586_dynamic(0, Some(25)),
    );
    let mut default = base.clone();
    default.expressions.push(node(
        21,
        Expression::RuntimeParameterDefault {
            name: "filled".into(),
            ty: ScalarType::String,
            default: 12,
        },
    ));
    default.failure_rules[0].message = Some(21);
    check_failure_source_error(
        "D586-24 potential parameter default",
        &default,
        failure586_dynamic(0, Some(12)),
    );
    let mut raise = base.clone();
    raise
        .expressions
        .push(node(22, Expression::Raise { message: Some(12) }));
    raise.failure_rules[0].message = Some(22);
    check_failure_source_error(
        "D586-25 potential Raise message",
        &raise,
        failure586_dynamic(0, Some(12)),
    );
    let mut call = base;
    call.expressions.push(node(
        23,
        Expression::Call {
            function: ScalarFunction::Trim,
            args: vec![12],
        },
    ));
    call.failure_rules[0].message = Some(23);
    check_failure_source_error(
        "D586-26 potential ordinary call argument",
        &call,
        failure586_dynamic(0, Some(12)),
    );
}

#[test]
fn private_dynamic_failure_preserves_target_and_owned_structural_priority() {
    let base = failure586_program();
    let mut target = base.clone();
    target.failure_rules.clear();
    failure586_expression(
        &mut target,
        100,
        Expression::SourceField {
            path: path(&["D", "Value"]),
            frame: None,
        },
    );
    check_failure_source_error(
        "D586-27 target retained envelope unchanged",
        &target,
        failure586_retained(100),
    );
    let mut structural = base.clone();
    structural.failure_rules[0] =
        failure_rule(&["D", "Rows"], codegen::FailureSelection::All, None);
    failure586_expression(
        &mut structural,
        100,
        owned(named("Missing"), &["Value"], None),
    );
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Structure {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 100,
                scope: None,
            },
            request: codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Named {
                    name: "Missing".into(),
                },
                path: path(&["Value"]),
                frame: None,
            },
            reason: codegen::OwnedSourceStructureReason::UnknownOwner,
        }),
    };
    check_failure_source_error(
        "D586-28 structure before dynamic failure",
        &structural,
        expected,
    );
    let mut owned_failure = base;
    owned_failure.failure_rules[0] =
        failure_rule(&["D", "Rows"], codegen::FailureSelection::All, Some(12));
    failure586_expression(
        &mut owned_failure,
        12,
        owned(SourceOwner::Primary, &["Value"], None),
    );
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 12,
                scope: None,
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Field {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Value"]),
                frame: None,
            }),
            reason: codegen::OwnedSourceCapabilityReason::FailureRule,
            origin: codegen::OwnedSourceCapabilityOrigin::Failure { index: 0 },
        }),
    };
    check_failure_source_error(
        "D586-29 owned failure context before dynamic iteration",
        &owned_failure,
        expected,
    );
}

#[test]
fn private_dynamic_failure_keeps_operator_envelopes_and_shared_public_gates() {
    let base = failure586_program();
    let mut specialized = base.clone();
    specialized.failure_rules[0].message = Some(12);
    failure586_expression(&mut specialized, 12, Expression::SourceDocumentPath);
    check_failure_source_error(
        "D586-30 specialized failure envelope matches CSharp",
        &specialized,
        expected_failure_capability(0, codegen::OwnedSourceCapabilityReason::FailureRule),
    );
    let mut value_map = base.clone();
    value_map.expressions.push(node(
        23,
        Expression::ValueMap {
            input: 11,
            input_type: None,
            table: vec![(
                Value::String("static".into()),
                Value::String("mapped".into()),
            )],
            default: None,
        },
    ));
    value_map.failure_rules[0].message = Some(23);
    failure586_accept(
        "D586-31 private static ValueMap envelope unchanged",
        &value_map,
    );
    let mut dynamic_map = value_map.clone();
    failure586_expression(
        &mut dynamic_map,
        23,
        Expression::ValueMap {
            input: 12,
            input_type: None,
            table: vec![],
            default: None,
        },
    );
    check_failure_source_error(
        "D586-32 ValueMap dependency keeps retained envelope",
        &dynamic_map,
        failure586_retained(12),
    );
    for (label, program) in [
        (
            "D586-33 shared/public ValueMap failure stays closed",
            value_map,
        ),
        (
            "D586-34 shared/public specialized failure stays closed",
            specialized,
        ),
    ] {
        let expected =
            expected_failure_capability(0, codegen::OwnedSourceCapabilityReason::FailureRule);
        let shared = codegen::validate_basic_owned_emission(&program);
        eprintln!(
            "INPUT {label} full Program={program:#?} RAW shared={shared:#?} EXPECTED={expected:#?}"
        );
        assert_eq!(shared, Err(expected.clone()), "{label}");
        let public = public519_result(label, &program, &public519_options("ferrule_diag_586"));
        match public {
            Err(EmitError::InvalidProgram(actual)) => assert_eq!(actual, expected, "{label}"),
            other => panic!("{label}: full captured public refusal differs: {other:#?}"),
        }
    }
    let mut dynamic = base;
    dynamic.failure_rules[0].message = Some(12);
    let expected = failure586_dynamic(0, Some(12));
    let shared = codegen::validate_basic_owned_emission(&dynamic);
    eprintln!(
        "INPUT D586-35 full Program={dynamic:#?} RAW shared={shared:#?} EXPECTED={expected:#?}"
    );
    assert_eq!(shared, Err(expected.clone()));
    let public = public519_result(
        "D586-35 unchanged public dynamic gate",
        &dynamic,
        &public519_options("ferrule_diag_586"),
    );
    match public {
        Err(EmitError::InvalidProgram(actual)) => assert_eq!(actual, expected),
        other => panic!("D586-35: complete captured public refusal differs: {other:#?}"),
    }
}
