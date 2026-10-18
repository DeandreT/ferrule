// Public generated-consumer controls share the Basic host and fixture builders.
fn retain_public_window_consumer(label: &str, program: &Program, body: &str) -> String {
    let artifacts = crate::emit_with_basic_owned_windows(program).expect("Basic windows emit");
    let source = String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .expect("emitted mapping is UTF-8");
    let host = public_basic_consumer(body, false);
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

fn window_703_program(windows: Vec<codegen::SequenceWindow>, output: IterationOutput) -> Program {
    let mut p = sort_669_program(codegen::SortFilterOrder::FilterThenSort, None);
    p.extra_targets.clear();
    p.expressions.extend([
        expression(
            30,
            Expression::Const {
                value: Value::Int(1),
            },
        ),
        expression(
            31,
            Expression::Const {
                value: Value::Int(2),
            },
        ),
        expression(
            32,
            Expression::Const {
                value: Value::Int(3),
            },
        ),
    ]);
    p.root.iteration = Some(IterationPlan::new(
        p.root.iteration.as_ref().unwrap().input().clone(),
        None,
        None,
        windows,
        output,
    ));
    if output == IterationOutput::First {
        p.root.repeating = false;
    }
    p.target = target_schema(&p.root, "Output");
    p
}
fn window_703_modify(
    scope: &mut TargetScope,
    filter: Option<u32>,
    sort: Option<codegen::SortPlan>,
    windows: Vec<codegen::SequenceWindow>,
    output: IterationOutput,
) {
    scope.iteration = Some(IterationPlan::new(
        scope.iteration.as_ref().unwrap().input().clone(),
        filter,
        sort,
        windows,
        output,
    ));
}
const PUBLIC703_INPUT: &str = r#"var input=G(("Value",S("P.root")),("Rows",R(G(("Value",S("A")),("Score",I(30))),G(("Value",S("B")),("Score",I(10))),G(("Value",S("C")),("Score",I(20))))));var limits=Policy();"#;

// These public hosts compile with warnings as errors. Include only the local
// assertion functions exercised by each host, without changing older fixtures.
const PUBLIC703_CLR: &str = r#"
void Clr(Exception error,string type)
{
    Check(error.GetType().FullName==type && error.HResult==unchecked((int)0x80131500) && error.ToString().StartsWith(type+": "+error.Message,StringComparison.Ordinal),"exact CLR type/HResult/native prefix; full stack retained in raw snapshot");
}
"#;
const PUBLIC703_RAISE: &str = r#"
void MappingRaise(Actual actual,uint node,string message)
{
    var error=Runtime(actual,FerruleRuntimeError.MappingException);
    Check(error.Node==node && error.MappingExceptionMessage==message && error.Message==$"node {node}: mapping exception: {message}" && error.InnerException is null,"complete window MappingException payload");
    NullMetadata(error,"Node","MappingExceptionMessage");Clr(error,"Ferrule.Runtime.FerruleRuntimeException");
}
"#;
const PUBLIC703_OWNED: &str = r#"
void MissingOwnedKey(Actual actual,uint node)
{
    OwnedAbsent(actual,"Ref",node);var error=(FerruleRuntimeException)actual.Error!;
    Clr(error,"Ferrule.Runtime.FerruleRuntimeException");Clr(error.OwnedSourceFailure!,"Ferrule.Runtime.FerruleOwnedSourceException");
    Check(error.ToString().Contains(" ---> Ferrule.Runtime.FerruleOwnedSourceException: Owned source operation failed.",StringComparison.Ordinal),"full CLR native inner cause retained");
}
"#;
fn window_703_tree(ids: &[&str], first: bool) -> String {
    let rows = ids
        .iter()
        .enumerate()
        .map(|(i, value)| {
            format!(
                "{{\"Value\":\"{value}\",\"OwnedPosition\":{},\"LegacyPosition\":{}}}",
                i + 1,
                i + 1
            )
        })
        .collect::<Vec<_>>();
    if first {
        rows.first().cloned().unwrap_or_else(|| "{}".into())
    } else {
        format!("[{}]", rows.join(","))
    }
}
fn window_703_one_body(id: &str, expected: &str) -> String {
    format!(
        "{PUBLIC703_INPUT}\nvar probe=new Probe(Array.Empty<NamedInput>());var actual=Reach({id:?},input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));Tree(actual,{expected:?});Check(((ExecutionOutputs)actual.Result!).Extras.Count==0,\"whole primary output\");Reads(actual,1,0);\n"
    )
}
#[test]
fn public_basic_windows_preserve_old_companions_and_require_explicit_admission() {
    let plain = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    assert_eq!(
        crate::emit_with_basic_owned_sources(&plain),
        crate::emit_with_basic_owned_filters(&plain)
    );
    assert_eq!(
        crate::emit_with_basic_owned_sources(&plain),
        crate::emit_with_basic_owned_sort(&plain)
    );
    let p = window_703_program(
        vec![codegen::SequenceWindow::First { count: 31 }],
        IterationOutput::Repeated,
    );
    for actual in [
        crate::emit_with_basic_owned_sources(&p),
        crate::emit_with_basic_owned_filters(&p),
        crate::emit_with_basic_owned_sort(&p),
    ] {
        assert_eq!(actual.map(|_| ()), Err(sort_669_scope_control()));
    }
    assert!(crate::emit_with_basic_owned_windows(&p).is_ok());
}
#[test]
fn public_basic_windows_render_all_operators_ordered_composition_and_first() {
    let cases = [
        (
            "skip",
            vec![codegen::SequenceWindow::SkipFirst { count: 30 }],
            vec!["B", "C"],
            false,
        ),
        (
            "first",
            vec![codegen::SequenceWindow::First { count: 31 }],
            vec!["A", "B"],
            false,
        ),
        (
            "from",
            vec![codegen::SequenceWindow::From { position: 31 }],
            vec!["B", "C"],
            false,
        ),
        (
            "range",
            vec![codegen::SequenceWindow::FromTo {
                first: 31,
                last: 32,
            }],
            vec!["B", "C"],
            false,
        ),
        (
            "last",
            vec![codegen::SequenceWindow::Last { count: 31 }],
            vec!["B", "C"],
            false,
        ),
        (
            "zero",
            vec![codegen::SequenceWindow::First { count: 30 }],
            vec![],
            false,
        ),
        (
            "ordered-first",
            vec![
                codegen::SequenceWindow::SkipFirst { count: 30 },
                codegen::SequenceWindow::Last { count: 30 },
            ],
            vec!["C"],
            true,
        ),
    ];
    for (label, windows, ids, first) in cases {
        let mut p = window_703_program(
            windows,
            if first {
                IterationOutput::First
            } else {
                IterationOutput::Repeated
            },
        );
        if label == "zero" {
            p.expressions
                .iter_mut()
                .find(|node| node.id == 30)
                .unwrap()
                .expression = Expression::Const {
                value: Value::Int(0),
            };
        }
        let body = window_703_one_body(&format!("N703-G-{label}"), &window_703_tree(&ids, first));
        let source =
            retain_public_window_consumer(&format!("public-window-703-{label}"), &p, &body);
        assert_eq!(source.matches(".VisitWindowedSourceCohort(").count(), 1);
        assert!(source.contains("parent => global::Ferrule.Runtime.FerruleSequenceWindow."));
        assert!(!source.contains(".WithPhasePosition("));
        if first {
            assert!(source.contains("window_bounds_0, true, candidate => ScopeItem_0(candidate)"));
        }
    }
}
#[test]
fn public_basic_windows_keep_scalar_shapes_and_all_selected_outputs() {
    let mut p = window_703_program(
        vec![codegen::SequenceWindow::Last { count: 31 }],
        IterationOutput::Repeated,
    );
    p.root.construction = TargetConstruction::Scalar {
        expression: 20,
        target_domain: ScalarType::Int.into(),
    };
    p.root.bindings.clear();
    p.target = target_schema(&p.root, "Output");
    let named_root = p.root.clone();
    add_target(&mut p, "Window", named_root);
    let body = format!(
        "{PUBLIC703_INPUT}{}",
        r#"
var supplied=Array.Empty<NamedInput>();var primary=FerruleTargetSelection.Primary();var named=FerruleTargetSelection.Named("Window");
var p0=new Probe(supplied);var a=Reach("N703-S01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(supplied);var b=Reach("N703-S02",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(supplied);var c=Reach("N703-S03",input,p2,limits,null,named,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,named,p2,limits,null));
Tree(a,"[10,20]");Tree(b,"[10,20]");Tree(c,"[10,20]");var all=(ExecutionOutputs)a.Result!;Check(all.Extras.Count==1&&all.Extras[0].Name=="Window"&&Json(Plain(all.Extras[0].Instance))=="[10,20]","full named scalar output");Reads(a,1,0);Reads(b,1,0);Reads(c,1,0);
"#
    );
    let source = retain_public_window_consumer("public-window-703-scalar", &p, &body);
    assert!(source.contains("TargetBuilder.Scalar(Node_20(context)"));
    assert_eq!(source.matches(".VisitWindowedSourceCohort(").count(), 2);
}
#[test]
fn public_basic_windows_batch_both_filter_orders_before_compact_targets() {
    for (label, order, expected) in [
        ("fts", codegen::SortFilterOrder::FilterThenSort, "B"),
        ("stf", codegen::SortFilterOrder::SortThenFilter, "C"),
    ] {
        let mut p = window_703_program(
            vec![codegen::SequenceWindow::First { count: 31 }],
            IterationOutput::Repeated,
        );
        p.expressions
            .iter_mut()
            .find(|node| node.id == 2)
            .unwrap()
            .expression = public582_map(
            1,
            None,
            vec![(Value::Int(2), Value::Bool(true))],
            Some(Value::Bool(false)),
        );
        window_703_modify(
            &mut p.root,
            Some(2),
            Some(codegen::SortPlan::new(
                codegen::SortKey {
                    expression: 20,
                    descending: false,
                },
                vec![],
                order,
            )),
            vec![codegen::SequenceWindow::First { count: 31 }],
            IterationOutput::Repeated,
        );
        let source = retain_public_window_consumer(
            &format!("public-window-703-{label}"),
            &p,
            &window_703_one_body(
                &format!("N703-P-{label}"),
                &window_703_tree(&[expected], false),
            ),
        );
        assert!(source.contains("Node_2(candidate), 2U"));
        assert!(!source.contains("BeginRetainedFilterPhase("));
        assert!(source.contains("parent => global::Ferrule.Runtime.FerruleSequenceWindow.First(global::Ferrule.Runtime.FerruleSequences.ItemCount(31U, Node_31(parent)))"));
    }
}
#[test]
fn public_basic_windows_use_authentic_parent_bounds_and_keep_nested_outer_positions() {
    let mut child = owned_iteration(
        named("Ref"),
        &["Rows"],
        vec![
            binding("Value", 3, ScalarType::String),
            binding("Outer", 1, ScalarType::Int),
            binding("Inner", 4, ScalarType::Int),
        ],
    );
    child.target_field = "Kids".into();
    window_703_modify(
        &mut child,
        None,
        None,
        vec![codegen::SequenceWindow::Last { count: 1 }],
        IterationOutput::Repeated,
    );
    let mut root = owned_iteration(SourceOwner::Primary, &["Rows"], vec![]);
    root.children.push(child);
    let mut p = basic(
        vec![
            expression(1, position(SourceOwner::Primary, Some(&["Rows"]))),
            expression(3, field(named("Ref"), &["Rows", "Value"], None)),
            expression(4, position(named("Ref"), Some(&["Rows"]))),
        ],
        root,
    );
    p.extra_sources.retain(|source| source.name == "Ref");
    let body = r#"
var input=G(("Rows",R(G(("Value",S("P1"))),G(("Value",S("P2"))))));var supplied=new[]{new NamedInput("Ref",G(("Rows",R(G(("Value",S("N1"))),G(("Value",S("N2"))),G(("Value",S("N3")))))))};var limits=Policy();var probe=new Probe(supplied);var a=Reach("N703-N01",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));Tree(a,"[{\"Kids\":[{\"Value\":\"N3\",\"Outer\":1,\"Inner\":1}]},{\"Kids\":[{\"Value\":\"N2\",\"Outer\":2,\"Inner\":1},{\"Value\":\"N3\",\"Outer\":2,\"Inner\":2}]}]");Check(((ExecutionOutputs)a.Result!).Extras.Count==0,"complete nested parent-bound output");Reads(a,1,1);
"#;
    let source = retain_public_window_consumer("public-window-703-nested", &p, body);
    assert!(source.contains("ItemCount(1U, Node_1(parent))"));
    assert!(!source.contains("ItemCount(1U, Node_1(candidate))"));
}
#[test]
fn public_basic_windows_evaluate_all_parent_bounds_on_empty_and_first_zero() {
    let mut p = window_703_program(
        vec![
            codegen::SequenceWindow::First { count: 30 },
            codegen::SequenceWindow::Last { count: 0 },
        ],
        IterationOutput::Repeated,
    );
    p.expressions
        .iter_mut()
        .find(|node| node.id == 30)
        .unwrap()
        .expression = Expression::Const {
        value: Value::Int(0),
    };
    p.expressions.push(expression(
        33,
        Expression::Const {
            value: Value::String(String::new()),
        },
    ));
    p.expressions
        .push(expression(0, Expression::Raise { message: Some(33) }));
    let body = format!(
        "{PUBLIC703_CLR}{PUBLIC703_RAISE}{}",
        r#"
var limits=Policy();var input=G(("Rows",R()));var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("N703-E01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));MappingRaise(a,0U,"");Reads(a,1,0);
var input2=G(("Rows",R(G(("Value",S("A")),("Score",I(1))))));var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("N703-E02",input2,p1,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input2,p1,limits,null));MappingRaise(b,0U,"");Reads(b,1,0);
"#
    );
    let source = retain_public_window_consumer("public-window-703-eager-empty", &p, &body);
    assert!(source.contains("ItemCount(0U, Node_0(parent))"));
    assert!(source.contains("NodeBody_0("));
    let mut p = window_703_program(
        vec![codegen::SequenceWindow::First { count: 30 }],
        IterationOutput::Repeated,
    );
    p.expressions
        .iter_mut()
        .find(|node| node.id == 2)
        .unwrap()
        .expression = Expression::If {
        condition: 23,
        then: 22,
        else_: 24,
    };
    p.expressions
        .iter_mut()
        .find(|node| node.id == 3)
        .unwrap()
        .expression = Expression::Raise { message: Some(25) };
    p.expressions.extend([
        expression(
            21,
            Expression::Const {
                value: Value::String("later predicate".into()),
            },
        ),
        expression(22, Expression::Raise { message: Some(21) }),
        expression(
            23,
            public582_map(
                1,
                None,
                vec![(Value::Int(2), Value::Bool(true))],
                Some(Value::Bool(false)),
            ),
        ),
        expression(
            24,
            Expression::Const {
                value: Value::Bool(true),
            },
        ),
        expression(
            25,
            Expression::Const {
                value: Value::String("earlier target".into()),
            },
        ),
    ]);
    window_703_modify(
        &mut p.root,
        Some(2),
        None,
        vec![codegen::SequenceWindow::First { count: 30 }],
        IterationOutput::Repeated,
    );
    let body = format!(
        "{PUBLIC703_INPUT}{PUBLIC703_CLR}{PUBLIC703_RAISE}{}",
        r#"
var probe=new Probe(Array.Empty<NamedInput>());var a=Reach("N703-E03",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));MappingRaise(a,22U,"later predicate");Reads(a,1,0);
"#
    );
    let source = retain_public_window_consumer("public-window-703-batch-priority", &p, &body);
    assert!(!source.contains("BeginRetainedFilterPhase("));
}
#[test]
fn public_basic_windows_keep_uint_count_and_owned_bound_error_payloads() {
    let mut p = window_703_program(
        vec![codegen::SequenceWindow::FromTo {
            first: 0,
            last: u32::MAX,
        }],
        IterationOutput::Repeated,
    );
    p.expressions.push(expression(
        0,
        Expression::Const {
            value: Value::Bool(false),
        },
    ));
    p.expressions
        .push(expression(u32::MAX, Expression::Raise { message: None }));
    let body = format!(
        "{PUBLIC703_INPUT}{}",
        r#"
var probe=new Probe(Array.Empty<NamedInput>());var a=Reach("N703-C01",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));var error=Runtime(a,FerruleRuntimeError.NotAnItemCount);Check(error.Node==0U&&error.FoundKind==FerruleValueKind.Bool&&error.Message=="Node 0: expected an item count, found Bool."&&error.InnerException is null,"exact first count error before last raise");NullMetadata(error,"Node","FoundKind");Reads(a,1,0);
"#
    );
    let source = retain_public_window_consumer("public-window-703-count", &p, &body);
    assert!(source.contains("FromTo(global::Ferrule.Runtime.FerruleSequences.ItemCount(0U, Node_0(parent)), global::Ferrule.Runtime.FerruleSequences.ItemCount(4294967295U, Node_4294967295(parent)))"));
    let mut p = window_703_program(
        vec![codegen::SequenceWindow::First { count: u32::MAX }],
        IterationOutput::Repeated,
    );
    p.extra_sources.push(NamedSourceProgram {
        name: "Ref".into(),
        source: source_schema("Static"),
        dynamic: None,
    });
    p.expressions
        .push(expression(u32::MAX, field(named("Ref"), &["Score"], None)));
    let body = format!(
        "{PUBLIC703_INPUT}{PUBLIC703_CLR}{PUBLIC703_OWNED}{}",
        r#"
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("N703-C02",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));MissingOwnedKey(a,4294967295U);Reads(a,1,0);
var p1=new Probe(new[]{new NamedInput("Ref",G(("Score",I(2))))});var b=Reach("N703-C03",input,p1,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p1,limits,null));Tree(b,"[{\"Value\":\"A\",\"OwnedPosition\":1,\"LegacyPosition\":1},{\"Value\":\"B\",\"OwnedPosition\":2,\"LegacyPosition\":2}]");Reads(b,1,1);
"#
    );
    let source = retain_public_window_consumer("public-window-703-owned-bound", &p, &body);
    assert!(source.contains("error.WithOwnedSourceNode(4294967295U)"));
}
#[test]
fn public_basic_windows_keep_unrelated_legacy_dispatch_and_literal_identifiers() {
    let mut child = group(vec![
        binding("Node_20", 1, ScalarType::String),
        binding("Scope_0", 2, ScalarType::Int),
        binding("ScopeItem_0", 3, ScalarType::String),
    ]);
    child.target_field = "Child".into();
    child.iteration = Some(IterationPlan::new(
        IterationPlan::source(vec![]).input().clone(),
        None,
        None,
        vec![],
        IterationOutput::First,
    ));
    let mut root = plain_iteration(&["Rows"], vec![]);
    root.children.push(child);
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::Const {
                    value: Value::String("Node_20 Scope_0 ScopeItem_0".into()),
                },
            ),
            expression(2, Expression::Position { collection: vec![] }),
            expression(
                3,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: Some(path(&["Rows"])),
                },
            ),
        ],
        root,
    );
    p.extra_sources.clear();
    let body = r#"
