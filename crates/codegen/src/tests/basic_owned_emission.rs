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
            driver: SourceIteration::new(Vec::new()).into(),
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
    assert_eq!(actual, Ok(()));
    // The closed Stage-A gate still refuses the admitted read as Basic-only.
    assert_eq!(
        crate::validate_owned_emission(&p),
        capability(
            Location::Expression {
                namespace: Namespace::Main,
                node: 99,
                scope: None
            },
            Some(Request::Field {
                owner: Owner::Primary,
                path: path(&["Value"]),
                frame: None
            }),
            Reason::UnsupportedBasicContext,
            Origin::RetainedExpression {
                namespace: Namespace::Main,
                node: 99
            }
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

// Explicit common capability controls. No renderer or execution entry point opts in.
fn filter_program(owner: Option<SourceOwner>) -> Program {
    let mut p = program(false);
    repeat_target(&mut p);
    p.expressions.push(node(
        2,
        Expression::Const {
            value: Value::Bool(true),
        },
    ));
    let input: crate::IterationSource = match owner.clone() {
        Some(owner) => crate::OwnedSourceIteration::new(OwnedSourceRef {
            owner,
            path: path(&["Rows"]),
        })
        .into(),
        None => SourceIteration::new(path(&["Rows"])).into(),
    };
    p.root.iteration = Some(IterationPlan::new(
        input,
        Some(2),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
    if let Some(owner) = owner {
        p.expressions[0].expression = field(owner, &["Rows", "Value"], Some(path(&["Rows"])));
    }
    p
}
fn filter_scope_error(
    owner: Option<Owner>,
    route: SourceScopeLocation,
    reason: Reason,
) -> Result<(), ProgramValidationError> {
    capability(
        Location::Iteration {
            scope: route.clone(),
        },
        owner.map(|owner| Request::Iteration {
            owner,
            path: path(&["Rows"]),
        }),
        reason,
        Origin::Scope { scope: route },
    )
}
fn filter_observed(label: &str, p: &Program) -> Result<(), ProgramValidationError> {
    eprintln!("INPUT {label} complete Program={p:#?}");
    let result = crate::validate_basic_owned_filter_emission(p);
    eprintln!("RAW {label} complete Result={result:#?}");
    let program_result = crate::validate_program_with_basic_owned_filters(p);
    eprintln!("RAW {label} complete opt-in Program Result={program_result:#?}");
    assert_eq!(program_result, result);
    result
}

#[test]
fn basic_filter_profile_requires_explicit_static_repeated_capability() {
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
        for frame in [None, Some(Vec::new()), Some(path(&["Rows"]))] {
            let mut p = filter_program(Some(owner.clone()));
            p.expressions[1].expression = field(owner.clone(), &["Rows", "Flag"], frame);
            let before = p.clone();
            let expected =
                filter_scope_error(Some((&owner).into()), primary(), Reason::ScopeControl);
            assert_eq!(validate_program(&p), expected);
            assert_eq!(validate_basic_owned_emission(&p), expected);
            assert_eq!(validate_owned_emission(&p), expected);
            assert_eq!(
                filter_observed("F643-01 exact owner/filter/frame", &p),
                Ok(())
            );
            assert_eq!(p, before);
        }
    }
    for owned_predicate in [false, true] {
        let mut p = filter_program(None);
        if owned_predicate {
            p.expressions[1].expression = field(
                SourceOwner::Named {
                    name: " Ref 雪 ".into(),
                },
                &["Rows", "Flag"],
                Some(path(&["Rows"])),
            );
        }
        assert_eq!(filter_observed("F643-02 legacy static filter", &p), Ok(()));
        assert_eq!(
            validate_basic_owned_emission(&p),
            if owned_predicate {
                capability(
                    Location::Expression {
                        namespace: Namespace::Main,
                        node: 2,
                        scope: Some(primary()),
                    },
                    Some(Request::Field {
                        owner: Owner::Named {
                            name: " Ref 雪 ".into(),
                        },
                        path: path(&["Rows", "Flag"]),
                        frame: Some(path(&["Rows"])),
                    }),
                    Reason::ScopeControl,
                    Origin::Scope { scope: primary() },
                )
            } else {
                scope_error(Reason::ScopeControl)
            }
        );
    }
}

#[test]
fn basic_filter_profile_lowering_retains_the_exact_predicate_and_default_refusal() {
    let mut project = super::supported_project();
    project.source = input_schema();
    project.target = SchemaNode::group(
        "Output",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    )
    .repeating();
    project.graph.nodes = std::collections::BTreeMap::from([
        (
            1,
            mapping::Node::OwnedSourceField(OwnedSourceField {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Rows", "Value"]),
                },
                frame: Some(path(&["Rows"])),
            }),
        ),
        (
            2,
            mapping::Node::OwnedSourceField(OwnedSourceField {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Rows", "Flag"]),
                },
                frame: Some(path(&["Rows"])),
            }),
        ),
        (
            99,
            mapping::Node::OwnedSourceField(OwnedSourceField {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Value"]),
                },
                frame: None,
            }),
        ),
    ]);
    project.root = mapping::Scope {
        iteration: mapping::ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            },
            output_path: None,
        },
        filter: Some(2),
        bindings: vec![mapping::Binding {
            target_field: "Value".into(),
            node: 1,
        }],
        ..Default::default()
    };
    let before = format!("{project:#?}");
    let ordinary = crate::lower(&project);
    let opted = crate::lower_with_basic_owned_filters(&project);
    eprintln!("INPUT F643-03 Project={project:#?} RAW default={ordinary:#?} opt-in={opted:#?}");
    let Err(ProgramValidationError::OwnedSource { diagnostic }) =
        filter_scope_error(Some(Owner::Primary), primary(), Reason::ScopeControl)
    else {
        unreachable!()
    };
    assert_eq!(
        ordinary,
        Err(crate::LowerError::new(vec![
            crate::Diagnostic::OwnedSource { diagnostic }
        ]))
    );
    let lowered = opted.unwrap();
    assert_eq!(lowered.root.iteration.as_ref().unwrap().filter(), Some(2));
    assert_eq!(
        lowered
            .root
            .iteration
            .as_ref()
            .unwrap()
            .owned_source_iteration()
            .unwrap()
            .source(),
        &OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"])
        }
    );
    assert_eq!(
        lowered.expressions,
        vec![
            node(
                1,
                field(
                    SourceOwner::Primary,
                    &["Rows", "Value"],
                    Some(path(&["Rows"]))
                )
            ),
            node(
                2,
                field(
                    SourceOwner::Primary,
                    &["Rows", "Flag"],
                    Some(path(&["Rows"]))
                )
            )
        ]
    );
    assert_eq!(
        filter_observed("F643-03 exact lowered controls", &lowered),
        Ok(())
    );
    assert_eq!(format!("{project:#?}"), before);
}

#[test]
fn basic_filter_profile_preserves_control_free_programs_and_lowering() {
    for owned in [false, true] {
        let p = program(owned);
        let before = p.clone();
        assert_eq!(
            filter_observed("F643-04 no filter", &p),
            validate_basic_owned_emission(&p)
        );
        assert_eq!(p, before);
    }
    let project = super::supported_project();
    let ordinary = crate::lower(&project);
    let opted = crate::lower_with_basic_owned_filters(&project);
    eprintln!("RAW F643-05 complete control-free default={ordinary:#?} opt-in={opted:#?}");
    assert_eq!(opted, ordinary);
    assert!(opted.is_ok());
}

