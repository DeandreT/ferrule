use super::*;

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
            driver: codegen::SourceIteration::new(path(&["Rows"])).into(),
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
            driver: codegen::SourceIteration::new(path(&["Rows"])).into(),
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
#[ignore = "builds and runs generated Rust consumers with cargo"]
fn private_source_failure_generated_corpus_retains_complete_raw_before_assertions() {
    let run = HostRun::new(
        "FERRULE_PRIVATE_RUST_FAILURE_EVIDENCE_ROOT",
        "private_rust_failure",
    );
    let (root, runtime) = (run.root.clone(), run.runtime.clone());
    assert!(root.is_absolute() && runtime.is_absolute());
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

pub(super) const FAILURE_COMMON: &str = r#"
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
