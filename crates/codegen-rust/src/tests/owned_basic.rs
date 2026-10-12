use super::*;
use mapping::{OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};
use std::error::Error;
use std::io::Write;

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn owned(owner: SourceOwner, parts: &[&str], frame: Option<&[&str]>) -> Expression {
    Expression::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: frame.map(path),
    })
}
fn position(owner: SourceOwner, collection: Option<&[&str]>) -> Expression {
    Expression::OwnedPosition(OwnedPosition {
        owner,
        collection: collection.map(path),
    })
}
fn named(name: &str) -> SourceOwner {
    SourceOwner::Named { name: name.into() }
}
fn node(id: u32, expression: Expression) -> ExpressionNode {
    ExpressionNode { id, expression }
}
fn source_schema() -> SchemaNode {
    SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group("Ref", vec![SchemaNode::scalar("Value", ScalarType::String)]),
            SchemaNode::group(
                "Details",
                vec![SchemaNode::scalar("Counter", ScalarType::Int)],
            ),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ScalarType::String),
                    SchemaNode::scalar("Score", ScalarType::Int),
                    SchemaNode::group(
                        "Details",
                        vec![SchemaNode::scalar("Counter", ScalarType::Int)],
                    ),
                    SchemaNode::group(
                        "Rows",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
fn scope(fields: &[(&str, u32, ScalarType)]) -> TargetScope {
    TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings: fields
            .iter()
            .map(|(name, expression, ty)| Binding {
                target_field: (*name).into(),
                expression: *expression,
                target_domain: ScalarTargetDomain::Single(*ty),
                repeating: false,
            })
            .collect(),
        children: Vec::new(),
    }
}
fn basic(nodes: Vec<ExpressionNode>, fields: &[(&str, u32, ScalarType)]) -> Program {
    Program {
        xml_boundary: None,
        source: source_schema(),
        extra_sources: ["Unused", "Ref", " Ref 雪 ", "ref", "primary"]
            .into_iter()
            .map(|name| NamedSourceProgram {
                name: name.into(),
                source: source_schema(),
                dynamic: None,
            })
            .collect(),
        target: SchemaNode::group(
            "Output",
            fields
                .iter()
                .map(|(name, _, ty)| SchemaNode::scalar(*name, *ty))
                .collect(),
        ),
        expressions: nodes,
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: scope(fields),
        extra_targets: Vec::new(),
    }
}
fn private_result(label: &str, program: &Program) -> Result<String, EmitError> {
    eprintln!("INPUT {label} program={program:?}");
    let actual = super::super::owned_basic::render_basic_owned_source(program);
    eprintln!("RAW {label} source_result={actual:?}");
    if let Err(error) = &actual {
        eprintln!(
            "RAW {label} source_cause={:?}",
            error.source().map(ToString::to_string)
        );
    }
    actual
}
fn private_source(label: &str, program: &Program) -> String {
    private_result(label, program).expect("complete private source result captured")
}

#[test]
fn private_basic_literals_preserve_exact_owner_and_present_empty_identity() {
    let p = basic(
        vec![
            node(1, owned(named(" Ref 雪 "), &["Value"], None)),
            node(2, position(named("primary"), Some(&[]))),
            node(
                3,
                Expression::Const {
                    value: Value::String("quote\" backslash\\ newline\n tab\t".into()),
                },
            ),
        ],
        &[("Value", 1, ScalarType::String)],
    );
    let source = private_source("literal", &p);
    let access = super::super::owned_basic::render_owned_access(&p.expressions[1].expression);
    eprintln!("RAW literal absent-from-target position_body={access:?}");
    assert!(source.contains("SourceOwnerRef::Named(\" Ref \\u{96ea} \")"));
    assert!(source.contains(
        "context.owned_field(SourceOwnerRef::Named(\" Ref \\u{96ea} \"), &[\"Value\"], None)"
    ));
    assert_eq!(access, Some("{ let position = context.owned_position(SourceOwnerRef::Named(\"primary\"), Some(&[][..]))?; Ok(Value::Int(position as i64)) }".into()));
    let escaped = rust_string("quote\" backslash\\ newline\n tab\t");
    eprintln!("RAW literal control_escape={escaped:?}");
    assert_eq!(escaped, "\"quote\\\" backslash\\\\ newline\\n tab\\t\"");
}

#[test]
fn private_basic_source_holds_real_cohorts_and_keeps_explicit_policy() {
    let mut p = basic(
        vec![node(1, owned(named("Ref"), &["Rows", "Value"], None))],
        &[("Value", 1, ScalarType::String)],
    );
    p.root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: named("Ref"),
        path: path(&["Rows"]),
    }));
    p.target = p.target.clone().repeating();
    p.root.repeating = true;
    let source = private_source("cohort", &p);
    assert!(source.contains("let cohort = context.walk_owned_source_cohort(SourceOwnerRef::Named(\"Ref\"), &[\"Rows\"])?;"));
    assert!(source.contains(
        "for candidate in cohort.iter() {\n        let item_context = candidate.context();"
    ));
    assert!(source.contains("declared: &'src [DeclaredNamedSourceRoot<'src>]"));
    assert!(source.contains("limits: OwnedSourceLimits"));
    assert!(source.contains("BasicSourceRoots::try_new(source, declared, limits)?"));
    assert!(!source.contains("OwnedSourceLimits {"));
    assert!(!source.contains("with_phase_position"));
    assert!(!source.contains("reindex_phase"));
    assert!(!source.contains("DynamicSourceLoader"));
    assert!(!source.contains("parse_json"));
    assert!(!source.contains("serialize_json"));
    let select = source.find("let selected = match selection").unwrap();
    let roots = source[select..].find("BasicSourceRoots::try_new").unwrap();
    let unknown = source[select..]
        .find("RuntimeError::UnknownTarget")
        .unwrap();
    assert!(unknown < roots);
}

