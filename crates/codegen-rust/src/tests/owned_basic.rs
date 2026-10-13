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
            owned_leaf_nodes: false,
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

// Source-only #541 prospective controls. The existing five tests and twenty-five
// generated recipes above remain exact; this bridge has a separate frozen corpus.
fn failure_rule(
    parts: &[&str],
    selection: codegen::FailureSelection,
    message: Option<u32>,
) -> codegen::FailureRule {
    codegen::FailureRule {
        iteration: codegen::FailureIteration::Source(codegen::SourceIteration::new(path(parts))),
        selection,
        message,
    }
}
fn failure_program(nodes: Vec<ExpressionNode>, rules: Vec<codegen::FailureRule>) -> Program {
    let mut nodes = nodes;
    nodes.push(node(100, owned(SourceOwner::Primary, &["Value"], None)));
    let mut program = basic(nodes, &[("Value", 100, ScalarType::String)]);
    program.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group("Ref", vec![SchemaNode::scalar("Value", ScalarType::String)]),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ScalarType::String),
                    SchemaNode::scalar("Flag", ScalarType::Bool),
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
            .repeating(),
        ],
    );
    program.failure_rules = rules;
    program
}
fn failure_source_result(label: &str, program: &Program) -> Result<String, EmitError> {
    eprintln!("INPUT {label} complete Program={program:#?}");
    let actual = super::super::owned_basic::render_basic_owned_source(program);
    eprintln!("RAW {label} complete renderer Result={actual:#?}");
    if let Err(error) = &actual {
        eprintln!("RAW {label} complete cause={:?}", error.source());
    }
    actual
}
fn expected_failure_capability(
    index: usize,
    reason: codegen::OwnedSourceCapabilityReason,
) -> ProgramValidationError {
    let scope = mapping::SourceScopeLocation::root(mapping::SourceScopeTarget::Primary);
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Iteration { scope },
            request: None,
            reason,
            origin: codegen::OwnedSourceCapabilityOrigin::Failure { index },
        }),
    }
}
fn check_failure_source_error(label: &str, program: &Program, expected: ProgramValidationError) {
    eprintln!("EXPECTED {label} independent full diagnostic={expected:#?}");
    let actual = failure_source_result(label, program);
    match actual {
        Err(EmitError::InvalidProgram(actual)) => assert_eq!(actual, expected, "{label}"),
        other => panic!("{label}: complete captured source refusal differs: {other:#?}"),
    }
}

#[test]
fn private_source_failure_bridge_preserves_ordinary_bytes_and_pre_target_order() {
    let empty = basic(
        vec![node(
            1,
            Expression::Const {
                value: Value::Int(7),
            },
        )],
        &[("Value", 1, ScalarType::Int)],
    );
    eprintln!("INPUT S541-01 complete Program={empty:#?}");
    let old = super::super::failure::render(&empty);
    eprintln!("RAW S541-01 complete ordinary renderer={old:#?}");
    assert_eq!(
        old.unwrap(),
        "fn evaluate_failure_rules(context: &ScopeContext<'_>) -> Result<(), RuntimeError> {\n    let _ = context;\n    Ok(())\n}\n\n"
    );
    let rules = vec![failure_rule(
        &["Rows"],
        codegen::FailureSelection::WhenTrue(10),
        Some(11),
    )];
    let p = failure_program(
        vec![
            node(
                10,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ),
            node(11, Expression::Raise { message: None }),
        ],
        rules,
    );
    let source = failure_source_result("S541-01 Basic order", &p).unwrap();
    let all = source
        .find("pub(crate) fn execute_private_owned_basic")
        .unwrap();
    let selected = source
        .find("pub(crate) fn execute_selected_private_owned_basic")
        .unwrap();
    let all_body = &source[all..selected];
    let selected_body = &source[selected..];
    assert!(
        all_body
            .find("evaluate_basic_failure_rules(&context)?;")
            .unwrap()
            < all_body.find("scope_root(&context)?").unwrap()
    );
    assert!(
        selected_body
            .find("evaluate_basic_failure_rules(&context)?;")
            .unwrap()
            < selected_body.find("scope_root(&context)?").unwrap()
    );
    assert!(
        selected_body.find("RuntimeError::UnknownTarget").unwrap()
            < selected_body.find("BasicSourceRoots::try_new").unwrap()
    );
    assert!(source.contains("let reached = context.walk_source_cohort(&[\"Rows\"]);"));
    assert!(source.contains("let cohort = reached?;"));
    assert!(source.contains(
        "for candidate in cohort.iter() {\n        let item_context = candidate.context();"
    ));
    assert!(source.contains("let reached_message = expression_11(item_context);"));
    assert!(source.find("if !selected").unwrap() < source.find("let reached_message").unwrap());
    assert!(source.contains("limits: OwnedSourceLimits"));
    assert!(!source.contains("OwnedSourceLimits {"));
}

#[test]
fn private_source_failure_bridge_keeps_generated_owned_and_dynamic_routes_closed() {
    let mut p = failure_program(
        vec![
            node(
                30,
                Expression::Const {
                    value: Value::Int(2),
                },
            ),
            node(
                31,
                Expression::SourceField {
                    path: Vec::new(),
                    frame: None,
                },
            ),
        ],
        vec![],
    );
    p.failure_rules = vec![codegen::FailureRule {
        iteration: codegen::FailureIteration::Generated(codegen::GeneratedSequence::Range {
            from: None,
            to: 30,
            item: 31,
        }),
        selection: codegen::FailureSelection::All,
        message: None,
    }];
    check_failure_source_error(
        "S541-02 generated",
        &p,
        expected_failure_capability(0, codegen::OwnedSourceCapabilityReason::FailureRule),
    );
    let p = failure_program(
        vec![node(12, owned(SourceOwner::Primary, &["Value"], None))],
        vec![failure_rule(
            &["Rows"],
            codegen::FailureSelection::All,
            Some(12),
        )],
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
    check_failure_source_error("S541-03 owned message", &p, expected);
    let mut p = failure_program(
        vec![node(
            30,
            Expression::Const {
                value: Value::String("d.json".into()),
            },
        )],
        vec![failure_rule(
            &["D", "Rows"],
            codegen::FailureSelection::All,
            None,
        )],
    );
    p.extra_sources.push(NamedSourceProgram {
        name: "D".into(),
        source: source_schema(),
        dynamic: Some(codegen::DynamicSourceProgram {
            path: 30,
            driver: codegen::SourceIteration::new(path(&["Rows"])),
        }),
    });
    check_failure_source_error(
        "S541-04 dynamic collection",
        &p,
        expected_failure_capability(0, codegen::OwnedSourceCapabilityReason::DynamicLoader),
    );
}

#[test]
fn private_source_failure_bridge_retains_only_potential_ordinary_dependencies() {
    let mut base = failure_program(
        vec![
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
                30,
                Expression::Const {
                    value: Value::String("d.json".into()),
                },
            ),
        ],
        vec![failure_rule(
            &["Rows"],
            codegen::FailureSelection::All,
            Some(11),
        )],
    );
    base.extra_sources.push(NamedSourceProgram {
        name: "D".into(),
        source: source_schema(),
        dynamic: Some(codegen::DynamicSourceProgram {
            path: 30,
            driver: codegen::SourceIteration::new(path(&["Rows"])),
        }),
    });
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 12,
                scope: None,
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::DynamicLoader,
            origin: codegen::OwnedSourceCapabilityOrigin::Failure { index: 0 },
        }),
    };
    let mut reached = base.clone();
    reached.failure_rules[0].message = Some(12);
    check_failure_source_error(
        "S541-05 retained dynamic message",
        &reached,
        expected.clone(),
    );
    let inert = failure_source_result("S541-06 unused metadata/dead leaf", &base);
    assert!(inert.is_ok());
    let mut empty = reached;
    empty
        .expressions
        .iter_mut()
        .find(|node| node.id == 12)
        .unwrap()
        .expression = Expression::SourceField {
        path: path(&["D", "Value"]),
        frame: Some(Vec::new()),
    };
    let actual = failure_source_result("S541-07 present empty frame", &empty);
    assert!(actual.is_ok());
    let mut lazy = base;
    lazy.expressions.extend([
        node(
            13,
            Expression::Const {
                value: Value::Bool(true),
            },
        ),
        node(
            14,
            Expression::If {
                condition: 13,
                then: 11,
                else_: 12,
            },
        ),
    ]);
    lazy.failure_rules[0].message = Some(14);
    check_failure_source_error(
        "S541-08 unchosen potential dynamic message",
        &lazy,
        expected,
    );
}

fn generated_failure_programs() -> Vec<(&'static str, Program, &'static str)> {
    use codegen::FailureSelection::{All, WhenFalse, WhenTrue};
    let mut out = Vec::new();
    out.push((
        "g541_01",
        failure_program(vec![], vec![failure_rule(&["Rows"], All, None)]),
        FAILURE_01,
    ));
    let flag = || {
        node(
            10,
            Expression::SourceField {
                path: path(&["Flag"]),
                frame: None,
            },
        )
    };
    let message = || {
        node(
            11,
            Expression::SourceField {
                path: path(&["Value"]),
                frame: Some(path(&["Rows"])),
            },
        )
    };
    out.push((
        "g541_02",
        failure_program(
            vec![flag(), message()],
            vec![failure_rule(&["Rows"], WhenTrue(10), Some(11))],
        ),
        FAILURE_02,
    ));
    out.push((
        "g541_03",
        failure_program(
            vec![flag(), message()],
            vec![failure_rule(&["Rows"], WhenFalse(10), Some(11))],
        ),
        FAILURE_03,
    ));
    out.push((
        "g541_04",
        failure_program(
            vec![
                node(
                    20,
                    Expression::Const {
                        value: Value::Bool(false),
                    },
                ),
                node(21, Expression::Raise { message: None }),
                node(
                    22,
                    Expression::Const {
                        value: Value::String("second".into()),
                    },
                ),
                node(23, Expression::Raise { message: None }),
            ],
            vec![
                failure_rule(&["Rows"], WhenTrue(20), Some(21)),
                failure_rule(&["Rows"], All, Some(22)),
                failure_rule(&["Rows"], All, Some(23)),
            ],
        ),
        FAILURE_04,
    ));
    out.push((
        "g541_05",
        failure_program(
            vec![node(11, Expression::Raise { message: None })],
            vec![failure_rule(&["Rows"], All, Some(11))],
        ),
        FAILURE_05,
    ));
    out.push((
        "g541_06",
        failure_program(
            vec![
                flag(),
                node(
                    11,
                    Expression::Position {
                        collection: path(&["Rows", "Rows"]),
                    },
                ),
            ],
            vec![failure_rule(&["Rows", "Rows"], WhenTrue(10), Some(11))],
        ),
        FAILURE_06,
    ));
    out.push((
        "g541_07",
        failure_program(
            vec![node(
                11,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: Some(path(&["ref", "Rows"])),
                },
            )],
            vec![failure_rule(&["ref", "Rows"], All, Some(11))],
        ),
        FAILURE_07,
    ));
    out.push((
        "g541_08",
        failure_program(
            vec![node(
                11,
                Expression::SourceField {
                    path: path(&["Ref", "Value"]),
                    frame: None,
                },
            )],
            vec![failure_rule(&["ref", "Rows"], All, Some(11))],
        ),
        FAILURE_08,
    ));
    let parameter = || {
        node(
            10,
            Expression::RuntimeParameter {
                name: "flag".into(),
                ty: ScalarType::Bool,
            },
        )
    };
    let note = || {
        node(
            11,
            Expression::RuntimeParameter {
                name: "note".into(),
                ty: ScalarType::String,
            },
        )
    };
    out.push((
        "g541_09",
        failure_program(
            vec![parameter(), note()],
            vec![failure_rule(&["Rows"], WhenTrue(10), Some(11))],
        ),
        FAILURE_09,
    ));
    out.push((
        "g541_10",
        failure_program(
            vec![parameter(), note()],
            vec![failure_rule(&["Rows"], WhenTrue(10), Some(11))],
        ),
        FAILURE_10,
    ));
    let clock = || {
        node(
            11,
            Expression::RuntimeValue {
                value: RuntimeValue::CurrentDateTime,
            },
        )
    };
    out.push((
        "g541_11",
        failure_program(vec![clock()], vec![failure_rule(&["Rows"], All, Some(11))]),
        FAILURE_11,
    ));
    out.push((
        "g541_12",
        failure_program(vec![clock()], vec![failure_rule(&["Rows"], All, Some(11))]),
        FAILURE_12,
    ));
    out.push((
        "g541_13",
        failure_program(
            vec![
                node(
                    10,
                    Expression::Const {
                        value: Value::Int(7),
                    },
                ),
                node(11, Expression::Raise { message: None }),
            ],
            vec![failure_rule(&["Rows"], WhenTrue(10), Some(11))],
        ),
        FAILURE_13,
    ));
    out.push((
        "g541_14",
        failure_program(
            vec![node(11, Expression::Raise { message: None })],
            vec![failure_rule(&["Rows"], All, Some(11))],
        ),
        FAILURE_14,
    ));
    out.push((
        "g541_15",
        failure_program(vec![], vec![failure_rule(&["Rows"], All, None)]),
        FAILURE_15,
    ));
    out.push((
        "g541_16",
        failure_program(
            vec![node(11, Expression::Const { value: Value::Null })],
            vec![failure_rule(&["Rows"], All, Some(11))],
        ),
        FAILURE_16,
    ));
    let mut p = failure_program(
        vec![node(71, Expression::Raise { message: None })],
        vec![failure_rule(&["Rows"], All, None)],
    );
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
    out.push(("g541_17", p, FAILURE_17));
    out.push((
        "g541_18",
        failure_program(
            vec![
                node(
                    10,
                    Expression::Const {
                        value: Value::Bool(false),
                    },
                ),
                node(11, Expression::Raise { message: None }),
            ],
            vec![failure_rule(&["Rows"], WhenTrue(10), Some(11))],
        ),
        FAILURE_18,
    ));
    out
}

#[test]
#[ignore = "requires separately admitted runtime source, fresh retained failure evidence root and shared generated host target"]
fn private_source_failure_generated_corpus_retains_complete_raw_before_assertions() {
    let root = PathBuf::from(
        std::env::var_os("FERRULE_PRIVATE_RUST_FAILURE_EVIDENCE_ROOT")
            .expect("fresh retained failure evidence root"),
    );
    let runtime = PathBuf::from(
        std::env::var_os("FERRULE_PRIVATE_RUST_BASIC_RUNTIME")
            .expect("explicit qualified runtime source"),
    );
    assert!(root.is_absolute() && runtime.is_absolute());
    assert!(
        std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR").is_some(),
        "explicit shared target/no duplicate cache"
    );
    fs::create_dir(&root).expect("fresh evidence namespace");
    fs::create_dir(root.join("src")).unwrap();
    let mut generated = String::new();
    for (name, program, check) in generated_failure_programs() {
        retained_file(
            &root.join(format!("{name}.Program.original")),
            format!("{program:#?}\n").as_bytes(),
        );
        let actual = failure_source_result(name, &program);
        retained_file(
            &root.join(format!("{name}.renderer-return.original")),
            format!("{actual:#?}\n").as_bytes(),
        );
        let source = actual.expect("raw complete source retained");
        retained_file(
            &root.join(format!("{name}.generated.rs")),
            source.as_bytes(),
        );
        generated.push_str(&format!(
            "mod {name} {{\n{source}\n{RUNNER_COMMON}\n{FAILURE_COMMON}\n{check}\n}}\n"
        ));
    }
    retained_file(&root.join("src/lib.rs"), generated.as_bytes());
    let manifest = format!(
        "[package]\nname = \"ferrule_private_source_failure\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n[dependencies]\ncodegen-runtime = {{ path = {} }}\n[workspace]\n",
        toml_string(runtime.to_str().expect("runtime Unicode path"))
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
            format!("{error:#?}\n").as_bytes(),
        ),
    }
    eprintln!("RAW G541 whole compiler/runner return={raw:#?}");
    let output = raw.expect("complete call failure retained");
    assert!(
        output.status.success(),
        "all raw originals retained before qualification at {root:?}"
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
    let expected: [(&str, u32, usize); 36] = [
        ("g541_01", 100, 0),
        ("g541_02", 10, 2),
        ("g541_02", 11, 1),
        ("g541_02", 100, 0),
        ("g541_03", 10, 1),
        ("g541_03", 11, 1),
        ("g541_04", 20, 2),
        ("g541_04", 21, 0),
        ("g541_04", 22, 1),
        ("g541_04", 23, 0),
        ("g541_05", 11, 0),
        ("g541_05", 100, 1),
        ("g541_06", 10, 2),
        ("g541_06", 11, 1),
        ("g541_06", 100, 0),
        ("g541_07", 11, 1),
        ("g541_08", 11, 1),
        ("g541_09", 10, 1),
        ("g541_09", 11, 1),
        ("g541_10", 10, 1),
        ("g541_10", 11, 0),
        ("g541_11", 11, 1),
        ("g541_12", 11, 1),
        ("g541_13", 10, 1),
        ("g541_13", 11, 0),
        ("g541_14", 11, 1),
        ("g541_15", 100, 0),
        ("g541_16", 11, 1),
        ("g541_17", 100, 0),
        ("g541_17", 71, 0),
        ("g541_18", 10, 2),
        ("g541_18", 11, 0),
        ("g541_18", 100, 1),
        ("g541_09", 100, 0),
        ("g541_10", 100, 0),
        ("g541_14", 100, 0),
    ];
    let observed = expected
        .iter()
        .map(|(module, node, _)| (*module, *node, count(module, *node)))
        .collect::<Vec<_>>();
    eprintln!("RAW G541 complete entry-count vector={observed:?}");
    for ((module, node, wanted), (_, _, actual)) in expected.iter().zip(&observed) {
        assert_eq!(actual, wanted, "{module} node={node}");
    }
}

