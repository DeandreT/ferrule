use std::error::Error;

use crate::{
    Binding, DynamicSourceProgram, Expression, ExpressionNode, FailureIteration, FailureRule,
    FailureSelection, GeneratedSequence, IterationOutput, IterationPlan, NamedSourceProgram,
    NamedTargetProgram, OwnedSourceCapabilityOrigin as Origin,
    OwnedSourceCapabilityReason as Reason, OwnedSourceDiagnostic as Diagnostic,
    OwnedSourceDiagnosticLocation as Location, OwnedSourceDiagnosticOwner as Owner,
    OwnedSourceDiagnosticRequest as Request, OwnedSourceNamespace as Namespace,
    OwnedSourceStructureReason as Structure, Program, ProgramValidationError, ScalarTargetDomain,
    SourceIteration, TargetConstruction, TargetScope, UserFunctionProgram,
    validate_basic_owned_emission, validate_owned_emission, validate_program,
};
use ir::{ScalarType, SchemaNode, Value};
use mapping::{
    FunctionId, OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner, SourceScopeLocation,
    SourceScopeTarget,
};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn node(id: u32, expression: Expression) -> ExpressionNode {
    ExpressionNode { id, expression }
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
fn input_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group(
                "D",
                vec![
                    SchemaNode::scalar("Value", ScalarType::String),
                    SchemaNode::scalar("Flag", ScalarType::Bool),
                ],
            ),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ScalarType::String),
                    SchemaNode::scalar("Flag", ScalarType::Bool),
                ],
            )
            .repeating(),
        ],
    )
}
fn program(owned: bool) -> Program {
    Program {
        xml_boundary: None,
        source: input_schema(),
        extra_sources: ["Ref", " Ref 雪 ", "ref", "primary"]
            .into_iter()
            .map(|name| NamedSourceProgram {
                name: name.into(),
                source: input_schema(),
                dynamic: None,
            })
            .collect(),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        expressions: vec![node(
            1,
            if owned {
                field(SourceOwner::Primary, &["Value"], None)
            } else {
                Expression::Const {
                    value: Value::String("P.root".into()),
                }
            },
        )],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![Binding {
                target_field: "Value".into(),
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::String),
                repeating: false,
            }],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}
fn observed(label: &str, program: &Program) -> Result<(), ProgramValidationError> {
    eprintln!("INPUT {label} complete Program={program:#?}");
    let actual = validate_basic_owned_emission(program);
    eprintln!(
        "RAW {label} complete Result={actual:#?} source={:?}",
        actual
            .as_ref()
            .err()
            .and_then(|error| error.source())
            .map(ToString::to_string)
    );
    actual
}
fn capability(
    location: Location,
    request: Option<Request>,
    reason: Reason,
    origin: Origin,
) -> Result<(), ProgramValidationError> {
    Err(ProgramValidationError::OwnedSource {
        diagnostic: Box::new(Diagnostic::Capability {
            location,
            request,
            reason,
            origin,
        }),
    })
}
fn primary() -> SourceScopeLocation {
    SourceScopeLocation::root(SourceScopeTarget::Primary)
}
fn retained(node: u32, reason: Reason) -> Result<(), ProgramValidationError> {
    capability(
        Location::Expression {
            namespace: Namespace::Main,
            node,
            scope: None,
        },
        None,
        reason,
        Origin::RetainedExpression {
            namespace: Namespace::Main,
            node,
        },
    )
}
fn scope_error(reason: Reason) -> Result<(), ProgramValidationError> {
    capability(
        Location::Iteration { scope: primary() },
        None,
        reason,
        Origin::Scope { scope: primary() },
    )
}
fn failure_error(reason: Reason) -> Result<(), ProgramValidationError> {
    capability(
        Location::Iteration { scope: primary() },
        None,
        reason,
        Origin::Failure { index: 0 },
    )
}
fn unused_function(p: &mut Program) {
    p.user_functions.push(UserFunctionProgram {
        id: FunctionId::new(7),
        library: "fixture".into(),
        name: "unused".into(),
        parameters: Vec::new(),
        output_type: ScalarType::String,
        expressions: vec![node(
            0,
            Expression::Const {
                value: Value::String("unused".into()),
            },
        )],
        output: 0,
    });
}
fn dynamic(p: &mut Program) {
    p.expressions.push(node(
        80,
        Expression::Const {
            value: Value::String("unused.json".into()),
        },
    ));
    p.extra_sources.push(NamedSourceProgram {
        name: "D".into(),
        source: input_schema(),
        dynamic: Some(DynamicSourceProgram {
            path: 80,
            driver: SourceIteration::new(Vec::new()),
        }),
    });
}
fn repeat_target(p: &mut Program) {
    p.target.repeating = true;
    p.root.repeating = true;
}
fn value_map() -> Expression {
    value_map_from(1)
}
fn value_map_from(input: u32) -> Expression {
    Expression::ValueMap {
        input,
        input_type: None,
        table: vec![(
            Value::String("P.root".into()),
            Value::String("mapped".into()),
        )],
        default: None,
    }
}