#[test]
fn private_expression_seam_keeps_legacy_golden_bytes_and_public_errors() {
    let functions = BTreeMap::new();
    let expression = Expression::Const {
        value: Value::Int(7),
    };
    let ordinary = render_expression(9, &expression, "expression_", None, &functions, false, None);
    let inner = render_expression_in_context(
        9,
        &expression,
        "expression_",
        None,
        &functions,
        ExpressionEmission {
            basic: false,
            filter_map: false,
            owner_function: None,
        },
    );
    eprintln!("RAW legacy ordinary={ordinary:?} common={inner:?}");
    assert_eq!(
        ordinary.unwrap(),
        "fn expression_9(context: &ScopeContext<'_>) -> Result<Value, RuntimeError> {\n    let _ = context;\n    Ok(Value::Int(7_i64))\n}\n\n"
    );
    assert_eq!(
        inner.unwrap(),
        "fn expression_9(context: &ScopeContext<'_>) -> Result<Value, RuntimeError> {\n    let _ = context;\n    Ok(Value::Int(7_i64))\n}\n\n"
    );
    let p = basic(
        vec![node(1, owned(SourceOwner::Primary, &["Value"], None))],
        &[("Value", 1, ScalarType::String)],
    );
    let actual = emit(
        &p,
        &Options {
            package_name: "valid_private_check".into(),
            runtime_dependency: RuntimeDependency::Version("0.0.0".into()),
        },
    );
    eprintln!("RAW public_gate={actual:?}");
    let expected = codegen::owned_expression_emission_error(
        codegen::OwnedSourceNamespace::Main,
        1,
        &p.expressions[0].expression,
    );
    match actual {
        Err(EmitError::InvalidProgram(error)) => assert_eq!(error, expected),
        other => panic!("captured public result differs: {other:?}"),
    }
    let invalid_package = emit(
        &p,
        &Options {
            package_name: "0invalid".into(),
            runtime_dependency: RuntimeDependency::Version("0.0.0".into()),
        },
    );
    eprintln!("RAW public_package={invalid_package:?}");
    assert!(
        matches!(invalid_package, Err(EmitError::InvalidPackageName(name)) if name == "0invalid")
    );
}

#[test]
fn private_basic_shape_and_control_denials_keep_full_typed_diagnostics() {
    let p = basic(
        vec![node(1, owned(named("Missing"), &["Value"], None))],
        &[("Value", 1, ScalarType::String)],
    );
    let actual = private_result("unknown-owner", &p);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Structure {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
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
    match actual {
        Err(EmitError::InvalidProgram(error)) => assert_eq!(error, expected),
        other => panic!("captured structural result differs: {other:?}"),
    }
    let mut p = basic(
        vec![
            node(1, owned(SourceOwner::Primary, &["Rows", "Value"], None)),
            node(
                2,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
        ],
        &[("Value", 1, ScalarType::String)],
    );
    p.target = p.target.clone().repeating();
    p.root.repeating = true;
    p.root.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        }),
        Some(2),
        None,
        Vec::new(),
        IterationOutput::Repeated,
    ));
    let actual = private_result("deferred-filter", &p);
    let route = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Iteration {
                scope: route.clone(),
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Iteration {
                owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                path: path(&["Rows"]),
            }),
            reason: codegen::OwnedSourceCapabilityReason::ScopeControl,
            origin: codegen::OwnedSourceCapabilityOrigin::Scope { scope: route },
        }),
    };
    match actual {
        Err(EmitError::InvalidProgram(error)) => assert_eq!(error, expected),
        other => panic!("captured control result differs: {other:?}"),
    }
}