#[test]
fn basic_filter_profile_preserves_structure_before_capability() {
    for (owner, parts, frame, reason) in [
        (
            SourceOwner::Named {
                name: "Absent".into(),
            },
            path(&["Rows", "Flag"]),
            Some(path(&["Rows"])),
            Structure::UnknownOwner,
        ),
        (
            SourceOwner::Primary,
            path(&["Rows", "Absent"]),
            Some(path(&["Rows"])),
            Structure::MissingOwnedPath { segment: 1 },
        ),
        (
            SourceOwner::Primary,
            path(&["Rows", "Flag"]),
            Some(path(&["D"])),
            Structure::IncompatibleFramePath,
        ),
    ] {
        let mut p = filter_program(Some(SourceOwner::Primary));
        p.expressions[1].expression = Expression::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner: owner.clone(),
                path: parts.clone(),
            },
            frame: frame.clone(),
        });
        let expected = Err(ProgramValidationError::OwnedSource {
            diagnostic: Box::new(Diagnostic::Structure {
                location: Location::Expression {
                    namespace: Namespace::Main,
                    node: 2,
                    scope: None,
                },
                request: Request::Field {
                    owner: (&owner).into(),
                    path: parts,
                    frame,
                },
                reason,
            }),
        });
        assert_eq!(
            filter_observed("F643-06 complete owned structural refusal", &p),
            expected
        );
        assert_eq!(validate_basic_owned_emission(&p), expected);
    }
    let mut p = filter_program(Some(SourceOwner::Primary));
    p.expressions.push(node(
        2,
        Expression::Const {
            value: Value::Bool(false),
        },
    ));
    assert_eq!(
        filter_observed("F643-07 duplicate before scope control", &p),
        Err(ProgramValidationError::DuplicateExpression { node: 2 })
    );
    let mut p = filter_program(Some(SourceOwner::Primary));
    p.expressions.pop();
    assert_eq!(
        filter_observed("F643-08 missing predicate before scope control", &p),
        Err(ProgramValidationError::MissingFilterExpression {
            target_path: Vec::new(),
            expression: 2
        })
    );
}

#[test]
fn basic_filter_profile_retains_full_combined_control_envelopes() {
    for control in 0..9 {
        let mut p = filter_program(Some(SourceOwner::Primary));
        p.expressions.push(node(
            3,
            Expression::Const {
                value: Value::Int(1),
            },
        ));
        let sort = (control == 0).then(|| {
            crate::SortPlan::new(
                crate::SortKey {
                    expression: 1,
                    descending: false,
                },
                Vec::new(),
                crate::SortFilterOrder::FilterThenSort,
            )
        });
        let windows = match control {
            1 => vec![crate::SequenceWindow::SkipFirst { count: 3 }],
            2 => vec![crate::SequenceWindow::First { count: 3 }],
            3 => vec![crate::SequenceWindow::From { position: 3 }],
            4 => vec![crate::SequenceWindow::FromTo { first: 3, last: 3 }],
            5 => vec![crate::SequenceWindow::Last { count: 3 }],
            _ => Vec::new(),
        };
        let output = if control == 8 {
            p.root.repeating = false;
            p.target.repeating = false;
            IterationOutput::First
        } else {
            IterationOutput::Repeated
        };
        let mut iteration = IterationPlan::new(
            crate::OwnedSourceIteration::new(OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            }),
            Some(2),
            sort,
            windows,
            output,
        );
        if control == 6 {
            iteration = iteration.with_grouping(crate::GroupingPlan::By { key: 1 });
        }
        if control == 7 {
            iteration = iteration.with_filtered_grouping(crate::GroupingPlan::By { key: 1 }, 2);
        }
        p.root.iteration = Some(iteration);
        let expected = filter_scope_error(
            Some(Owner::Primary),
            primary(),
            if control == 6 || control == 7 {
                Reason::Grouping
            } else {
                Reason::ScopeControl
            },
        );
        assert_eq!(validate_basic_owned_emission(&p), expected);
        assert_eq!(
            filter_observed("F643-09 combined controls remain closed", &p),
            expected
        );
    }
}

#[test]
fn basic_filter_profile_preserves_nested_inherited_closure_and_named_routes() {
    let mut p = filter_program(None);
    let mut child = filter_program(Some(SourceOwner::Primary)).root;
    child.target_field = "Child".into();
    p.root.children.push(child);
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group(
                "Child",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
        ],
    )
    .repeating();
    p.expressions[0].expression = field(
        SourceOwner::Primary,
        &["Rows", "Value"],
        Some(path(&["Rows"])),
    );
    let child_route = primary().descendant(mapping::SourceScopeStep::Child(0));
    assert_eq!(
        filter_observed("F643-10 nested filter capability", &p),
        Ok(())
    );
    assert_eq!(
        validate_basic_owned_emission(&p),
        capability(
            Location::Expression {
                namespace: Namespace::Main,
                node: 1,
                scope: Some(primary())
            },
            Some(Request::Field {
                owner: Owner::Primary,
                path: path(&["Rows", "Value"]),
                frame: Some(path(&["Rows"]))
            }),
            Reason::ScopeControl,
            Origin::Scope { scope: primary() }
        )
    );
    p.expressions[0].expression = Expression::Const {
        value: Value::String("root".into()),
    };
    let parent = p.root.iteration.as_ref().unwrap().input().clone();
    p.expressions.push(node(
        3,
        Expression::Const {
            value: Value::Int(1),
        },
    ));
    p.root.iteration = Some(IterationPlan::new(
        parent,
        Some(2),
        None,
        vec![crate::SequenceWindow::First { count: 3 }],
        IterationOutput::Repeated,
    ));
    let expected = capability(
        Location::Iteration { scope: child_route },
        Some(Request::Iteration {
            owner: Owner::Primary,
            path: path(&["Rows"]),
        }),
        Reason::ScopeControl,
        Origin::Scope { scope: primary() },
    );
    assert_eq!(
        filter_observed("F643-11 child inherits earlier parent closure", &p),
        expected
    );
    assert_eq!(validate_basic_owned_emission(&p), expected);
    let mut p = filter_program(Some(SourceOwner::Primary));
    let mut named = filter_program(Some(SourceOwner::Named { name: "Ref".into() }));
    named.expressions[0].expression = Expression::Const {
        value: Value::String("named".into()),
    };
    let input = named.root.iteration.as_ref().unwrap().input().clone();
    named.root.iteration = Some(IterationPlan::new(
        input,
        Some(2),
        None,
        vec![crate::SequenceWindow::First { count: 3 }],
        IterationOutput::Repeated,
    ));
    p.expressions.push(node(
        3,
        Expression::Const {
            value: Value::Int(1),
        },
    ));
    p.extra_targets.push(NamedTargetProgram {
        name: " Target 雪 ".into(),
        target: named.target,
        root: named.root,
    });
    let route = SourceScopeLocation::root(SourceScopeTarget::Named {
        index: 0,
        name: " Target 雪 ".into(),
    });
    assert_eq!(
        filter_observed("F643-12 exact named target route", &p),
        filter_scope_error(
            Some(Owner::Named { name: "Ref".into() }),
            route,
            Reason::ScopeControl
        )
    );
    assert_eq!(
        validate_basic_owned_emission(&p),
        filter_scope_error(Some(Owner::Primary), primary(), Reason::ScopeControl)
    );
}

#[test]
fn basic_filter_profile_retains_dead_and_potential_dynamic_predicate_edges() {
    for input_kind in 0..4 {
        for frame in [None, Some(path(&["D"])), Some(Vec::new())] {
            for reached in [false, true] {
                let mut p = filter_program(None);
                dynamic(&mut p);
                p.expressions[1].expression = Expression::Const {
                    value: Value::Bool(false),
                };
                p.expressions.push(node(
                    99,
                    Expression::SourceField {
                        path: path(&["D", "Flag"]),
                        frame: frame.clone(),
                    },
                ));
                let predicate = match input_kind {
                    0 => Expression::If {
                        condition: 2,
                        then: 99,
                        else_: 2,
                    },
                    1 => Expression::RuntimeParameterDefault {
                        name: "predicate".into(),
                        ty: ScalarType::Bool,
                        default: 99,
                    },
                    2 => Expression::Raise { message: Some(99) },
                    _ => Expression::ValueMap {
                        input: 99,
                        input_type: Some(ScalarType::Bool),
                        table: vec![(Value::Bool(false), Value::Bool(true))],
                        default: Some(Value::Bool(true)),
                    },
                };
                p.expressions.push(node(3, predicate));
                if reached {
                    let input = p.root.iteration.as_ref().unwrap().input().clone();
                    p.root.iteration = Some(IterationPlan::new(
                        input,
                        Some(3),
                        None,
                        Vec::new(),
                        IterationOutput::Repeated,
                    ));
                }
                let before = p.clone();
                assert_eq!(
                    filter_observed("F643-13 potential predicate dependencies", &p),
                    if reached && frame.as_ref().is_none_or(|frame| !frame.is_empty()) {
                        retained(99, Reason::UnsupportedBasicContext)
                    } else {
                        Ok(())
                    }
                );
                assert_eq!(p, before);
            }
        }
    }
}

