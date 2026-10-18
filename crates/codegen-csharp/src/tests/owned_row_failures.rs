// Explicit row-failure permission; the old companions remain closed and exact.
fn row_809_program() -> Program {
    let mut p = basic(
        vec![
            expression(1, Expression::Raise { message: None }),
            expression(10, field(SourceOwner::Primary, &["Rows", "Value"], None)),
            expression(11, position(SourceOwner::Primary, Some(&["Rows"]))),
            expression(
                12,
                Expression::Const {
                    value: Value::Int(2),
                },
            ),
            expression(
                13,
                Expression::Call {
                    function: ScalarFunction::Equal,
                    args: vec![11, 12],
                },
            ),
            expression(
                20,
                Expression::Const {
                    value: Value::String("later rule".into()),
                },
            ),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.failure_rules = vec![
        source_failure(&["Rows"], codegen::FailureSelection::WhenTrue(13), Some(10)),
        source_failure(&["Rows"], codegen::FailureSelection::All, Some(20)),
    ];
    add_target(
        &mut p,
        "Named",
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p
}

fn retain_public_row_failure_consumer(label: &str, program: &Program, body: &str) -> String {
    let artifacts =
        crate::emit_with_basic_owned_row_failures(program).expect("explicit primary failures emit");
    let source = String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .unwrap();
    let host = PUBLIC_BASIC_CONSUMER
        .replace("__ERROR_SNAPSHOT__", HOST_ERROR_SNAPSHOT)
        .replace("__BODY__", body);
    if let Some(base) = std::env::var_os("FERRULE_PUBLIC_BASIC_CSHARP_HOST_DIR") {
        let base = std::path::PathBuf::from(base);
        assert!(base.is_absolute(), "host directory must be absolute");
        let root = base.join(label);
        std::fs::create_dir(&root).expect("fresh consumer namespace");
        std::fs::create_dir(root.join("generated")).unwrap();
        std::fs::create_dir(root.join("consumer")).unwrap();
        for file in artifacts.files() {
            let path = root.join("generated").join(file.path.as_str());
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            let mut reached = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
                .unwrap();
            std::io::Write::write_all(&mut reached, &file.contents).unwrap();
        }
        for (name, contents) in [
            (
                "consumer/PublicConsumer.csproj",
                PUBLIC_BASIC_CONSUMER_PROJECT.to_string(),
            ),
            ("consumer/Program.cs", host),
            ("Program.ir.original.txt", format!("{program:#?}\n")),
        ] {
            let mut reached = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join(name))
                .unwrap();
            std::io::Write::write_all(&mut reached, contents.as_bytes()).unwrap();
        }
    }
    source
}

#[test]
fn public_basic_row_failures_are_global_ordered_and_do_not_own_legacy_targets() {
    let p = row_809_program();
    let source = retain_public_row_failure_consumer(
        "public-row-809-global-order",
        &p,
        r#"
var input=Primary();var limits=Policy();
foreach(var selection in new FerruleTargetSelection?[]{null,FerruleTargetSelection.Primary(),FerruleTargetSelection.Named("Named")})
{
    var probe=new Probe(Array.Empty<NamedInput>());
    var actual=Reach("N809.global-order",input,probe,limits,null,selection,()=>selection is null
        ? (object)GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null)
        : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,probe,limits,null));
    Failure(actual,1,"P2");Reads(actual,1,0);
}
"#,
    );
    assert_eq!(source.matches(".VisitPrimaryFailureRows(").count(), 1);
    assert!(source.contains("context.WalkSourceCohort(path_failure_1)"));
    assert!(source.contains("BasicFailureRule_0(context);\n        BasicFailureRule_1(context);"));
    assert!(source.contains("return Scope_0(context.LegacyView)"));
    assert!(source.contains("error.WithOwnedSourceNode(10U)"));
    assert!(source.contains("error.WithOwnedSourceNode(11U)"));
    assert!(source.contains("RequireBoolean(selection_failure_0, 13U)"));
    for old in [
        crate::emit(&p),
        crate::emit_with_basic_owned_sources(&p),
        crate::emit_with_basic_owned_filters(&p),
        crate::emit_with_basic_owned_sort(&p),
        crate::emit_with_basic_owned_windows(&p),
    ] {
        assert!(old.is_err(), "new permission never opens an old companion");
    }
}