const FAILURE_COMMON: &str = r#"
    fn failure_input()->Instance {
        expected([("Value",s("P.root")),("Ref",expected([("Value",s("P.collision"))])),
            ("Rows",repeated([
                expected([("Value",s("r1")),("Flag",scalar(Value::Bool(false))),("Rows",repeated([
                    expected([("Value",s("i1")),("Flag",scalar(Value::Bool(false)))]),
                    expected([("Value",s("i2")),("Flag",scalar(Value::Bool(true)))]),
                ]))]),
                expected([("Value",s("r2")),("Flag",scalar(Value::Bool(true))),("Rows",repeated([
                    expected([("Value",s("i3")),("Flag",scalar(Value::Bool(true)))]),
                ]))]),
            ]))])
    }
    fn failure_declarations<'a>(reference:&'a Instance)->[DeclaredNamedSourceRoot<'a>;5] {
        [DeclaredNamedSourceRoot{name:"Unused",instance:None},DeclaredNamedSourceRoot{name:"Ref",instance:Some(reference)},
            DeclaredNamedSourceRoot{name:" Ref 雪 ",instance:None},DeclaredNamedSourceRoot{name:"ref",instance:None},DeclaredNamedSourceRoot{name:"primary",instance:None}]
    }
    fn failure_observation<T:std::fmt::Debug>(label:&str,actual:Result<T,RuntimeError>)->Result<T,RuntimeError> {
        eprintln!("RAW {label} FULL_RETURN={actual:#?}");
        if let Err(error)=&actual {eprintln!("RAW {label} FULL_CAUSE={:?} display={}",error.source(),error);
            if let RuntimeError::OwnedSource(_)=error {eprintln!("RAW {label} OWNED_DOWNCAST={:?}",error.source().and_then(|source|source.downcast_ref::<OwnedSourceError>()));}}
        actual
    }
    fn failure_run(label:&str,source:&Instance,declared:&[DeclaredNamedSourceRoot<'_>],execution:Option<&ExecutionContext<'_>>,limits:OwnedSourceLimits)->Result<ExecutionOutputs,RuntimeError> {
        capture_input(label,source,declared,limits);
        eprintln!("INPUT {label} supplied execution present={}",execution.is_some());
        failure_observation(label,execute_private_owned_basic(source,declared,execution,limits))
    }
    fn failure_expected(label:&str,actual:Result<ExecutionOutputs,RuntimeError>,error:RuntimeError) {
        eprintln!("EXPECTED {label} independently specified complete error={error:#?}");
        if let RuntimeError::OwnedSource(wanted)=&error {
            let actual_error=actual.as_ref().err().expect("complete result captured before downcast");
            let cause=actual_error.source().and_then(|source|source.downcast_ref::<OwnedSourceError>());
            eprintln!("RAW {label} bare error downcast before comparison={cause:#?}");assert_eq!(cause,Some(wanted));
        }
        assert_eq!(actual,Err(error),"{label}");
    }
"#;
const FAILURE_01: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    let actual=failure_run("G541-01",&source,&declared,None,policy());failure_expected("G541-01",actual,RuntimeError::MappingFailure{rule:1,message:None});}}
"#;
const FAILURE_02: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    let actual=failure_run("G541-02",&source,&declared,None,policy());failure_expected("G541-02",actual,RuntimeError::MappingFailure{rule:1,message:Some("r2".into())});}}
"#;
const FAILURE_03: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    let actual=failure_run("G541-03",&source,&declared,None,policy());failure_expected("G541-03",actual,RuntimeError::MappingFailure{rule:1,message:Some("r1".into())});}}
"#;
const FAILURE_04: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    let actual=failure_run("G541-04",&source,&declared,None,policy());failure_expected("G541-04",actual,RuntimeError::MappingFailure{rule:2,message:Some("second".into())});}}
"#;
const FAILURE_05: &str = r#"#[test] fn run(){let source=expected([("Value",s("P.root")),("Rows",repeated(Vec::<Instance>::new()))]);let reference=reference();let declared=failure_declarations(&reference);
    let actual=failure_run("G541-05",&source,&declared,None,policy());assert_eq!(actual,Ok(ExecutionOutputs{primary:expected([("Value",s("P.root"))]),extras:Vec::new()}));}}
"#;
const FAILURE_06: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    let positions={
        let roots=captured_roots("G541-06 separate direct public cohort roots",&source,&declared,policy());let root=captured_root("G541-06 separate direct public cohort context",&roots);
        let cohort=captured_cohort("G541-06 separate direct public cohort",root.walk_source_cohort(&["Rows","Rows"]));
        let positions=cohort.iter().map(|candidate|candidate.context().legacy_position(&["Rows","Rows"])).collect::<Vec<_>>();
        eprintln!("RAW G541-06 complete separate direct legacy positions={positions:?}");positions
    };
    let actual=failure_run("G541-06",&source,&declared,None,policy());
    assert_eq!(positions,vec![1,2,1]);failure_expected("G541-06",actual,RuntimeError::MappingFailure{rule:1,message:Some("2".into())});}}
"#;
const FAILURE_07: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let mut declared=failure_declarations(&reference);declared[3].instance=Some(&reference);
    let actual=failure_run("G541-07",&source,&declared,None,policy());failure_expected("G541-07",actual,RuntimeError::MappingFailure{rule:1,message:Some("N1".into())});}}
"#;
const FAILURE_08: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let mut declared=failure_declarations(&reference);declared[3].instance=Some(&reference);
    let actual=failure_run("G541-08",&source,&declared,None,policy());failure_expected("G541-08",actual,RuntimeError::MappingFailure{rule:1,message:Some("P.collision".into())});}}
"#;
const FAILURE_09: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    let mut parameters=codegen_runtime::RuntimeParameters::new();let flag=parameters.insert("flag",Value::Bool(true));eprintln!("RAW G541-09 flag insert={flag:#?}");flag.expect("raw insertion captured");
    let note=parameters.insert("note",Value::String("runtime".into()));eprintln!("RAW G541-09 note insert={note:#?}");note.expect("raw insertion captured");
    let execution=ExecutionContext::new(std::path::Path::new("relative/map.ferrule")).with_parameters(&parameters);
    eprintln!("INPUT G541-09 execution path=relative/map.ferrule parameters={parameters:#?}");
    let actual=failure_run("G541-09",&source,&declared,Some(&execution),policy());failure_expected("G541-09",actual,RuntimeError::MappingFailure{rule:1,message:Some("runtime".into())});}}
"#;
const FAILURE_10: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    let actual=failure_run("G541-10",&source,&declared,None,policy());failure_expected("G541-10",actual,RuntimeError::MissingRuntimeParameter{node:10,name:"flag".into()});}}
"#;
const FAILURE_11: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    let actual=failure_run("G541-11",&source,&declared,None,policy());failure_expected("G541-11",actual,RuntimeError::MissingRuntimeValue{value:RuntimeValue::CurrentDateTime});}}
"#;
const FAILURE_12: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    let execution=ExecutionContext::new(std::path::Path::new("relative/map.ferrule")).with_current_datetime("2026-07-19T11:22:33.45-07:00");
    eprintln!("INPUT G541-12 execution path=relative/map.ferrule datetime=2026-07-19T11:22:33.45-07:00");
    let actual=failure_run("G541-12",&source,&declared,Some(&execution),policy());failure_expected("G541-12",actual,RuntimeError::MappingFailure{rule:1,message:Some("2026-07-19T11:22:33.45-07:00".into())});}}
"#;
const FAILURE_13: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    let actual=failure_run("G541-13",&source,&declared,None,policy());failure_expected("G541-13",actual,RuntimeError::NotABool{node:10,found:"int"});}}
"#;
const FAILURE_14: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    let actual=failure_run("G541-14",&source,&declared,None,policy());failure_expected("G541-14",actual,RuntimeError::MappingException{node:11,message:None});}}
"#;
const FAILURE_15: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);let mut limits=policy();limits.max_walk_contexts=1;
    let actual=failure_run("G541-15",&source,&declared,None,limits);failure_expected("G541-15",actual,RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit{kind:OwnedSourceLimitKind::WalkContexts,used:1,requested:1,max:1}));}}
"#;
const FAILURE_16: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    let actual=failure_run("G541-16",&source,&declared,None,policy());failure_expected("G541-16",actual,RuntimeError::MappingFailure{rule:1,message:Some(String::new())});}}
"#;
const FAILURE_17: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    capture_input("G541-17",&source,&declared,policy());eprintln!("INPUT G541-17 complete selection sequence Primary,Named(Bad),Named(Unknown)");
    let primary=failure_observation("G541-17 primary",execute_selected_private_owned_basic(&source,&declared,None,policy(),TargetSelection::Primary));
    let named=failure_observation("G541-17 named",execute_selected_private_owned_basic(&source,&declared,None,policy(),TargetSelection::Named("Bad")));
    let unknown=failure_observation("G541-17 unknown",execute_selected_private_owned_basic(&source,&declared,None,policy(),TargetSelection::Named("Unknown")));
    assert_eq!(primary,Err(RuntimeError::MappingFailure{rule:1,message:None}));assert_eq!(named,Err(RuntimeError::MappingFailure{rule:1,message:None}));
    assert_eq!(unknown,Err(RuntimeError::UnknownTarget{name:"Unknown".into()}));}}
"#;
const FAILURE_18: &str = r#"#[test] fn run(){let source=failure_input();let reference=reference();let declared=failure_declarations(&reference);
    let actual=failure_run("G541-18",&source,&declared,None,policy());assert_eq!(actual,Ok(ExecutionOutputs{primary:expected([("Value",s("P.root"))]),extras:Vec::new()}));}}
"#;

fn checked_private_result(label: &str, program: &Program) -> Result<String, EmitError> {
    eprintln!("INPUT {label} complete_program={program:#?}");
    let actual = super::super::render_checked_private_owned_basic_source(program);
    eprintln!("RAW {label} complete_checked_source={actual:#?}");
    if let Err(error) = &actual {
        eprintln!(
            "RAW {label} complete_cause={:?}",
            error.source().map(ToString::to_string)
        );
    }
    actual
}
fn checked_catalogue_program() -> Program {
    let mut p = basic(
        vec![
            node(1, owned(SourceOwner::Primary, &["Value"], None)),
            node(
                2,
                Expression::SourceField {
                    path: path(&["Ref", "Value"]),
                    frame: None,
                },
            ),
            node(3, owned(named("ref"), &["Value"], None)),
            node(
                99,
                Expression::Const {
                    value: Value::String("unused.json".into()),
                },
            ),
        ],
        &[("Value", 1, ScalarType::String)],
    );
    p.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(Vec::new()),
    });
    for (name, expression) in [("legacy", 2), ("owned", 3)] {
        p.extra_targets.push(NamedTargetProgram {
            name: name.into(),
            target: p.target.clone(),
            root: scope(&[("Value", expression, ScalarType::String)]),
        });
    }
    p
}
fn catalogue_rows<'a>(
    catalogue: &codegen::NamedInputCatalogue<'a>,
) -> Vec<(usize, &'a str, codegen::NamedInputAvailability)> {
    catalogue
        .entries
        .iter()
        .map(|entry| (entry.declaration_index, entry.name, entry.availability))
        .collect()
}

#[test]
fn checked_private_catalogues_keep_complete_names_ordinals_and_target_modes() {
    use codegen::NamedInputAvailability::{
        Optional as O, RequiredLegacy as R, ReservedUnavailable as U,
    };
    let p = checked_catalogue_program();
    eprintln!("INPUT C01_C03 complete_program={p:#?}");
    let plan = codegen::legacy_named_input_admission_plan(&p);
    eprintln!("RAW C01_C03 complete_plan={plan:#?}");
    let plan = plan.expect("full plan retained");
    let all = catalogue_rows(&plan.all_outputs);
    let primary = catalogue_rows(&plan.primary);
    let named = plan
        .named_targets
        .iter()
        .map(|target| {
            (
                target.target_index,
                target.name,
                catalogue_rows(&target.catalogue),
            )
        })
        .collect::<Vec<_>>();
    eprintln!("RAW C01_C03 all={all:#?} primary={primary:#?} every_named={named:#?}");
    let optional = vec![
        (0, "Unused", U),
        (1, "Ref", O),
        (2, " Ref 雪 ", O),
        (3, "ref", O),
        (4, "primary", O),
    ];
    let required = vec![
        (0, "Unused", U),
        (1, "Ref", R),
        (2, " Ref 雪 ", O),
        (3, "ref", O),
        (4, "primary", O),
    ];
    assert_eq!(all, required);
    assert_eq!(primary, optional);
    assert_eq!(named, [(0, "legacy", required), (1, "owned", optional)]);
    let source = checked_private_result("C01_C03", &p).expect("full checked source retained");
    assert!(
        source.contains("const PRIVATE_CHECKED_ALL_INPUTS: &[BasicNamedSourceDeclaration] = &[")
    );
    assert!(source.contains("BasicNamedSourceDeclaration { name: \" Ref \\u{96ea} \", mode: BasicNamedSourceMode::Optional }, // declaration 2"));
    assert!(source.contains("BasicNamedSourceDeclaration { name: \"Unused\", mode: BasicNamedSourceMode::ReservedUnavailable }, // declaration 0"));
    assert!(source.contains("\"legacy\" => (1, PRIVATE_CHECKED_NAMED_INPUTS_0)"));
    assert!(source.contains("\"owned\" => (2, PRIVATE_CHECKED_NAMED_INPUTS_1)"));
}

#[test]
fn checked_private_entries_preserve_prepared_source_and_select_before_lazy_consumption() {
    let p = checked_catalogue_program();
    let prepared = private_source("C04 prepared", &p);
    let checked = checked_private_result("C04 checked", &p).expect("full source retained");
    assert!(checked.starts_with(&prepared));
    let tail = &checked[prepared.len()..];
    assert!(tail.contains("supplied: &[BasicNamedSourceInput<'name, 'src>]"));
    assert!(tail.contains(
        "supplied.iter().copied().map(|input| { private_checked_input_reached(); input }), limits)?"
    ));
    assert!(!tail.contains(".collect"));
    assert!(!tail.contains("OwnedSourceLimits {"));
    assert!(!tail.contains("clone()"));
    let selected = tail
        .find("pub(crate) fn execute_selected_checked_private_owned_basic")
        .unwrap();
    let selected = &tail[selected..];
    let unknown = selected.find("RuntimeError::UnknownTarget").unwrap();
    let factory = selected.find("private_checked_factory_reached();").unwrap();
    let normalization = selected
        .find("BasicSourceRoots::try_from_named_inputs")
        .unwrap();
    assert!(unknown < factory && factory < normalization);
    assert_eq!(
        tail.matches("execute_private_checked_basic_context(&context,")
            .count(),
        2
    );
    assert!(tail.contains("#[cfg(test)]\nstd::thread_local!"));
    assert!(prepared.contains("BasicSourceRoots::try_new(source, declared, limits)?"));
}

#[test]
fn checked_private_inventory_keeps_potential_unsupported_branches_and_dynamic_frames_closed() {
    let mut p = basic(
        vec![
            node(1, owned(SourceOwner::Primary, &["Value"], None)),
            node(
                2,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            node(
                3,
                Expression::SourceRootField {
                    path: path(&["Value"]),
                    required: false,
                },
            ),
            node(
                4,
                Expression::If {
                    condition: 2,
                    then: 1,
                    else_: 3,
                },
            ),
        ],
        &[("Value", 4, ScalarType::String)],
    );
    p.source = serde_json::from_str(r#"{"name":"Root","xml_type_alternatives":true,"xml_default_type":"Base","kind":{"kind":"group","children":[{"name":"Code","attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Value","kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"Base","members":["Code","Value"]},{"name":"Derived","members":["Code","Extra","Value"]}]}}"#).unwrap();
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 3,
                scope: None,
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::UnsupportedBasicContext,
            origin: codegen::OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 3,
            },
        }),
    };
    let actual = checked_private_result("C05 potential unsupported", &p);
    match actual {
        Err(EmitError::InvalidProgram(error)) => assert_eq!(error, expected),
        other => panic!("complete source outcome retained: {other:?}"),
    }
    p.source = source_schema();
    p.expressions[2].expression = Expression::Const {
        value: Value::String("unused.json".into()),
    };
    p.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 3,
        driver: codegen::SourceIteration::new(Vec::new()),
    });
    p.expressions[0].expression = Expression::SourceField {
        path: path(&["Value"]),
        frame: Some(path(&["Unused"])),
    };
    let expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
                scope: None,
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::UnsupportedBasicContext,
            origin: codegen::OwnedSourceCapabilityOrigin::RetainedExpression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 1,
            },
        }),
    };
    let actual = checked_private_result("C05 retained dynamic frame", &p);
    match actual {
        Err(EmitError::InvalidProgram(error)) => assert_eq!(error, expected),
        other => panic!("complete source outcome retained: {other:?}"),
    }
}