#[test]
fn private_basic_omits_unused_valid_leaves_but_keeps_structural_admission() {
    let mut p = basic(
        vec![
            node(1, owned(SourceOwner::Primary, &["Value"], None)),
            node(99, owned(named("Ref"), &["Value"], None)),
        ],
        &[("Value", 1, ScalarType::String)],
    );
    p.user_functions.push(UserFunctionProgram {
        id: FunctionId::new(9),
        library: "unused".into(),
        name: "ordinary".into(),
        parameters: Vec::new(),
        output_type: ScalarType::Int,
        expressions: vec![node(
            1,
            Expression::Const {
                value: Value::Int(7),
            },
        )],
        output: 1,
    });
    let source = private_source("dead valid leaf", &p);
    assert!(source.contains("fn expression_1("));
    assert!(!source.contains("fn expression_99("));
    assert!(!source.contains("fn function_9("));
    p.expressions[1].expression = owned(named("Missing"), &["Value"], None);
    let actual = private_result("dead malformed owner", &p);
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Structure {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 99,
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
    match actual {
        Err(EmitError::InvalidProgram(error)) => assert_eq!(error, expected),
        other => panic!("captured dead structural result differs: {other:?}"),
    }
}

// Ignored generated execution is admitted separately by the root operator. No
// extra build cache or automatic cleanup is created by these source proposals.
fn retained_file(path: &Path, bytes: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .unwrap();
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
}

fn generated_programs() -> Vec<(&'static str, Program, &'static str)> {
    let mut cases = Vec::new();
    let p01 = basic(
        vec![node(1, owned(SourceOwner::Primary, &["Value"], None))],
        &[("Value", 1, ScalarType::String)],
    );
    cases.push(("p01", p01.clone(), CHECK_P01));
    let p02 = basic(
        vec![
            node(1, owned(named("Ref"), &["Value"], None)),
            node(
                2,
                Expression::SourceField {
                    path: path(&["Ref", "Value"]),
                    frame: None,
                },
            ),
        ],
        &[
            ("Named", 1, ScalarType::String),
            ("Legacy", 2, ScalarType::String),
        ],
    );
    cases.push(("p02", p02, CHECK_P02));
    let p03 = basic(
        vec![
            node(1, owned(named(" Ref 雪 "), &["Value"], None)),
            node(2, owned(named("ref"), &["Value"], None)),
            node(3, owned(named("primary"), &["Value"], None)),
        ],
        &[
            ("Exact", 1, ScalarType::String),
            ("Lower", 2, ScalarType::String),
            ("LiteralPrimary", 3, ScalarType::String),
        ],
    );
    cases.push(("p03", p03, CHECK_P03));
    let mut p04 = basic(
        vec![
            node(1, owned(named("Ref"), &["Rows", "Value"], None)),
            node(2, position(named("Ref"), Some(&["Rows"]))),
            node(
                3,
                Expression::Position {
                    collection: path(&["Ref", "Rows"]),
                },
            ),
        ],
        &[
            ("Value", 1, ScalarType::String),
            ("OwnedPosition", 2, ScalarType::Int),
            ("LegacyPosition", 3, ScalarType::Int),
        ],
    );
    p04.root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: named("Ref"),
        path: path(&["Rows"]),
    }));
    p04.target = p04.target.clone().repeating();
    p04.root.repeating = true;
    cases.push(("p04", p04, CHECK_P04));
    let mut p05 = basic(
        vec![
            node(
                1,
                owned(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
            ),
            node(
                2,
                owned(SourceOwner::Primary, &["Rows", "Rows", "Value"], None),
            ),
            node(3, position(SourceOwner::Primary, Some(&["Rows"]))),
            node(4, position(SourceOwner::Primary, Some(&["Rows", "Rows"]))),
        ],
        &[
            ("Outer", 1, ScalarType::String),
            ("Inner", 2, ScalarType::String),
            ("ParentPosition", 3, ScalarType::Int),
            ("Position", 4, ScalarType::Int),
        ],
    );
    p05.root.iteration = Some(IterationPlan::source(path(&["Rows", "Rows"])));
    p05.target = p05.target.clone().repeating();
    p05.root.repeating = true;
    cases.push(("p05", p05, CHECK_P05));
    let p06 = basic(
        vec![
            node(
                1,
                owned(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
            ),
            node(
                2,
                owned(SourceOwner::Primary, &["Rows", "Rows", "Value"], None),
            ),
            node(3, position(SourceOwner::Primary, Some(&["Rows"]))),
            node(4, position(SourceOwner::Primary, Some(&["Rows", "Rows"]))),
        ],
        &[
            ("Outer", 1, ScalarType::String),
            ("Inner", 2, ScalarType::String),
            ("ParentPosition", 3, ScalarType::Int),
            ("Position", 4, ScalarType::Int),
        ],
    );
    cases.push(("p06", p06, CHECK_P06));
    let p07 = basic(
        vec![
            node(
                1,
                owned(named("Ref"), &["Rows", "Details", "Counter"], None),
            ),
            node(
                2,
                owned(
                    named("Ref"),
                    &["Rows", "Details", "Counter"],
                    Some(&["Rows"]),
                ),
            ),
            node(
                3,
                Expression::SourceField {
                    path: path(&["Details", "Counter"]),
                    frame: None,
                },
            ),
        ],
        &[
            ("Unframed", 1, ScalarType::Int),
            ("Pinned", 2, ScalarType::Int),
            ("Legacy", 3, ScalarType::Int),
        ],
    );
    cases.push(("p07", p07, CHECK_P07));
    let mut p08 = basic(
        vec![
            node(
                1,
                owned(SourceOwner::Primary, &["Details", "Counter"], None),
            ),
            node(2, position(SourceOwner::Primary, None)),
            node(
                3,
                Expression::Position {
                    collection: Vec::new(),
                },
            ),
        ],
        &[("Counter", 1, ScalarType::Int)],
    );
    p08.root.iteration = Some(IterationPlan::source(path(&["Details"])));
    p08.target = p08.target.clone().repeating();
    p08.root.repeating = true;
    p08.extra_targets.push(NamedTargetProgram {
        name: "probes".into(),
        target: SchemaNode::group(
            "Probes",
            vec![
                SchemaNode::scalar("OwnedPosition", ScalarType::Int),
                SchemaNode::scalar("LegacyPosition", ScalarType::Int),
            ],
        ),
        root: scope(&[
            ("OwnedPosition", 2, ScalarType::Int),
            ("LegacyPosition", 3, ScalarType::Int),
        ]),
    });
    cases.push(("p08", p08, CHECK_P08));
    let mut p09 = basic(
        vec![
            node(
                1,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            node(
                2,
                Expression::Const {
                    value: Value::String("chosen".into()),
                },
            ),
            node(3, position(SourceOwner::Primary, Some(&["Rows"]))),
            node(
                4,
                Expression::If {
                    condition: 1,
                    then: 2,
                    else_: 3,
                },
            ),
        ],
        &[("Value", 4, ScalarType::String)],
    );
    let union = ir::ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap();
    p09.target = SchemaNode::group("Output", vec![SchemaNode::scalar_union("Value", union)]);
    p09.root.bindings[0].target_domain = ScalarTargetDomain::Union(union);
    cases.push(("p09_true", p09.clone(), CHECK_P09_TRUE));
    p09.expressions[0].expression = Expression::Const {
        value: Value::Bool(false),
    };
    cases.push(("p09_false", p09, CHECK_P09_FALSE));
    cases.push(("p10_unused", p01.clone(), CHECK_P10_UNUSED));
    let p10 = basic(
        vec![node(1, owned(named("Unused"), &["Value"], None))],
        &[("Value", 1, ScalarType::String)],
    );
    cases.push(("p10_demand", p10, CHECK_P10_DEMAND));
    let p11 = basic(
        vec![node(
            1,
            owned(SourceOwner::Primary, &["Details", "Counter"], None),
        )],
        &[("Counter", 1, ScalarType::Int)],
    );
    cases.push(("p11", p11, CHECK_P11));
    let mut p12 = basic(
        vec![
            node(1, owned(SourceOwner::Primary, &["Value"], None)),
            node(2, position(SourceOwner::Primary, Some(&[]))),
        ],
        &[
            ("Value", 1, ScalarType::String),
            ("Position", 2, ScalarType::Int),
        ],
    );
    p12.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    p12.root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: Vec::new(),
    }));
    p12.target = p12.target.clone().repeating();
    p12.root.repeating = true;
    cases.push(("p12", p12, CHECK_P12));
    let p13 = basic(
        vec![
            node(1, position(SourceOwner::Primary, Some(&["Rows"]))),
            node(2, position(named("Ref"), Some(&["Rows"]))),
        ],
        &[
            ("PrimaryPosition", 1, ScalarType::Int),
            ("NamedPosition", 2, ScalarType::Int),
        ],
    );
    cases.push(("p13", p13, CHECK_P13));
    let mut p17 = basic(
        vec![
            node(
                1,
                owned(SourceOwner::Primary, &["Rows", "Details", "Counter"], None),
            ),
            node(2, position(SourceOwner::Primary, Some(&["Rows"]))),
        ],
        &[
            ("Counter", 1, ScalarType::Int),
            ("Position", 2, ScalarType::Int),
        ],
    );
    p17.source = source_schema();
    cases.push(("p17", p17, CHECK_P17));
    let mut p18 = basic(
        vec![
            node(1, owned(SourceOwner::Primary, &["Value"], None)),
            node(2, position(SourceOwner::Primary, Some(&[]))),
        ],
        &[("Value", 1, ScalarType::String)],
    );
    p18.source = SchemaNode::group(
        "Input",
        vec![SchemaNode::scalar("Value", ScalarType::String)],
    );
    p18.root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: Vec::new(),
    }));
    p18.target = p18.target.clone().repeating();
    p18.root.repeating = true;
    cases.push(("p18", p18, CHECK_P18));
    let mut p19 = p01.clone();
    p19.expressions
        .push(node(71, Expression::Raise { message: None }));
    p19.extra_targets.push(NamedTargetProgram {
        name: "named".into(),
        target: SchemaNode::group(
            "Named",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        root: scope(&[("Value", 71, ScalarType::String)]),
    });
    cases.push(("p19", p19, CHECK_P19));
    let mut p20 = p01;
    p20.expressions
        .push(node(2, owned(named("Ref"), &["Value"], None)));
    p20.extra_targets.push(NamedTargetProgram {
        name: "named".into(),
        target: SchemaNode::group(
            "Named",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        root: scope(&[("Value", 2, ScalarType::String)]),
    });
    cases.push(("p20", p20, CHECK_P20));
    let mut r02 = basic(
        vec![node(1, owned(SourceOwner::Primary, &[], None))],
        &[("Value", 1, ScalarType::String)],
    );
    r02.source = SchemaNode::scalar("Input", ScalarType::String);
    r02.extra_sources.clear();
    cases.push(("r04", r02.clone(), CHECK_R04));
    cases.push(("r02", r02, CHECK_R02));
    let mut r03 = basic(
        vec![node(
            1,
            owned(SourceOwner::Primary, &["Rows", "Value"], None),
        )],
        &[("Value", 1, ScalarType::String)],
    );
    r03.root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: path(&["Rows"]),
    }));
    r03.target = r03.target.clone().repeating();
    r03.root.repeating = true;
    cases.push(("r03", r03, CHECK_R03));
    let r01 = basic(
        vec![node(1, owned(SourceOwner::Primary, &["Value"], None))],
        &[("Value", 1, ScalarType::String)],
    );
    cases.push(("r01", r01.clone(), CHECK_R01));
    cases.push(("r05", r01, CHECK_R05));
    let host = basic(
        vec![
            node(1, owned(SourceOwner::Primary, &["Value"], None)),
            node(
                100,
                Expression::Const {
                    value: Value::String(" trimmed ".into()),
                },
            ),
            node(
                101,
                Expression::Call {
                    function: ScalarFunction::Trim,
                    args: vec![100],
                },
            ),
            node(
                102,
                Expression::RuntimeValue {
                    value: RuntimeValue::CurrentDateTime,
                },
            ),
            node(
                103,
                Expression::RuntimeParameter {
                    name: "control_number".into(),
                    ty: ScalarType::Int,
                },
            ),
            node(104, Expression::Raise { message: None }),
            node(
                105,
                Expression::RuntimeParameterDefault {
                    name: "control_number".into(),
                    ty: ScalarType::Int,
                    default: 104,
                },
            ),
            node(
                106,
                Expression::Const {
                    value: Value::Int(9),
                },
            ),
            node(
                107,
                Expression::RuntimeParameterDefault {
                    name: "absent".into(),
                    ty: ScalarType::Int,
                    default: 106,
                },
            ),
        ],
        &[
            ("Value", 1, ScalarType::String),
            ("Trim", 101, ScalarType::String),
            ("Datetime", 102, ScalarType::String),
            ("Parameter", 103, ScalarType::Int),
            ("PresentDefault", 105, ScalarType::Int),
            ("Fallback", 107, ScalarType::Int),
        ],
    );
    cases.push(("ordinary_host", host, CHECK_ORDINARY_HOST));
    cases
}