var input=G(("Rows",R(G(("Value",S("A"))),G(("Value",S("B"))))));var limits=Policy();var probe=new Probe(Array.Empty<NamedInput>());var a=Reach("N703-L01",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));Tree(a,"[{\"Child\":{\"Node_20\":\"Node_20 Scope_0 ScopeItem_0\",\"Scope_0\":1,\"ScopeItem_0\":\"A\"}},{\"Child\":{\"Node_20\":\"Node_20 Scope_0 ScopeItem_0\",\"Scope_0\":1,\"ScopeItem_0\":\"B\"}}]");Reads(a,1,0);
"#;
    let source = retain_public_window_consumer("public-window-703-legacy-literals", &p, body);
    assert!(source.contains("return Scope_0(context.LegacyView)"));
    assert!(source.contains("Scope_1(global::Ferrule.Runtime.ScopeContext context)"));
    assert!(!source.contains("LegacyNode_"));
    assert!(source.contains("\"Node_20 Scope_0 ScopeItem_0\""));
    assert!(!source.contains(".VisitWindowedSourceCohort("));
    let mut plain = plain_iteration(
        &["Rows"],
        vec![
            binding("Value", 1, ScalarType::String),
            binding("Position", 2, ScalarType::Int),
            binding("Root", 4, ScalarType::String),
        ],
    );
    window_703_modify(
        &mut plain,
        None,
        None,
        vec![codegen::SequenceWindow::SkipFirst { count: 30 }],
        IterationOutput::Repeated,
    );
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: Some(path(&["Rows"])),
                },
            ),
            expression(2, Expression::Position { collection: vec![] }),
            expression(4, field(SourceOwner::Primary, &["Value"], Some(&[]))),
            expression(
                30,
                Expression::Const {
                    value: Value::Int(1),
                },
            ),
        ],
        plain,
    );
    p.extra_sources.clear();
    let body = format!(
        "{PUBLIC703_INPUT}{}",
        r#"
var probe=new Probe(Array.Empty<NamedInput>());var a=Reach("N703-L02",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));Tree(a,"[{\"Value\":\"B\",\"Position\":1,\"Root\":\"P.root\"},{\"Value\":\"C\",\"Position\":2,\"Root\":\"P.root\"}]");Reads(a,1,0);
"#
    );
    let source = retain_public_window_consumer("public-window-703-plain-paired", &p, &body);
    assert!(source.contains("context.WalkSourceCohort("));
    assert!(source.contains(".VisitWindowedSourceCohort("));
    assert!(!source.contains("return Scope_0(context.LegacyView)"));
}
#[test]
fn public_basic_windows_retain_potential_bounds_and_structural_refusal_precedence() {
    let mut p = window_703_program(
        vec![codegen::SequenceWindow::FromTo {
            first: 30,
            last: 99,
        }],
        IterationOutput::Repeated,
    );
    let expected = Err(EmitError::ProgramValidation(
        ProgramValidationError::MissingWindowExpression {
            target_path: vec![],
            window: 0,
            bound: 1,
            expression: 99,
        },
    ));
    let actual = crate::emit_with_basic_owned_windows(&p);
    assert_eq!(actual.map(|_| ()), expected);
    p.root.iteration = Some(IterationPlan::new(
        p.root.iteration.as_ref().unwrap().input().clone(),
        None,
        None,
        vec![codegen::SequenceWindow::First { count: 30 }],
        IterationOutput::Repeated,
    ));
    p.expressions
        .iter_mut()
        .find(|node| node.id == 30)
        .unwrap()
        .expression = Expression::If {
        condition: 2,
        then: 31,
        else_: 99,
    };
    p.expressions
        .push(expression(99, Expression::SourceDocumentPath));
    let actual = crate::emit_with_basic_owned_windows(&p);
    assert_eq!(
        actual.map(|_| ()),
        Err(EmitError::ProgramValidation(retained585_leaf_expected(
            99,
            codegen::OwnedSourceCapabilityReason::Document
        )))
    );
}