#[test]
fn basic_filter_profile_keeps_failure_function_and_dynamic_loader_ownership_closed() {
    let mut p = filter_program(Some(SourceOwner::Primary));
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(path(&["Rows"]))),
        selection: FailureSelection::All,
        message: Some(1),
    });
    assert_eq!(
        filter_observed("F643-14 ordinary failure ownership stays closed", &p),
        capability(
            Location::Expression {
                namespace: Namespace::Main,
                node: 1,
                scope: None
            },
            Some(Request::Field {
                owner: Owner::Primary,
                path: path(&["Rows", "Value"]),
                frame: Some(path(&["Rows"]))
            }),
            Reason::FailureRule,
            Origin::Failure { index: 0 }
        )
    );
    let mut p = filter_program(Some(SourceOwner::Primary));
    dynamic(&mut p);
    p.extra_sources[4].dynamic.as_mut().unwrap().path = 1;
    assert_eq!(
        filter_observed("F643-15 retained dynamic path ownership stays closed", &p),
        capability(
            Location::Expression {
                namespace: Namespace::Main,
                node: 1,
                scope: None
            },
            Some(Request::Field {
                owner: Owner::Primary,
                path: path(&["Rows", "Value"]),
                frame: Some(path(&["Rows"]))
            }),
            Reason::DynamicLoader,
            Origin::DynamicSource {
                index: 4,
                name: "D".into()
            }
        )
    );
    let mut p = filter_program(None);
    unused_function(&mut p);
    assert_eq!(
        filter_observed("F643-16 isolated function stays closed", &p),
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
fn basic_filter_profile_keeps_generated_and_stored_specialized_domains_closed() {
    let mut p = filter_program(None);
    p.expressions.extend([
        node(
            3,
            Expression::Const {
                value: Value::Int(2),
            },
        ),
        node(
            4,
            Expression::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
    ]);
    p.root.iteration = Some(IterationPlan::new(
        GeneratedSequence::Range {
            from: None,
            to: 3,
            item: 4,
        },
        Some(2),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
    assert_eq!(
        filter_observed("F643-17 generated filter stays closed", &p),
        scope_error(Reason::GeneratedItem)
    );
    let mut p = filter_program(None);
    p.expressions.push(node(99, Expression::SourceDocumentPath));
    assert_eq!(
        filter_observed("F643-18 stored document leaf stays closed", &p),
        retained(99, Reason::Document)
    );
    let mut p = filter_program(None);
    dynamic(&mut p);
    p.root.iteration = Some(IterationPlan::new(
        SourceIteration::new(path(&["D", "Rows"])),
        Some(2),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
    assert_eq!(
        filter_observed("F643-19 dynamic legacy iteration stays closed", &p),
        scope_error(Reason::DynamicLoader)
    );
}

#[test]
fn basic_filter_profile_retains_dynamic_owner_requests_on_every_potential_edge() {
    for input_kind in 0..4 {
        for reached in [false, true] {
            let mut p = filter_program(None);
            dynamic(&mut p);
            p.expressions[1].expression = Expression::Const {
                value: Value::Bool(false),
            };
            p.expressions.push(node(
                99,
                field(
                    SourceOwner::Named { name: "D".into() },
                    &["Rows", "Flag"],
                    Some(path(&["Rows"])),
                ),
            ));
            let predicate = match input_kind {
                0 => Expression::If {
                    condition: 2,
                    then: 99,
                    else_: 2,
                },
                1 => Expression::RuntimeParameterDefault {
                    name: "predicate".into(),
                    ty: ScalarType::Bool,
                    default: 99,
                },
                2 => Expression::Raise { message: Some(99) },
                _ => Expression::ValueMap {
                    input: 99,
                    input_type: Some(ScalarType::Bool),
                    table: Vec::new(),
                    default: Some(Value::Bool(true)),
                },
            };
            p.expressions.push(node(3, predicate));
            if reached {
                let input = p.root.iteration.as_ref().unwrap().input().clone();
                p.root.iteration = Some(IterationPlan::new(
                    input,
                    Some(3),
                    None,
                    Vec::new(),
                    IterationOutput::Repeated,
                ));
            }
            assert_eq!(
                filter_observed("F643-20 retained dynamic owner keeps its exact origin", &p),
                capability(
                    Location::Expression {
                        namespace: Namespace::Main,
                        node: 99,
                        scope: reached.then(primary)
                    },
                    Some(Request::Field {
                        owner: Owner::Named { name: "D".into() },
                        path: path(&["Rows", "Flag"]),
                        frame: Some(path(&["Rows"]))
                    }),
                    Reason::DynamicOwner,
                    Origin::DynamicSource {
                        index: 4,
                        name: "D".into()
                    }
                )
            );
        }
    }
}

fn sort_program(
    owner: Option<SourceOwner>,
    filtered: bool,
    order: crate::SortFilterOrder,
) -> Program {
    let mut p = filter_program(owner);
    let input = p.root.iteration.as_ref().unwrap().input().clone();
    p.root.iteration = Some(IterationPlan::new(
        input,
        filtered.then_some(2),
        Some(crate::SortPlan::new(
            crate::SortKey {
                expression: 1,
                descending: false,
            },
            Vec::new(),
            order,
        )),
        Vec::new(),
        IterationOutput::Repeated,
    ));
    p
}

fn sort_observed(label: &str, p: &Program) -> Result<(), ProgramValidationError> {
    eprintln!("INPUT {label} complete Program={p:#?}");
    let result = crate::validate_basic_owned_sort_emission(p);
    let full = crate::validate_program_with_basic_owned_sort(p);
    eprintln!("RAW {label} complete Basic Result={result:#?} Program Result={full:#?}");
    assert_eq!(full, result);
    result
}

#[test]
fn basic_sort_profile_requires_explicit_static_repeated_capability() {
    for order in [
        crate::SortFilterOrder::SortThenFilter,
        crate::SortFilterOrder::FilterThenSort,
    ] {
        for filtered in [false, true] {
            for owner in [
                Some(SourceOwner::Primary),
                Some(SourceOwner::Named { name: "Ref".into() }),
                None,
            ] {
                let p = sort_program(owner.clone(), filtered, order);
                let before = p.clone();
                let expected = filter_scope_error(
                    owner.as_ref().map(Owner::from),
                    primary(),
                    Reason::ScopeControl,
                );
                assert_eq!(crate::validate_basic_owned_filter_emission(&p), expected);
                assert_eq!(validate_basic_owned_emission(&p), expected);
                if owner.is_some() {
                    assert_eq!(validate_program(&p), expected);
                }
                assert_eq!(
                    sort_observed("S667-01 explicit sort/no-filter/both orders/owners", &p),
                    Ok(())
                );
                assert_eq!(p, before);
            }
        }
    }
}

#[test]
fn basic_sort_profile_lowering_retains_ordered_keys_filters_and_directions() {
    for order in [
        mapping::SortFilterOrder::SortThenFilter,
        mapping::SortFilterOrder::FilterThenSort,
    ] {
        let mut project = super::supported_project();
        project.source = input_schema();
        project.target = SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        )
        .repeating();
        let owned_field = OwnedSourceField {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows", "Value"]),
            },
            frame: Some(path(&["Rows"])),
        };
        let position = OwnedPosition {
            owner: SourceOwner::Primary,
            collection: Some(path(&["Rows"])),
        };
        project.graph.nodes = std::collections::BTreeMap::from([
            (1, mapping::Node::OwnedSourceField(owned_field.clone())),
            (
                2,
                mapping::Node::Const {
                    value: Value::Bool(true),
                },
            ),
            (3, mapping::Node::OwnedPosition(position.clone())),
            (
                99,
                mapping::Node::Const {
                    value: Value::String("unused".into()),
                },
            ),
        ]);
        project.root = mapping::Scope {
            iteration: mapping::ScopeIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Rows"]),
                },
                output_path: None,
            },
            filter: Some(2),
            sort_by: Some(1),
            sort_descending: true,
            sort_then_by: vec![mapping::SortKey {
                node: 3,
                descending: false,
            }],
            sort_filter_order: order,
            bindings: vec![mapping::Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Default::default()
        };
        let before = format!("{project:#?}");
        let ordinary = crate::lower(&project);
        let filter_only = crate::lower_with_basic_owned_filters(&project);
        let actual = crate::lower_with_basic_owned_sort(&project);
        eprintln!(
            "INPUT S667-02 full Project={project:#?} RAW default={ordinary:#?} filter={filter_only:#?} sort={actual:#?}"
        );
        let Err(ProgramValidationError::OwnedSource { diagnostic }) =
            filter_scope_error(Some(Owner::Primary), primary(), Reason::ScopeControl)
        else {
            unreachable!()
        };
        let closed = Err(crate::LowerError::new(vec![
            crate::Diagnostic::OwnedSource { diagnostic },
        ]));
        assert_eq!(ordinary, closed);
        assert_eq!(filter_only, closed);
        let mut expected = sort_program(Some(SourceOwner::Primary), true, order.into());
        expected.extra_sources.clear();
        expected.expressions = vec![
            node(1, Expression::OwnedSourceField(owned_field)),
            node(
                2,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            node(3, Expression::OwnedPosition(position)),
        ];
        expected.root.iteration = Some(IterationPlan::new(
            crate::OwnedSourceIteration::new(OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            }),
            Some(2),
            Some(crate::SortPlan::new(
                crate::SortKey {
                    expression: 1,
                    descending: true,
                },
                vec![crate::SortKey {
                    expression: 3,
                    descending: false,
                }],
                order.into(),
            )),
            Vec::new(),
            IterationOutput::Repeated,
        ));
        assert_eq!(actual, Ok(expected.clone()));
        assert_eq!(
            sort_observed("S667-02 exact lowered Program", &expected),
            Ok(())
        );
        assert_eq!(format!("{project:#?}"), before);
    }
}

#[test]
fn basic_sort_profile_preserves_control_free_filter_only_and_default_results() {
    for owned in [false, true] {
        let p = program(owned);
        assert_eq!(
            sort_observed("S667-03 control-free", &p),
            validate_basic_owned_emission(&p)
        );
        assert_eq!(
            sort_observed("S667-03 control-free", &p),
            crate::validate_basic_owned_filter_emission(&p)
        );
    }
    for owner in [
        Some(SourceOwner::Primary),
        Some(SourceOwner::Named {
            name: " Ref 雪 ".into(),
        }),
        None,
    ] {
        let p = filter_program(owner);
        let before = p.clone();
        assert_eq!(
            sort_observed("S667-03 filter-only slice", &p),
            crate::validate_basic_owned_filter_emission(&p)
        );
        assert_eq!(p, before);
    }
    let project = super::supported_project();
    let before = format!("{project:#?}");
    assert_eq!(
        crate::lower_with_basic_owned_sort(&project),
        crate::lower(&project)
    );
    assert_eq!(
        crate::lower_with_basic_owned_sort(&project),
        crate::lower_with_basic_owned_filters(&project)
    );
    assert_eq!(format!("{project:#?}"), before);
}

#[test]
fn basic_sort_profile_keeps_structure_before_capability() {
    let mut p = sort_program(
        Some(SourceOwner::Primary),
        true,
        crate::SortFilterOrder::FilterThenSort,
    );
    p.expressions
        .push(node(1, Expression::Raise { message: None }));
    assert_eq!(
        sort_observed("S667-04 duplicate before sorted capability", &p),
        Err(ProgramValidationError::DuplicateExpression { node: 1 })
    );
    let mut p = sort_program(
        Some(SourceOwner::Primary),
        true,
        crate::SortFilterOrder::SortThenFilter,
    );
    let input = p.root.iteration.as_ref().unwrap().input().clone();
    p.root.iteration = Some(IterationPlan::new(
        input,
        Some(2),
        Some(crate::SortPlan::new(
            crate::SortKey {
                expression: 1,
                descending: false,
            },
            vec![crate::SortKey {
                expression: 99,
                descending: true,
            }],
            crate::SortFilterOrder::SortThenFilter,
        )),
        Vec::new(),
        IterationOutput::Repeated,
    ));
    assert_eq!(
        sort_observed("S667-04 missing secondary key", &p),
        Err(ProgramValidationError::MissingSortExpression {
            target_path: Vec::new(),
            key: 1,
            expression: 99
        })
    );
    let mut p = sort_program(
        Some(SourceOwner::Primary),
        true,
        crate::SortFilterOrder::SortThenFilter,
    );
    p.expressions[0].expression = field(
        SourceOwner::Named {
            name: "Absent".into(),
        },
        &["Rows", "Value"],
        Some(path(&["Rows"])),
    );
    let expected = Err(ProgramValidationError::OwnedSource {
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
                path: path(&["Rows", "Value"]),
                frame: Some(path(&["Rows"])),
            },
            reason: Structure::UnknownOwner,
        }),
    });
    assert_eq!(
        sort_observed("S667-04 unknown stored key owner", &p),
        expected
    );
    assert_eq!(validate_basic_owned_emission(&p), expected);
}