#[test]
#[ignore = "requires separately admitted runtime source, retained evidence root and exclusive generated host target"]
fn private_basic_generated_typed_corpus_retains_raw_before_assertions() {
    let root = PathBuf::from(
        std::env::var_os("FERRULE_PRIVATE_RUST_BASIC_EVIDENCE_ROOT")
            .expect("explicit retained evidence root"),
    );
    let runtime = PathBuf::from(
        std::env::var_os("FERRULE_PRIVATE_RUST_BASIC_RUNTIME")
            .expect("explicit qualified runtime source"),
    );
    assert!(root.is_absolute() && runtime.is_absolute());
    assert!(
        std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR").is_some(),
        "explicit admitted host target; no duplicate cache"
    );
    fs::create_dir(&root).expect("fresh evidence root; preserve existing evidence");
    fs::create_dir(root.join("src")).unwrap();
    let mut generated = String::new();
    for (name, program, check) in generated_programs() {
        eprintln!("INPUT generated case={name} program={program:?}");
        let source = private_source(name, &program);
        retained_file(
            &root.join(format!("{name}.generated.rs")),
            source.as_bytes(),
        );
        generated.push_str(&format!(
            "mod {name} {{\n{source}\n{RUNNER_COMMON}\n{check}\n}}\n"
        ));
    }
    retained_file(&root.join("src/lib.rs"), generated.as_bytes());
    let manifest = format!(
        "[package]\nname = \"ferrule_private_basic\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n[dependencies]\ncodegen-runtime = {{ path = {} }}\n[workspace]\n",
        toml_string(runtime.to_str().expect("runtime path Unicode"))
    );
    retained_file(&root.join("Cargo.toml"), manifest.as_bytes());
    retained_file(
        &root.join("COMMAND.txt"),
        b"cargo test --offline -- --nocapture --test-threads=1\n",
    );
    let raw = Command::new("cargo")
        .args(["test", "--offline", "--", "--nocapture", "--test-threads=1"])
        .current_dir(&root)
        .generated_host_output(&root);
    match &raw {
        Ok(output) => {
            retained_file(&root.join("stdout.complete.original"), &output.stdout);
            retained_file(&root.join("stderr.complete.original"), &output.stderr);
            retained_file(
                &root.join("status.complete.original"),
                format!("{:?}\n", output.status).as_bytes(),
            );
        }
        Err(error) => retained_file(
            &root.join("call-error.complete.original"),
            format!("{error:?}\n").as_bytes(),
        ),
    }
    eprintln!("RAW generated full_call={raw:?}");
    let output = raw.expect("complete call refusal retained");
    assert!(
        output.status.success(),
        "complete compiler/run originals retained at {root:?}"
    );
    let trace = String::from_utf8_lossy(&output.stderr);
    let count = |module: &str, node: u32| {
        trace
            .lines()
            .filter(|line| {
                line.starts_with("RAW PRIVATE_BASIC_EXPRESSION ")
                    && line.contains(module)
                    && line.ends_with(&format!("node={node}"))
            })
            .count()
    };
    let true_else = count("p09_true", 3);
    let false_else = count("p09_false", 3);
    eprintln!("RAW generated lazy true_else={true_else} false_else={false_else}");
    assert_eq!(true_else, 0);
    assert_eq!(false_else, 1);
    let named_raise = count("p19", 71);
    let trim_arg = count("ordinary_host", 100);
    let present_default = count("ordinary_host", 104);
    let fallback = count("ordinary_host", 106);
    eprintln!(
        "RAW generated named_raise={named_raise} trim_arg={trim_arg} present_default={present_default} fallback={fallback}"
    );
    assert_eq!(named_raise, 1);
    assert_eq!(trim_arg, 1);
    assert_eq!(present_default, 0);
    assert_eq!(fallback, 1);
}