#[test]
fn basic_gate_accepts_exact_owners_positions_and_scalar_root() {
    for owner in [
        SourceOwner::Primary,
        SourceOwner::Named { name: "Ref".into() },
        SourceOwner::Named {
            name: " Ref 雪 ".into(),
        },
        SourceOwner::Named { name: "ref".into() },
        SourceOwner::Named {
            name: "primary".into(),
        },
    ] {
        let mut p = program(true);
        p.expressions[0].expression = field(owner, &["Value"], None);
        let actual = observed("A01 exact owner", &p);
        assert_eq!(actual, Ok(()));
    }
    for collection in [None, Some(Vec::new())] {
        let mut p = program(false);
        p.expressions.push(node(
            99,
            Expression::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Primary,
                collection: collection.clone(),
            }),
        ));
        let actual = observed("A02 absent versus present root selector", &p);
        assert_eq!(actual, Ok(()));
        assert_eq!(
            p.expressions[1].expression,
            Expression::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Primary,
                collection
            })
        );
    }
    let mut p = program(true);
    p.source = SchemaNode::scalar("Input", ScalarType::String);
    p.expressions[0].expression = field(SourceOwner::Primary, &[], None);
    let actual = observed("A03 R02 scalar primary endpoint", &p);
    assert_eq!(actual, Ok(()));
}

#[test]
fn basic_gate_accepts_only_control_free_schema_valid_collection_forms() {
    for owned in [false, true] {
        let mut p = program(true);
        repeat_target(&mut p);
        p.root.iteration = Some(if owned {
            IterationPlan::owned_source(OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            })
        } else {
            IterationPlan::source(path(&["Rows"]))
        });
        p.expressions[0].expression = if owned {
            field(
                SourceOwner::Primary,
                &["Rows", "Value"],
                Some(path(&["Rows"])),
            )
        } else {
            Expression::SourceField {
                path: path(&["Value"]),
                frame: None,
            }
        };
        let actual = observed("A04 legacy versus owned repeated scope", &p);
        assert_eq!(actual, Ok(()));
    }
    let mut p = program(true);
    repeat_target(&mut p);
    p.extra_sources[0].source = SchemaNode::group(
        "RefRoot",
        vec![SchemaNode::scalar("Values", ScalarType::String).repeating()],
    );
    p.root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: SourceOwner::Named { name: "Ref".into() },
        path: path(&["Values"]),
    }));
    p.expressions[0].expression = field(
        SourceOwner::Named { name: "Ref".into() },
        &["Values"],
        Some(path(&["Values"])),
    );
    let actual = observed("A05 repeated scalar terminal", &p);
    assert_eq!(actual, Ok(()));
    let mut p = program(true);
    repeat_target(&mut p);
    p.root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: Vec::new(),
    }));
    p.expressions[0].expression = field(SourceOwner::Primary, &["Value"], Some(Vec::new()));
    let actual = observed("A06 flat nonrepeating Group root", &p);
    assert_eq!(actual, Ok(()));
}