#[test]
fn public_basic_row_failures_keep_ordinary_rule_and_window_artifacts_exact() {
    let mut p = window_703_program(
        vec![codegen::SequenceWindow::First { count: 31 }],
        IterationOutput::Repeated,
    );
    assert_eq!(
        crate::emit_with_basic_owned_row_failures(&p),
        crate::emit_with_basic_owned_windows(&p)
    );
    p.failure_rules.push(source_failure(
        &["Rows"],
        codegen::FailureSelection::All,
        Some(30),
    ));
    assert_eq!(
        crate::emit_with_basic_owned_row_failures(&p),
        crate::emit_with_basic_owned_windows(&p)
    );
    assert!(!codegen::is_basic_owned_primary_row_failure(
        &p,
        &p.failure_rules[0]
    ));
}

#[test]
fn public_basic_row_failures_keep_lazy_named_root_message_and_missing_owner_node() {
    for supplied in [true, false] {
        let mut p = row_809_program();
        p.failure_rules.truncate(1);
        p.expressions.extend([
            expression(30, field(named("Unused"), &["Value"], Some(&[]))),
            expression(
                31,
                Expression::Const {
                    value: Value::Bool(supplied),
                },
            ),
            expression(
                32,
                Expression::If {
                    condition: 31,
                    then: 10,
                    else_: 30,
                },
            ),
        ]);
        p.failure_rules[0].message = Some(32);
        let body = if supplied {
            r#"var input=Primary();var probe=new Probe(Array.Empty<NamedInput>());var limits=Policy();var actual=Reach("N809.lazy-root",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));Failure(actual,1,"P2");Reads(actual,1,0);"#
        } else {
            r#"var input=Primary();var probe=new Probe(Array.Empty<NamedInput>());var limits=Policy();var actual=Reach("N809.reached-root",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));OwnedAbsent(actual,"Unused",30);Reads(actual,1,0);"#
        };
        let source = retain_public_row_failure_consumer(
            if supplied {
                "public-row-809-lazy"
            } else {
                "public-row-809-reached"
            },
            &p,
            body,
        );
        assert!(source.contains("return Node_10(context);"));
        assert!(source.contains("return Node_30(context);"));
        assert!(source.contains("OwnedRootIsDeclaredNonrepeating("));
    }
}

#[test]
fn public_basic_row_failures_coexist_with_independent_target_windows() {
    let mut p = row_809_program();
    p.failure_rules.truncate(1);
    p.expressions.push(expression(
        30,
        Expression::Const {
            value: Value::Bool(false),
        },
    ));
    p.failure_rules[0].selection = codegen::FailureSelection::WhenTrue(30);
    p.root = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![binding("Value", 10, ScalarType::String)],
    );
    p.root.iteration = Some(IterationPlan::new(
        p.root.iteration.as_ref().unwrap().input().clone(),
        None,
        None,
        vec![codegen::SequenceWindow::First { count: 12 }],
        IterationOutput::Repeated,
    ));
    p.target = target_schema(&p.root, "Output");
    p.extra_targets.clear();
    let source = retain_public_row_failure_consumer(
        "public-row-809-target-windows",
        &p,
        r#"
var input=Primary();var probe=new Probe(Array.Empty<NamedInput>());var limits=Policy();
var actual=Reach("N809.target-windows",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));
Tree(actual,"[{\"Value\":\"P1\"},{\"Value\":\"P2\"}]");Reads(actual,1,0);
"#,
    );
    assert_eq!(source.matches(".VisitPrimaryFailureRows(").count(), 1);
    assert_eq!(source.matches(".VisitWindowedSourceCohort(").count(), 1);
}

#[test]
fn public_basic_row_failures_direct_program_uses_shared_complete_preflight() {
    let mut p = row_809_program();
    p.expressions.extend([
        expression(
            30,
            Expression::Const {
                value: Value::Bool(true),
            },
        ),
        expression(31, Expression::SourceDocumentPath),
        expression(
            32,
            Expression::If {
                condition: 30,
                then: 10,
                else_: 31,
            },
        ),
    ]);
    p.failure_rules[0].message = Some(32);
    let shared = codegen::validate_basic_owned_row_failure_emission(&p).unwrap_err();
    assert_eq!(
        crate::emit_with_basic_owned_row_failures(&p),
        Err(EmitError::ProgramValidation(shared))
    );
    p.expressions
        .iter_mut()
        .find(|node| node.id == 31)
        .unwrap()
        .expression = field(named("Ref"), &["Rows", "Value"], Some(&["Rows"]));
    let shared = codegen::validate_basic_owned_row_failure_emission(&p).unwrap_err();
    assert_eq!(
        crate::emit_with_basic_owned_row_failures(&p),
        Err(EmitError::ProgramValidation(shared))
    );
    p.failure_rules[0].message = Some(404);
    let shared = codegen::validate_basic_owned_row_failure_emission(&p).unwrap_err();
    assert_eq!(
        crate::emit_with_basic_owned_row_failures(&p),
        Err(EmitError::ProgramValidation(shared))
    );
}