const RUNNER_COMMON: &str = r#"
#[cfg(test)]
mod checks {
    use super::*;
    use codegen_runtime::{OwnedSourceError, OwnedSourceIdentity, OwnedSourceLimitKind, SourcePathError, InstanceKind};
    use std::error::Error;
    fn s(text: &str) -> Instance { scalar(Value::String(text.into())) }
    fn i(value: i64) -> Instance { scalar(Value::Int(value)) }
    fn expected(fields: impl IntoIterator<Item=(&'static str, Instance)>) -> Instance {
        group(fields.into_iter().map(|(name, value)| field(name, value)))
    }
    fn primary() -> Instance {
        expected([
            ("Value", s("P.root")), ("Ref", expected([("Value", s("P.collision"))])),
            ("Details", expected([("Counter", i(99))])),
            ("Rows", repeated([
                expected([("Value", s("P1")), ("Score", i(30)), ("Rows", repeated([
                    expected([("Value", s("P1.1"))]), expected([("Value", s("P1.2"))]),
                ]))]),
                expected([("Value", s("P2")), ("Score", i(10))]),
                expected([("Value", s("P3")), ("Score", i(20)), ("Details", group(Vec::new()))]),
            ])),
        ])
    }
    fn reference() -> Instance {
        expected([("Value", s("N.root")), ("Rows", repeated([
            expected([("Value", s("N1")), ("Score", i(30)), ("Details", scalar(Value::Null))]),
            expected([("Value", s("N2")), ("Score", i(10)), ("Details", expected([("Counter", i(7))]))]),
        ]))])
    }
    fn declarations<'a>(reference: &'a Instance, exact: &'a Instance, lower: &'a Instance, named_primary: &'a Instance) -> [DeclaredNamedSourceRoot<'a>;5] {
        [DeclaredNamedSourceRoot { name:"Unused", instance:None },
            DeclaredNamedSourceRoot { name:"Ref", instance:Some(reference) },
            DeclaredNamedSourceRoot { name:" Ref 雪 ", instance:Some(exact) },
            DeclaredNamedSourceRoot { name:"ref", instance:Some(lower) },
            DeclaredNamedSourceRoot { name:"primary", instance:Some(named_primary) }]
    }
    fn policy() -> OwnedSourceLimits {
        OwnedSourceLimits { max_roots:16, max_active_frames:8, max_path_segments:16,
            max_metadata_bytes:1048576, max_walk_contexts:64 }
    }
    fn raw<T: std::fmt::Debug>(label:&str, actual:Result<T,RuntimeError>) -> Result<T,RuntimeError> {
        eprintln!("RAW {label} RETURN={actual:?}");
        if let Err(error)=&actual { eprintln!("RAW {label} CAUSE={:?}",error.source().map(ToString::to_string)); }
        actual
    }
    fn capture_input(label:&str, source:&Instance, declared:&[DeclaredNamedSourceRoot<'_>], limits:OwnedSourceLimits) {
        eprintln!("INPUT {label} primary={source:?} declared={declared:?} limits={limits:?}");
    }
    // These runtime containers deliberately expose no Debug/internal frame
    // snapshot. Capture the complete public return classification and every
    // public candidate identity; scalar results/errors are captured separately.
    fn captured_roots<'src>(label:&str,source:&'src Instance,declared:&'src [DeclaredNamedSourceRoot<'src>],limits:OwnedSourceLimits)->BasicSourceRoots<'src> {
        capture_input(label,source,declared,limits);
        let actual=root_return(label,source,declared,limits);
        actual.expect("constructor result captured before propagation")
    }
    fn root_return<'src>(label:&str,source:&'src Instance,declared:&'src [DeclaredNamedSourceRoot<'src>],limits:OwnedSourceLimits)->Result<BasicSourceRoots<'src>,RuntimeError> {
        let actual=BasicSourceRoots::try_new(source,declared,limits);
        match &actual {Ok(_)=>eprintln!("RAW {label} ROOTS=Ok(opaque registry)"),Err(error)=>eprintln!("RAW {label} ROOTS=Err({error:?}) CAUSE={:?}",error.source().map(ToString::to_string))}
        actual
    }
    fn captured_root<'ctx,'src:'ctx>(label:&str,roots:&'ctx BasicSourceRoots<'src>)->BasicScopeContext<'ctx> {
        let actual=roots.root_context();
        match &actual {Ok(_)=>eprintln!("RAW {label} CONTEXT=Ok(opaque paired root)"),Err(error)=>eprintln!("RAW {label} CONTEXT=Err({error:?}) CAUSE={:?}",error.source().map(ToString::to_string))}
        actual.expect("root result captured before propagation")
    }
    fn captured_cohort<'ctx>(label:&str,actual:Result<codegen_runtime::RetainedSourceCohort<'ctx>,RuntimeError>)->codegen_runtime::RetainedSourceCohort<'ctx> {
        cohort_return(label,actual).expect("complete public cohort return captured before propagation")
    }
    fn cohort_return<'ctx>(label:&str,actual:Result<codegen_runtime::RetainedSourceCohort<'ctx>,RuntimeError>)->Result<codegen_runtime::RetainedSourceCohort<'ctx>,RuntimeError> {
        match &actual {
            Ok(cohort)=>eprintln!("RAW {label} COHORT=Ok count={} complete_public_candidates={:?}",cohort.len(),cohort.iter().map(|c|(c.source_candidate_id(),c.raw_source_ordinal())).collect::<Vec<_>>()),
            Err(error)=>eprintln!("RAW {label} COHORT=Err({error:?}) CAUSE={:?}",error.source().map(ToString::to_string)),
        }
        actual
    }
    fn checked_cohort_error(label:&str,actual:Result<codegen_runtime::RetainedSourceCohort<'_>,RuntimeError>,expected:RuntimeError) {
        let actual=cohort_return(label,actual);
        let error=match actual {Err(error)=>error,Ok(_)=>panic!("complete success captured; expected refusal")};
        if let RuntimeError::OwnedSource(expected_cause)=&expected {let cause=error.source().and_then(|value|value.downcast_ref::<OwnedSourceError>());eprintln!("RAW {label} BARE_DOWNCAST={cause:?}");assert_eq!(cause,Some(expected_cause));}
        assert_eq!(error,expected);
    }
    fn checked_error<T:std::fmt::Debug>(label:&str, actual:Result<T,RuntimeError>, expected:RuntimeError) {
        let actual=raw(label,actual);
        let error=actual.expect_err("complete error captured before comparison");
        if let RuntimeError::OwnedSource(expected_cause)=&expected {
            let cause=error.source().and_then(|value|value.downcast_ref::<OwnedSourceError>());
            eprintln!("RAW {label} BARE_DOWNCAST={cause:?}");
            assert_eq!(cause,Some(expected_cause));
        }
        assert_eq!(error,expected);
    }
    fn check_primary(label:&str, source:&Instance, declared:&[DeclaredNamedSourceRoot<'_>], expected_tree:Instance) {
        capture_input(label,source,declared,policy());
        let actual=raw(label,execute_private_owned_basic(source,declared,None,policy()));
        let actual=actual.expect("complete outputs captured");
        assert_eq!(actual.primary,expected_tree); assert!(actual.extras.is_empty());
    }
"#;

const CHECK_P01: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        check_primary("G439-P01",&source,&declared,expected([("Value",s("P.root"))]));
    }
}
"#;
const CHECK_P02: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        check_primary("G439-P02",&source,&declared,expected([("Named",s("N.root")),("Legacy",s("P.collision"))]));
    }
}
"#;
const CHECK_P03: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        capture_input("G439-P03",&source,&declared,policy());
        let names=declared.iter().map(|d|d.name).collect::<Vec<_>>();eprintln!("RAW G439-P03 names={names:?}");
        assert_eq!(names,["Unused","Ref"," Ref 雪 ","ref","primary"]);
        check_primary("G439-P03",&source,&declared,expected([("Exact",s("N.exact")),("Lower",s("N.lower")),("LiteralPrimary",s("N.literal.primary"))]));
    }
}
"#;
const CHECK_P04: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        check_primary("G439-P04",&source,&declared,repeated([
            expected([("Value",s("N1")),("OwnedPosition",i(1)),("LegacyPosition",i(1))]),
            expected([("Value",s("N2")),("OwnedPosition",i(2)),("LegacyPosition",i(2))]),
        ]));
    }
}
"#;
const CHECK_P05: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        check_primary("G439-P05",&source,&declared,repeated([
            expected([("Outer",s("P1")),("Inner",s("P1.1")),("ParentPosition",i(1)),("Position",i(1))]),
            expected([("Outer",s("P1")),("Inner",s("P1.2")),("ParentPosition",i(1)),("Position",i(2))]),
        ]));
    }
}
"#;
const CHECK_P06: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        capture_input("G439-P06",&source,&declared,policy());
        let roots=captured_roots("G439-P06 roots",&source,&declared,policy());
        let root=captured_root("public root",&roots);let outer=captured_cohort("P06 outer",root.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Rows"]));
        let first=outer.get(0).unwrap().context();let inner=captured_cohort("P06 inner",first.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Rows","Rows"]));
        let selected=inner.get(1).unwrap().context();let identity=captured_cohort("P06 identity",selected.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Rows"]));
        eprintln!("RAW G439-P06 outer={} inner={} identity={} source_ids={:?} raw_ordinals={:?}",outer.len(),inner.len(),identity.len(),identity.iter().map(|c|c.source_candidate_id()).collect::<Vec<_>>(),identity.iter().map(|c|c.raw_source_ordinal()).collect::<Vec<_>>());
        let context=identity.get(0).unwrap().context();
        let values=[raw("G439-P06 outer-value",expression_1(context)),raw("G439-P06 inner-value",expression_2(context)),raw("G439-P06 parent-position",expression_3(context)),raw("G439-P06 inner-position",expression_4(context))];
        eprintln!("RAW G439-P06 complete={values:?}");
        assert_eq!(values,[Ok(Value::String("P1".into())),Ok(Value::String("P1.2".into())),Ok(Value::Int(1)),Ok(Value::Int(2))]);
        assert_eq!(identity.len(),1);assert_eq!(identity.get(0).unwrap().raw_source_ordinal(),None);
    }
}
"#;
const CHECK_P07: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        capture_input("G439-P07",&source,&declared,policy());
        let roots=captured_roots("public roots",&source,&declared,policy());let root=captured_root("public root",&roots);let cohort=captured_cohort("P07 named rows",root.walk_owned_source_cohort(SourceOwnerRef::Named("Ref"),&["Rows"]));
        eprintln!("RAW G439-P07 cohort_count={} raw_ordinals={:?}",cohort.len(),cohort.iter().map(|c|c.raw_source_ordinal()).collect::<Vec<_>>());
        let context=cohort.get(0).unwrap().context();let values=[raw("G439-P07 unframed",expression_1(context)),raw("G439-P07 pinned",expression_2(context)),raw("G439-P07 legacy",expression_3(context))];
        eprintln!("RAW G439-P07 complete={values:?}");assert_eq!(values,[Ok(Value::Null),Ok(Value::Null),Ok(Value::Int(99))]);
    }
}
"#;
const CHECK_P08: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        capture_input("G439-P08",&source,&declared,policy());
        let actual=raw("G439-P08",execute_selected_private_owned_basic(&source,&declared,None,policy(),TargetSelection::Primary));
        assert_eq!(actual,Ok(SelectedTargetOutput::Primary(repeated([expected([("Counter",i(99))])]))));
        let roots=captured_roots("public roots",&source,&declared,policy());let root=captured_root("public root",&roots);let plain=captured_cohort("P08 plain",root.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Details"]));let context=plain.get(0).unwrap().context();
        eprintln!("RAW G439-P08 plain_count={} raw={:?}",plain.len(),plain.get(0).unwrap().raw_source_ordinal());
        let legacy=raw("G439-P08 legacy",expression_3(context));assert_eq!(legacy,Ok(Value::Int(1)));
        checked_error("G439-P08 owned",expression_2(context),RuntimeError::OwnedSource(OwnedSourceError::InactiveSourceFrame { owner:OwnedSourceIdentity::Primary,collection:None }));
    }
}
"#;
const CHECK_P09_TRUE: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        check_primary("G439-P09 true",&source,&declared,expected([("Value",s("chosen"))]));
    }
}
"#;
const CHECK_P09_FALSE: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        capture_input("G439-P09 false",&source,&declared,policy());
        checked_error("G439-P09 false",execute_private_owned_basic(&source,&declared,None,policy()),RuntimeError::OwnedSource(OwnedSourceError::InactiveSourceFrame {owner:OwnedSourceIdentity::Primary,collection:Some(vec!["Rows".into()])}));
    }
}
"#;
const CHECK_P10_UNUSED: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        check_primary("G439-P10 unused absent declaration",&source,&declared,expected([("Value",s("P.root"))]));
    }
}
"#;
const CHECK_P10_DEMAND: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        capture_input("G439-P10 demanded",&source,&declared,policy());
        checked_error("G439-P10 demanded",execute_private_owned_basic(&source,&declared,None,policy()),RuntimeError::OwnedSource(OwnedSourceError::SourceUnavailable {name:"Unused".into()}));
    }
}
"#;
const CHECK_P11: &str = r#"
    #[test] fn run() {
        let source=expected([("Details",i(7))]);let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        capture_input("G439-P11",&source,&declared,policy());
        checked_error("G439-P11",execute_private_owned_basic(&source,&declared,None,policy()),RuntimeError::OwnedSource(OwnedSourceError::Traversal {owner:OwnedSourceIdentity::Primary,source:SourcePathError::CannotTraverse {path:vec!["Details".into(),"Counter".into()],segment:1,found:InstanceKind::Scalar}}));
    }
}
"#;
const CHECK_P12: &str = r#"
    #[test] fn run() {
        let source=repeated([expected([("Value",s("flat1"))]),expected([("Value",s("flat2"))])]);let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        check_primary("G439-P12",&source,&declared,repeated([expected([("Value",s("flat1")),("Position",i(1))]),expected([("Value",s("flat2")),("Position",i(2))])]));
    }
}
"#;
const CHECK_P13: &str = r#"
    #[test] fn run() {
        let source=primary();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&source,&exact,&lower,&literal);
        capture_input("G439-P13 shared physical source",&source,&declared,policy());
        let roots=captured_roots("public roots",&source,&declared,policy());let root=captured_root("public root",&roots);let primary=captured_cohort("P13 primary",root.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Rows"]));let p=primary.get(0).unwrap().context();let named=captured_cohort("P13 named",p.walk_owned_source_cohort(SourceOwnerRef::Named("Ref"),&["Rows"]));let context=named.get(1).unwrap().context();
        eprintln!("RAW G439-P13 declared_index=1 primary_count={} named_count={} primary_raw={:?} named_raw={:?}",primary.len(),named.len(),primary.get(0).unwrap().raw_source_ordinal(),named.get(1).unwrap().raw_source_ordinal());
        let values=[raw("G439-P13 primary",expression_1(context)),raw("G439-P13 named",expression_2(context))];eprintln!("RAW G439-P13 complete={values:?}");assert_eq!(values,[Ok(Value::Int(1)),Ok(Value::Int(2))]);
    }
}
"#;
const CHECK_P17: &str = r#"
    #[test] fn run() {
        let source=expected([
            ("Rows",repeated([
                expected([("Value",s("r1")),("Details",expected([("Counter",i(1))]))]),
                expected([("Value",s("r2")),("Details",expected([("Counter",i(2))]))]),
            ])),
        ]);let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        capture_input("G439-P17",&source,&declared,policy());let roots=captured_roots("public roots",&source,&declared,policy());let root=captured_root("public root",&roots);let parent=captured_cohort("P17 parent",root.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Rows"]));let p=parent.get(1).unwrap().context();let child=captured_cohort("P17 child",p.walk_owned_source_cohort(SourceOwnerRef::Primary,&["Rows","Details"]));let context=child.get(0).unwrap().context();
        eprintln!("RAW G439-P17 parent_raw={:?} child_count={} child_raw={:?}",parent.get(1).unwrap().raw_source_ordinal(),child.len(),child.get(0).unwrap().raw_source_ordinal());let values=[raw("G439-P17 counter",expression_1(context)),raw("G439-P17 position",expression_2(context))];eprintln!("RAW G439-P17 complete={values:?}");assert_eq!(values,[Ok(Value::Int(2)),Ok(Value::Int(2))]);assert_eq!(child.len(),1);assert_eq!(child.get(0).unwrap().raw_source_ordinal(),None);
    }
}
"#;
const CHECK_P18: &str = r#"
    #[test] fn run() {
        let source=repeated([scalar(Value::Null),expected([("Value",s("kept"))]),scalar(Value::json_null())]);let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        check_primary("G439-P18",&source,&declared,repeated([expected([("Value",s("kept"))])]));
        let roots=captured_roots("public roots",&source,&declared,policy());let root=captured_root("public root",&roots);let owned=captured_cohort("P18 owned",root.walk_owned_source_cohort(SourceOwnerRef::Primary,&[]));let legacy=captured_cohort("P18 legacy",root.walk_source_cohort(&[]));let ordinals=owned.iter().map(|c|c.raw_source_ordinal()).collect::<Vec<_>>();let legacy_ordinals=legacy.iter().map(|c|c.raw_source_ordinal()).collect::<Vec<_>>();eprintln!("RAW G439-P18 owned_count={} legacy_count={} owned_raw={ordinals:?} legacy_raw={legacy_ordinals:?}",owned.len(),legacy.len());assert_eq!(ordinals,[Some(2)]);assert_eq!(legacy_ordinals,[Some(1),Some(2),Some(3)]);
    }
}
"#;
const CHECK_P19: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        capture_input("G439-P19 selected",&source,&declared,policy());let actual=raw("G439-P19 selected",execute_selected_private_owned_basic(&source,&declared,None,policy(),TargetSelection::Primary));assert_eq!(actual,Ok(SelectedTargetOutput::Primary(expected([("Value",s("P.root"))]))));
        capture_input("G439-P19 all",&source,&declared,policy());checked_error("G439-P19 all",execute_private_owned_basic(&source,&declared,None,policy()),RuntimeError::MappingException {node:71,message:None});
    }
}
"#;
const CHECK_P20: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        capture_input("G439-P20 all",&source,&declared,policy());let actual=raw("G439-P20 all",execute_private_owned_basic(&source,&declared,None,policy()));assert_eq!(actual,Ok(ExecutionOutputs {primary:expected([("Value",s("P.root"))]),extras:vec![NamedOutput {name:"named".into(),instance:expected([("Value",s("N.root"))])}]}));
        let zero=OwnedSourceLimits {max_roots:0,max_active_frames:0,max_path_segments:0,max_metadata_bytes:0,max_walk_contexts:0};capture_input("G439-P20 unknown",&source,&declared,zero);checked_error("G439-P20 unknown",execute_selected_private_owned_basic(&source,&declared,None,zero,TargetSelection::Named("missing")),RuntimeError::UnknownTarget {name:"missing".into()});
    }
}
"#;
const CHECK_R01: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);let mut limits=policy();limits.max_metadata_bytes=0;capture_input("G439-R01",&source,&declared,limits);checked_error("G439-R01",execute_private_owned_basic(&source,&declared,None,limits),RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {kind:OwnedSourceLimitKind::MetadataBytes,used:0,requested:6,max:0}));
    }
}
"#;
const CHECK_R02: &str = r#"
    #[test] fn run() {
        let source=s("P.root");let limits=OwnedSourceLimits {max_roots:1,max_active_frames:0,max_path_segments:0,max_metadata_bytes:0,max_walk_contexts:0};capture_input("G439-R02",&source,&[],limits);let actual=raw("G439-R02",execute_private_owned_basic(&source,&[],None,limits));assert_eq!(actual,Ok(ExecutionOutputs {primary:expected([("Value",s("P.root"))]),extras:Vec::new()}));
    }
}
"#;
const CHECK_R03: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);let mut limits=policy();limits.max_walk_contexts=2;capture_input("G439-R03",&source,&declared,limits);checked_error("G439-R03",execute_private_owned_basic(&source,&declared,None,limits),RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {kind:OwnedSourceLimitKind::WalkContexts,used:2,requested:1,max:2}));
    }
}
"#;
const CHECK_R05: &str = r#"
    #[test] fn run() {
        let source=primary();let limits=OwnedSourceLimits {max_roots:usize::MAX,max_active_frames:0,max_path_segments:0,max_metadata_bytes:0,max_walk_contexts:0};capture_input("G439-R05",&source,&[],limits);checked_error("G439-R05",execute_private_owned_basic(&source,&[],None,limits),RuntimeError::OwnedSource(OwnedSourceError::MetadataArithmeticOverflow {kind:OwnedSourceLimitKind::MetadataSlots}));
    }
}
"#;

