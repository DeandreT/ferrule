use super::*;

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
#[ignore = "builds and runs generated Rust consumers with cargo"]
fn public_basic_owned_generated_external_api_retains_all_twelve_reached_results() {
    let run = HostRun::new(
        "FERRULE_PUBLIC_RUST_BASIC_EVIDENCE_ROOT",
        "public_rust_basic",
    );
    let (root, runtime) = (run.root.clone(), run.runtime.clone());
    assert!(root.is_absolute() && runtime.is_absolute());
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
pub(super) const PUBLIC519_HOST_COMMON: &str = r#"
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
pub(super) const PUBLIC519_LITERAL_IMPORT: &str = "use codegen_runtime::{BasicNamedSourceDeclaration, BasicNamedSourceInput, BasicNamedSourceMode};";
pub(super) fn public519_literal_program() -> Program {
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
#[ignore = "builds and runs generated Rust consumers with cargo"]
fn public_basic_owned_literal_import_external_consumer_retains_exact_value() {
    let run = HostRun::new(
        "FERRULE_PUBLIC_RUST_BASIC_LITERAL_EVIDENCE_ROOT",
        "public_rust_basic_literal",
    );
    let (root, runtime) = (run.root.clone(), run.runtime.clone());
    assert!(root.is_absolute() && runtime.is_absolute());
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