fn checked_generated_programs() -> Vec<(&'static str, Program, &'static str)> {
    let mut cases = Vec::new();
    cases.push((
        "checked_selection",
        checked_catalogue_program(),
        CHECKED_SELECTION,
    ));
    let optional = basic(
        vec![node(1, owned(named("Ref"), &["Value"], None))],
        &[("Value", 1, ScalarType::String)],
    );
    cases.push(("checked_optional", optional, CHECKED_OPTIONAL));
    let required = basic(
        vec![node(
            1,
            Expression::SourceField {
                path: path(&["Ref", "Value"]),
                frame: None,
            },
        )],
        &[("Value", 1, ScalarType::String)],
    );
    cases.push(("checked_required", required, CHECKED_REQUIRED));
    let primary = basic(
        vec![node(1, owned(SourceOwner::Primary, &["Value"], None))],
        &[("Value", 1, ScalarType::String)],
    );
    cases.push(("checked_supplied", primary.clone(), CHECKED_SUPPLIED));
    cases.push(("checked_limits", primary.clone(), CHECKED_LIMITS));
    let exact = basic(
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
    cases.push(("checked_exact", exact, CHECKED_EXACT));
    for (name, flag, check) in [
        ("checked_lazy_true", true, CHECKED_LAZY_TRUE),
        ("checked_lazy_false", false, CHECKED_LAZY_FALSE),
    ] {
        let p = basic(
            vec![
                node(
                    1,
                    Expression::Const {
                        value: Value::Bool(flag),
                    },
                ),
                node(
                    2,
                    Expression::Const {
                        value: Value::String("chosen".into()),
                    },
                ),
                node(3, owned(named("Unused"), &["Value"], None)),
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
        cases.push((name, p, check));
    }
    let eager = basic(
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
            node(
                3,
                Expression::SourceField {
                    path: path(&["Ref", "Value"]),
                    frame: None,
                },
            ),
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
    cases.push(("checked_potential_legacy", eager, CHECKED_POTENTIAL_LEGACY));
    let mut raise = primary.clone();
    raise
        .expressions
        .push(node(71, Expression::Raise { message: None }));
    raise.extra_targets.push(NamedTargetProgram {
        name: "fail".into(),
        target: raise.target.clone(),
        root: scope(&[("Value", 71, ScalarType::String)]),
    });
    cases.push(("checked_raise", raise, CHECKED_RAISE));
    let mut zero = basic(
        vec![node(1, owned(SourceOwner::Primary, &[], None))],
        &[("Value", 1, ScalarType::String)],
    );
    zero.source = SchemaNode::scalar("Input", ScalarType::String);
    zero.extra_sources.clear();
    cases.push(("checked_zero", zero, CHECKED_ZERO));
    cases
}

#[test]
#[ignore = "requires explicit checked evidence root, qualified runtime and exclusive shared target admission"]
fn private_checked_generated_inputs_capture_complete_results_before_comparisons() {
    let root = PathBuf::from(
        std::env::var_os("FERRULE_PRIVATE_RUST_CHECKED_EVIDENCE_ROOT")
            .expect("fresh checked evidence root"),
    );
    let runtime = PathBuf::from(
        std::env::var_os("FERRULE_PRIVATE_RUST_BASIC_RUNTIME").expect("qualified runtime"),
    );
    assert!(root.is_absolute() && runtime.is_absolute());
    assert!(
        std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR").is_some(),
        "one admitted shared target"
    );
    fs::create_dir(&root).expect("fresh root; retain evidence on refusal");
    fs::create_dir(root.join("src")).unwrap();
    let mut generated = String::new();
    for (name, program, check) in checked_generated_programs() {
        let source = checked_private_result(name, &program).expect("full source result retained");
        retained_file(
            &root.join(format!("{name}.generated.rs")),
            source.as_bytes(),
        );
        generated.push_str(&format!(
            "mod {name} {{\n{source}\n{RUNNER_COMMON}\n{CHECKED_RUNNER_COMMON}\n{check}\n}}\n"
        ));
    }
    retained_file(&root.join("src/lib.rs"), generated.as_bytes());
    let manifest = format!(
        "[package]\nname = \"ferrule_private_checked_basic\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n[dependencies]\ncodegen-runtime = {{ path = {} }}\n[workspace]\n",
        toml_string(runtime.to_str().expect("Unicode runtime path"))
    );
    retained_file(&root.join("Cargo.toml"), manifest.as_bytes());
    retained_file(
        &root.join("COMMAND.txt"),
        b"cargo test --offline -- --nocapture --test-threads=1\n",
    );
    let actual = Command::new("cargo")
        .args(["test", "--offline", "--", "--nocapture", "--test-threads=1"])
        .current_dir(&root)
        .generated_host_output(&root);
    match &actual {
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
    eprintln!("RAW checked generated complete_call={actual:#?}");
    let output = actual.expect("complete refusal retained");
    assert!(
        output.status.success(),
        "complete compiler/test originals retained"
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
    let reached = (
        count("checked_lazy_true", 3),
        count("checked_lazy_false", 3),
        count("checked_raise", 71),
    );
    eprintln!("RAW checked full reached expression counts={reached:?}");
    assert_eq!(reached, (0, 1, 2));
}

const CHECKED_RUNNER_COMMON: &str = r#"
    fn checked_capture(label:&str,source:&Instance,supplied:&[BasicNamedSourceInput<'_,'_>],limits:OwnedSourceLimits,selection:Option<TargetSelection<'_>>) {
        eprintln!("INPUT {label} complete_primary={source:?} complete_supplied={supplied:?} policy={limits:?} selection={selection:?} all_catalogue={PRIVATE_CHECKED_ALL_INPUTS:?} primary_catalogue={PRIVATE_CHECKED_PRIMARY_INPUTS:?}");
    }
    fn checked_all<'name,'src>(label:&str,source:&'src Instance,supplied:&[BasicNamedSourceInput<'name,'src>],limits:OwnedSourceLimits)->(Result<ExecutionOutputs,RuntimeError>,(usize,usize)) {
        checked_capture(label,source,supplied,limits,None); private_checked_reset_reaches();
        let actual=raw(label,execute_checked_private_owned_basic(source,supplied,None,limits));
        let reaches=private_checked_reaches();eprintln!("RAW {label} observed_factory_calls_and_mapped_items={reaches:?}");(actual,reaches)
    }
    fn checked_selected<'name,'src>(label:&str,source:&'src Instance,supplied:&[BasicNamedSourceInput<'name,'src>],limits:OwnedSourceLimits,selection:TargetSelection<'_>)->(Result<SelectedTargetOutput,RuntimeError>,(usize,usize)) {
        checked_capture(label,source,supplied,limits,Some(selection));private_checked_reset_reaches();
        let actual=raw(label,execute_selected_checked_private_owned_basic(source,supplied,None,limits,selection));
        let reaches=private_checked_reaches();eprintln!("RAW {label} observed_factory_calls_and_mapped_items={reaches:?}");(actual,reaches)
    }
    fn checked_cause<T:std::fmt::Debug>(label:&str,actual:Result<T,RuntimeError>,expected:RuntimeError) {
        checked_error(label,actual,expected);
    }
    fn checked_primary()->ExecutionOutputs { ExecutionOutputs{primary:expected([("Value",s("P.root"))]),extras:Vec::new()} }
"#;

const CHECKED_SELECTION: &str = r#"
    #[test] fn run() {
        let p=primary();let r=reference();let lower=expected([("Value",s("N.lower"))]);
        let supplied=[BasicNamedSourceInput{name:"ref",instance:&lower},BasicNamedSourceInput{name:"Ref",instance:&r}];
        let (all,c)=checked_all("H01",&p,&supplied,policy());
        assert_eq!(all,Ok(ExecutionOutputs{primary:expected([("Value",s("P.root"))]),extras:vec![NamedOutput{name:"legacy".into(),instance:expected([("Value",s("P.collision"))])},NamedOutput{name:"owned".into(),instance:expected([("Value",s("N.lower"))])}]}));assert_eq!(c,(1,2));
        let (actual,c)=checked_selected("H02",&p,&[],policy(),TargetSelection::Primary);assert_eq!(actual,Ok(SelectedTargetOutput::Primary(expected([("Value",s("P.root"))]))));assert_eq!(c,(1,0));
        let (actual,c)=checked_selected("H03 missing",&p,&[],policy(),TargetSelection::Named("legacy"));checked_cause("H03 missing",actual,RuntimeError::MissingNamedSource{name:"Ref"});assert_eq!(c,(1,0));
        let (actual,c)=checked_selected("H03 present",&p,&supplied[1..],policy(),TargetSelection::Named("legacy"));assert_eq!(actual,Ok(SelectedTargetOutput::Named(NamedOutput{name:"legacy".into(),instance:expected([("Value",s("P.collision"))])})));assert_eq!(c,(1,1));
        let (actual,c)=checked_selected("H04",&p,&supplied[..1],policy(),TargetSelection::Named("owned"));assert_eq!(actual,Ok(SelectedTargetOutput::Named(NamedOutput{name:"owned".into(),instance:expected([("Value",s("N.lower"))])})));assert_eq!(c,(1,1));
        let bad=[BasicNamedSourceInput{name:"X",instance:&r}];let zero=OwnedSourceLimits{max_roots:0,max_active_frames:0,max_path_segments:0,max_metadata_bytes:0,max_walk_contexts:0};
        let (actual,c)=checked_selected("H05",&p,&bad,zero,TargetSelection::Named("missing"));checked_cause("H05",actual,RuntimeError::UnknownTarget{name:"missing".into()});assert_eq!(c,(0,0));
        let (actual,c)=checked_selected("H08 absent",&p,&[],policy(),TargetSelection::Primary);assert_eq!(actual,Ok(SelectedTargetOutput::Primary(expected([("Value",s("P.root"))]))));assert_eq!(c,(1,0));
        let reserved=[BasicNamedSourceInput{name:"Unused",instance:&r}];let (actual,c)=checked_selected("H08 supplied",&p,&reserved,policy(),TargetSelection::Primary);checked_cause("H08 supplied",actual,RuntimeError::UnexpectedNamedSource{name:"Unused".into()});assert_eq!(c,(1,1));
    }
}
"#;
const CHECKED_OPTIONAL: &str = r#"
    #[test] fn run() {
        let p=primary();let r=reference();let (actual,c)=checked_all("H06",&p,&[],policy());checked_cause("H06",actual,RuntimeError::OwnedSource(OwnedSourceError::SourceUnavailable{name:"Ref".into()}));assert_eq!(c,(1,0));
        let input=[BasicNamedSourceInput{name:"Ref",instance:&r}];let (actual,c)=checked_all("H07",&p,&input,policy());assert_eq!(actual,Ok(ExecutionOutputs{primary:expected([("Value",s("N.root"))]),extras:Vec::new()}));assert_eq!(c,(1,1));
        let names=String::from("Ref");let actual={let input=[BasicNamedSourceInput{name:names.as_str(),instance:&r}];checked_all("H25",&p,&input,policy())};drop(names);eprintln!("RAW H25 after_transient_name_drop={actual:?}");assert_eq!(actual.0,Ok(ExecutionOutputs{primary:expected([("Value",s("N.root"))]),extras:Vec::new()}));assert_eq!(actual.1,(1,1));
    }
}
"#;
const CHECKED_REQUIRED: &str = r#"
    #[test] fn run() {
        let p=primary();let r=reference();let (actual,c)=checked_all("H03 required",&p,&[],policy());checked_cause("H03 required",actual,RuntimeError::MissingNamedSource{name:"Ref"});assert_eq!(c,(1,0));
        let input=[BasicNamedSourceInput{name:"Ref",instance:&r}];let (actual,c)=checked_all("H03 required present",&p,&input,policy());assert_eq!(actual,Ok(ExecutionOutputs{primary:expected([("Value",s("P.collision"))]),extras:Vec::new()}));assert_eq!(c,(1,1));
    }
}
"#;
const CHECKED_SUPPLIED: &str = r#"
    #[test] fn run() {
        let p=primary();let r=reference();let other=expected([("Value",s("different"))]);
        let scenarios=[
            ("H09",vec![BasicNamedSourceInput{name:"X",instance:&r},BasicNamedSourceInput{name:"Ref",instance:&r}],RuntimeError::UnexpectedNamedSource{name:"X".into()},1),
            ("H10",vec![BasicNamedSourceInput{name:"Ref",instance:&r},BasicNamedSourceInput{name:"Ref",instance:&r}],RuntimeError::DuplicateNamedSource{name:"Ref"},2),
            ("H11",vec![BasicNamedSourceInput{name:"Ref",instance:&r},BasicNamedSourceInput{name:"Ref",instance:&other}],RuntimeError::DuplicateNamedSource{name:"Ref"},2),
            ("H12",vec![BasicNamedSourceInput{name:"",instance:&r}],RuntimeError::UnexpectedNamedSource{name:"".into()},1),
            ("H13 unknown-first",vec![BasicNamedSourceInput{name:"X",instance:&r},BasicNamedSourceInput{name:"Ref",instance:&r},BasicNamedSourceInput{name:"Ref",instance:&r}],RuntimeError::UnexpectedNamedSource{name:"X".into()},1),
            ("H13 duplicate-first",vec![BasicNamedSourceInput{name:"Ref",instance:&r},BasicNamedSourceInput{name:"Ref",instance:&r},BasicNamedSourceInput{name:"X",instance:&r}],RuntimeError::DuplicateNamedSource{name:"Ref"},2),
        ];
        for (label,input,error,items) in scenarios {let (actual,c)=checked_all(label,&p,&input,policy());checked_cause(label,actual,error);assert_eq!(c,(1,items));}
        let (actual,c)=checked_all("H09 empty valid",&p,&[],policy());assert_eq!(actual,Ok(checked_primary()));assert_eq!(c,(1,0));
    }
}
"#;
const CHECKED_EXACT: &str = r#"
    #[test] fn run() {
        let p=primary();let exact=expected([("Value",s("N.exact"))]);let lower=expected([("Value",s("N.lower"))]);let named_primary=expected([("Value",s("N.literal.primary"))]);
        let input=[BasicNamedSourceInput{name:"primary",instance:&named_primary},BasicNamedSourceInput{name:"ref",instance:&lower},BasicNamedSourceInput{name:" Ref 雪 ",instance:&exact}];
        let (actual,c)=checked_all("H14",&p,&input,policy());assert_eq!(actual,Ok(ExecutionOutputs{primary:expected([("Exact",s("N.exact")),("Lower",s("N.lower")),("LiteralPrimary",s("N.literal.primary"))]),extras:Vec::new()}));assert_eq!(c,(1,3));
        let names=PRIVATE_CHECKED_ALL_INPUTS.iter().map(|d|d.name).collect::<Vec<_>>();eprintln!("RAW H14 complete_original_catalogue_names={names:?}");assert_eq!(names,["Unused","Ref"," Ref 雪 ","ref","primary"]);
    }
}
"#;
const CHECKED_LIMITS: &str = r#"
    fn limit(kind:OwnedSourceLimitKind,used:usize,requested:usize,max:usize)->RuntimeError {RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit{kind,used,requested,max})}
    #[test] fn run() {
        let p=primary();let r=reference();let input=[BasicNamedSourceInput{name:"Ref",instance:&r}];let mut limits=policy();limits.max_roots=6;
        let (actual,c)=checked_all("H15",&p,&input,limits);assert_eq!(actual,Ok(checked_primary()));assert_eq!(c,(1,1));
        limits.max_roots=5;let (actual,c)=checked_all("H16",&p,&input,limits);checked_cause("H16",actual,limit(OwnedSourceLimitKind::Roots,0,6,5));assert_eq!(c,(1,0));
        limits.max_roots=0;let (actual,c)=checked_all("H17",&p,&input,limits);checked_cause("H17",actual,limit(OwnedSourceLimitKind::Roots,0,6,0));assert_eq!(c,(1,0));
        limits=policy();limits.max_metadata_bytes=0;let (actual,c)=checked_all("H18",&p,&input,limits);checked_cause("H18",actual,limit(OwnedSourceLimitKind::MetadataBytes,0,6,0));assert_eq!(c,(1,0));
        limits=policy();limits.max_path_segments=0;let (actual,c)=checked_all("H19",&p,&[],limits);checked_cause("H19",actual,limit(OwnedSourceLimitKind::PathSegments,0,1,0));assert_eq!(c,(1,0));
    }
}
"#;
const CHECKED_LAZY_TRUE: &str = r#"
    #[test] fn run() {let p=primary();let (actual,c)=checked_all("H20",&p,&[],policy());assert_eq!(actual,Ok(ExecutionOutputs{primary:expected([("Value",s("chosen"))]),extras:Vec::new()}));assert_eq!(c,(1,0));}
}
"#;
const CHECKED_LAZY_FALSE: &str = r#"
    #[test] fn run() {let p=primary();let (actual,c)=checked_all("H21",&p,&[],policy());checked_cause("H21",actual,RuntimeError::OwnedSource(OwnedSourceError::SourceUnavailable{name:"Unused".into()}));assert_eq!(c,(1,0));}
}
"#;
const CHECKED_POTENTIAL_LEGACY: &str = r#"
    #[test] fn run() {let p=primary();let (actual,c)=checked_all("H22",&p,&[],policy());checked_cause("H22",actual,RuntimeError::MissingNamedSource{name:"Ref"});assert_eq!(c,(1,0));}
}
"#;
const CHECKED_RAISE: &str = r#"
    #[test] fn run() {
        let p=primary();let (actual,c)=checked_selected("H23 primary",&p,&[],policy(),TargetSelection::Primary);assert_eq!(actual,Ok(SelectedTargetOutput::Primary(expected([("Value",s("P.root"))]))));assert_eq!(c,(1,0));
        let (actual,c)=checked_selected("H23 fail",&p,&[],policy(),TargetSelection::Named("fail"));checked_cause("H23 fail",actual,RuntimeError::MappingException{node:71,message:None});assert_eq!(c,(1,0));
        let (actual,c)=checked_all("H23 all",&p,&[],policy());checked_cause("H23 all",actual,RuntimeError::MappingException{node:71,message:None});assert_eq!(c,(1,0));
    }
}
"#;
const CHECKED_ZERO: &str = r#"
    #[test] fn run() {let p=s("P.root");let limits=OwnedSourceLimits{max_roots:1,max_active_frames:0,max_path_segments:0,max_metadata_bytes:0,max_walk_contexts:0};let (actual,c)=checked_all("H24",&p,&[],limits);assert_eq!(actual,Ok(ExecutionOutputs{primary:expected([("Value",s("P.root"))]),extras:Vec::new()}));assert_eq!(c,(1,0));}
}
"#;