const CHECK_R04: &str = r#"
    fn limit(kind:OwnedSourceLimitKind,used:usize,requested:usize,max:usize)->RuntimeError {
        RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit {kind,used,requested,max})
    }
    #[test] fn run() {
        // Exact small boundary recipes exercise the real registry and paired
        // walk API used by generated entries; no mirrored resource calculator.
        let scalar_root=s("P.root");let absent=[DeclaredNamedSourceRoot {name:"X",instance:None}];let mut limits=policy();limits.max_roots=2;
        let roots=captured_roots("R04 Roots inclusive",&scalar_root,&absent,limits);let root=captured_root("R04 Roots root",&roots);let value=raw("R04 Roots value",root.owned_field(SourceOwnerRef::Primary,&[],None));assert_eq!(value,Ok(Value::String("P.root".into())));
        let excess=[DeclaredNamedSourceRoot {name:"X",instance:None},DeclaredNamedSourceRoot {name:"Y",instance:None}];capture_input("R04 Roots one above",&scalar_root,&excess,limits);let actual=root_return("R04 Roots one above",&scalar_root,&excess,limits);let error=match actual {Err(error)=>error,Ok(_)=>panic!("captured unexpected root success")};assert_eq!(error,limit(OwnedSourceLimitKind::Roots,1,2,2));
        let one=repeated([s("one")]);limits=policy();limits.max_active_frames=1;let roots=captured_roots("R04 Frames inclusive",&one,&[],limits);let root=captured_root("R04 Frames root",&roots);let rows=captured_cohort("R04 Frames inclusive",root.walk_owned_source_cohort(SourceOwnerRef::Primary,&[]));assert_eq!(rows.len(),1);
        let nested=repeated([repeated([s("nested")])]);let roots=captured_roots("R04 Frames one above",&nested,&[],limits);let root=captured_root("R04 Frames nested root",&roots);checked_cohort_error("R04 Frames one above",root.walk_owned_source_cohort(SourceOwnerRef::Primary,&[]),limit(OwnedSourceLimitKind::Frames,1,1,1));
        let single=expected([("x",s("exact"))]);limits=policy();limits.max_path_segments=1;let roots=captured_roots("R04 Segments inclusive",&single,&[],limits);let root=captured_root("R04 Segments root",&roots);let value=raw("R04 Segments inclusive",root.owned_field(SourceOwnerRef::Primary,&["x"],None));assert_eq!(value,Ok(Value::String("exact".into())));
        limits.max_path_segments=0;let roots=captured_roots("R04 Segments one above",&single,&[],limits);let root=captured_root("R04 Segments zero root",&roots);checked_error("R04 Segments one above",root.owned_field(SourceOwnerRef::Primary,&["x"],None),limit(OwnedSourceLimitKind::PathSegments,0,1,0));
        // The one-byte query is reserved twice, plus its one-byte maximum
        // diagnostic segment; registry/current-frame paths carry no text.
        limits=policy();limits.max_metadata_bytes=3;let roots=captured_roots("R04 Bytes inclusive",&single,&[],limits);let root=captured_root("R04 Bytes root",&roots);let value=raw("R04 Bytes inclusive",root.owned_field(SourceOwnerRef::Primary,&["x"],None));assert_eq!(value,Ok(Value::String("exact".into())));
        limits.max_metadata_bytes=2;let roots=captured_roots("R04 Bytes one above",&single,&[],limits);let root=captured_root("R04 Bytes low root",&roots);checked_error("R04 Bytes one above",root.owned_field(SourceOwnerRef::Primary,&["x"],None),limit(OwnedSourceLimitKind::MetadataBytes,2,1,2));
        let two=repeated([s("one"),s("two")]);limits=policy();limits.max_walk_contexts=2;let roots=captured_roots("R04 Walk inclusive",&two,&[],limits);let root=captured_root("R04 Walk root",&roots);let rows=captured_cohort("R04 Walk inclusive",root.walk_owned_source_cohort(SourceOwnerRef::Primary,&[]));let values=rows.iter().map(|candidate|raw("R04 Walk each result",candidate.context().owned_field(SourceOwnerRef::Primary,&[],None))).collect::<Vec<_>>();eprintln!("RAW R04 Walk all results={values:?}");assert_eq!(values,[Ok(Value::String("one".into())),Ok(Value::String("two".into()))]);
        let three=repeated([s("one"),s("two"),s("three")]);let roots=captured_roots("R04 Walk cumulative one above",&three,&[],limits);let root=captured_root("R04 Walk third root",&roots);checked_cohort_error("R04 Walk cumulative one above",root.walk_owned_source_cohort(SourceOwnerRef::Primary,&[]),limit(OwnedSourceLimitKind::WalkContexts,2,1,2));
    }
}
"#;