#[test]
fn basic_gate_retains_ordered_ordinary_global_failures_and_unrelated_scalar_raise() {
    let mut p = program(true);
    p.expressions.extend([
        node(
            2,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        node(
            3,
            Expression::SourceField {
                path: path(&["Value"]),
                frame: Some(path(&["Rows"])),
            },
        ),
        node(4, Expression::Raise { message: None }),
        node(71, Expression::Raise { message: None }),
    ]);
    p.failure_rules = vec![
        FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(path(&["Rows"]))),
            selection: FailureSelection::WhenTrue(2),
            message: Some(4),
        },
        FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(path(&["Rows"]))),
            selection: FailureSelection::All,
            message: Some(3),
        },
    ];
    p.extra_targets.push(NamedTargetProgram {
        name: "Bad".into(),
        target: SchemaNode::scalar("Bad", ScalarType::String),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Scalar {
                expression: 71,
                target_domain: ScalarTargetDomain::Single(ScalarType::String),
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
    });
    let original = p.clone();
    let actual = observed("A07 global rules and P19 Raise", &p);
    assert_eq!(actual, Ok(()));
    assert_eq!(p, original);
}

#[test]
fn basic_gate_preserves_structural_priority_and_stored_context_validation() {
    let mut p = program(true);
    unused_function(&mut p);
    p.expressions[0].expression = field(
        SourceOwner::Named {
            name: "Absent".into(),
        },
        &["Value"],
        None,
    );
    let actual = observed("R01 structural owner before stored function", &p);
    assert_eq!(
        actual,
        Err(ProgramValidationError::OwnedSource {
            diagnostic: Box::new(Diagnostic::Structure {
                location: Location::Expression {
                    namespace: Namespace::Main,
                    node: 1,
                    scope: None
                },
                request: Request::Field {
                    owner: Owner::Named {
                        name: "Absent".into()
                    },
                    path: path(&["Value"]),
                    frame: None
                },
                reason: Structure::UnknownOwner
            })
        })
    );
    let mut p = program(false);
    p.root.construction = TargetConstruction::CopyCurrentSource;
    p.expressions
        .push(node(1, Expression::Raise { message: None }));
    let actual = observed("R02 duplicate ID before unsupported construction", &p);
    assert_eq!(
        actual,
        Err(ProgramValidationError::DuplicateExpression { node: 1 })
    );
    let mut p = program(true);
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
        selection: FailureSelection::All,
        message: Some(1),
    });
    let actual = observed("R03 owned failure expression stays closed", &p);
    assert_eq!(
        actual,
        capability(
            Location::Expression {
                namespace: Namespace::Main,
                node: 1,
                scope: None
            },
            Some(Request::Field {
                owner: Owner::Primary,
                path: path(&["Value"]),
                frame: None
            }),
            Reason::FailureRule,
            Origin::Failure { index: 0 }
        )
    );
    let mut p = program(true);
    dynamic(&mut p);
    p.extra_sources[4].dynamic.as_mut().unwrap().path = 1;
    let actual = observed("R04 Basic node reused in stored dynamic path", &p);
    assert_eq!(
        actual,
        capability(
            Location::Expression {
                namespace: Namespace::Main,
                node: 1,
                scope: None
            },
            Some(Request::Field {
                owner: Owner::Primary,
                path: path(&["Value"]),
                frame: None
            }),
            Reason::DynamicLoader,
            Origin::DynamicSource {
                index: 4,
                name: "D".into()
            }
        )
    );
    let mut p = program(false);
    p.expressions
        .push(node(99, field(SourceOwner::Primary, &["Value"], None)));
    let route = SourceScopeLocation::root(SourceScopeTarget::Named {
        index: 0,
        name: "OwnedScalar".into(),
    });
    p.extra_targets.push(NamedTargetProgram {
        name: "OwnedScalar".into(),
        target: SchemaNode::scalar("OwnedScalar", ScalarType::String),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Scalar {
                expression: 99,
                target_domain: ScalarTargetDomain::Single(ScalarType::String),
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
    });
    let actual = observed("R05 named scalar owned construction", &p);
    assert_eq!(
        actual,
        capability(
            Location::Expression {
                namespace: Namespace::Main,
                node: 99,
                scope: Some(route.clone())
            },
            Some(Request::Field {
                owner: Owner::Primary,
                path: path(&["Value"]),
                frame: None
            }),
            Reason::SpecialConstruction,
            Origin::Scope { scope: route }
        )
    );
}