#[test]
fn basic_sort_profile_closes_explicit_implicit_windows_and_grouping() {
    for order in [
        crate::SortFilterOrder::SortThenFilter,
        crate::SortFilterOrder::FilterThenSort,
    ] {
        for control in 0..8 {
            let mut p = sort_program(Some(SourceOwner::Primary), true, order);
            p.expressions.push(node(
                3,
                Expression::Const {
                    value: Value::Int(1),
                },
            ));
            let current = p.root.iteration.as_ref().unwrap();
            let windows = match control {
                0 => vec![crate::SequenceWindow::SkipFirst { count: 3 }],
                1 => vec![crate::SequenceWindow::First { count: 3 }],
                2 => vec![crate::SequenceWindow::From { position: 3 }],
                3 => vec![crate::SequenceWindow::FromTo { first: 3, last: 3 }],
                4 => vec![crate::SequenceWindow::Last { count: 3 }],
                _ => Vec::new(),
            };
            let output = if control == 5 {
                p.root.repeating = false;
                p.target.repeating = false;
                IterationOutput::First
            } else {
                IterationOutput::Repeated
            };
            let mut iteration = IterationPlan::new(
                current.input().clone(),
                current.filter(),
                current.sort().cloned(),
                windows,
                output,
            );
            if control == 6 {
                iteration = iteration.with_grouping(crate::GroupingPlan::By { key: 1 });
            }
            if control == 7 {
                iteration = iteration.with_filtered_grouping(crate::GroupingPlan::By { key: 1 }, 2);
            }
            p.root.iteration = Some(iteration);
            let expected = filter_scope_error(
                Some(Owner::Primary),
                primary(),
                if control >= 6 {
                    Reason::Grouping
                } else {
                    Reason::ScopeControl
                },
            );
            assert_eq!(
                sort_observed(
                    "S667-05 explicit/implicit windows and grouping stay closed",
                    &p
                ),
                expected
            );
        }
    }
}

