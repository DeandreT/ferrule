use super::*;

// #595 retains complete upstream/rendered observations before comparisons.
const PRIVATE_FAILURE595_VARIANTS: [&str; 30] = [
    "Aggregate",
    "Call",
    "CollectionFind",
    "Const",
    "DelimitedTextField",
    "DynamicSourceField",
    "FunctionParameter",
    "If",
    "JoinAggregate",
    "JoinField",
    "JoinPosition",
    "Lookup",
    "OwnedPosition",
    "OwnedSourceField",
    "Position",
    "Raise",
    "RuntimeParameter",
    "RuntimeParameterDefault",
    "RuntimeValue",
    "SequenceAggregate",
    "SequenceExists",
    "SequenceItemAt",
    "SourceDocumentPath",
    "SourceField",
    "SourceRootField",
    "SourceRootXmlTypeEquals",
    "UserFunctionCall",
    "ValueMap",
    "XmlMixedContent",
    "XmlSerialize",
];
fn private_failure595_program() -> Program {
    let mut p = failure586_program();
    p.failure_rules[0].selection = codegen::FailureSelection::All;
    p.expressions.extend([
        node(
            31,
            Expression::Const {
                value: Value::Int(1),
            },
        ),
        node(
            32,
            Expression::SourceField {
                frame: None,
                path: Vec::new(),
            },
        ),
    ]);
    p
}
fn private_failure595_sequence() -> codegen::GeneratedSequence {
    codegen::GeneratedSequence::Range {
        from: None,
        to: 31,
        item: 32,
    }
}
fn private_failure595_function(
    id: u64,
    output: u32,
    with_argument: bool,
) -> codegen::UserFunctionProgram {
    codegen::UserFunctionProgram {
        id: ::mapping::FunctionId::new(id),
        library: "fixture595".into(),
        name: format!("scalar_{id}"),
        parameters: if with_argument {
            vec![codegen::UserFunctionParameter {
                id: ::mapping::FunctionParameterId::new(1),
                ty: ScalarType::String,
            }]
        } else {
            Vec::new()
        },
        output_type: ScalarType::String,
        expressions: vec![node(
            output,
            Expression::Const {
                value: Value::String("body595".into()),
            },
        )],
        output,
    }
}
fn private_failure595_owned(
    node: u32,
    position_request: bool,
    unknown: bool,
) -> ProgramValidationError {
    use codegen::{
        OwnedSourceDiagnostic as D, OwnedSourceDiagnosticLocation as L, OwnedSourceNamespace as N,
    };
    let location = L::Expression {
        namespace: N::Main,
        node,
        scope: None,
    };
    let owner = if unknown {
        codegen::OwnedSourceDiagnosticOwner::Named {
            name: "Absent595".into(),
        }
    } else {
        codegen::OwnedSourceDiagnosticOwner::Primary
    };
    let request = if position_request {
        codegen::OwnedSourceDiagnosticRequest::Position {
            owner,
            collection: None,
        }
    } else {
        codegen::OwnedSourceDiagnosticRequest::Field {
            owner,
            path: path(&["Value"]),
            frame: None,
        }
    };
    let diagnostic = if unknown {
        D::Structure {
            location,
            request,
            reason: codegen::OwnedSourceStructureReason::UnknownOwner,
        }
    } else {
        D::Capability {
            location,
            request: Some(request),
            reason: codegen::OwnedSourceCapabilityReason::FailureRule,
            origin: codegen::OwnedSourceCapabilityOrigin::Failure { index: 0 },
        }
    };
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(diagnostic),
    }
}
fn private_failure595_called_body() -> ProgramValidationError {
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Function(::mapping::FunctionId::new(9)),
                node: 29,
                scope: None,
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::IsolatedFunction,
            origin: codegen::OwnedSourceCapabilityOrigin::FunctionBody {
                function: ::mapping::FunctionId::new(9),
            },
        }),
    }
}
fn private_failure595_observe(
    label: &str,
    p: &Program,
    validation_error: Option<ProgramValidationError>,
    render_error: Option<ProgramValidationError>,
) {
    let before = p.clone();
    eprintln!("INPUT {label} complete Program={p:#?}");
    let validation = codegen::validate_program(p);
    eprintln!("RAW {label} complete upstream Result={validation:#?}");
    let actual = failure_source_result(label, p);
    eprintln!(
        "EXPECTED {label} complete upstream={validation_error:#?} renderer={render_error:#?}"
    );
    assert_eq!(
        validation,
        validation_error.map_or(Ok(()), Err),
        "{label}: upstream classification"
    );
    match render_error {
        Some(expected) => match actual {
            Err(EmitError::InvalidProgram(actual)) => {
                assert_eq!(actual, expected, "{label}: full envelope")
            }
            other => panic!("{label}: complete captured renderer differs: {other:#?}"),
        },
        None => assert!(
            actual.is_ok(),
            "{label}: full accepted source captured above"
        ),
    }
    assert_eq!(p, &before, "{label}: immutable Program");
}
fn private_failure595_fixture(
    name: &str,
) -> (
    Program,
    Option<ProgramValidationError>,
    Option<ProgramValidationError>,
) {
    let mut p = private_failure595_program();
    p.failure_rules[0].message = Some(40);
    let mut upstream = None;
    let mut rendered = Some(expected_failure_capability(
        0,
        codegen::OwnedSourceCapabilityReason::FailureRule,
    ));
    let expression = match name {
        "Aggregate" => Expression::Aggregate {
            function: codegen::AggregateFunction::Count,
            collection: path(&["Rows"]),
            value: codegen::AggregateValue::Path(path(&["Value"])),
            arg: None,
        },
        "Call" => {
            rendered = None;
            Expression::Call {
                function: ScalarFunction::Upper,
                args: vec![11],
            }
        }
        "CollectionFind" => Expression::CollectionFind {
            collection: path(&["Rows"]),
            predicate: 10,
            value: 11,
        },
        "Const" => {
            rendered = None;
            Expression::Const {
                value: Value::String("literal595".into()),
            }
        }
        "DelimitedTextField" => {
            let layout = ::mapping::FlexTextLayout::new(
                "Root",
                ::mapping::FlexCommand::DelimitedRecords {
                    name: "Row".into(),
                    dialect: ::mapping::DelimitedDialect::new_with_field_separator(
                        ",", "\n", '"', '"',
                    )
                    .unwrap(),
                    fields: vec![
                        ::mapping::DelimitedRecordField::new("Value", ScalarType::String).unwrap(),
                    ],
                },
                ::mapping::FlexLineEnding::Lf,
                false,
            )
            .unwrap();
            let descriptor = serde_json::to_string(&layout).unwrap();
            Expression::DelimitedTextField {
                input: 11,
                parser: codegen::DelimitedTextField::from_descriptors(
                    &descriptor,
                    r#"["Row","Value"]"#,
                )
                .unwrap(),
            }
        }
        "DynamicSourceField" => {
            rendered = Some(failure586_dynamic(0, Some(40)));
            Expression::DynamicSourceField {
                object: Vec::new(),
                frame: None,
                key: 11,
            }
        }
        "FunctionParameter" => {
            upstream = Some(ProgramValidationError::FunctionParameterInMain {
                node: 40,
                parameter: ::mapping::FunctionParameterId::new(595),
            });
            rendered = upstream.clone();
            Expression::FunctionParameter {
                parameter: ::mapping::FunctionParameterId::new(595),
            }
        }
        "If" => {
            rendered = None;
            Expression::If {
                condition: 10,
                then: 11,
                else_: 11,
            }
        }
        "JoinAggregate" => {
            upstream = Some(ProgramValidationError::JoinAggregateRequiresRootContext {
                node: 40,
                join: codegen::JoinId::new(595),
            });
            rendered = upstream.clone();
            p.extra_sources.push(NamedSourceProgram {
                name: "JoinR595".into(),
                source: p.source.clone(),
                dynamic: None,
            });
            let plan = codegen::JoinPlan::new(
                codegen::JoinSource::new(path(&["Rows", "Rows"])),
                codegen::JoinSource::new(path(&["JoinR595", "Rows"])),
                codegen::JoinConditions::new(codegen::JoinKey::new(
                    path(&["Rows", "Rows"]),
                    path(&["Value"]),
                    path(&["Value"]),
                )),
            )
            .unwrap();
            Expression::JoinAggregate {
                function: codegen::AggregateFunction::Count,
                join: codegen::InnerJoin::new(codegen::JoinId::new(595), plan),
                expression: None,
                arg: None,
            }
        }
        "JoinField" => {
            upstream = Some(ProgramValidationError::InactiveJoinExpression {
                node: 40,
                join: codegen::JoinId::new(595),
            });
            rendered = upstream.clone();
            Expression::JoinField {
                join: codegen::JoinId::new(595),
                collection: path(&["Rows"]),
                path: path(&["Value"]),
            }
        }
        "JoinPosition" => {
            upstream = Some(ProgramValidationError::InactiveJoinExpression {
                node: 40,
                join: codegen::JoinId::new(595),
            });
            rendered = upstream.clone();
            Expression::JoinPosition {
                join: codegen::JoinId::new(595),
            }
        }
        "Lookup" => Expression::Lookup {
            collection: path(&["Rows"]),
            key: path(&["Flag"]),
            matches: 10,
            value: path(&["Value"]),
        },
        "OwnedPosition" => {
            upstream = Some(private_failure595_owned(40, true, false));
            rendered = upstream.clone();
            position(SourceOwner::Primary, None)
        }
        "OwnedSourceField" => {
            upstream = Some(private_failure595_owned(40, false, false));
            rendered = upstream.clone();
            owned(SourceOwner::Primary, &["Value"], None)
        }
        "Position" => {
            rendered = None;
            Expression::Position {
                collection: path(&["Rows"]),
            }
        }
        "Raise" => {
            rendered = None;
            Expression::Raise { message: Some(11) }
        }
        "RuntimeParameter" => {
            rendered = None;
            Expression::RuntimeParameter {
                name: "parameter595".into(),
                ty: ScalarType::String,
            }
        }
        "RuntimeParameterDefault" => {
            rendered = None;
            Expression::RuntimeParameterDefault {
                name: "default595".into(),
                ty: ScalarType::String,
                default: 11,
            }
        }
        "RuntimeValue" => {
            rendered = None;
            Expression::RuntimeValue {
                value: codegen::RuntimeValue::MappingFilePath,
            }
        }
        "SequenceAggregate" => Expression::SequenceAggregate {
            function: codegen::AggregateFunction::Count,
            sequence: private_failure595_sequence(),
            predicate: None,
            expression: None,
            arg: None,
        },
        "SequenceExists" => Expression::SequenceExists {
            sequence: private_failure595_sequence(),
            predicate: 10,
        },
        "SequenceItemAt" => Expression::SequenceItemAt {
            sequence: private_failure595_sequence(),
            index: 31,
        },
        "SourceDocumentPath" => Expression::SourceDocumentPath,
        "SourceField" => {
            rendered = None;
            Expression::SourceField {
                frame: None,
                path: path(&["Value"]),
            }
        }
        "SourceRootField" => {
            upstream = Some(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 40 });
            rendered = upstream.clone();
            Expression::SourceRootField {
                path: path(&["Value"]),
                required: false,
            }
        }
        "SourceRootXmlTypeEquals" => {
            upstream = Some(ProgramValidationError::PrimaryRootRequiresStaticBinding { node: 40 });
            rendered = upstream.clone();
            Expression::SourceRootXmlTypeEquals {
                canonical_expanded_type: "{urn:fixture595}Input".into(),
            }
        }
        "UserFunctionCall" => {
            p.user_functions
                .push(private_failure595_function(595, 1595, false));
            Expression::UserFunctionCall {
                function: ::mapping::FunctionId::new(595),
                args: Vec::new(),
            }
        }
        "ValueMap" => {
            rendered = None;
            Expression::ValueMap {
                input: 11,
                input_type: None,
                table: vec![(
                    Value::String("static".into()),
                    Value::String("mapped595".into()),
                )],
                default: None,
            }
        }
        "XmlMixedContent" => {
            let mut text = SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String);
            text.text = true;
            let ir::SchemaKind::Group { children, .. } = &mut p.source.kind else {
                unreachable!()
            };
            children.push(text);
            Expression::XmlMixedContent {
                frame: None,
                path: Vec::new(),
                replacements: Vec::new(),
            }
        }
        "XmlSerialize" => Expression::XmlSerialize {
            frame: None,
            path: Vec::new(),
            schema: Box::new(p.source.clone()),
            declaration: false,
            indent: false,
            namespace: None,
        },
        other => panic!("unlisted expression variant {other}"),
    };
    p.expressions.push(node(40, expression));
    (p, upstream, rendered)
}
#[test]
fn private_failure595_covers_all_thirty_variants_and_upstream_boundaries() {
    assert_eq!(
        PRIVATE_FAILURE595_VARIANTS
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        30
    );
    for (index, name) in PRIVATE_FAILURE595_VARIANTS.into_iter().enumerate() {
        let (p, upstream, rendered) = private_failure595_fixture(name);
        private_failure595_observe(
            &format!("F595-M{:02} {name}", index + 1),
            &p,
            upstream,
            rendered,
        );
    }
}