// Checked normalization and ordinary global source rules are composed only in
// the private prototype. Existing prepared, checked, and failure controls above
// remain unchanged; these interaction cases have a separately frozen contract.
fn composed_ordered_failure_program() -> Program {
    use codegen::FailureSelection::{All, WhenTrue};
    let mut p = failure_program(
        vec![
            node(
                20,
                Expression::Const {
                    value: Value::Bool(false),
                },
            ),
            node(21, Expression::Raise { message: None }),
            node(
                10,
                Expression::SourceField {
                    path: path(&["Flag"]),
                    frame: None,
                },
            ),
            node(
                11,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: Some(path(&["Rows"])),
                },
            ),
            node(23, Expression::Raise { message: None }),
            node(71, Expression::Raise { message: None }),
        ],
        vec![
            failure_rule(&["Rows"], WhenTrue(20), Some(21)),
            failure_rule(&["Rows"], WhenTrue(10), Some(11)),
            failure_rule(&["Rows"], All, Some(23)),
        ],
    );
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
    p
}

#[test]
fn private_checked_global_failure_composition_keeps_one_context_and_pre_target_order() {
    let p = composed_ordered_failure_program();
    let actual = checked_private_result("C519 composed ordered program", &p);
    let source = actual.expect("complete input/source/cause retained before comparisons");
    let prepared = failure_source_result("C519 independent prepared failure entry", &p)
        .expect("complete prepared input/source/cause retained");
    assert!(source.starts_with(&prepared));
    let shared = &source[source
        .rfind("\nfn execute_private_checked_basic_context(")
        .unwrap()..];
    let failure = shared
        .find("evaluate_basic_failure_rules(context)?;")
        .unwrap();
    let selected = shared.find("if let Some(selected) = selected").unwrap();
    let all = shared.find("let primary = scope_root(context)?;").unwrap();
    assert!(failure < selected && selected < all);
    assert_eq!(
        shared
            .matches("evaluate_basic_failure_rules(context)?;")
            .count(),
        1
    );
    let checked = &source[prepared.len()..];
    assert_eq!(
        checked
            .matches("BasicSourceRoots::try_from_named_inputs")
            .count(),
        2
    );
    assert_eq!(
        checked
            .matches("execute_private_checked_basic_context(&context,")
            .count(),
        2
    );
    assert!(!checked.contains("BasicSourceRoots::try_new"));
    let selected_entry = &checked[checked
        .find("pub(crate) fn execute_selected_checked_private_owned_basic")
        .unwrap()..];
    assert!(
        selected_entry.find("RuntimeError::UnknownTarget").unwrap()
            < selected_entry
                .find("private_checked_factory_reached();")
                .unwrap()
    );
    assert!(source.contains("let cohort = reached?;"));
    assert!(
        source
            .find("evaluate_basic_failure_rule_0(context)?;")
            .unwrap()
            < source
                .find("evaluate_basic_failure_rule_1(context)?;")
                .unwrap()
    );
}

fn composed_checked_failure_programs() -> Vec<(&'static str, Program, &'static str)> {
    use codegen::FailureSelection::{All, WhenTrue};
    let mut cases = vec![
        ("g519_01", composed_ordered_failure_program(), COMPOSED_01),
        ("g519_02", composed_ordered_failure_program(), COMPOSED_02),
    ];
    cases.push((
        "g519_03",
        failure_program(
            vec![node(
                11,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: Some(path(&["ref", "Rows"])),
                },
            )],
            vec![failure_rule(&["ref", "Rows"], All, Some(11))],
        ),
        COMPOSED_03,
    ));
    cases.push((
        "g519_04",
        failure_program(Vec::new(), vec![failure_rule(&["Rows"], All, None)]),
        COMPOSED_04,
    ));
    cases.push((
        "g519_05",
        failure_program(
            vec![
                node(
                    10,
                    Expression::SourceField {
                        path: path(&["Flag"]),
                        frame: None,
                    },
                ),
                node(
                    11,
                    Expression::Position {
                        collection: path(&["Rows", "Rows"]),
                    },
                ),
            ],
            vec![failure_rule(&["Rows", "Rows"], WhenTrue(10), Some(11))],
        ),
        COMPOSED_05,
    ));
    cases.push((
        "g519_06",
        failure_program(
            vec![
                node(
                    10,
                    Expression::Const {
                        value: Value::Bool(false),
                    },
                ),
                node(11, Expression::Raise { message: None }),
            ],
            vec![failure_rule(&["Rows"], WhenTrue(10), Some(11))],
        ),
        COMPOSED_06,
    ));
    cases
}

#[test]
#[ignore = "requires fresh composed evidence root, qualified runtime and one separately admitted shared generated-host target"]
fn private_checked_global_failure_interactions_retain_full_inputs_returns_and_trace() {
    let root = PathBuf::from(
        std::env::var_os("FERRULE_PRIVATE_RUST_COMPOSED_EVIDENCE_ROOT")
            .expect("fresh composed evidence root"),
    );
    let runtime = PathBuf::from(
        std::env::var_os("FERRULE_PRIVATE_RUST_BASIC_RUNTIME")
            .expect("explicit qualified runtime source"),
    );
    assert!(root.is_absolute() && runtime.is_absolute());
    assert!(
        std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR").is_some(),
        "one shared target; no duplicate cache"
    );
    fs::create_dir(&root).expect("fresh namespace; preserve reached originals on refusal");
    fs::create_dir(root.join("src")).unwrap();
    let mut generated = String::new();
    for (name, program, check) in composed_checked_failure_programs() {
        retained_file(
            &root.join(format!("{name}.Program.original")),
            format!("{program:#?}\n").as_bytes(),
        );
        let actual = checked_private_result(name, &program);
        retained_file(
            &root.join(format!("{name}.renderer-return.original")),
            format!("{actual:#?}\n").as_bytes(),
        );
        let source = actual.expect("complete source/refusal retained before qualification");
        retained_file(
            &root.join(format!("{name}.generated.rs")),
            source.as_bytes(),
        );
        generated.push_str(&format!("mod {name} {{\n{source}\n{RUNNER_COMMON}\n{CHECKED_RUNNER_COMMON}\n{FAILURE_COMMON}\n{check}\n}}\n"));
    }
    retained_file(&root.join("src/lib.rs"), generated.as_bytes());
    let manifest = format!(
        "[package]\nname = \"ferrule_private_checked_global_failure\"\nversion = \"0.1.0\"\nedition = \"2024\"\npublish = false\n[dependencies]\ncodegen-runtime = {{ path = {} }}\n[workspace]\n",
        toml_string(runtime.to_str().expect("Unicode runtime path"))
    );
    retained_file(&root.join("Cargo.toml"), manifest.as_bytes());
    retained_file(
        &root.join("COMMAND.txt"),
        b"cargo test --offline -- --nocapture --test-threads=1\n",
    );
    let actual = Command::new("cargo")
        .args(["test", "--offline", "--", "--nocapture", "--test-threads=1"])
        .current_dir(&root)
        .generated_host_output(&root);
    match &actual {
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
            format!("{error:#?}\n").as_bytes(),
        ),
    }
    eprintln!("RAW G519 complete compiler/runner return={actual:#?}");
    let output = actual.expect("complete call error retained before propagation");
    assert!(
        output.status.success(),
        "whole compiler/runner records retained at {root:?}"
    );
    let trace = String::from_utf8_lossy(&output.stderr);
    let expected = [
        ("g519_01", 10, 0),
        ("g519_01", 11, 0),
        ("g519_01", 20, 0),
        ("g519_01", 21, 0),
        ("g519_01", 23, 0),
        ("g519_01", 71, 0),
        ("g519_01", 100, 0),
        ("g519_02", 20, 6),
        ("g519_02", 21, 0),
        ("g519_02", 10, 6),
        ("g519_02", 11, 3),
        ("g519_02", 23, 0),
        ("g519_02", 71, 0),
        ("g519_02", 100, 0),
        ("g519_03", 11, 1),
        ("g519_03", 100, 0),
        ("g519_04", 100, 0),
        ("g519_05", 10, 2),
        ("g519_05", 11, 1),
        ("g519_05", 100, 0),
        ("g519_06", 10, 4),
        ("g519_06", 11, 0),
        ("g519_06", 100, 2),
    ];
    let observed = expected
        .iter()
        .map(|(module, node, _)| {
            let count = trace
                .lines()
                .filter(|line| {
                    line.starts_with("RAW PRIVATE_BASIC_EXPRESSION ")
                        && line.contains(module)
                        && line.ends_with(&format!("node={node}"))
                })
                .count();
            (*module, *node, count)
        })
        .collect::<Vec<_>>();
    let nested = trace
        .lines()
        .filter(|line| {
            line.starts_with("RAW PRIVATE_BASIC_FAILURE_CANDIDATE ")
                && line.contains("g519_05")
                && line.contains(" rule=1 ")
        })
        .collect::<Vec<_>>();
    let nested_count = trace
        .lines()
        .filter(|line| {
            line.starts_with("RAW PRIVATE_BASIC_FAILURE_COHORT ")
                && line.contains("g519_05")
                && line.ends_with("rule=1 count=3")
        })
        .count();
    retained_file(
        &root.join("entry-counts.complete.original"),
        format!("{observed:#?}\n").as_bytes(),
    );
    retained_file(
        &root.join("nested-cohort.complete.original"),
        format!("count_records={nested_count}\n{nested:#?}\n").as_bytes(),
    );
    eprintln!(
        "RAW G519 complete entry counts={observed:#?} nested_count={nested_count} nested={nested:#?}"
    );
    for ((module, node, wanted), (_, _, got)) in expected.iter().zip(&observed) {
        assert_eq!(got, wanted, "{module} node={node}");
    }
    assert_eq!(nested_count, 1);
    assert_eq!(nested.len(), 3);
    for (index, (line, position)) in nested.iter().zip([1, 2, 1]).enumerate() {
        assert!(
            line.ends_with(&format!(
                "id={index} raw_ordinal=Some({position}) raw_position={position}"
            )),
            "complete witnessed raw position record"
        );
    }
}

const COMPOSED_01: &str = r#"#[test] fn run(){
    let p=failure_input();let r=reference();let supplied=[BasicNamedSourceInput{name:"X",instance:&r}];
    let zero=OwnedSourceLimits{max_roots:0,max_active_frames:0,max_path_segments:0,max_metadata_bytes:0,max_walk_contexts:0};
    let (unknown,c1)=checked_selected("G519-01 unknown",&p,&supplied,zero,TargetSelection::Named("missing"));
    let (normalization,c2)=checked_all("G519-01 normalization",&p,&supplied,policy());
    checked_cause("G519-01 unknown",unknown,RuntimeError::UnknownTarget{name:"missing".into()});assert_eq!(c1,(0,0));
    checked_cause("G519-01 normalization",normalization,RuntimeError::UnexpectedNamedSource{name:"X".into()});assert_eq!(c2,(1,1));
}}
"#;
const COMPOSED_02: &str = r#"#[test] fn run(){
    let p=failure_input();let (all,c1)=checked_all("G519-02 all",&p,&[],policy());
    let (primary,c2)=checked_selected("G519-02 primary",&p,&[],policy(),TargetSelection::Primary);
    let (named,c3)=checked_selected("G519-02 named",&p,&[],policy(),TargetSelection::Named("Bad"));
    checked_cause("G519-02 all",all,RuntimeError::MappingFailure{rule:2,message:Some("r2".into())});
    checked_cause("G519-02 primary",primary,RuntimeError::MappingFailure{rule:2,message:Some("r2".into())});
    checked_cause("G519-02 named",named,RuntimeError::MappingFailure{rule:2,message:Some("r2".into())});
    assert_eq!((c1,c2,c3),((1,0),(1,0),(1,0)));
}}
"#;
const COMPOSED_03: &str = r#"#[test] fn run(){
    let p=failure_input();let r=reference();let supplied=[BasicNamedSourceInput{name:"ref",instance:&r}];
    let (missing,c1)=checked_selected("G519-03 required",&p,&[],policy(),TargetSelection::Primary);
    let (reached,c2)=checked_selected("G519-03 supplied",&p,&supplied,policy(),TargetSelection::Primary);
    checked_cause("G519-03 required",missing,RuntimeError::MissingNamedSource{name:"ref"});assert_eq!(c1,(1,0));
    checked_cause("G519-03 supplied",reached,RuntimeError::MappingFailure{rule:1,message:Some("N1".into())});assert_eq!(c2,(1,1));
}}
"#;
const COMPOSED_04: &str = r#"#[test] fn run(){
    let p=failure_input();let mut limits=policy();limits.max_walk_contexts=1;
    let (all,c1)=checked_all("G519-04 all",&p,&[],limits);
    let (selected,c2)=checked_selected("G519-04 primary",&p,&[],limits,TargetSelection::Primary);
    checked_cause("G519-04 all",all,RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit{kind:OwnedSourceLimitKind::WalkContexts,used:1,requested:1,max:1}));
    checked_cause("G519-04 primary",selected,RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit{kind:OwnedSourceLimitKind::WalkContexts,used:1,requested:1,max:1}));
    assert_eq!((c1,c2),((1,0),(1,0)));
}}
"#;
const COMPOSED_05: &str = r#"#[test] fn run(){
    let p=failure_input();let (actual,c)=checked_all("G519-05 nested",&p,&[],policy());
    checked_cause("G519-05 nested",actual,RuntimeError::MappingFailure{rule:1,message:Some("2".into())});assert_eq!(c,(1,0));
}}
"#;
const COMPOSED_06: &str = r#"#[test] fn run(){
    let p=failure_input();let (all,c1)=checked_all("G519-06 all",&p,&[],policy());
    let (selected,c2)=checked_selected("G519-06 primary",&p,&[],policy(),TargetSelection::Primary);
    assert_eq!(all,Ok(ExecutionOutputs{primary:expected([("Value",s("P.root"))]),extras:Vec::new()}));
    assert_eq!(selected,Ok(SelectedTargetOutput::Primary(expected([("Value",s("P.root"))]))));assert_eq!((c1,c2),((1,0),(1,0)));
}}
"#;