#[test]
fn basic_sort_profile_scopes_scalar_permission_and_preserves_inherited_closure() {
    for order in [
        crate::SortFilterOrder::SortThenFilter,
        crate::SortFilterOrder::FilterThenSort,
    ] {
        let mut p = sort_program(Some(SourceOwner::Primary), true, order);
        p.target = SchemaNode::scalar("Output", ScalarType::String).repeating();
        p.root.bindings.clear();
        p.root.construction = TargetConstruction::Scalar {
            expression: 1,
            target_domain: ScalarTargetDomain::Single(ScalarType::String),
        };
        assert_eq!(
            sort_observed("S667-06 sorted Scalar own marker is Basic", &p),
            Ok(())
        );
        let old = filter_scope_error(Some(Owner::Primary), primary(), Reason::ScopeControl);
        assert_eq!(validate_basic_owned_emission(&p), old);
        assert_eq!(crate::validate_basic_owned_filter_emission(&p), old);
        let input = p.root.iteration.as_ref().unwrap().input().clone();
        p.root.iteration = Some(IterationPlan::new(
            input,
            Some(2),
            None,
            Vec::new(),
            IterationOutput::Repeated,
        ));
        // Scalar construction is common Basic; only the control decides here.
        assert_eq!(
            sort_observed("S667-06 unsorted filtered Scalar is Basic", &p),
            Ok(())
        );
        assert_eq!(crate::validate_basic_owned_filter_emission(&p), Ok(()));
        assert_eq!(
            validate_basic_owned_emission(&p),
            filter_scope_error(Some(Owner::Primary), primary(), Reason::ScopeControl)
        );
    }
    for generated in [false, true] {
        let mut p = sort_program(
            Some(SourceOwner::Primary),
            true,
            crate::SortFilterOrder::FilterThenSort,
        );
        let mut child = p.root.clone();
        child.target_field = "Child".into();
        child.bindings.clear();
        child.construction = TargetConstruction::Scalar {
            expression: 1,
            target_domain: ScalarTargetDomain::Single(ScalarType::String),
        };
        p.target = SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Child", ScalarType::String).repeating()],
        )
        .repeating();
        p.root.bindings.clear();
        p.root.children = vec![child];
        p.expressions.push(node(
            3,
            Expression::Const {
                value: Value::Int(2),
            },
        ));
        let input: crate::IterationSource = if generated {
            p.expressions.push(node(
                4,
                Expression::SourceField {
                    path: Vec::new(),
                    frame: None,
                },
            ));
            GeneratedSequence::Range {
                from: None,
                to: 3,
                item: 4,
            }
            .into()
        } else {
            SourceIteration::new(path(&["Rows"])).into()
        };
        p.root.iteration = Some(IterationPlan::new(
            input,
            None,
            None,
            if generated {
                Vec::new()
            } else {
                vec![crate::SequenceWindow::First { count: 3 }]
            },
            IterationOutput::Repeated,
        ));
        let route = primary().descendant(mapping::SourceScopeStep::Child(0));
        let expected = capability(
            Location::Iteration { scope: route },
            Some(Request::Iteration {
                owner: Owner::Primary,
                path: path(&["Rows"]),
            }),
            if generated {
                Reason::GeneratedItem
            } else {
                Reason::ScopeControl
            },
            Origin::Scope { scope: primary() },
        );
        assert_eq!(
            sort_observed(
                "S667-06 sorted Scalar does not clear inherited parent marker",
                &p
            ),
            expected
        );
    }
}

#[test]
fn basic_sort_profile_retains_potential_secondary_key_edges_and_frame_pins() {
    for order in [
        crate::SortFilterOrder::SortThenFilter,
        crate::SortFilterOrder::FilterThenSort,
    ] {
        for kind in 0..4 {
            for frame in [None, Some(path(&["D"])), Some(Vec::new())] {
                for reached in [false, true] {
                    let mut p = sort_program(None, false, order);
                    dynamic(&mut p);
                    p.expressions[1].expression = Expression::Const {
                        value: Value::Bool(false),
                    };
                    p.expressions.push(node(
                        99,
                        Expression::SourceField {
                            path: path(&["D", "Flag"]),
                            frame: frame.clone(),
                        },
                    ));
                    let dependency = match kind {
                        0 => Expression::If {
                            condition: 2,
                            then: 99,
                            else_: 2,
                        },
                        1 => Expression::RuntimeParameterDefault {
                            name: "key".into(),
                            ty: ScalarType::Bool,
                            default: 99,
                        },
                        2 => Expression::Raise { message: Some(99) },
                        _ => Expression::ValueMap {
                            input: 99,
                            input_type: Some(ScalarType::Bool),
                            table: vec![(Value::Bool(false), Value::Bool(true))],
                            default: Some(Value::Bool(true)),
                        },
                    };
                    p.expressions.push(node(3, dependency));
                    if reached {
                        let input = p.root.iteration.as_ref().unwrap().input().clone();
                        p.root.iteration = Some(IterationPlan::new(
                            input,
                            None,
                            Some(crate::SortPlan::new(
                                crate::SortKey {
                                    expression: 1,
                                    descending: false,
                                },
                                vec![crate::SortKey {
                                    expression: 3,
                                    descending: true,
                                }],
                                order,
                            )),
                            Vec::new(),
                            IterationOutput::Repeated,
                        ));
                    }
                    let before = p.clone();
                    assert_eq!(
                        sort_observed("S667-07 all potential secondary key edges/frame pins", &p),
                        if reached && frame.as_ref().is_none_or(|frame| !frame.is_empty()) {
                            retained(99, Reason::UnsupportedBasicContext)
                        } else {
                            Ok(())
                        }
                    );
                    assert_eq!(p, before);
                }
            }
        }
    }
}

#[test]
fn basic_sort_profile_keeps_failure_function_dynamic_and_stored_domains_closed() {
    let mut p = sort_program(
        Some(SourceOwner::Primary),
        true,
        crate::SortFilterOrder::SortThenFilter,
    );
    p.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(path(&["Rows"]))),
        selection: FailureSelection::All,
        message: Some(1),
    });
    let failure = capability(
        Location::Expression {
            namespace: Namespace::Main,
            node: 1,
            scope: None,
        },
        Some(Request::Field {
            owner: Owner::Primary,
            path: path(&["Rows", "Value"]),
            frame: Some(path(&["Rows"])),
        }),
        Reason::FailureRule,
        Origin::Failure { index: 0 },
    );
    assert_eq!(
        sort_observed("S667-08 failure ownership closed", &p),
        failure
    );
    let mut p = sort_program(None, false, crate::SortFilterOrder::FilterThenSort);
    unused_function(&mut p);
    let function = capability(
        Location::Expression {
            namespace: Namespace::Function(FunctionId::new(7)),
            node: 0,
            scope: None,
        },
        None,
        Reason::IsolatedFunction,
        Origin::FunctionBody {
            function: FunctionId::new(7),
        },
    );
    assert_eq!(
        sort_observed("S667-08 stored function closed", &p),
        function
    );
    let mut p = sort_program(None, false, crate::SortFilterOrder::FilterThenSort);
    p.expressions.push(node(99, Expression::SourceDocumentPath));
    assert_eq!(
        sort_observed("S667-08 stored document closed", &p),
        retained(99, Reason::Document)
    );
    let mut p = sort_program(
        Some(SourceOwner::Primary),
        false,
        crate::SortFilterOrder::SortThenFilter,
    );
    dynamic(&mut p);
    p.extra_sources[4].dynamic.as_mut().unwrap().path = 1;
    let loader = capability(
        Location::Expression {
            namespace: Namespace::Main,
            node: 1,
            scope: None,
        },
        Some(Request::Field {
            owner: Owner::Primary,
            path: path(&["Rows", "Value"]),
            frame: Some(path(&["Rows"])),
        }),
        Reason::DynamicLoader,
        Origin::DynamicSource {
            index: 4,
            name: "D".into(),
        },
    );
    assert_eq!(
        sort_observed("S667-08 stored dynamic path ownership closed", &p),
        loader
    );
}