#[test]
fn private_failure595_preserves_per_rule_operand_and_stored_priority() {
    for specialized_first in [false, true] {
        let mut p = private_failure595_program();
        p.failure_rules[0].message = Some(60);
        p.expressions.push(node(
            60,
            Expression::If {
                condition: 10,
                then: 40,
                else_: 12,
            },
        ));
        let specialized = node(40, Expression::SourceDocumentPath);
        if specialized_first {
            p.expressions.insert(0, specialized);
        } else {
            p.expressions.push(specialized);
        }
        let expected = if specialized_first {
            expected_failure_capability(0, codegen::OwnedSourceCapabilityReason::FailureRule)
        } else {
            failure586_dynamic(0, Some(12))
        };
        private_failure595_observe(
            "F595-P01 stored priority even unselected branch",
            &p,
            None,
            Some(expected),
        );
    }
    for specialized_predicate in [false, true] {
        let mut p = private_failure595_program();
        p.expressions.push(node(40, Expression::SourceDocumentPath));
        p.failure_rules[0].selection =
            codegen::FailureSelection::WhenTrue(if specialized_predicate { 40 } else { 13 });
        p.failure_rules[0].message = Some(if specialized_predicate { 12 } else { 40 });
        let expected = if specialized_predicate {
            expected_failure_capability(0, codegen::OwnedSourceCapabilityReason::FailureRule)
        } else {
            failure586_dynamic(0, Some(13))
        };
        private_failure595_observe(
            "F595-P02 predicate before message",
            &p,
            None,
            Some(expected),
        );
    }
    for later_generated in [false, true] {
        let mut p = private_failure595_program();
        p.expressions.push(node(40, Expression::SourceDocumentPath));
        p.failure_rules[0].message = Some(40);
        p.failure_rules.push(codegen::FailureRule {
            iteration: if later_generated {
                codegen::FailureIteration::Generated(private_failure595_sequence())
            } else {
                codegen::FailureIteration::Source(codegen::SourceIteration::new(path(&[
                    "D", "Rows",
                ])))
            },
            selection: codegen::FailureSelection::All,
            message: Some(11),
        });
        private_failure595_observe(
            "F595-P03 earlier specialized before later iteration",
            &p,
            None,
            Some(expected_failure_capability(
                0,
                codegen::OwnedSourceCapabilityReason::FailureRule,
            )),
        );
    }
    for index in 0..3 {
        let mut p = private_failure595_program();
        p.expressions.push(node(40, Expression::SourceDocumentPath));
        p.failure_rules = (0..=index)
            .map(|i| {
                failure_rule(
                    &["Rows"],
                    codegen::FailureSelection::All,
                    Some(if i == index { 40 } else { 11 }),
                )
            })
            .collect();
        private_failure595_observe(
            "F595-P04 exact rule index",
            &p,
            None,
            Some(expected_failure_capability(
                index,
                codegen::OwnedSourceCapabilityReason::FailureRule,
            )),
        );
    }
    for generated in [false, true] {
        let mut p = private_failure595_program();
        p.expressions.push(node(40, Expression::SourceDocumentPath));
        p.failure_rules[0].message = Some(40);
        p.failure_rules[0].iteration = if generated {
            codegen::FailureIteration::Generated(private_failure595_sequence())
        } else {
            codegen::FailureIteration::Source(codegen::SourceIteration::new(path(&["D", "Rows"])))
        };
        let expected = expected_failure_capability(
            0,
            if generated {
                codegen::OwnedSourceCapabilityReason::FailureRule
            } else {
                codegen::OwnedSourceCapabilityReason::DynamicLoader
            },
        );
        private_failure595_observe(
            "F595-P05 iteration precedes operands",
            &p,
            None,
            Some(expected),
        );
    }
}
#[test]
fn private_failure595_preserves_value_map_barriers_and_general_inventory() {
    let mut map = private_failure595_program();
    map.expressions.extend([
        node(40, Expression::SourceDocumentPath),
        node(
            50,
            Expression::ValueMap {
                input: 40,
                input_type: None,
                table: Vec::new(),
                default: None,
            },
        ),
    ]);
    map.failure_rules[0].message = Some(50);
    private_failure595_observe(
        "F595-B01 ValueMap input remains general",
        &map,
        None,
        Some(failure586_retained(40)),
    );
    let mut target = private_failure595_program();
    target
        .expressions
        .push(node(40, Expression::SourceDocumentPath));
    target.root.bindings[0].expression = 40;
    private_failure595_observe(
        "F595-B02 target envelope unchanged",
        &target,
        None,
        Some(failure586_retained(40)),
    );
    let mut dead = private_failure595_program();
    dead.expressions
        .push(node(40, Expression::SourceDocumentPath));
    private_failure595_observe("F595-B03 dead specialized remains inert", &dead, None, None);
    for unknown in [false, true] {
        let mut p = private_failure595_program();
        p.expressions.extend([
            node(
                40,
                owned(
                    if unknown {
                        named("Absent595")
                    } else {
                        SourceOwner::Primary
                    },
                    &["Value"],
                    None,
                ),
            ),
            node(
                50,
                Expression::ValueMap {
                    input: 40,
                    input_type: None,
                    table: Vec::new(),
                    default: None,
                },
            ),
        ]);
        p.failure_rules[0].message = Some(50);
        let expected = private_failure595_owned(40, false, unknown);
        private_failure595_observe(
            "F595-B04 upstream owned validation crosses map barrier",
            &p,
            Some(expected.clone()),
            Some(expected),
        );
    }
    let mut missing = private_failure595_program();
    missing.expressions.push(node(
        50,
        Expression::ValueMap {
            input: 595595,
            input_type: None,
            table: Vec::new(),
            default: None,
        },
    ));
    missing.failure_rules[0].message = Some(50);
    let expected = ProgramValidationError::MissingDependency {
        node: 50,
        dependency: 595595,
    };
    private_failure595_observe(
        "F595-B05 whole-graph missing map input first",
        &missing,
        Some(expected.clone()),
        Some(expected),
    );
}
#[test]
fn private_failure595_aligns_called_failure_and_preserves_actual_target_body() {
    let mut base = private_failure595_program();
    base.user_functions = vec![
        private_failure595_function(7, 17, false),
        private_failure595_function(9, 29, true),
    ];
    base.expressions.extend([
        node(
            40,
            Expression::UserFunctionCall {
                function: ::mapping::FunctionId::new(9),
                args: vec![41],
            },
        ),
        node(
            41,
            Expression::Const {
                value: Value::String("argument595".into()),
            },
        ),
    ]);
    let mut direct = base.clone();
    direct.failure_rules[0].message = Some(40);
    private_failure595_observe(
        "F595-C01 actual called failure is per-rule",
        &direct,
        None,
        Some(expected_failure_capability(
            0,
            codegen::OwnedSourceCapabilityReason::FailureRule,
        )),
    );
    let mut target = base.clone();
    target.root.bindings[0].expression = 40;
    private_failure595_observe(
        "F595-C02 target actual body skips unrelated definition",
        &target,
        None,
        Some(private_failure595_called_body()),
    );
    let mut map = base.clone();
    map.expressions.push(node(
        50,
        Expression::ValueMap {
            input: 40,
            input_type: None,
            table: Vec::new(),
            default: None,
        },
    ));
    map.failure_rules[0].message = Some(50);
    private_failure595_observe(
        "F595-C03 map input call retains actual-body envelope",
        &map,
        None,
        Some(private_failure595_called_body()),
    );
    let mut scalar = base.clone();
    scalar.expressions.push(node(
        50,
        Expression::Call {
            function: ScalarFunction::Upper,
            args: vec![40],
        },
    ));
    scalar.failure_rules[0].message = Some(50);
    private_failure595_observe(
        "F595-C04 ordinary scalar potential called dependency",
        &scalar,
        None,
        Some(expected_failure_capability(
            0,
            codegen::OwnedSourceCapabilityReason::FailureRule,
        )),
    );
    let mut invalid = direct;
    failure586_expression(
        &mut invalid,
        41,
        owned(named("Absent595"), &["Value"], None),
    );
    let expected = private_failure595_owned(41, false, true);
    private_failure595_observe(
        "F595-C05 called argument owned structure first",
        &invalid,
        Some(expected.clone()),
        Some(expected),
    );
    for argument_first in [false, true] {
        let mut p = base.clone();
        p.expressions.retain(|n| n.id != 40 && n.id != 41);
        let call = node(
            40,
            Expression::UserFunctionCall {
                function: ::mapping::FunctionId::new(9),
                args: vec![41],
            },
        );
        let argument = node(41, Expression::SourceDocumentPath);
        p.expressions.extend(if argument_first {
            [argument, call]
        } else {
            [call, argument]
        });
        let expected = if argument_first {
            failure586_retained(41)
        } else {
            private_failure595_called_body()
        };
        private_failure595_observe(
            "F595-C06 stored general call/argument priority unchanged",
            &p,
            None,
            Some(expected),
        );
    }
}
