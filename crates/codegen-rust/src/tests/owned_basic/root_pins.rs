use super::*;

pub(super) fn root_pin632_program(
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
pub(super) fn root_pin632_rows_program() -> Program {
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
#[ignore = "builds and runs generated Rust consumers with cargo"]
fn public_root_pin632_generated_six_programs_retain_all_twenty_two_results() {
    let run = HostRun::new(
        "FERRULE_PUBLIC_RUST_ROOT_PIN632_EVIDENCE_ROOT",
        "public_rust_root_pin632",
    );
    let (root, runtime) = (run.root.clone(), run.runtime.clone());
    assert!(root.is_absolute() && runtime.is_absolute());
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

// Explicit public streaming-filter controls; old control-free tests stay exact.