// Additive public Basic family. Each prospective result is retained before
// propagation or comparison; generated-host execution requires separate admission.
fn public519_options(name: &str) -> Options {
    Options {
        package_name: name.into(),
        runtime_dependency: RuntimeDependency::Version("0.1.0".into()),
    }
}
fn public519_result(
    label: &str,
    program: &Program,
    options: &Options,
) -> Result<ArtifactSet, EmitError> {
    eprintln!("INPUT {label} complete_program={program:#?} complete_options={options:#?}");
    let actual = emit_with_basic_owned_sources(program, options);
    eprintln!("RAW {label} complete_artifacts_or_error={actual:#?}");
    if let Err(error) = &actual {
        eprintln!("RAW {label} complete_cause={:?}", error.source());
    }
    actual
}
fn public519_outputs_program() -> Program {
    let mut p = basic(
        vec![
            node(1, owned(SourceOwner::Primary, &["Value"], None)),
            node(2, owned(named("Ref"), &["Value"], None)),
        ],
        &[("Value", 1, ScalarType::String)],
    );
    p.extra_targets.push(NamedTargetProgram {
        name: "Named".into(),
        target: SchemaNode::group(
            "Named",
            vec![SchemaNode::scalar("Value", ScalarType::String)],
        ),
        root: scope(&[("Value", 2, ScalarType::String)]),
    });
    p
}
fn public519_source(artifact: &ArtifactSet) -> &str {
    std::str::from_utf8(
        &artifact
            .files()
            .iter()
            .find(|f| f.path.as_str() == "src/lib.rs")
            .expect("captured artifact contains library")
            .contents,
    )
    .expect("generated library is UTF-8")
}
#[test]
fn public_basic_owned_family_has_exact_borrowed_policy_api_and_nonpublic_hooks() {
    let p = public519_outputs_program();
    let options = public519_options("public_basic_api");
    let private = super::super::owned_basic::render_checked_basic_owned_source(&p);
    eprintln!("INPUT A519-01 complete_program={p:#?} options={options:#?}");
    eprintln!("RAW A519-01 complete_private_renderer={private:#?}");
    let actual = public519_result("A519-01", &p, &options);
    let files = actual.expect("complete artifacts retained");
    let source = public519_source(&files);
    let private = private.expect("complete private renderer retained");
    assert_eq!(files.len(), 2);
    assert!(
        source.contains(
            "pub use codegen_runtime::{BasicNamedSourceInput, ExecutionContext, Instance,"
        )
    );
    assert!(source.contains("pub struct NamedOutput"));
    assert!(source.contains("pub struct ExecutionOutputs"));
    assert!(source.contains("pub enum SelectedTargetOutput"));
    assert!(source.contains("pub fn execute_with_basic_owned_sources<'name, 'src>("));
    assert!(
        source.contains("source: &'src Instance, supplied: &[BasicNamedSourceInput<'name, 'src>],")
    );
    assert!(
        source.contains("execution: Option<&ExecutionContext<'src>>, limits: OwnedSourceLimits,")
    );
    assert!(source.contains("pub fn execute_selected_with_basic_owned_sources<'name, 'src>("));
    assert!(source.contains("selection: TargetSelection<'_>,"));
    assert!(
        source.contains("execute_checked_private_owned_basic(source, supplied, execution, limits)")
    );
    assert!(source.contains("execute_selected_checked_private_owned_basic(source, supplied, execution, limits, selection)"));
    assert!(source.contains("pub(crate) fn execute_private_owned_basic"));
    assert!(source.contains("pub(crate) fn execute_checked_private_owned_basic"));
    assert!(!source.contains("pub fn private_checked_"));
    assert!(!source.contains("pub static PRIVATE_CHECKED"));
    assert_eq!(
        source
            .matches("BasicSourceRoots::try_from_named_inputs(")
            .count(),
        private
            .matches("BasicSourceRoots::try_from_named_inputs(")
            .count()
    );
    assert_eq!(
        source.matches("BasicSourceRoots::try_new(").count(),
        private.matches("BasicSourceRoots::try_new(").count()
    );
}
#[test]
fn public_basic_owned_family_preserves_ordinary_artifact_bytes_and_closed_gate() {
    let p = basic(
        vec![node(
            1,
            Expression::Const {
                value: Value::String("ordinary".into()),
            },
        )],
        &[("Value", 1, ScalarType::String)],
    );
    let options = public519_options("ordinary_conservation");
    eprintln!("INPUT A519-02 complete_program={p:#?} complete_options={options:#?}");
    let old = emit(&p, &options);
    eprintln!("RAW A519-02 ordinary_artifacts={old:#?}");
    let reference_source = render_source(&p);
    eprintln!("RAW A519-02 ordinary_source={reference_source:#?}");
    let reference_manifest = render_manifest(&options);
    eprintln!("RAW A519-02 ordinary_manifest={reference_manifest:?}");
    let owned = public519_outputs_program();
    eprintln!("INPUT A519-02 owned_complete_program={owned:#?} complete_options={options:#?}");
    let closed = emit(&owned, &options);
    eprintln!("RAW A519-02 ordinary_owned_refusal={closed:#?}");
    let files = old.expect("complete old artifact result retained");
    assert_eq!(
        public519_source(&files),
        reference_source.expect("complete source result retained")
    );
    assert_eq!(
        files
            .files()
            .iter()
            .find(|f| f.path.as_str() == "Cargo.toml")
            .unwrap()
            .contents,
        reference_manifest.as_bytes()
    );
    assert!(matches!(
        closed,
        Err(EmitError::InvalidProgram(
            ProgramValidationError::OwnedSource { .. }
        ))
    ));
    assert!(!public519_source(&files).contains("execute_with_basic_owned_sources"));
}
#[test]
fn public_basic_owned_family_keeps_structure_package_then_intersection_precedence() {
    use codegen::{
        OwnedSourceDiagnostic as D, OwnedSourceDiagnosticLocation as L,
        OwnedSourceDiagnosticOwner as W, OwnedSourceDiagnosticRequest as Q,
        OwnedSourceNamespace as N, OwnedSourceStructureReason as R,
    };
    let malformed = basic(
        vec![node(1, owned(named("Unknown"), &["Value"], None))],
        &[("Value", 1, ScalarType::String)],
    );
    let mut unsupported = basic(
        vec![
            node(
                1,
                Expression::Const {
                    value: Value::String("plain".into()),
                },
            ),
            node(
                2,
                Expression::ValueMap {
                    input: 1,
                    input_type: None,
                    table: vec![(
                        Value::String("plain".into()),
                        Value::String("mapped".into()),
                    )],
                    default: None,
                },
            ),
        ],
        &[("Value", 2, ScalarType::String)],
    );
    unsupported.extra_sources.clear();
    let bad = public519_options("not a package");
    let good = public519_options("valid_basic_package");
    let first = public519_result("A519-03 malformed+bad-package", &malformed, &bad);
    let second = public519_result("A519-03 unsupported+bad-package", &unsupported, &bad);
    let third = public519_result("A519-03 unsupported+valid-package", &unsupported, &good);
    let structure = D::Structure {
        location: L::Expression {
            namespace: N::Main,
            node: 1,
            scope: None,
        },
        request: Q::Field {
            owner: W::Named {
                name: "Unknown".into(),
            },
            path: path(&["Value"]),
            frame: None,
        },
        reason: R::UnknownOwner,
    };
    eprintln!(
        "EXPECTED A519-03 structure={structure:#?} package={:?} target ValueMap admission=Ok",
        bad.package_name
    );
    match first {
        Err(EmitError::InvalidProgram(ProgramValidationError::OwnedSource { diagnostic })) => {
            assert_eq!(*diagnostic, structure)
        }
        other => panic!("complete captured first result: {other:?}"),
    }
    match second {
        Err(EmitError::InvalidPackageName(name)) => assert_eq!(name, bad.package_name),
        other => panic!("complete captured second result: {other:?}"),
    }
    let files = third.expect("complete successful target ValueMap artifact result retained");
    let source = public519_source(&files);
    assert_eq!(files.len(), 2);
    assert_eq!(
        source
            .matches("let input = expression_1(context)?;")
            .count(),
        1
    );
    assert!(source.contains("Ok(value_map(input,"));
    assert!(source.contains("pub fn execute_with_basic_owned_sources"));
}
#[test]
fn public_basic_owned_family_keeps_single_checked_context_and_failure_before_targets() {
    let p = composed_ordered_failure_program();
    let options = public519_options("public_basic_failure_order");
    let actual = public519_result("A519-04", &p, &options);
    let files = actual.expect("complete generated artifacts retained");
    let source = public519_source(&files);
    let shared = &source[source
        .rfind("\nfn execute_private_checked_basic_context(")
        .unwrap()..];
    assert!(
        shared
            .find("evaluate_basic_failure_rules(context)?;")
            .unwrap()
            < shared.find("if let Some(selected) = selected").unwrap()
    );
    assert_eq!(
        shared
            .matches("evaluate_basic_failure_rules(context)?;")
            .count(),
        1
    );
    let selected = &source[source
        .find("pub(crate) fn execute_selected_checked_private_owned_basic")
        .unwrap()..];
    assert!(
        selected.find("RuntimeError::UnknownTarget").unwrap()
            < selected.find("private_checked_factory_reached();").unwrap()
    );
    let wrappers = &source[source
        .find("pub fn execute_with_basic_owned_sources")
        .unwrap()..];
    assert!(!wrappers.contains("try_from_named_inputs"));
    assert!(!wrappers.contains("try_new"));
    assert_eq!(
        wrappers
            .matches("execute_checked_private_owned_basic(")
            .count(),
        1
    );
    assert_eq!(
        wrappers
            .matches("execute_selected_checked_private_owned_basic(")
            .count(),
        1
    );
}

fn public519_host_programs() -> Vec<(&'static str, Program, &'static str)> {
    let outputs = public519_outputs_program();
    let ordered = composed_ordered_failure_program();
    let mut no_rule = basic(
        vec![
            node(1, owned(SourceOwner::Primary, &["Value"], None)),
            node(71, Expression::Raise { message: None }),
        ],
        &[("Value", 1, ScalarType::String)],
    );
    no_rule.extra_targets.push(NamedTargetProgram {
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
    vec![
        ("outputs", outputs.clone(), PUBLIC519_OUTPUTS),
        ("selection", outputs, PUBLIC519_SELECTION),
        ("failure", ordered, PUBLIC519_FAILURE),
        ("raise", no_rule, PUBLIC519_RAISE),
    ]
}
#[test]
#[ignore = "requires independently accepted public proposal, qualified prerequisites, fresh evidence and serial shared generated-host admission"]
fn public_basic_owned_generated_external_api_retains_all_twelve_reached_results() {
    let root = PathBuf::from(
        std::env::var_os("FERRULE_PUBLIC_RUST_BASIC_EVIDENCE_ROOT")
            .expect("fresh public evidence root"),
    );
    let runtime = PathBuf::from(
        std::env::var_os("FERRULE_PRIVATE_RUST_BASIC_RUNTIME").expect("qualified runtime path"),
    );
    assert!(root.is_absolute() && runtime.is_absolute());
    assert!(
        std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR").is_some(),
        "one separately admitted compatible target"
    );
    fs::create_dir(&root).expect("fresh namespace, preserve failures");
    fs::create_dir(root.join("src")).unwrap();
    fs::create_dir(root.join("tests")).unwrap();
    let mut libraries = String::new();
    let mut checks = String::new();
    for (name, program, check) in public519_host_programs() {
        let options = public519_options("ferrule_public_basic_519");
        retained_file(
            &root.join(format!("{name}.Program.original")),
            format!("{program:#?}\n").as_bytes(),
        );
        retained_file(
            &root.join(format!("{name}.Options.original")),
            format!("{options:#?}\n").as_bytes(),
        );
        let actual = public519_result(name, &program, &options);
        retained_file(
            &root.join(format!("{name}.renderer-result.original")),
            format!("{actual:#?}\n").as_bytes(),
        );
        let files = actual.expect("full Program/options/return retained before any qualification");
        let source = public519_source(&files);
        retained_file(
            &root.join(format!("{name}.generated.original.rs")),
            source.as_bytes(),
        );
        libraries.push_str(&format!("pub mod {name} {{\n{source}\n}}\n"));
        checks.push_str(&format!("mod {name} {{\nuse ferrule_public_basic_519::{name}::*;\n{PUBLIC519_HOST_COMMON}\n{check}\n}}\n"));
    }
    retained_file(&root.join("src/lib.rs"), libraries.as_bytes());
    retained_file(&root.join("tests/public_api.rs"), checks.as_bytes());
    let options = Options {
        package_name: "ferrule_public_basic_519".into(),
        runtime_dependency: RuntimeDependency::Path(
            runtime
                .to_str()
                .expect("qualified Unicode runtime path")
                .into(),
        ),
    };
    retained_file(
        &root.join("Cargo.toml"),
        render_manifest(&options).as_bytes(),
    );
    retained_file(
        &root.join("COMMAND.original"),
        b"cargo test --offline --test public_api -- --nocapture --test-threads=1\n",
    );
    let actual = Command::new("cargo")
        .args([
            "test",
            "--offline",
            "--test",
            "public_api",
            "--",
            "--nocapture",
            "--test-threads=1",
        ])
        .current_dir(&root)
        .generated_host_output(&root);
    match &actual {
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
            format!("{error:#?}\n").as_bytes(),
        ),
    }
    eprintln!("RAW PUBLIC519 complete compiler/host return={actual:#?}");
    let output = actual.expect("complete returned or reached-error originals retained");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for id in [
        "G519-P01", "G519-P02", "G519-P03", "G519-P04", "G519-P05", "G519-P06", "G519-P07",
        "G519-P08", "G519-P09", "G519-P10", "G519-P11", "G519-P12",
    ] {
        let marker = format!("RAW {id} FULL_RETURN=");
        assert_eq!(
            stdout.matches(&marker).count() + stderr.matches(&marker).count(),
            1,
            "{id}"
        );
    }
}
const PUBLIC519_HOST_COMMON: &str = r#"
use codegen_runtime::{field, group, repeated, scalar, Value, OwnedSourceError, OwnedSourceLimitKind};
use std::error::Error;
fn text(s:&str)->Instance { scalar(Value::String(s.into())) }
fn fields(items:impl IntoIterator<Item=(&'static str,Instance)>)->Instance { group(items.into_iter().map(|(n,v)|field(n,v))) }
fn source()->Instance { fields([("Value",text("P.root")),("Ref",fields([("Value",text("P.collision"))]))]) }
fn reference()->Instance { fields([("Value",text("N.root"))]) }
fn limits()->OwnedSourceLimits { OwnedSourceLimits { max_roots:16,max_active_frames:8,max_path_segments:16,max_metadata_bytes:1048576,max_walk_contexts:64 } }
fn capture(label:&str,p:&Instance,supplied:&[BasicNamedSourceInput<'_, '_>],limits:OwnedSourceLimits,selection:Option<TargetSelection<'_>>) {
    eprintln!("INPUT {label} primary={p:#?} supplied_full={:#?} execution=None limits={limits:#?} selection={selection:#?}", supplied.iter().map(|i|(i.name,i.instance)).collect::<Vec<_>>());
}
fn raw<T:std::fmt::Debug>(label:&str,result:Result<T,RuntimeError>)->Result<T,RuntimeError> {
    eprintln!("RAW {label} FULL_RETURN={result:#?}");
    if let Err(error)=&result { eprintln!("RAW {label} FULL_CAUSE={:?} DISPLAY={error}", error.source()); }
    result
}
fn all(label:&str,p:&Instance,supplied:&[BasicNamedSourceInput<'_, '_>],limits:OwnedSourceLimits)->Result<ExecutionOutputs,RuntimeError> {
    capture(label,p,supplied,limits,None); raw(label,execute_with_basic_owned_sources(p,supplied,None,limits))
}
fn selected(label:&str,p:&Instance,supplied:&[BasicNamedSourceInput<'_, '_>],limits:OwnedSourceLimits,selection:TargetSelection<'_>)->Result<SelectedTargetOutput,RuntimeError> {
    capture(label,p,supplied,limits,Some(selection)); raw(label,execute_selected_with_basic_owned_sources(p,supplied,None,limits,selection))
}
fn ordered_input()->Instance { fields([("Value",text("P.root")),("Rows",repeated([
    fields([("Value",text("r1")),("Flag",scalar(Value::Bool(false)))]),
    fields([("Value",text("r2")),("Flag",scalar(Value::Bool(true)))]),
]))]) }
"#;
const PUBLIC519_OUTPUTS: &str = r#"
#[test] fn run() {
 let p=source();let r=reference();let supplied=[BasicNamedSourceInput{name:"Ref",instance:&r}];let policy=limits();
 let a=all("G519-P01",&p,&supplied,policy);
 let b=selected("G519-P02",&p,&supplied,policy,TargetSelection::Primary);
 let c=selected("G519-P03",&p,&supplied,policy,TargetSelection::Named("Named"));
 assert_eq!(a,Ok(ExecutionOutputs{primary:fields([("Value",text("P.root"))]),extras:vec![NamedOutput{name:"Named".into(),instance:fields([("Value",text("N.root"))])}]}));
 assert_eq!(b,Ok(SelectedTargetOutput::Primary(fields([("Value",text("P.root"))]))));
 assert_eq!(c,Ok(SelectedTargetOutput::Named(NamedOutput{name:"Named".into(),instance:fields([("Value",text("N.root"))])})));
}
"#;
const PUBLIC519_SELECTION: &str = r#"
#[test] fn run() {
 let p=source();let r=reference();let invalid=[BasicNamedSourceInput{name:"X",instance:&r}];let mut zero=limits();zero.max_roots=0;
 let a=selected("G519-P04",&p,&invalid,zero,TargetSelection::Named("missing"));
 let b=all("G519-P05",&p,&invalid,limits());
 let c=selected("G519-P06",&p,&[],zero,TargetSelection::Primary);
 assert_eq!(a,Err(RuntimeError::UnknownTarget{name:"missing".into()}));
 assert_eq!(b,Err(RuntimeError::UnexpectedNamedSource{name:"X".into()}));
 assert_eq!(c,Err(RuntimeError::OwnedSource(OwnedSourceError::MetadataLimit{kind:OwnedSourceLimitKind::Roots,used:0,requested:6,max:0})));
}
"#;
const PUBLIC519_FAILURE: &str = r#"
#[test] fn run() {
 let p=ordered_input();let policy=limits();
 let a=all("G519-P07",&p,&[],policy);
 let b=selected("G519-P08",&p,&[],policy,TargetSelection::Primary);
 let c=selected("G519-P09",&p,&[],policy,TargetSelection::Named("Bad"));
 assert_eq!(a,Err(RuntimeError::MappingFailure{rule:2,message:Some("r2".into())}));
 assert_eq!(b,Err(RuntimeError::MappingFailure{rule:2,message:Some("r2".into())}));
 assert_eq!(c,Err(RuntimeError::MappingFailure{rule:2,message:Some("r2".into())}));
}
"#;
const PUBLIC519_RAISE: &str = r#"
#[test] fn run() {
 let p=source();let policy=limits();
 let a=selected("G519-P10",&p,&[],policy,TargetSelection::Primary);
 let b=selected("G519-P11",&p,&[],policy,TargetSelection::Named("Bad"));
 let c=all("G519-P12",&p,&[],policy);
 assert_eq!(a,Ok(SelectedTargetOutput::Primary(fields([("Value",text("P.root"))]))));
 assert_eq!(b,Err(RuntimeError::MappingException{node:71,message:None}));assert_eq!(c,Err(RuntimeError::MappingException{node:71,message:None}));
}
"#;