#[test]
fn basic_gate_uses_scope_then_stored_function_inventory_order() {
    let mut p = program(false);
    repeat_target(&mut p);
    unused_function(&mut p);
    p.expressions.push(node(
        2,
        Expression::Const {
            value: Value::Bool(true),
        },
    ));
    p.root.iteration = Some(IterationPlan::new(
        SourceIteration::new(path(&["Rows"])),
        Some(2),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
    let actual = observed("R06 scope before stored function", &p);
    assert_eq!(actual, scope_error(Reason::ScopeControl));
    let mut p = program(false);
    unused_function(&mut p);
    let ordinary = validate_owned_emission(&p);
    eprintln!("RAW R07 unchanged ordinary gate={ordinary:#?}");
    let actual = observed("R07 unused stored ordinary function", &p);
    assert_eq!(ordinary, Ok(()));
    assert_eq!(
        actual,
        capability(
            Location::Expression {
                namespace: Namespace::Function(FunctionId::new(7)),
                node: 0,
                scope: None
            },
            None,
            Reason::IsolatedFunction,
            Origin::FunctionBody {
                function: FunctionId::new(7)
            }
        )
    );
}

#[test]
fn basic_gate_checks_stored_specialized_leaves_without_changing_ordinary_gates() {
    for reached in [false, true] {
        let mut p = program(false);
        p.expressions.push(node(99, value_map()));
        if reached {
            p.root.bindings[0].expression = 99;
        }
        let ordinary = validate_owned_emission(&p);
        eprintln!("RAW R08 unchanged ordinary gate={ordinary:#?}");
        let actual = observed("A10 stored target ValueMap", &p);
        assert_eq!(ordinary, Ok(()));
        assert_eq!(actual, Ok(()));
    }
    for reached in [false, true] {
        let mut p = program(false);
        p.expressions.push(node(99, Expression::SourceDocumentPath));
        if reached {
            p.root.bindings[0].expression = 99;
        }
        let actual = observed("R09 stored document leaf", &p);
        assert_eq!(actual, retained(99, Reason::Document));
    }
}

#[test]
fn basic_gate_preserves_legacy_dynamic_laziness_and_explicit_frame_replacement() {
    let mut p = program(false);
    dynamic(&mut p);
    p.expressions.push(node(
        99,
        Expression::SourceField {
            path: path(&["D", "Value"]),
            frame: None,
        },
    ));
    let actual = observed("A08 dead ordinary dynamic leaf remains inert", &p);
    assert_eq!(actual, Ok(()));
    p.root.bindings[0].expression = 99;
    let actual = observed("R10 retained dynamic leaf", &p);
    assert_eq!(actual, retained(99, Reason::UnsupportedBasicContext));
    for (frame, accepted) in [
        (None, false),
        (Some(path(&["D"])), false),
        (Some(Vec::new()), true),
    ] {
        let mut p = program(false);
        dynamic(&mut p);
        p.expressions.push(node(
            99,
            Expression::SourceField {
                path: path(&["D", "Value"]),
                frame,
            },
        ));
        p.root.bindings[0].expression = 99;
        let actual = observed("R11 frame replaces legacy selector", &p);
        assert_eq!(
            actual,
            if accepted {
                Ok(())
            } else {
                retained(99, Reason::UnsupportedBasicContext)
            }
        );
    }
    let mut p = program(false);
    dynamic(&mut p);
    repeat_target(&mut p);
    p.root.iteration = Some(IterationPlan::source(path(&["D", "Rows"])));
    let actual = observed("R12 dynamic legacy iteration", &p);
    assert_eq!(actual, scope_error(Reason::DynamicLoader));
}

#[test]
fn basic_gate_failure_dynamic_refusals_keep_exact_rule_origin_and_lazy_potential_edges() {
    let mut p = program(true);
    dynamic(&mut p);
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(path(&["D", "Rows"]))),
        selection: FailureSelection::All,
        message: None,
    });
    let actual = observed("R13 dynamic failure source", &p);
    assert_eq!(actual, failure_error(Reason::DynamicLoader));
    let mut p = program(true);
    dynamic(&mut p);
    p.expressions.push(node(
        99,
        Expression::SourceField {
            path: path(&["Value"]),
            frame: Some(path(&["D", "Rows"])),
        },
    ));
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(path(&["Rows"]))),
        selection: FailureSelection::All,
        message: Some(99),
    });
    let actual = observed("R14 dynamic failure message", &p);
    assert_eq!(
        actual,
        capability(
            Location::Expression {
                namespace: Namespace::Main,
                node: 99,
                scope: None
            },
            None,
            Reason::DynamicLoader,
            Origin::Failure { index: 0 }
        )
    );
    let mut p = program(true);
    dynamic(&mut p);
    p.expressions.extend([
        node(
            2,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        node(
            99,
            Expression::SourceField {
                path: path(&["D", "Flag"]),
                frame: None,
            },
        ),
        node(
            3,
            Expression::If {
                condition: 2,
                then: 99,
                else_: 2,
            },
        ),
    ]);
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(path(&["Rows"]))),
        selection: FailureSelection::WhenTrue(3),
        message: None,
    });
    let actual = observed("R15 unchosen dynamic failure branch", &p);
    assert_eq!(
        actual,
        capability(
            Location::Expression {
                namespace: Namespace::Main,
                node: 99,
                scope: None
            },
            None,
            Reason::DynamicLoader,
            Origin::Failure { index: 0 }
        )
    );
}

