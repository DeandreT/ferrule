use super::*;

// Checked normalization and ordinary global source rules are composed only in
// the private prototype. Existing prepared, checked, and failure controls above
// remain unchanged; these interaction cases have a separately frozen contract.
pub(super) fn composed_ordered_failure_program() -> Program {
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
#[ignore = "builds and runs generated Rust consumers with cargo"]
fn private_checked_global_failure_interactions_retain_full_inputs_returns_and_trace() {
    let run = HostRun::new(
        "FERRULE_PRIVATE_RUST_COMPOSED_EVIDENCE_ROOT",
        "private_rust_composed",
    );
    let (root, runtime) = (run.root.clone(), run.runtime.clone());
    assert!(root.is_absolute() && runtime.is_absolute());
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