// A caller literal must never capture the late checked-import rewrite.
const PUBLIC519_LITERAL_IMPORT: &str = "use codegen_runtime::{BasicNamedSourceDeclaration, BasicNamedSourceInput, BasicNamedSourceMode};";
fn public519_literal_program() -> Program {
    basic(
        vec![node(
            1,
            Expression::Const {
                value: Value::String(PUBLIC519_LITERAL_IMPORT.into()),
            },
        )],
        &[("Value", 1, ScalarType::String)],
    )
}
#[test]
fn public_basic_owned_literal_import_is_preserved_and_checked_import_is_unique() {
    let p = public519_literal_program();
    let options = public519_options("public_basic_literal_519");
    eprintln!(
        "INPUT A519-05 complete_program={p:#?} complete_options={options:#?} expected_literal={PUBLIC519_LITERAL_IMPORT:?}"
    );
    let private = super::super::owned_basic::render_checked_basic_owned_source(&p);
    eprintln!("RAW A519-05 complete_private_source_result={private:#?}");
    let actual = public519_result("A519-05", &p, &options);
    let private = private.expect("complete private renderer retained before propagation");
    let files = actual.expect("complete public artifacts or error retained before propagation");
    let source = public519_source(&files);
    let private_start = private.find("fn expression_1(").unwrap();
    let private_end =
        private[private_start..].find("\n}\n\n").unwrap() + private_start + "\n}\n\n".len();
    let public_start = source.find("fn expression_1(").unwrap();
    let public_end =
        source[public_start..].find("\n}\n\n").unwrap() + public_start + "\n}\n\n".len();
    eprintln!(
        "RAW A519-05 complete_private_const_function={:?} complete_public_const_function={:?}",
        &private[private_start..private_end],
        &source[public_start..public_end]
    );
    assert_eq!(
        &source[public_start..public_end],
        &private[private_start..private_end]
    );
    assert!(source[public_start..public_end].contains(&rust_string(PUBLIC519_LITERAL_IMPORT)));
    assert_eq!(
        source
            .lines()
            .filter(|line| *line == PUBLIC519_LITERAL_IMPORT)
            .count(),
        0
    );
    assert_eq!(
        source
            .lines()
            .filter(|line| *line
                == "use codegen_runtime::{BasicNamedSourceDeclaration, BasicNamedSourceMode};")
            .count(),
        1
    );
    assert_eq!(
        source
            .matches("pub use codegen_runtime::{BasicNamedSourceInput, ExecutionContext, Instance,")
            .count(),
        1
    );
    assert!(source.contains("pub struct NamedOutput"));
    assert!(source.contains("pub struct ExecutionOutputs"));
    assert!(source.contains("pub enum SelectedTargetOutput"));
}
#[test]
#[ignore = "requires independently accepted import successor and separately admitted external public-consumer compilation"]
fn public_basic_owned_literal_import_external_consumer_retains_exact_value() {
    let root = PathBuf::from(
        std::env::var_os("FERRULE_PUBLIC_RUST_BASIC_LITERAL_EVIDENCE_ROOT")
            .expect("fresh literal evidence root"),
    );
    let runtime = PathBuf::from(
        std::env::var_os("FERRULE_PRIVATE_RUST_BASIC_RUNTIME").expect("qualified runtime"),
    );
    assert!(root.is_absolute() && runtime.is_absolute());
    assert!(
        std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR").is_some(),
        "one separately admitted compatible target"
    );
    fs::create_dir(&root).expect("fresh namespace, preserve reached originals");
    fs::create_dir(root.join("src")).unwrap();
    fs::create_dir(root.join("tests")).unwrap();
    let p = public519_literal_program();
    let options = Options {
        package_name: "public_basic_literal_519".into(),
        runtime_dependency: RuntimeDependency::Path(
            runtime.to_str().expect("qualified Unicode path").into(),
        ),
    };
    retained_file(
        &root.join("Program.complete.original"),
        format!("{p:#?}\n").as_bytes(),
    );
    retained_file(
        &root.join("Options.complete.original"),
        format!("{options:#?}\n").as_bytes(),
    );
    let actual = public519_result("G519-P13/P14 generation", &p, &options);
    retained_file(
        &root.join("renderer-result.complete.original"),
        format!("{actual:#?}\n").as_bytes(),
    );
    let files = actual.expect("complete input and artifact/result retained before propagation");
    for file in files.files() {
        retained_file(&root.join(file.path.as_str()), &file.contents);
    }
    retained_file(
        &root.join("tests/public_literal.rs"),
        PUBLIC519_LITERAL_EXTERNAL.as_bytes(),
    );
    retained_file(
        &root.join("COMMAND.complete.original"),
        b"cargo test --offline --test public_literal -- --nocapture --test-threads=1\n",
    );
    let reached = Command::new("cargo")
        .args([
            "test",
            "--offline",
            "--test",
            "public_literal",
            "--",
            "--nocapture",
            "--test-threads=1",
        ])
        .current_dir(&root)
        .generated_host_output(&root);
    match &reached {
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
            format!("{error:#?}\n").as_bytes(),
        ),
    }
    eprintln!("RAW G519-P13/P14 complete external compilation/consumer return={reached:#?}");
    let output = reached.expect("complete raw streams/status or reached error retained");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for id in ["G519-P13", "G519-P14"] {
        let marker = format!("RAW {id} FULL_RETURN=");
        assert_eq!(
            stdout.matches(&marker).count() + stderr.matches(&marker).count(),
            1,
            "{id}"
        );
    }
}
const PUBLIC519_LITERAL_EXTERNAL: &str = r#"
use public_basic_literal_519::{BasicNamedSourceInput, ExecutionOutputs, Instance, OwnedSourceLimits,
    RuntimeError, SelectedTargetOutput, TargetSelection,
    execute_with_basic_owned_sources, execute_selected_with_basic_owned_sources};
use codegen_runtime::{field, group, scalar, Value};
use std::error::Error;
const EXPECTED_LITERAL:&str="use codegen_runtime::{BasicNamedSourceDeclaration, BasicNamedSourceInput, BasicNamedSourceMode};";
fn raw<T:std::fmt::Debug>(id:&str,actual:Result<T,RuntimeError>)->Result<T,RuntimeError> {
    eprintln!("RAW {id} FULL_RETURN={actual:#?}");
    if let Err(error)=&actual { eprintln!("RAW {id} FULL_CAUSE={:?}",error.source()); }
    actual
}
#[test] fn exact_literal() {
    let primary:Instance=group([]);
    let supplied:[BasicNamedSourceInput<'_, '_>;0]=[];
    let limits=OwnedSourceLimits{max_roots:16,max_active_frames:8,max_path_segments:16,max_metadata_bytes:1048576,max_walk_contexts:64};
    eprintln!("INPUT G519-P13 primary={primary:#?} supplied={supplied:#?} execution=None limits={limits:#?} selection=all expected_literal={EXPECTED_LITERAL:?}");
    let all=raw("G519-P13",execute_with_basic_owned_sources(&primary,&supplied,None,limits));
    let selection=TargetSelection::Primary;
    eprintln!("INPUT G519-P14 primary={primary:#?} supplied={supplied:#?} execution=None limits={limits:#?} selection={selection:#?} expected_literal={EXPECTED_LITERAL:?}");
    let selected=raw("G519-P14",execute_selected_with_basic_owned_sources(&primary,&supplied,None,limits,selection));
    let expected=||group([field("Value",scalar(Value::String(EXPECTED_LITERAL.into())))]);
    assert_eq!(all,Ok(ExecutionOutputs{primary:expected(),extras:Vec::new()}));
    assert_eq!(selected,Ok(SelectedTargetOutput::Primary(expected())));
}
"#;

// #586 source controls: full typed refusals are retained before comparisons.
fn failure586_program() -> Program {
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
            driver: codegen::SourceIteration::new(path(&["Rows"])),
        }),
    });
    program
}

fn failure586_expression(program: &mut Program, id: u32, expression: Expression) {
    program
        .expressions
        .iter_mut()
        .find(|node| node.id == id)
        .unwrap()
        .expression = expression;
}