#[test]
fn basic_gate_retains_closed_generated_and_specialized_failure_domains() {
    let mut p = program(true);
    p.expressions.extend([
        node(
            2,
            Expression::Const {
                value: Value::Int(2),
            },
        ),
        node(
            3,
            Expression::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ]);
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Generated(GeneratedSequence::Range {
            from: None,
            to: 2,
            item: 3,
        }),
        selection: FailureSelection::All,
        message: None,
    });
    let actual = observed("R16 generated failure", &p);
    assert_eq!(actual, failure_error(Reason::FailureRule));
    let mut p = program(false);
    p.expressions.push(node(99, value_map()));
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
        selection: FailureSelection::All,
        message: Some(99),
    });
    let actual = observed("R17 specialized failure leaf before main inventory", &p);
    assert_eq!(actual, failure_error(Reason::FailureRule));
}

#[test]
fn basic_gate_does_not_change_project_dead_node_omission_or_ordinary_programs() {
    let p = super::supported_project();
    let baseline = crate::lower(&p);
    eprintln!("RAW A09 complete baseline Project={p:#?} lower={baseline:#?}");
    let original = baseline.unwrap();
    let mut candidate = p;
    candidate.graph.nodes.insert(
        99,
        mapping::Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["First"]),
            },
            frame: None,
        }),
    );
    let actual = crate::lower(&candidate);
    eprintln!("RAW A09 complete candidate Project={candidate:#?} lower={actual:#?}");
    let lowered = actual.unwrap();
    let gate = observed("A09 omitted Project leaf", &lowered);
    let old_gate = validate_owned_emission(&lowered);
    let ordinary_structure = validate_program(&lowered);
    eprintln!("RAW A09 unchanged old gate={old_gate:#?} structure={ordinary_structure:#?}");
    assert_eq!(lowered, original);
    assert_eq!(gate, Ok(()));
    assert_eq!(old_gate, Ok(()));
    assert_eq!(ordinary_structure, Ok(()));
}