const CHECK_ORDINARY_HOST: &str = r#"
    #[test] fn run() {
        let source=primary();let reference=reference();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let literal=expected([("Value",s("N.literal.primary"))]);let declared=declarations(&reference,&exact,&lower,&literal);
        let mut parameters=codegen_runtime::RuntimeParameters::new();let inserted=parameters.insert("control_number",Value::String(" 42 ".into()));eprintln!("RAW ordinary_host parameter_insert={inserted:?}");inserted.expect("complete parameter insert captured");
        let execution=ExecutionContext::new(std::path::Path::new("relative/map.ferrule")).with_current_datetime("2026-07-19T11:22:33.45-07:00").with_parameters(&parameters);
        capture_input("ordinary_host",&source,&declared,policy());eprintln!("INPUT ordinary_host execution mapping=relative/map.ferrule datetime=2026-07-19T11:22:33.45-07:00 parameters={parameters:?}");
        let actual=raw("ordinary_host",execute_private_owned_basic(&source,&declared,Some(&execution),policy()));
        assert_eq!(actual,Ok(ExecutionOutputs {primary:expected([("Value",s("P.root")),("Trim",s("trimmed")),("Datetime",s("2026-07-19T11:22:33.45-07:00")),("Parameter",i(42)),("PresentDefault",i(42)),("Fallback",i(9))]),extras:Vec::new()}));
        let roots=captured_roots("ordinary_host no execution",&source,&declared,policy());let root=captured_root("ordinary_host no execution root",&roots);
        checked_error("ordinary_host missing clock",expression_102(&root),RuntimeError::MissingRuntimeValue {value:RuntimeValue::CurrentDateTime});
        checked_error("ordinary_host missing parameter",expression_103(&root),RuntimeError::MissingRuntimeParameter {node:103,name:"control_number".into()});
    }
}
"#;
