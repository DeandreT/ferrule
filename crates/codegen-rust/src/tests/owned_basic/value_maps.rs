use super::*;

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
pub(super) fn public582_lazy_program() -> Program {
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

pub(super) fn public582_map_body(source: &str, id: u32) -> &str {
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
#[ignore = "builds and runs generated Rust consumers with cargo"]
fn public_basic_owned_generated_value_maps_retain_all_seven_reached_results() {
    let run = HostRun::new(
        "FERRULE_PUBLIC_RUST_VALUE_MAP_EVIDENCE_ROOT",
        "public_rust_value_map",
    );
    let (root, runtime) = (run.root.clone(), run.runtime.clone());
    assert!(root.is_absolute() && runtime.is_absolute());
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