#[test]
fn basic_gate_accepts_target_value_map_payloads_and_owned_scalar_inputs() {
    for owner in [
        SourceOwner::Primary,
        SourceOwner::Named {
            name: " Ref 雪 ".into(),
        },
    ] {
        for input_kind in 0..6 {
            let mut p = program(true);
            p.expressions[0].expression = field(owner.clone(), &["Value"], None);
            p.expressions.push(node(
                2,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ));
            let expression = match input_kind {
                0 => None,
                1 => Some(Expression::Call {
                    function: crate::ScalarFunction::Upper,
                    args: vec![1],
                }),
                2 => Some(Expression::If {
                    condition: 2,
                    then: 1,
                    else_: 1,
                }),
                3 => Some(Expression::RuntimeParameterDefault {
                    name: "Input".into(),
                    ty: ScalarType::String,
                    default: 1,
                }),
                4 => Some(Expression::Raise { message: Some(1) }),
                _ => Some(value_map_from(1)),
            };
            let input = if let Some(expression) = expression {
                p.expressions.push(node(4, expression));
                4
            } else {
                1
            };
            p.expressions.push(node(
                3,
                Expression::ValueMap {
                    input,
                    input_type: [
                        None,
                        Some(ScalarType::String),
                        Some(ScalarType::Int),
                        Some(ScalarType::Float),
                        Some(ScalarType::Bool),
                        None,
                    ][input_kind],
                    table: vec![
                        (Value::Null, Value::String("absent".into())),
                        (Value::XmlNil(ir::XmlNil), Value::XmlNil(ir::XmlNil)),
                        (Value::Bool(false), Value::Bool(true)),
                        (Value::Int(1), Value::Int(2)),
                        (Value::Float(1.0), Value::Float(2.0)),
                        (Value::String("1".into()), Value::String("first".into())),
                        (Value::String("1".into()), Value::String("later".into())),
                    ],
                    default: (input_kind % 2 == 0).then_some(Value::Null),
                },
            ));
            p.root.bindings[0].expression = 3;
            let original = p.clone();
            let actual = observed(
                "VM01 exact target map payload and potential scalar input",
                &p,
            );
            assert_eq!(actual, Ok(()));
            assert_eq!(p, original);
        }
    }
}

#[test]
fn basic_gate_retains_target_value_map_input_edges_and_empty_frame_replacement() {
    for input_kind in 0..6 {
        for frame in [None, Some(path(&["D"])), Some(Vec::new())] {
            for reached in [false, true] {
                let mut p = program(false);
                dynamic(&mut p);
                p.expressions.extend([
                    node(
                        2,
                        Expression::Const {
                            value: Value::Bool(false),
                        },
                    ),
                    node(
                        99,
                        Expression::SourceField {
                            path: path(&["D", "Value"]),
                            frame: frame.clone(),
                        },
                    ),
                ]);
                let expression = match input_kind {
                    0 => None,
                    1 => Some(Expression::Call {
                        function: crate::ScalarFunction::Upper,
                        args: vec![99],
                    }),
                    2 => Some(Expression::If {
                        condition: 2,
                        then: 99,
                        else_: 1,
                    }),
                    3 => Some(Expression::RuntimeParameterDefault {
                        name: "Input".into(),
                        ty: ScalarType::String,
                        default: 99,
                    }),
                    4 => Some(Expression::Raise { message: Some(99) }),
                    _ => Some(value_map_from(99)),
                };
                let input = if let Some(expression) = expression {
                    p.expressions.push(node(4, expression));
                    4
                } else {
                    99
                };
                p.expressions.push(node(3, value_map_from(input)));
                // The outer lazy branch retains the potential map input even
                // when its condition would select the ordinary constant.
                p.expressions.push(node(
                    5,
                    Expression::If {
                        condition: 2,
                        then: 3,
                        else_: 1,
                    },
                ));
                let expression = if reached { 5 } else { 1 };
                match input_kind % 3 {
                    0 => p.root.bindings[0].expression = expression,
                    1 => {
                        p.target = SchemaNode::group(
                            "Output",
                            vec![
                                SchemaNode::scalar("Value", ScalarType::String),
                                SchemaNode::scalar("Child", ScalarType::String),
                            ],
                        );
                        p.root.children.push(TargetScope {
                            target_field: "Child".into(),
                            repeating: false,
                            iteration: None,
                            construction: TargetConstruction::Scalar {
                                expression,
                                target_domain: ScalarTargetDomain::Single(ScalarType::String),
                            },
                            bindings: Vec::new(),
                            children: Vec::new(),
                        });
                    }
                    _ => p.extra_targets.push(NamedTargetProgram {
                        name: "Mapped".into(),
                        target: SchemaNode::scalar("Mapped", ScalarType::String),
                        root: TargetScope {
                            target_field: String::new(),
                            repeating: false,
                            iteration: None,
                            construction: TargetConstruction::Scalar {
                                expression,
                                target_domain: ScalarTargetDomain::Single(ScalarType::String),
                            },
                            bindings: Vec::new(),
                            children: Vec::new(),
                        },
                    }),
                }
                let original = p.clone();
                let ordinary = validate_owned_emission(&p);
                let actual = observed("VM02 target-only map edge and replacing frame", &p);
                eprintln!("RAW VM02 unchanged ordinary gate={ordinary:#?}");
                assert_eq!(ordinary, Ok(()));
                assert_eq!(
                    actual,
                    if reached && frame.as_ref().is_none_or(|frame| !frame.is_empty()) {
                        retained(99, Reason::UnsupportedBasicContext)
                    } else {
                        Ok(())
                    }
                );
                assert_eq!(p, original);
            }
        }
    }
}

