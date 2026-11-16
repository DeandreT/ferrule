use super::*;

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
        driver: codegen::SourceIteration::new(Vec::new()).into(),
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
#[ignore = "builds and runs generated Rust consumers with cargo"]
fn private_checked_generated_inputs_capture_complete_results_before_comparisons() {
    let run = HostRun::new(
        "FERRULE_PRIVATE_RUST_CHECKED_EVIDENCE_ROOT",
        "private_rust_checked",
    );
    let (root, runtime) = (run.root.clone(), run.runtime.clone());
    assert!(root.is_absolute() && runtime.is_absolute());
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

pub(super) const CHECKED_RUNNER_COMMON: &str = r#"
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