#[test]
fn basic_sort_profile_preserves_named_scope_routes_and_dynamic_owner_requests() {
    let mut p = sort_program(
        Some(SourceOwner::Primary),
        true,
        crate::SortFilterOrder::SortThenFilter,
    );
    let mut named = sort_program(
        Some(SourceOwner::Named { name: "Ref".into() }),
        true,
        crate::SortFilterOrder::FilterThenSort,
    );
    named.root.repeating = false;
    named.target.repeating = false;
    let iteration = named.root.iteration.as_ref().unwrap();
    named.root.iteration = Some(IterationPlan::new(
        iteration.input().clone(),
        iteration.filter(),
        iteration.sort().cloned(),
        Vec::new(),
        IterationOutput::First,
    ));
    p.extra_targets.push(NamedTargetProgram {
        name: " Target 雪 ".into(),
        target: named.target,
        root: named.root,
    });
    let route = SourceScopeLocation::root(SourceScopeTarget::Named {
        index: 0,
        name: " Target 雪 ".into(),
    });
    assert_eq!(
        sort_observed("S667-09 exact named unsupported window route", &p),
        filter_scope_error(
            Some(Owner::Named { name: "Ref".into() }),
            route,
            Reason::ScopeControl
        )
    );
    let mut p = sort_program(None, false, crate::SortFilterOrder::SortThenFilter);
    dynamic(&mut p);
    p.expressions.push(node(
        99,
        field(
            SourceOwner::Named { name: "D".into() },
            &["Rows", "Flag"],
            Some(path(&["Rows"])),
        ),
    ));
    let input = p.root.iteration.as_ref().unwrap().input().clone();
    p.root.iteration = Some(IterationPlan::new(
        input,
        None,
        Some(crate::SortPlan::new(
            crate::SortKey {
                expression: 1,
                descending: false,
            },
            vec![crate::SortKey {
                expression: 99,
                descending: false,
            }],
            crate::SortFilterOrder::SortThenFilter,
        )),
        Vec::new(),
        IterationOutput::Repeated,
    ));
    let expected = capability(
        Location::Expression {
            namespace: Namespace::Main,
            node: 99,
            scope: Some(primary()),
        },
        Some(Request::Field {
            owner: Owner::Named { name: "D".into() },
            path: path(&["Rows", "Flag"]),
            frame: Some(path(&["Rows"])),
        }),
        Reason::DynamicOwner,
        Origin::DynamicSource {
            index: 4,
            name: "D".into(),
        },
    );
    assert_eq!(
        sort_observed(
            "S667-09 exact potential secondary owned dynamic request",
            &p
        ),
        expected
    );
}

// Window capability is explicit; these checks use complete public results.
mod window_profile {
    use super::*;

    fn windows_program(
        owner: Option<SourceOwner>,
        filtered: bool,
        sort: Option<crate::SortFilterOrder>,
        windows: Vec<crate::SequenceWindow>,
        output: IterationOutput,
    ) -> Program {
        let mut p = filter_program(owner);
        p.expressions.extend([
            node(
                3,
                Expression::Const {
                    value: Value::Int(2),
                },
            ),
            node(
                4,
                Expression::Const {
                    value: Value::Int(5),
                },
            ),
        ]);
        let input = p.root.iteration.as_ref().unwrap().input().clone();
        p.root.iteration = Some(IterationPlan::new(
            input,
            filtered.then_some(2),
            sort.map(|order| {
                crate::SortPlan::new(
                    crate::SortKey {
                        expression: 1,
                        descending: true,
                    },
                    vec![crate::SortKey {
                        expression: 3,
                        descending: false,
                    }],
                    order,
                )
            }),
            windows,
            output,
        ));
        if output == IterationOutput::First {
            p.target.repeating = false;
            p.root.repeating = false;
        }
        p
    }

    fn windows_observed(label: &str, p: &Program) -> Result<(), ProgramValidationError> {
        eprintln!("INPUT W698 {label} complete Program={p:#?}");
        let basic = crate::validate_basic_owned_window_emission(p);
        let complete = crate::validate_program_with_basic_owned_windows(p);
        eprintln!("RAW W698 {label} complete Basic={basic:#?} Program={complete:#?}");
        assert_eq!(complete, basic);
        basic
    }

    #[test]
    fn basic_window_profile_requires_opt_in_for_all_operators_and_first() {
        let controls = [
            (
                vec![crate::SequenceWindow::SkipFirst { count: 3 }],
                IterationOutput::Repeated,
            ),
            (
                vec![crate::SequenceWindow::First { count: 3 }],
                IterationOutput::Repeated,
            ),
            (
                vec![crate::SequenceWindow::From { position: 3 }],
                IterationOutput::Repeated,
            ),
            (
                vec![crate::SequenceWindow::FromTo { first: 3, last: 4 }],
                IterationOutput::Repeated,
            ),
            (
                vec![crate::SequenceWindow::Last { count: 3 }],
                IterationOutput::Repeated,
            ),
            (
                vec![
                    crate::SequenceWindow::SkipFirst { count: 3 },
                    crate::SequenceWindow::First { count: 4 },
                ],
                IterationOutput::Repeated,
            ),
            (
                vec![
                    crate::SequenceWindow::First { count: 4 },
                    crate::SequenceWindow::SkipFirst { count: 3 },
                ],
                IterationOutput::First,
            ),
            (Vec::new(), IterationOutput::First),
        ];
        for owner in [
            Some(SourceOwner::Primary),
            Some(SourceOwner::Named { name: "Ref".into() }),
            None,
        ] {
            for order in [
                crate::SortFilterOrder::SortThenFilter,
                crate::SortFilterOrder::FilterThenSort,
            ] {
                for (windows, output) in &controls {
                    let p =
                        windows_program(owner.clone(), true, Some(order), windows.clone(), *output);
                    let before = p.clone();
                    let old = filter_scope_error(
                        owner.as_ref().map(Owner::from),
                        primary(),
                        Reason::ScopeControl,
                    );
                    assert_eq!(validate_basic_owned_emission(&p), old);
                    assert_eq!(crate::validate_basic_owned_filter_emission(&p), old);
                    assert_eq!(crate::validate_basic_owned_sort_emission(&p), old);
                    if owner.is_some() {
                        assert_eq!(validate_program(&p), old);
                        assert_eq!(validate_owned_emission(&p), old);
                    }
                    assert_eq!(
                        windows_observed("operators/ordered composition/First/owners/orders", &p),
                        Ok(())
                    );
                    assert_eq!(p, before);
                }
            }
        }
    }