fn failure586_dynamic(index: usize, node: Option<u32>) -> ProgramValidationError {
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

fn failure586_retained(node: u32) -> ProgramValidationError {
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

fn public582_map(
    input: u32,
    input_type: Option<ScalarType>,
    table: Vec<(Value, Value)>,
    default: Option<Value>,
) -> Expression {
    Expression::ValueMap {
        input,
        input_type,
        table,
        default,
    }
}
fn public582_matrix_program() -> Program {
    let mut p = basic(
        vec![
            node(
                1,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: None,
                },
            ),
            node(
                2,
                public582_map(
                    1,
                    None,
                    vec![
                        (
                            Value::String("P.root".into()),
                            Value::String("first".into()),
                        ),
                        (
                            Value::String("P.root".into()),
                            Value::String("second".into()),
                        ),
                    ],
                    Some(Value::String("miss".into())),
                ),
            ),
            node(3, owned(SourceOwner::Primary, &["Value"], None)),
            node(
                4,
                public582_map(
                    3,
                    None,
                    vec![(
                        Value::String("P.root".into()),
                        Value::String("owned".into()),
                    )],
                    None,
                ),
            ),
            node(
                5,
                owned(SourceOwner::Primary, &["Details", "Counter"], None),
            ),
            node(
                6,
                public582_map(
                    5,
                    None,
                    vec![(Value::Null, Value::String("absent".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            node(
                7,
                Expression::Const {
                    value: Value::json_null(),
                },
            ),
            node(
                8,
                public582_map(
                    7,
                    None,
                    vec![
                        (Value::Null, Value::String("wrong".into())),
                        (Value::json_null(), Value::String("json".into())),
                    ],
                    None,
                ),
            ),
            node(
                9,
                public582_map(
                    7,
                    Some(ScalarType::Int),
                    vec![
                        (Value::Null, Value::String("null".into())),
                        (Value::json_null(), Value::String("wrong".into())),
                    ],
                    None,
                ),
            ),
            node(10, Expression::Const { value: Value::Null }),
            node(
                11,
                public582_map(
                    10,
                    None,
                    vec![
                        (Value::Null, Value::String("null".into())),
                        (Value::json_null(), Value::String("wrong".into())),
                    ],
                    None,
                ),
            ),
            node(
                12,
                Expression::Const {
                    value: Value::String("miss".into()),
                },
            ),
            node(
                13,
                public582_map(12, None, vec![], Some(Value::String("fallback".into()))),
            ),
            node(14, public582_map(12, None, vec![], None)),
            node(
                15,
                public582_map(
                    14,
                    None,
                    vec![(Value::Null, Value::String("no-default-null".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            node(16, public582_map(12, None, vec![], Some(Value::Null))),
            node(
                17,
                public582_map(
                    16,
                    None,
                    vec![(Value::Null, Value::String("literal-null".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            node(
                18,
                Expression::Const {
                    value: Value::XmlNil(ir::XmlNil),
                },
            ),
            node(
                19,
                public582_map(
                    18,
                    None,
                    vec![
                        (Value::Null, Value::String("wrong".into())),
                        (Value::XmlNil(ir::XmlNil), Value::String("xmlnil".into())),
                    ],
                    None,
                ),
            ),
            node(
                20,
                Expression::Const {
                    value: Value::String(" 7 ".into()),
                },
            ),
            node(
                21,
                public582_map(
                    20,
                    Some(ScalarType::Int),
                    vec![(Value::Int(7), Value::String("trim-int".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            node(
                22,
                Expression::Const {
                    value: Value::Float(7.0),
                },
            ),
            node(
                23,
                public582_map(
                    22,
                    Some(ScalarType::Int),
                    vec![(Value::Int(7), Value::String("float-int".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            node(
                24,
                Expression::Const {
                    value: Value::Int(7),
                },
            ),
            node(
                25,
                public582_map(
                    24,
                    Some(ScalarType::Float),
                    vec![(Value::Float(7.0), Value::String("int-float".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            node(
                26,
                Expression::Const {
                    value: Value::String("bad".into()),
                },
            ),
            node(
                27,
                public582_map(
                    26,
                    Some(ScalarType::Int),
                    vec![(
                        Value::String("bad".into()),
                        Value::String("retained-string".into()),
                    )],
                    Some(Value::String("wrong".into())),
                ),
            ),
            node(
                28,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            node(
                29,
                public582_map(
                    28,
                    Some(ScalarType::String),
                    vec![(
                        Value::String("true".into()),
                        Value::String("bool-string".into()),
                    )],
                    Some(Value::String("wrong".into())),
                ),
            ),
            node(
                30,
                Expression::Const {
                    value: Value::String(" true ".into()),
                },
            ),
            node(
                31,
                public582_map(
                    30,
                    Some(ScalarType::Bool),
                    vec![(Value::Bool(true), Value::String("trim-bool".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            node(
                32,
                Expression::Const {
                    value: Value::Int(7),
                },
            ),
            node(
                33,
                public582_map(
                    32,
                    None,
                    vec![(Value::Float(7.0), Value::String("wrong".into()))],
                    Some(Value::String("tagged-int".into())),
                ),
            ),
            node(
                34,
                Expression::Const {
                    value: Value::Float(1.5),
                },
            ),
            node(
                35,
                public582_map(
                    34,
                    Some(ScalarType::Int),
                    vec![(Value::Float(1.5), Value::String("retained-float".into()))],
                    Some(Value::String("wrong".into())),
                ),
            ),
            node(
                36,
                Expression::SourceField {
                    path: path(&["Ref", "Value"]),
                    frame: Some(Vec::new()),
                },
            ),
            node(
                37,
                public582_map(
                    36,
                    None,
                    vec![(
                        Value::String("P.collision".into()),
                        Value::String("primary-frame".into()),
                    )],
                    Some(Value::String("wrong".into())),
                ),
            ),
        ],
        &[
            ("Legacy", 2, ScalarType::String),
            ("Owned", 4, ScalarType::String),
            ("Absent", 6, ScalarType::String),
            ("UntypedJson", 8, ScalarType::String),
            ("TypedJson", 9, ScalarType::String),
            ("Null", 11, ScalarType::String),
            ("Default", 13, ScalarType::String),
            ("NoDefault", 15, ScalarType::String),
            ("LiteralNull", 17, ScalarType::String),
            ("XmlNil", 19, ScalarType::String),
            ("TrimInt", 21, ScalarType::String),
            ("FloatInt", 23, ScalarType::String),
            ("IntFloat", 25, ScalarType::String),
            ("RetainedString", 27, ScalarType::String),
            ("BoolString", 29, ScalarType::String),
            ("TrimBool", 31, ScalarType::String),
            ("TaggedInt", 33, ScalarType::String),
            ("RetainedFloat", 35, ScalarType::String),
            ("EmptyFrame", 37, ScalarType::String),
        ],
    );
    // The flat repeated input creates a real collection at the empty frame.
    p.root.repeating = true;
    p.root.iteration = Some(IterationPlan::source(Vec::new()));
    p.target = p.target.repeating();
    p
}
fn public582_lazy_program() -> Program {
    let mut p = basic(
        vec![
            node(
                1,
                Expression::RuntimeParameter {
                    name: "flag".into(),
                    ty: ScalarType::Bool,
                },
            ),
            node(2, owned(SourceOwner::Primary, &["Value"], None)),
            node(
                3,
                public582_map(
                    2,
                    None,
                    vec![(
                        Value::String("P.root".into()),
                        Value::String("primary".into()),
                    )],
                    Some(Value::String("fallback".into())),
                ),
            ),
            node(4, owned(named("Ref"), &["Value"], None)),
            node(
                5,
                public582_map(
                    4,
                    None,
                    vec![(
                        Value::String("N.root".into()),
                        Value::String("named".into()),
                    )],
                    Some(Value::String("fallback".into())),
                ),
            ),
            node(
                6,
                Expression::If {
                    condition: 1,
                    then: 3,
                    else_: 5,
                },
            ),
            node(
                70,
                Expression::Const {
                    value: Value::String("map input".into()),
                },
            ),
            node(71, Expression::Raise { message: Some(70) }),
            node(
                72,
                public582_map(71, None, Vec::new(), Some(Value::String("fallback".into()))),
            ),
            node(
                81,
                Expression::RuntimeParameter {
                    name: "value".into(),
                    ty: ScalarType::Int,
                },
            ),
            node(
                82,
                public582_map(
                    81,
                    None,
                    vec![(Value::Int(7), Value::String("seven".into()))],
                    Some(Value::String("fallback".into())),
                ),
            ),
        ],
        &[("Value", 6, ScalarType::String)],
    );
    for (name, expression) in [("Raise", 72), ("Missing", 82)] {
        p.extra_targets.push(NamedTargetProgram {
            name: name.into(),
            target: SchemaNode::scalar(name, ScalarType::String),
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: None,
                construction: TargetConstruction::Scalar {
                    expression,
                    target_domain: ScalarType::String.into(),
                },
                bindings: Vec::new(),
                children: Vec::new(),
            },
        });
    }
    p
}

fn public582_map_body(source: &str, id: u32) -> &str {
    let start = source
        .find(&format!("fn expression_{id}("))
        .expect("represented map method");
    let end = source[start..]
        .find("\n}\n\n")
        .expect("complete represented map method")
        + start
        + "\n}\n\n".len();
    &source[start..end]
}
#[test]
fn public_basic_owned_value_maps_preserve_ordered_tags_defaults_and_one_input_assignment() {
    let p = public582_matrix_program();
    let files = public519_result(
        "A582-matrix",
        &p,
        &public519_options("public_value_map_matrix"),
    )
    .expect("full public artifacts retained");
    let source = public519_source(&files);
    assert_eq!(
        public582_map_body(source, 2)
            .matches("let input = expression_1(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 4)
            .matches("let input = expression_3(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 6)
            .matches("let input = expression_5(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 8)
            .matches("let input = expression_7(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 9)
            .matches("let input = expression_7(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 11)
            .matches("let input = expression_10(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 13)
            .matches("let input = expression_12(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 14)
            .matches("let input = expression_12(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 15)
            .matches("let input = expression_14(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 16)
            .matches("let input = expression_12(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 17)
            .matches("let input = expression_16(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 19)
            .matches("let input = expression_18(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 21)
            .matches("let input = expression_20(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 23)
            .matches("let input = expression_22(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 25)
            .matches("let input = expression_24(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 27)
            .matches("let input = expression_26(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 29)
            .matches("let input = expression_28(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 31)
            .matches("let input = expression_30(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 33)
            .matches("let input = expression_32(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 35)
            .matches("let input = expression_34(context)?;")
            .count(),
        1
    );
    assert_eq!(
        public582_map_body(source, 37)
            .matches("let input = expression_36(context)?;")
            .count(),
        1
    );
}
#[test]
fn public_basic_owned_value_maps_keep_lazy_owners_and_input_errors_before_literal_defaults() {
    let p = public582_lazy_program();
    let files = public519_result(
        "A582-lazy-errors",
        &p,
        &public519_options("public_value_map_lazy"),
    )
    .expect("full public artifacts retained");
    let source = public519_source(&files);
    for (id, input) in [(3, 2), (5, 4), (72, 71), (82, 81)] {
        assert_eq!(
            public582_map_body(source, id)
                .matches(&format!("let input = expression_{input}(context)?;"))
                .count(),
            1
        );
    }
    assert!(source.contains("BasicNamedSourceMode::Optional"));
}
fn public582_host_programs() -> Vec<(&'static str, Program, &'static str)> {
    vec![
        ("matrix", public582_matrix_program(), PUBLIC582_MATRIX),
        ("lazy", public582_lazy_program(), PUBLIC582_LAZY_ERRORS),
    ]
}
#[test]
#[ignore = "requires independently accepted public proposal, qualified prerequisites, fresh evidence and serial shared generated-host admission"]
fn public_basic_owned_generated_value_maps_retain_all_seven_reached_results() {
    let root = PathBuf::from(
        std::env::var_os("FERRULE_PUBLIC_RUST_VALUE_MAP_EVIDENCE_ROOT")
            .expect("fresh public evidence root"),
    );
    let runtime = PathBuf::from(
        std::env::var_os("FERRULE_PRIVATE_RUST_BASIC_RUNTIME").expect("qualified runtime path"),
    );
    assert!(root.is_absolute() && runtime.is_absolute());
    assert!(
        std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR").is_some(),
        "one separately admitted compatible target"
    );
    fs::create_dir(&root).expect("fresh namespace, preserve failures");
    fs::create_dir(root.join("src")).unwrap();
    fs::create_dir(root.join("tests")).unwrap();
    let mut libraries = String::new();
    let mut checks = String::new();
    for (name, program, check) in public582_host_programs() {
        let options = public519_options("ferrule_public_value_maps_582");
        retained_file(
            &root.join(format!("{name}.Program.original")),
            format!("{program:#?}\n").as_bytes(),
        );
        retained_file(
            &root.join(format!("{name}.Options.original")),
            format!("{options:#?}\n").as_bytes(),
        );
        let actual = public519_result(name, &program, &options);
        retained_file(
            &root.join(format!("{name}.renderer-result.original")),
            format!("{actual:#?}\n").as_bytes(),
        );
        let files = actual.expect("full Program/options/return retained before any qualification");
        let source = public519_source(&files);
        retained_file(
            &root.join(format!("{name}.generated.original.rs")),
            source.as_bytes(),
        );
        libraries.push_str(&format!("pub mod {name} {{\n{source}\n}}\n"));
        checks.push_str(&format!("mod {name} {{\nuse ferrule_public_value_maps_582::{name}::*;\n{PUBLIC582_HOST_COMMON}\n{check}\n}}\n"));
    }
    retained_file(&root.join("src/lib.rs"), libraries.as_bytes());
    retained_file(&root.join("tests/public_value_maps.rs"), checks.as_bytes());
    let options = Options {
        package_name: "ferrule_public_value_maps_582".into(),
        runtime_dependency: RuntimeDependency::Path(
            runtime
                .to_str()
                .expect("qualified Unicode runtime path")
                .into(),
        ),
    };
    retained_file(
        &root.join("Cargo.toml"),
        render_manifest(&options).as_bytes(),
    );
    retained_file(
        &root.join("COMMAND.original"),
        b"cargo test --offline --jobs 2 --test public_value_maps -- --nocapture --test-threads=1\n",
    );
    let actual = Command::new("cargo")
        .args([
            "test",
            "--offline",
            "--jobs",
            "2",
            "--test",
            "public_value_maps",
            "--",
            "--nocapture",
            "--test-threads=1",
        ])
        .current_dir(&root)
        .generated_host_output(&root);
    match &actual {
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
            format!("{error:#?}\n").as_bytes(),
        ),
    }
    eprintln!("RAW PUBLIC582 complete compiler/host return={actual:#?}");
    let output = actual.expect("complete returned or reached-error originals retained");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for id in [
        "VM582-M01",
        "VM582-M02",
        "VM582-L03",
        "VM582-L04",
        "VM582-L05",
        "VM582-L06",
        "VM582-L07",
    ] {
        let marker = format!("RAW {id} FULL_RETURN=");
        assert_eq!(
            stdout.matches(&marker).count() + stderr.matches(&marker).count(),
            1,
            "{id}"
        );
    }
}
const PUBLIC582_HOST_COMMON: &str = r#"
use codegen_runtime::{field, group, repeated, scalar, Value, RuntimeParameters, OwnedSourceError};
use std::error::Error;
fn text(value:&str)->Instance { scalar(Value::String(value.into())) }
fn fields(items:impl IntoIterator<Item=(&'static str,Instance)>)->Instance { group(items.into_iter().map(|(name,value)|field(name,value))) }
fn source()->Instance { fields([("Value",text("P.root")),("Ref",fields([("Value",text("P.collision"))]))]) }
fn limits()->OwnedSourceLimits { OwnedSourceLimits { max_roots:16,max_active_frames:8,max_path_segments:16,max_metadata_bytes:1048576,max_walk_contexts:64 } }
fn capture(label:&str,p:&Instance,supplied:&[BasicNamedSourceInput<'_, '_>],execution:Option<(&ExecutionContext<'_>,&RuntimeParameters)>,policy:OwnedSourceLimits,selection:Option<TargetSelection<'_>>) {
 let execution_snapshot=execution.map(|(_,parameters)|("mapping.test","mapping.test",None::<&str>,parameters));
 eprintln!("INPUT {label} FULL_PRIMARY={p:#?} FULL_SUPPLIED={:#?} FULL_EXECUTION={execution_snapshot:#?} FULL_LIMITS={policy:#?} FULL_SELECTION={selection:#?}",supplied.iter().map(|v|(v.name,v.instance)).collect::<Vec<_>>());
}
fn raw<T:std::fmt::Debug>(label:&str,actual:Result<T,RuntimeError>)->Result<T,RuntimeError> {
 eprintln!("RAW {label} FULL_RETURN={actual:#?}");if let Err(error)=&actual {eprintln!("RAW {label} FULL_CAUSE={:?} DISPLAY={error}",error.source());} actual
}
fn all<'src>(label:&str,p:&'src Instance,supplied:&[BasicNamedSourceInput<'_, 'src>],execution:Option<(&ExecutionContext<'src>,&RuntimeParameters)>,policy:OwnedSourceLimits)->Result<ExecutionOutputs,RuntimeError> {
 capture(label,p,supplied,execution,policy,None);raw(label,execute_with_basic_owned_sources(p,supplied,execution.map(|(context,_)|context),policy))
}
fn selected<'src>(label:&str,p:&'src Instance,supplied:&[BasicNamedSourceInput<'_, 'src>],execution:Option<(&ExecutionContext<'src>,&RuntimeParameters)>,policy:OwnedSourceLimits,selection:TargetSelection<'_>)->Result<SelectedTargetOutput,RuntimeError> {
 capture(label,p,supplied,execution,policy,Some(selection));raw(label,execute_selected_with_basic_owned_sources(p,supplied,execution.map(|(context,_)|context),policy,selection))
}
"#;
const PUBLIC582_MATRIX: &str = r#"
#[test] fn run() {
 let p=repeated([source()]);let policy=limits();
 let a=all("VM582-M01",&p,&[],None,policy);
 let b=selected("VM582-M02",&p,&[],None,policy,TargetSelection::Primary);
 let expected=repeated([fields([("Legacy",text("first")),("Owned",text("owned")),("Absent",text("absent")),("UntypedJson",text("json")),("TypedJson",text("null")),("Null",text("null")),("Default",text("fallback")),("NoDefault",text("no-default-null")),("LiteralNull",text("literal-null")),("XmlNil",text("xmlnil")),("TrimInt",text("trim-int")),("FloatInt",text("float-int")),("IntFloat",text("int-float")),("RetainedString",text("retained-string")),("BoolString",text("bool-string")),("TrimBool",text("trim-bool")),("TaggedInt",text("tagged-int")),("RetainedFloat",text("retained-float")),("EmptyFrame",text("primary-frame"))])]);
 assert_eq!(a,Ok(ExecutionOutputs{primary:expected.clone(),extras:Vec::new()}));
 assert_eq!(b,Ok(SelectedTargetOutput::Primary(expected)));
}
"#;
const PUBLIC582_LAZY_ERRORS: &str = r#"
#[test] fn run() {
 let p=source();let reference=fields([("Value",text("N.root"))]);let policy=limits();
 let mut t=RuntimeParameters::new();let inserted=t.insert("flag",Value::Bool(true));eprintln!("RAW TRUE_PARAMETER_INSERT={inserted:#?}");inserted.unwrap();
 let mut f=RuntimeParameters::new();let inserted=f.insert("flag",Value::Bool(false));eprintln!("RAW FALSE_PARAMETER_INSERT={inserted:#?}");inserted.unwrap();
 let yes=ExecutionContext::new(std::path::Path::new("mapping.test")).with_parameters(&t);let no=ExecutionContext::new(std::path::Path::new("mapping.test")).with_parameters(&f);
 let a=selected("VM582-L03",&p,&[],Some((&yes,&t)),policy,TargetSelection::Primary);
 let b=selected("VM582-L04",&p,&[],Some((&no,&f)),policy,TargetSelection::Primary);
 let supplied=[BasicNamedSourceInput{name:"Ref",instance:&reference}];let c=selected("VM582-L05",&p,&supplied,Some((&no,&f)),policy,TargetSelection::Primary);
 let d=selected("VM582-L06",&p,&[],None,policy,TargetSelection::Named("Raise"));
 let parameters=RuntimeParameters::new();let execution=ExecutionContext::new(std::path::Path::new("mapping.test")).with_parameters(&parameters);
 let e=selected("VM582-L07",&p,&[],Some((&execution,&parameters)),policy,TargetSelection::Named("Missing"));
 assert_eq!(a,Ok(SelectedTargetOutput::Primary(fields([("Value",text("primary"))]))));
 assert_eq!(b,Err(RuntimeError::OwnedSourceAtNode{node:4,source:OwnedSourceError::SourceUnavailable{name:"Ref".into()}}));
 assert_eq!(c,Ok(SelectedTargetOutput::Primary(fields([("Value",text("named"))]))));
 assert_eq!(d,Err(RuntimeError::MappingException{node:71,message:Some("map input".into())}));
 assert_eq!(e,Err(RuntimeError::MissingRuntimeParameter{node:81,name:"value".into()}));
}
"#;

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

fn root_pin632_program(
    frame: Option<&[&str]>,
    primary_repeating: bool,
    named_repeating: bool,
) -> Program {
    let mut program = public519_outputs_program();
    program.extra_sources.retain(|source| source.name == "Ref");
    program.source.repeating = primary_repeating;
    program.extra_sources[0].source.repeating = named_repeating;
    program.expressions[0].expression = owned(SourceOwner::Primary, &["Value"], frame);
    program.expressions[1].expression = owned(named("Ref"), &["Value"], frame);
    program
}
fn root_pin632_repeated_program() -> Program {
    let mut program = root_pin632_program(Some(&[]), true, true);
    program.target.repeating = true;
    program.root.repeating = true;
    program.root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: Vec::new(),
    }));
    program.extra_targets[0].target.repeating = true;
    program.extra_targets[0].root.repeating = true;
    program.extra_targets[0].root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: named("Ref"),
        path: Vec::new(),
    }));
    program
}
fn root_pin632_rows_program() -> Program {
    let mut program = root_pin632_program(Some(&[]), false, false);
    program.expressions.extend([
        node(3, owned(SourceOwner::Primary, &["Rows", "Value"], None)),
        node(4, owned(named("Ref"), &["Rows", "Value"], None)),
        node(5, position(SourceOwner::Primary, None)),
        node(6, position(named("Ref"), None)),
    ]);
    let fields = || {
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Row", ScalarType::String),
            SchemaNode::scalar("Position", ScalarType::Int),
        ]
    };
    program.target = SchemaNode::group("Output", fields()).repeating();
    program.root = scope(&[
        ("Value", 1, ScalarType::String),
        ("Row", 3, ScalarType::String),
        ("Position", 5, ScalarType::Int),
    ]);
    program.root.repeating = true;
    program.root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: SourceOwner::Primary,
        path: path(&["Rows"]),
    }));
    program.extra_targets[0].target = SchemaNode::group("Named", fields()).repeating();
    program.extra_targets[0].root = scope(&[
        ("Value", 2, ScalarType::String),
        ("Row", 4, ScalarType::String),
        ("Position", 6, ScalarType::Int),
    ]);
    program.extra_targets[0].root.repeating = true;
    program.extra_targets[0].root.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner: named("Ref"),
        path: path(&["Rows"]),
    }));
    program
}
fn root_pin632_host_programs() -> Vec<(&'static str, Program, &'static str)> {
    vec![
        (
            "none",
            root_pin632_program(None, false, false),
            ROOT_PIN632_NONE,
        ),
        (
            "empty",
            root_pin632_program(Some(&[]), false, false),
            ROOT_PIN632_EMPTY,
        ),
        (
            "primary_repeating",
            root_pin632_program(Some(&[]), true, false),
            ROOT_PIN632_PRIMARY_DECLARED_REPEATING,
        ),
        (
            "named_repeating",
            root_pin632_program(Some(&[]), false, true),
            ROOT_PIN632_NAMED_DECLARED_REPEATING,
        ),
        (
            "repeated",
            root_pin632_repeated_program(),
            ROOT_PIN632_REPEATED,
        ),
        ("rows", root_pin632_rows_program(), ROOT_PIN632_ROWS),
    ]
}

#[test]
fn public_root_pin632_emits_exact_owner_declaration_repetition_without_registry_flags() {
    for (primary_repeating, named_repeating) in
        [(false, false), (true, false), (false, true), (true, true)]
    {
        let program = root_pin632_program(Some(&[]), primary_repeating, named_repeating);
        let files = public519_result(
            "R632-schema",
            &program,
            &public519_options("root_pin_schema_632"),
        )
        .expect("complete public artifacts retained");
        let source = public519_source(&files);
        assert!(source.contains(&format!(
            "SourceOwnerRef::Primary => {},",
            !primary_repeating
        )));
        assert!(source.contains(&format!("\"Ref\" => {},", !named_repeating)));
        assert_eq!(
            source
                .matches("fn basic_declared_root_nonrepeating(")
                .count(),
            1
        );
        assert_eq!(
            source.matches("context.owned_field_with_root_pin(").count(),
            2
        );
        assert!(source.contains("context.owned_field_with_root_pin(SourceOwnerRef::Primary, &[\"Value\"], Some(&[][..]), basic_declared_root_nonrepeating(SourceOwnerRef::Primary))"));
        assert!(source.contains("context.owned_field_with_root_pin(SourceOwnerRef::Named(\"Ref\"), &[\"Value\"], Some(&[][..]), basic_declared_root_nonrepeating(SourceOwnerRef::Named(\"Ref\")))"));
        assert!(!source.contains("declared_nonrepeating: Vec"));
    }
    let program = basic(
        vec![node(1, owned(named(" Ref 雪 "), &["Value"], Some(&[])))],
        &[("Value", 1, ScalarType::String)],
    );
    let source = private_source("R632-literal", &program);
    assert!(source.contains("\" Ref \\u{96ea} \" => true,"));
    assert!(
        source.contains(
            "basic_declared_root_nonrepeating(SourceOwnerRef::Named(\" Ref \\u{96ea} \"))"
        )
    );
}

#[test]
fn public_root_pin632_leaves_none_nonempty_positions_and_unused_policy_exact() {
    let mut program = root_pin632_program(None, false, false);
    program
        .expressions
        .push(node(99, owned(named("Ref"), &["Value"], Some(&[]))));
    let files = public519_result(
        "R632-unused",
        &program,
        &public519_options("root_pin_unused_632"),
    )
    .expect("full unused-leaf result retained");
    let source = public519_source(&files);
    assert!(!source.contains("basic_declared_root_nonrepeating"));
    assert!(!source.contains("owned_field_with_root_pin"));
    assert!(!source.contains("fn expression_99("));
    for (expression, expected) in [
        (
            owned(SourceOwner::Primary, &["Value"], None),
            "context.owned_field(SourceOwnerRef::Primary, &[\"Value\"], None)",
        ),
        (
            owned(named("Ref"), &["Rows", "Value"], Some(&["Rows"])),
            "context.owned_field(SourceOwnerRef::Named(\"Ref\"), &[\"Rows\", \"Value\"], Some(&[\"Rows\"][..]))",
        ),
        (
            position(SourceOwner::Primary, Some(&[])),
            "{ let position = context.owned_position(SourceOwnerRef::Primary, Some(&[][..]))?; Ok(Value::Int(position as i64)) }",
        ),
    ] {
        assert_eq!(
            super::super::owned_basic::render_owned_access(&expression),
            Some(expected.into())
        );
    }
}

#[test]
fn public_root_pin632_six_programs_keep_complete_public_source_and_scalar_union_child() {
    let programs = root_pin632_host_programs();
    assert_eq!(programs.len(), 6);
    for (name, program, _) in programs {
        let files = public519_result(name, &program, &public519_options("root_pin_public_632"))
            .expect("full generated result retained");
        let source = public519_source(&files);
        assert!(source.contains("pub fn execute_with_basic_owned_sources"));
        assert!(source.contains("pub fn execute_selected_with_basic_owned_sources"));
        assert_eq!(
            source.matches("context.owned_field_with_root_pin(").count(),
            if name == "none" { 0 } else { 2 }
        );
    }
    let mut program = root_pin632_program(Some(&[]), false, false);
    let union = ir::ScalarTypeSet::new([ScalarType::String, ScalarType::Int]).unwrap();
    program.source = SchemaNode::group("Input", vec![SchemaNode::scalar_union("Value", union)]);
    program.extra_sources[0].source = program.source.clone();
    let files = public519_result(
        "R632-union",
        &program,
        &public519_options("root_pin_union_632"),
    )
    .expect("existing scalar union field admission retained");
    let source = public519_source(&files);
    assert!(source.contains("SourceOwnerRef::Primary => true,"));
    assert!(source.contains("\"Ref\" => true,"));
}

#[test]
#[ignore = "requires independently accepted public proposal, qualified prerequisites, fresh evidence and serial shared generated-host admission"]
fn public_root_pin632_generated_six_programs_retain_all_twenty_two_results() {
    let root = PathBuf::from(
        std::env::var_os("FERRULE_PUBLIC_RUST_ROOT_PIN632_EVIDENCE_ROOT")
            .expect("fresh public evidence root"),
    );
    let runtime = PathBuf::from(
        std::env::var_os("FERRULE_PRIVATE_RUST_BASIC_RUNTIME").expect("qualified runtime path"),
    );
    assert!(root.is_absolute() && runtime.is_absolute());
    assert!(
        std::env::var_os("FERRULE_CODEGEN_HOST_TARGET_DIR").is_some(),
        "one separately admitted compatible target"
    );
    fs::create_dir(&root).expect("fresh namespace, preserve failures");
    fs::create_dir(root.join("src")).unwrap();
    fs::create_dir(root.join("tests")).unwrap();
    let mut libraries = String::new();
    let mut checks = String::new();
    for (name, program, check) in root_pin632_host_programs() {
        let options = public519_options("ferrule_public_root_pins_632");
        retained_file(
            &root.join(format!("{name}.Program.original")),
            format!("{program:#?}\n").as_bytes(),
        );
        retained_file(
            &root.join(format!("{name}.Options.original")),
            format!("{options:#?}\n").as_bytes(),
        );
        let actual = public519_result(name, &program, &options);
        retained_file(
            &root.join(format!("{name}.renderer-result.original")),
            format!("{actual:#?}\n").as_bytes(),
        );
        let files = actual.expect("full Program/options/return retained before any qualification");
        let source = public519_source(&files);
        retained_file(
            &root.join(format!("{name}.generated.original.rs")),
            source.as_bytes(),
        );
        libraries.push_str(&format!("pub mod {name} {{\n{source}\n}}\n"));
        checks.push_str(&format!("mod {name} {{\nuse ferrule_public_root_pins_632::{name}::*;\n{PUBLIC519_HOST_COMMON}\n{check}\n}}\n"));
    }
    retained_file(&root.join("src/lib.rs"), libraries.as_bytes());
    retained_file(&root.join("tests/root_pins.rs"), checks.as_bytes());
    let options = Options {
        package_name: "ferrule_public_root_pins_632".into(),
        runtime_dependency: RuntimeDependency::Path(
            runtime
                .to_str()
                .expect("qualified Unicode runtime path")
                .into(),
        ),
    };
    retained_file(
        &root.join("Cargo.toml"),
        render_manifest(&options).as_bytes(),
    );
    retained_file(
        &root.join("COMMAND.original"),
        b"cargo test --offline --jobs 1 --test root_pins -- --nocapture --test-threads=1\n",
    );
    let actual = Command::new("cargo")
        .args([
            "test",
            "--offline",
            "--jobs",
            "1",
            "--test",
            "root_pins",
            "--",
            "--nocapture",
            "--test-threads=1",
        ])
        .current_dir(&root)
        .generated_host_output(&root);
    match &actual {
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
            format!("{error:#?}\n").as_bytes(),
        ),
    }
    eprintln!("RAW ROOT_PIN632 complete compiler/host return={actual:#?}");
    let output = actual.expect("complete returned or reached-error originals retained");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    for id in [
        "R632-G01", "R632-G02", "R632-G03", "R632-G04", "R632-G05", "R632-G06", "R632-G07",
        "R632-G08", "R632-G09", "R632-G10", "R632-G11", "R632-G12", "R632-G13", "R632-G14",
        "R632-G15", "R632-G16", "R632-G17", "R632-G18", "R632-G19", "R632-G20", "R632-G21",
        "R632-G22",
    ] {
        let marker = format!("RAW {id} FULL_RETURN=");
        assert_eq!(
            stdout.matches(&marker).count() + stderr.matches(&marker).count(),
            1,
            "{id}"
        );
    }
}
const ROOT_PIN632_NONE: &str = r#"
#[test] fn run() {
 let p=source();let r=reference();let supplied=[BasicNamedSourceInput{name:"Ref",instance:&r}];let policy=limits();
 let a=all("R632-G01",&p,&supplied,policy);
 let b=selected("R632-G02",&p,&supplied,policy,TargetSelection::Primary);
 let c=selected("R632-G03",&p,&supplied,policy,TargetSelection::Named("Named"));
 assert_eq!(a,Ok(ExecutionOutputs{primary:fields([("Value",text("P.root"))]),extras:vec![NamedOutput{name:"Named".into(),instance:fields([("Value",text("N.root"))])}]}));
 assert_eq!(b,Ok(SelectedTargetOutput::Primary(fields([("Value",text("P.root"))]))));
 assert_eq!(c,Ok(SelectedTargetOutput::Named(NamedOutput{name:"Named".into(),instance:fields([("Value",text("N.root"))])})));
}
"#;
const ROOT_PIN632_EMPTY: &str = r#"
#[test] fn run() {
 let p=source();let r=reference();let supplied=[BasicNamedSourceInput{name:"Ref",instance:&r}];let policy=limits();
 let a=all("R632-G04",&p,&supplied,policy);
 let b=selected("R632-G05",&p,&supplied,policy,TargetSelection::Primary);
 let c=selected("R632-G06",&p,&supplied,policy,TargetSelection::Named("Named"));
 assert_eq!(a,Ok(ExecutionOutputs{primary:fields([("Value",text("P.root"))]),extras:vec![NamedOutput{name:"Named".into(),instance:fields([("Value",text("N.root"))])}]}));
 assert_eq!(b,Ok(SelectedTargetOutput::Primary(fields([("Value",text("P.root"))]))));
 assert_eq!(c,Ok(SelectedTargetOutput::Named(NamedOutput{name:"Named".into(),instance:fields([("Value",text("N.root"))])})));
 let p=repeated([source()]);let r=repeated([reference()]);let supplied=[BasicNamedSourceInput{name:"Ref",instance:&r}];
 let a=selected("R632-G11",&p,&supplied,policy,TargetSelection::Primary);
 let b=selected("R632-G12",&p,&supplied,policy,TargetSelection::Named("Named"));
 assert_eq!(a,Err(RuntimeError::OwnedSourceAtNode{node:1,source:OwnedSourceError::InactiveSourceFrame{owner:codegen_runtime::OwnedSourceIdentity::Primary,collection:Some(Vec::new())}}));
 assert_eq!(b,Err(RuntimeError::OwnedSourceAtNode{node:2,source:OwnedSourceError::InactiveSourceFrame{owner:codegen_runtime::OwnedSourceIdentity::Named("Ref".into()),collection:Some(Vec::new())}}));
 let p=Instance::DocumentSet(Vec::new());let doc=selected("R632-G19",&p,&[],policy,TargetSelection::Primary);
 assert_eq!(doc,Err(RuntimeError::OwnedSourceAtNode{node:1,source:OwnedSourceError::UnsupportedSourceKind{owner:codegen_runtime::OwnedSourceIdentity::Primary,path:vec!["Value".into()],found:codegen_runtime::InstanceKind::DocumentSet}}));
 let p=Instance::MappedSequence(Vec::new());let mapped=selected("R632-G20",&p,&[],policy,TargetSelection::Primary);
 assert_eq!(mapped,Err(RuntimeError::OwnedSourceAtNode{node:1,source:OwnedSourceError::UnsupportedSourceKind{owner:codegen_runtime::OwnedSourceIdentity::Primary,path:vec!["Value".into()],found:codegen_runtime::InstanceKind::MappedSequence}}));
 let p=source();let missing=selected("R632-G21",&p,&[],policy,TargetSelection::Named("Named"));
 assert_eq!(missing,Err(RuntimeError::OwnedSourceAtNode{node:2,source:OwnedSourceError::SourceUnavailable{name:"Ref".into()}}));
 let mut zero=policy;zero.max_roots=0;let unknown=selected("R632-G22",&p,&[],zero,TargetSelection::Named("unknown"));
 assert_eq!(unknown,Err(RuntimeError::UnknownTarget{name:"unknown".into()}));
}
"#;
const ROOT_PIN632_PRIMARY_DECLARED_REPEATING: &str = r#"
#[test] fn run() {
 let p=source();let r=reference();let supplied=[BasicNamedSourceInput{name:"Ref",instance:&r}];let policy=limits();
 let a=selected("R632-G07",&p,&supplied,policy,TargetSelection::Primary);
 let b=selected("R632-G08",&p,&supplied,policy,TargetSelection::Named("Named"));
 assert_eq!(a,Err(RuntimeError::OwnedSourceAtNode{node:1,source:OwnedSourceError::InactiveSourceFrame{owner:codegen_runtime::OwnedSourceIdentity::Primary,collection:Some(Vec::new())}}));
 assert_eq!(b,Ok(SelectedTargetOutput::Named(NamedOutput{name:"Named".into(),instance:fields([("Value",text("N.root"))])})));
}
"#;
const ROOT_PIN632_NAMED_DECLARED_REPEATING: &str = r#"
#[test] fn run() {
 let p=source();let r=reference();let supplied=[BasicNamedSourceInput{name:"Ref",instance:&r}];let policy=limits();
 let a=selected("R632-G09",&p,&supplied,policy,TargetSelection::Primary);
 let b=selected("R632-G10",&p,&supplied,policy,TargetSelection::Named("Named"));
 assert_eq!(a,Ok(SelectedTargetOutput::Primary(fields([("Value",text("P.root"))]))));
 assert_eq!(b,Err(RuntimeError::OwnedSourceAtNode{node:2,source:OwnedSourceError::InactiveSourceFrame{owner:codegen_runtime::OwnedSourceIdentity::Named("Ref".into()),collection:Some(Vec::new())}}));
}
"#;
const ROOT_PIN632_REPEATED: &str = r#"
#[test] fn run() {
 let p=repeated([fields([("Value",text("P1"))]),fields([("Value",text("P2"))])]);
 let r=repeated([fields([("Value",text("N1"))]),fields([("Value",text("N2"))]),fields([("Value",text("N3"))])]);
 let supplied=[BasicNamedSourceInput{name:"Ref",instance:&r}];let policy=limits();
 let a=all("R632-G13",&p,&supplied,policy);
 let b=selected("R632-G14",&p,&supplied,policy,TargetSelection::Primary);
 let c=selected("R632-G15",&p,&supplied,policy,TargetSelection::Named("Named"));
 let primary=repeated([fields([("Value",text("P1"))]),fields([("Value",text("P2"))])]);
 let named=repeated([fields([("Value",text("N1"))]),fields([("Value",text("N2"))]),fields([("Value",text("N3"))])]);
 assert_eq!(a,Ok(ExecutionOutputs{primary:primary.clone(),extras:vec![NamedOutput{name:"Named".into(),instance:named.clone()}]}));
 assert_eq!(b,Ok(SelectedTargetOutput::Primary(primary)));
 assert_eq!(c,Ok(SelectedTargetOutput::Named(NamedOutput{name:"Named".into(),instance:named})));
}
"#;
const ROOT_PIN632_ROWS: &str = r#"
#[test] fn run() {
 let p=fields([("Value",text("P.root")),("Ref",fields([("Value",text("P.collision"))])),("Rows",repeated([fields([("Value",text("P1"))]),fields([("Value",text("P2"))])]))]);
 let r=fields([("Value",text("N.root")),("Rows",repeated([fields([("Value",text("N1"))]),fields([("Value",text("N2"))]),fields([("Value",text("N3"))])]))]);
 let supplied=[BasicNamedSourceInput{name:"Ref",instance:&r}];let policy=limits();
 let a=all("R632-G16",&p,&supplied,policy);
 let b=selected("R632-G17",&p,&supplied,policy,TargetSelection::Primary);
 let c=selected("R632-G18",&p,&supplied,policy,TargetSelection::Named("Named"));
 let output=|root,rows:&[&str]|repeated(rows.iter().enumerate().map(|(i,row)|fields([("Value",text(root)),("Row",text(row)),("Position",scalar(Value::Int((i+1)as i64)))])));
 let primary=output("P.root",&["P1","P2"]);let named=output("N.root",&["N1","N2","N3"]);
 assert_eq!(a,Ok(ExecutionOutputs{primary:primary.clone(),extras:vec![NamedOutput{name:"Named".into(),instance:named.clone()}]}));
 assert_eq!(b,Ok(SelectedTargetOutput::Primary(primary)));
 assert_eq!(c,Ok(SelectedTargetOutput::Named(NamedOutput{name:"Named".into(),instance:named})));
}
"#;

#[test]
fn public_basic_owned_leaf_nodes_cover_exact_pins_and_unsigned_boundaries() {
    for owner in [SourceOwner::Primary, named("Ref")] {
        for pin in [None, Some(&[][..]), Some(&["Rows"][..])] {
            let parts = if pin == Some(&["Rows"][..]) {
                &["Rows", "Value"][..]
            } else {
                &["Value"][..]
            };
            for (id, expression, ty) in [
                (0, owned(owner.clone(), parts, pin), ScalarType::String),
                (u32::MAX, position(owner.clone(), pin), ScalarType::Int),
            ] {
                let program = basic(vec![node(id, expression)], &[("Value", id, ty)]);
                let files = public519_result(
                    "N650-pins",
                    &program,
                    &public519_options("public_owned_leaf_nodes"),
                )
                .expect("complete program and artifacts retained");
                let public = public582_map_body(public519_source(&files), id);
                eprintln!(
                    "RAW N650-pins owner={owner:?} pin={pin:?} id={id} complete_leaf={public:?}"
                );
                assert!(public.contains("(|| -> Result<Value, RuntimeError> {"));
                assert!(public.contains(&format!(".with_owned_source_node({id})")));
                assert_eq!(public.matches(".with_owned_source_node(").count(), 1);
                if ty == ScalarType::Int {
                    let early = public.find("context.owned_position(").unwrap();
                    let annotation = public.find(".map_err(|error|").unwrap();
                    assert!(
                        early < annotation
                            && public[early..annotation]
                                .contains("?; Ok(Value::Int(position as i64))")
                    );
                }
            }
        }
    }
}

#[test]
fn public_basic_owned_nodes_leave_enclosing_calls_maps_and_lazy_branches_exact() {
    let mut program = public582_lazy_program();
    program.expressions.push(node(
        7,
        Expression::Call {
            function: codegen::ScalarFunction::Upper,
            args: vec![6],
        },
    ));
    program.root.bindings[0].expression = 7;
    let private = super::super::owned_basic::render_checked_basic_owned_source(&program);
    eprintln!("RAW N650-nested complete_private={private:#?}");
    let files = public519_result(
        "N650-nested",
        &program,
        &public519_options("public_owned_nested_nodes"),
    )
    .expect("complete public artifacts retained");
    let private = private.expect("complete private rendering retained");
    let public = public519_source(&files);
    for id in [1, 3, 5, 6, 7, 70, 71, 72, 81, 82] {
        assert_eq!(
            public582_map_body(public, id),
            public582_map_body(&private, id),
            "enclosing/nonowned expression {id}"
        );
    }
    for id in [2, 4] {
        assert!(public582_map_body(public, id).contains(&format!(".with_owned_source_node({id})")));
        assert!(!public582_map_body(&private, id).contains("with_owned_source_node"));
    }
    assert_eq!(public.matches(".with_owned_source_node(").count(), 2);
    assert!(public582_map_body(public, 6).contains("expression_3(context)"));
    assert!(public582_map_body(public, 6).contains("expression_5(context)"));
}

#[test]
fn public_basic_owned_nodes_preserve_private_leaf_bytes_and_unannotated_setup_walks() {
    let program = root_pin632_rows_program();
    let prepared = super::super::owned_basic::render_basic_owned_source(&program);
    let checked = super::super::owned_basic::render_checked_basic_owned_source(&program);
    eprintln!("RAW N650-private complete_prepared={prepared:#?} complete_checked={checked:#?}");
    let files = public519_result(
        "N650-private",
        &program,
        &public519_options("public_owned_private_preservation"),
    )
    .expect("complete public artifacts retained");
    let prepared = prepared.expect("complete prepared rendering retained");
    let checked = checked.expect("complete checked rendering retained");
    assert!(!prepared.contains("with_owned_source_node"));
    assert!(!checked.contains("with_owned_source_node"));
    let public = public519_source(&files);
    for name in [
        "scope_root",
        "scope_extra_0",
        "execute_private_checked_basic_context",
    ] {
        let marker = format!("fn {name}(");
        let start = public.find(&marker).unwrap();
        let end = public[start..].find("\n}\n").unwrap() + start + "\n}\n".len();
        let private_start = checked.find(&marker).unwrap();
        let private_end =
            checked[private_start..].find("\n}\n").unwrap() + private_start + "\n}\n".len();
        assert_eq!(&public[start..end], &checked[private_start..private_end]);
        assert!(!public[start..end].contains("with_owned_source_node"));
    }
    let golden = "fn expression_5(context: &BasicScopeContext<'_>) -> Result<Value, RuntimeError> {\n    #[cfg(test)]\n    eprintln!(\"RAW PRIVATE_BASIC_EXPRESSION module={} node=5\", module_path!());\n    let _ = context;\n    { let position = context.owned_position(SourceOwnerRef::Primary, None)?; Ok(Value::Int(position as i64)) }\n}\n\n";
    assert_eq!(public582_map_body(&prepared, 5), golden);
    assert_eq!(public582_map_body(&checked, 5), golden);
}