#[test]
fn basic_gate_keeps_target_value_map_structural_priority() {
    let mut p = program(false);
    unused_function(&mut p);
    p.expressions.push(node(99, value_map_from(123)));
    p.root.bindings[0].expression = 99;
    let actual = observed("VM03 missing map input before function capability", &p);
    assert_eq!(
        actual,
        Err(ProgramValidationError::MissingDependency {
            node: 99,
            dependency: 123,
        })
    );
    let mut p = program(true);
    unused_function(&mut p);
    p.expressions[0].expression = field(
        SourceOwner::Named {
            name: "Absent".into(),
        },
        &["Value"],
        None,
    );
    p.expressions.extend([
        node(
            2,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        node(
            4,
            Expression::If {
                condition: 2,
                then: 1,
                else_: 2,
            },
        ),
        node(3, value_map_from(4)),
    ]);
    p.root.bindings[0].expression = 3;
    let actual = observed(
        "VM04 potential owned map input keeps structural priority",
        &p,
    );
    assert_eq!(
        actual,
        Err(ProgramValidationError::OwnedSource {
            diagnostic: Box::new(Diagnostic::Structure {
                location: Location::Expression {
                    namespace: Namespace::Main,
                    node: 1,
                    scope: None,
                },
                request: Request::Field {
                    owner: Owner::Named {
                        name: "Absent".into(),
                    },
                    path: path(&["Value"]),
                    frame: None,
                },
                reason: Structure::UnknownOwner,
            }),
        })
    );
}

#[test]
fn basic_gate_keeps_failure_value_maps_closed_and_preserves_dynamic_priority() {
    for separate_dynamic in [false, true] {
        for dynamic_first in [false, true] {
            let mut p = program(false);
            dynamic(&mut p);
            let dynamic_input = node(
                99,
                Expression::SourceField {
                    path: path(&["D", "Value"]),
                    frame: None,
                },
            );
            let map = node(3, value_map_from(99));
            p.expressions.extend(if dynamic_first {
                [dynamic_input, map]
            } else {
                [map, dynamic_input]
            });
            let message = if separate_dynamic {
                p.expressions.push(node(
                    4,
                    Expression::Call {
                        function: crate::ScalarFunction::Concat,
                        args: vec![99, 3],
                    },
                ));
                4
            } else {
                3
            };
            p.failure_rules.push(FailureRule {
                iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
                selection: FailureSelection::All,
                message: Some(message),
            });
            let ordinary = validate_owned_emission(&p);
            let actual = observed(
                "VM05 closed failure map input and stored refusal priority",
                &p,
            );
            eprintln!("RAW VM05 unchanged ordinary gate={ordinary:#?}");
            assert_eq!(ordinary, Ok(()));
            assert_eq!(
                actual,
                if separate_dynamic && dynamic_first {
                    capability(
                        Location::Expression {
                            namespace: Namespace::Main,
                            node: 99,
                            scope: None,
                        },
                        None,
                        Reason::DynamicLoader,
                        Origin::Failure { index: 0 },
                    )
                } else {
                    failure_error(Reason::FailureRule)
                }
            );
        }
    }
}