    #[test]
    fn basic_window_profile_lowering_preserves_the_complete_ordered_program() {
        for order in [
            mapping::SortFilterOrder::SortThenFilter,
            mapping::SortFilterOrder::FilterThenSort,
        ] {
            let mut project = crate::tests::supported_project();
            project.source = input_schema();
            project.target = SchemaNode::group(
                "Output",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            );
            let owned_field = OwnedSourceField {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Rows", "Value"]),
                },
                frame: Some(path(&["Rows"])),
            };
            let position = OwnedPosition {
                owner: SourceOwner::Primary,
                collection: Some(path(&["Rows"])),
            };
            project.graph.nodes = std::collections::BTreeMap::from([
                (1, mapping::Node::OwnedSourceField(owned_field.clone())),
                (
                    2,
                    mapping::Node::Const {
                        value: Value::Bool(true),
                    },
                ),
                (3, mapping::Node::OwnedPosition(position.clone())),
                (
                    4,
                    mapping::Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    5,
                    mapping::Node::Const {
                        value: Value::Int(5),
                    },
                ),
                (
                    99,
                    mapping::Node::Const {
                        value: Value::String("unused".into()),
                    },
                ),
            ]);
            project.root = mapping::Scope {
                iteration: mapping::ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: path(&["Rows"]),
                    },
                    output_path: None,
                },
                iteration_output: mapping::IterationOutput::First,
                filter: Some(2),
                sort_by: Some(1),
                sort_descending: true,
                sort_then_by: vec![mapping::SortKey {
                    node: 3,
                    descending: false,
                }],
                sort_filter_order: order,
                windows: vec![
                    mapping::SequenceWindow::SkipFirst { count: 4 },
                    mapping::SequenceWindow::First { count: 5 },
                    mapping::SequenceWindow::From { position: 4 },
                    mapping::SequenceWindow::FromTo { first: 4, last: 5 },
                    mapping::SequenceWindow::Last { count: 4 },
                ],
                bindings: vec![mapping::Binding {
                    target_field: "Value".into(),
                    node: 1,
                }],
                ..Default::default()
            };
            let before = format!("{project:#?}");
            let Err(ProgramValidationError::OwnedSource { diagnostic }) =
                filter_scope_error(Some(Owner::Primary), primary(), Reason::ScopeControl)
            else {
                unreachable!()
            };
            let closed = Err(crate::LowerError::new(vec![
                crate::Diagnostic::OwnedSource { diagnostic },
            ]));
            let ordinary = crate::lower(&project);
            let filtered = crate::lower_with_basic_owned_filters(&project);
            let sorted = crate::lower_with_basic_owned_sort(&project);
            let actual = crate::lower_with_basic_owned_windows(&project);
            eprintln!(
                "INPUT W698 genuine Project={project:#?} RAW default={ordinary:#?} filter={filtered:#?} sort={sorted:#?} windows={actual:#?}"
            );
            assert_eq!(ordinary, closed);
            assert_eq!(filtered, closed);
            assert_eq!(sorted, closed);
            let mut expected = program(false);
            expected.extra_sources.clear();
            expected.expressions = vec![
                node(1, Expression::OwnedSourceField(owned_field)),
                node(
                    2,
                    Expression::Const {
                        value: Value::Bool(true),
                    },
                ),
                node(3, Expression::OwnedPosition(position)),
                node(
                    4,
                    Expression::Const {
                        value: Value::Int(2),
                    },
                ),
                node(
                    5,
                    Expression::Const {
                        value: Value::Int(5),
                    },
                ),
            ];
            expected.root.iteration = Some(IterationPlan::new(
                crate::OwnedSourceIteration::new(OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: path(&["Rows"]),
                }),
                Some(2),
                Some(crate::SortPlan::new(
                    crate::SortKey {
                        expression: 1,
                        descending: true,
                    },
                    vec![crate::SortKey {
                        expression: 3,
                        descending: false,
                    }],
                    order.into(),
                )),
                vec![
                    crate::SequenceWindow::SkipFirst { count: 4 },
                    crate::SequenceWindow::First { count: 5 },
                    crate::SequenceWindow::From { position: 4 },
                    crate::SequenceWindow::FromTo { first: 4, last: 5 },
                    crate::SequenceWindow::Last { count: 4 },
                ],
                IterationOutput::First,
            ));
            assert_eq!(actual, Ok(expected.clone()));
            assert_eq!(windows_observed("whole lowered Program", &expected), Ok(()));
            assert_eq!(format!("{project:#?}"), before);
        }
    }

    #[test]
    fn basic_window_profile_preserves_structural_error_precedence() {
        let mut p = windows_program(
            Some(SourceOwner::Primary),
            false,
            None,
            vec![crate::SequenceWindow::FromTo { first: 3, last: 99 }],
            IterationOutput::Repeated,
        );
        let missing = Err(ProgramValidationError::MissingWindowExpression {
            target_path: Vec::new(),
            window: 0,
            bound: 1,
            expression: 99,
        });
        assert_eq!(windows_observed("missing second endpoint", &p), missing);
        assert_eq!(validate_program(&p), missing);
        p.expressions.push(node(
            3,
            Expression::Const {
                value: Value::Int(1),
            },
        ));
        let duplicate = Err(ProgramValidationError::DuplicateExpression { node: 3 });
        assert_eq!(
            windows_observed("duplicate precedes missing endpoint/capability", &p),
            duplicate
        );
        assert_eq!(validate_program(&p), duplicate);

        let mut p = windows_program(
            Some(SourceOwner::Primary),
            false,
            None,
            vec![crate::SequenceWindow::First { count: 3 }],
            IterationOutput::Repeated,
        );
        p.expressions[2].expression = field(
            SourceOwner::Named {
                name: "Absent".into(),
            },
            &["Value"],
            Some(Vec::new()),
        );
        let unknown = Err(ProgramValidationError::OwnedSource {
            diagnostic: Box::new(Diagnostic::Structure {
                location: Location::Expression {
                    namespace: Namespace::Main,
                    node: 3,
                    scope: None,
                },
                request: Request::Field {
                    owner: Owner::Named {
                        name: "Absent".into(),
                    },
                    path: path(&["Value"]),
                    frame: Some(Vec::new()),
                },
                reason: Structure::UnknownOwner,
            }),
        });
        assert_eq!(
            windows_observed("stored bound owner structure before capability", &p),
            unknown
        );
        assert_eq!(validate_basic_owned_emission(&p), unknown);
    }

    #[test]
    fn basic_window_profile_limits_scalar_permission_and_preserves_inherited_special_context() {
        for implicit in [false, true] {
            let mut p = windows_program(
                Some(SourceOwner::Primary),
                false,
                None,
                vec![crate::SequenceWindow::First { count: 3 }],
                IterationOutput::Repeated,
            );
            p.target = SchemaNode::scalar("Output", ScalarType::String).repeating();
            p.root.bindings.clear();
            p.root.construction = TargetConstruction::Scalar {
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::String),
            };
            assert_eq!(windows_observed("windowed Scalar own marker", &p), Ok(()));
            let input = p.root.iteration.as_ref().unwrap().input().clone();
            if implicit {
                p.target.repeating = false;
                p.root.repeating = false;
                p.root.iteration = Some(IterationPlan::new(
                    input,
                    None,
                    None,
                    Vec::new(),
                    IterationOutput::First,
                ));
                assert_eq!(
                    windows_observed("Scalar First remains structurally invalid", &p),
                    Err(ProgramValidationError::InvalidIterationOutput {
                        target_path: Vec::new(),
                        output: IterationOutput::First
                    })
                );
            } else {
                p.root.iteration = Some(IterationPlan::new(
                    input,
                    None,
                    None,
                    Vec::new(),
                    IterationOutput::Repeated,
                ));
                assert_eq!(
                    windows_observed("unsorted unwindowed Scalar own marker is Basic", &p),
                    Ok(())
                );
                assert_eq!(crate::validate_basic_owned_sort_emission(&p), Ok(()));
                assert_eq!(validate_basic_owned_emission(&p), Ok(()));
            }
        }
        for generated in [false, true] {
            let mut p = windows_program(
                Some(SourceOwner::Primary),
                false,
                None,
                vec![crate::SequenceWindow::First { count: 3 }],
                IterationOutput::Repeated,
            );
            let mut child = p.root.clone();
            child.target_field = "Child".into();
            child.bindings.clear();
            child.construction = TargetConstruction::Scalar {
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::String),
            };
            p.target = SchemaNode::group(
                "Output",
                vec![SchemaNode::scalar("Child", ScalarType::String).repeating()],
            )
            .repeating();
            p.root.bindings.clear();
            p.root.children = vec![child];
            let input: crate::IterationSource = if generated {
                p.expressions.push(node(
                    5,
                    Expression::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ));
                GeneratedSequence::Range {
                    from: None,
                    to: 3,
                    item: 5,
                }
                .into()
            } else {
                SourceIteration::new(path(&["Rows"])).into()
            };
            let mut parent =
                IterationPlan::new(input, None, None, Vec::new(), IterationOutput::Repeated);
            if !generated {
                parent = parent.with_grouping(crate::GroupingPlan::By { key: 3 });
            }
            p.root.iteration = Some(parent);
            let route = primary().descendant(mapping::SourceScopeStep::Child(0));
            let expected = capability(
                Location::Iteration { scope: route },
                Some(Request::Iteration {
                    owner: Owner::Primary,
                    path: path(&["Rows"]),
                }),
                if generated {
                    Reason::GeneratedItem
                } else {
                    Reason::Grouping
                },
                Origin::Scope { scope: primary() },
            );
            assert_eq!(
                windows_observed(
                    "child Scalar window does not erase inherited closed parent",
                    &p
                ),
                expected
            );
        }
    }

    #[test]
    fn basic_window_profile_distinguishes_parent_bounds_from_the_same_item_key() {
        for key in [false, true] {
            let mut p = windows_program(
                Some(SourceOwner::Primary),
                false,
                None,
                vec![crate::SequenceWindow::First { count: 3 }],
                IterationOutput::Repeated,
            );
            p.expressions[2].expression = Expression::SourceDocumentPath;
            if key {
                let input = p.root.iteration.as_ref().unwrap().input().clone();
                p.root.iteration = Some(IterationPlan::new(
                    input,
                    None,
                    Some(crate::SortPlan::new(
                        crate::SortKey {
                            expression: 3,
                            descending: false,
                        },
                        Vec::new(),
                        crate::SortFilterOrder::SortThenFilter,
                    )),
                    vec![crate::SequenceWindow::First { count: 3 }],
                    IterationOutput::Repeated,
                ));
            }
            let expected = if key {
                capability(
                    Location::Expression {
                        namespace: Namespace::Main,
                        node: 3,
                        scope: Some(primary()),
                    },
                    None,
                    Reason::XmlOperator,
                    Origin::Expression {
                        namespace: Namespace::Main,
                        node: 3,
                    },
                )
            } else {
                retained(3, Reason::Document)
            };
            assert_eq!(
                windows_observed("same node bound-only versus bound-plus-item-key", &p),
                expected
            );
        }
        let mut p = windows_program(
            Some(SourceOwner::Primary),
            false,
            None,
            Vec::new(),
            IterationOutput::Repeated,
        );
        let mut child = p.root.clone();
        child.target_field = "Child".into();
        child.iteration = Some(IterationPlan::new(
            crate::OwnedSourceIteration::new(OwnedSourceRef {
                owner: SourceOwner::Named { name: "Ref".into() },
                path: path(&["Rows"]),
            }),
            None,
            None,
            vec![crate::SequenceWindow::First { count: 3 }],
            IterationOutput::Repeated,
        ));
        p.expressions[2].expression = Expression::SourceDocumentPath;
        p.target = SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Child",
                    vec![SchemaNode::scalar("Value", ScalarType::String)],
                )
                .repeating(),
            ],
        )
        .repeating();
        p.root.bindings.clear();
        p.root.children = vec![child];
        let route = primary().descendant(mapping::SourceScopeStep::Child(0));
        assert_eq!(
            windows_observed(
                "bound inherits outer owned frame but not new inner frame",
                &p
            ),
            capability(
                Location::Expression {
                    namespace: Namespace::Main,
                    node: 3,
                    scope: Some(route)
                },
                None,
                Reason::XmlOperator,
                Origin::Expression {
                    namespace: Namespace::Main,
                    node: 3
                },
            )
        );
    }

    #[test]
    fn basic_window_profile_retains_all_potential_bound_edges_and_exact_frame_pins() {
        for kind in 0..4 {
            for frame in [None, Some(path(&["D"])), Some(Vec::new())] {
                for reached in [false, true] {
                    let mut p = windows_program(
                        Some(SourceOwner::Primary),
                        false,
                        None,
                        vec![crate::SequenceWindow::First {
                            count: if reached { 3 } else { 4 },
                        }],
                        IterationOutput::Repeated,
                    );
                    dynamic(&mut p);
                    p.expressions[1].expression = Expression::Const {
                        value: Value::Bool(false),
                    };
                    p.expressions.push(node(
                        99,
                        Expression::SourceField {
                            path: path(&["D", "Flag"]),
                            frame: frame.clone(),
                        },
                    ));
                    p.expressions[2].expression = match kind {
                        0 => Expression::If {
                            condition: 2,
                            then: 99,
                            else_: 4,
                        },
                        1 => Expression::RuntimeParameterDefault {
                            name: "count".into(),
                            ty: ScalarType::Int,
                            default: 99,
                        },
                        2 => Expression::Raise { message: Some(99) },
                        _ => Expression::ValueMap {
                            input: 99,
                            input_type: Some(ScalarType::Bool),
                            table: vec![(Value::Bool(false), Value::Int(2))],
                            default: Some(Value::Int(0)),
                        },
                    };
                    let before = p.clone();
                    assert_eq!(
                        windows_observed(
                            "potential inactive/default/Raise/ValueMap bound dependencies",
                            &p
                        ),
                        if reached && frame.as_ref().is_none_or(|frame| !frame.is_empty()) {
                            retained(99, Reason::UnsupportedBasicContext)
                        } else {
                            Ok(())
                        }
                    );
                    assert_eq!(p, before);
                }
            }
        }
        let mut p = windows_program(
            Some(SourceOwner::Primary),
            false,
            None,
            vec![crate::SequenceWindow::First { count: 3 }],
            IterationOutput::Repeated,
        );
        dynamic(&mut p);
        p.expressions[2].expression = field(
            SourceOwner::Named { name: "D".into() },
            &["Rows", "Flag"],
            Some(path(&["Rows"])),
        );
        assert_eq!(
            windows_observed("parent bound preserves exact dynamic owned request", &p),
            capability(
                Location::Expression {
                    namespace: Namespace::Main,
                    node: 3,
                    scope: Some(primary())
                },
                Some(Request::Field {
                    owner: Owner::Named { name: "D".into() },
                    path: path(&["Rows", "Flag"]),
                    frame: Some(path(&["Rows"]))
                }),
                Reason::DynamicOwner,
                Origin::DynamicSource {
                    index: 4,
                    name: "D".into()
                },
            )
        );
    }

    #[test]
    fn basic_window_profile_preserves_older_supported_results_and_closed_domains() {
        for p in [
            program(false),
            program(true),
            filter_program(Some(SourceOwner::Primary)),
            sort_program(
                Some(SourceOwner::Primary),
                true,
                crate::SortFilterOrder::FilterThenSort,
            ),
        ] {
            let old = crate::validate_basic_owned_sort_emission(&p);
            assert_eq!(
                windows_observed("older supported complete Program", &p),
                old
            );
        }
        let project = crate::tests::supported_project();
        let old = crate::lower_with_basic_owned_sort(&project);
        let actual = crate::lower_with_basic_owned_windows(&project);
        eprintln!(
            "INPUT W698 unchanged complete Project={project:#?} RAW sort={old:#?} windows={actual:#?}"
        );
        assert_eq!(actual, old);
        assert!(actual.is_ok());

        let mut p = windows_program(
            Some(SourceOwner::Primary),
            false,
            None,
            vec![crate::SequenceWindow::First { count: 3 }],
            IterationOutput::Repeated,
        );
        p.root.iteration = Some(
            p.root
                .iteration
                .take()
                .unwrap()
                .with_grouping(crate::GroupingPlan::By { key: 3 }),
        );
        assert_eq!(
            windows_observed("grouping stays closed", &p),
            filter_scope_error(Some(Owner::Primary), primary(), Reason::Grouping)
        );
        let mut p = windows_program(
            None,
            false,
            None,
            vec![crate::SequenceWindow::First { count: 3 }],
            IterationOutput::Repeated,
        );
        p.expressions.push(node(
            5,
            Expression::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ));
        p.root.iteration = Some(IterationPlan::new(
            GeneratedSequence::Range {
                from: None,
                to: 3,
                item: 5,
            },
            None,
            None,
            vec![crate::SequenceWindow::First { count: 3 }],
            IterationOutput::Repeated,
        ));
        assert_eq!(
            windows_observed("generated window input stays closed", &p),
            filter_scope_error(None, primary(), Reason::GeneratedItem)
        );
        let mut p = windows_program(
            None,
            false,
            None,
            vec![crate::SequenceWindow::First { count: 3 }],
            IterationOutput::Repeated,
        );
        unused_function(&mut p);
        assert_eq!(
            windows_observed("stored function remains isolated", &p),
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
                },
            )
        );
        let mut p = windows_program(
            Some(SourceOwner::Primary),
            false,
            None,
            vec![crate::SequenceWindow::First { count: 3 }],
            IterationOutput::Repeated,
        );
        p.failure_rules.push(FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(path(&["Rows"]))),
            selection: FailureSelection::All,
            message: Some(1),
        });
        assert_eq!(
            windows_observed("failure ownership remains closed", &p),
            capability(
                Location::Expression {
                    namespace: Namespace::Main,
                    node: 1,
                    scope: None
                },
                Some(Request::Field {
                    owner: Owner::Primary,
                    path: path(&["Rows", "Value"]),
                    frame: Some(path(&["Rows"]))
                }),
                Reason::FailureRule,
                Origin::Failure { index: 0 },
            )
        );
    }
}
