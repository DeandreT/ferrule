use super::*;

pub(super) fn source_failure(
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
pub(super) fn failure_program(mut nodes: Vec<ExpressionNode>) -> Program {
    nodes.insert(
        0,
        expression(1, field(SourceOwner::Primary, &["Value"], None)),
    );
    basic(nodes, group(vec![binding("Value", 1, ScalarType::String)]))
}
pub(super) fn failure_legacy(id: u32, parts: &[&str], frame: Option<&[&str]>) -> ExpressionNode {
    expression(
        id,
        Expression::SourceField {
            path: path(parts),
            frame: frame.map(path),
        },
    )
}
pub(super) fn failure_scores(selection: codegen::FailureSelection) -> Program {
    let mut program = failure_program(vec![
        expression(
            100,
            Expression::Call {
                function: ScalarFunction::LessThan,
                args: vec![101, 102],
            },
        ),
        failure_legacy(101, &["Score"], Some(&["Rows"])),
        expression(
            102,
            Expression::Const {
                value: Value::Int(20),
            },
        ),
        failure_legacy(103, &["Value"], Some(&["Rows"])),
    ]);
    program
        .failure_rules
        .push(source_failure(&["Rows"], selection, Some(103)));
    program
}
fn retain_failure_host(id: &str, program: &Program, body: &str) -> String {
    let actual = render_basic_owned_mapping(program);
    eprintln!("INPUT {id} complete Program={program:#?} complete bridge host subject={body}");
    eprintln!("RAW {id} complete private source Result={actual:#?}");
    if let Err(error) = &actual {
        use std::error::Error as _;
        eprintln!(
            "RAW {id} complete typed cause={:?}",
            error.source().map(|value| format!("{value:#?}"))
        );
    }
    let source = actual.expect("complete bridge source result retained before comparison");
    let mut host = HOST_PREFIX.to_string();
    host.push_str(HOST_ERROR_SNAPSHOT);
    host.push_str(FAILURE_HOST_HELPERS);
    host.push_str("\n    public static void Main()\n    {\n");
    host.push_str(body);
    host.push_str(HOST_SUFFIX);
    eprintln!("RAW {id} complete prospective typed bridge host={host}");
    if let Some(directory) = std::env::var_os("FERRULE_PRIVATE_OWNED_CSHARP_HOST_DIR") {
        let directory = std::path::PathBuf::from(directory).join(id);
        std::fs::create_dir_all(&directory).expect("protected root supplied private host output");
        std::fs::write(directory.join("GeneratedMapping.cs"), &source)
            .expect("retain bridge mapping");
        std::fs::write(
            directory.join("GeneratedTargetBuilder.cs"),
            crate::runtime::TARGET_BUILDER,
        )
        .expect("retain exact existing helper");
        std::fs::write(directory.join("Program.cs"), host)
            .expect("retain complete prospective host");
        std::fs::write(directory.join("Program.ir.txt"), format!("{program:#?}\n"))
            .expect("retain complete Program");
    }
    source
}

#[test]
fn private_source_failure_scalar_order_and_lazy_message_bridge_hosts() {
    use codegen::FailureSelection::{All, WhenTrue};
    let mut scalar = failure_program(vec![expression(
        101,
        Expression::Const {
            value: Value::String(String::new()),
        },
    )]);
    scalar.expressions[0].expression = field(SourceOwner::Primary, &[], None);
    scalar.source = SchemaNode::scalar("Primary", ScalarType::String);
    scalar.extra_sources.clear();
    scalar.failure_rules.push(source_failure(&[], All, None));
    let source = retain_failure_host(
        "F542-01",
        &scalar,
        r#"Input=S("scalar");Declared=Array.Empty<FerruleDeclaredNamedSourceRoot>();var a=BridgeReach("F542-01",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,null);Check(a.Actual.Nodes.Length==0,"All/no-message before target");ExpectCohort(a,0,1,Array.Empty<string>(),new long[]{1});"#,
    );
    assert!(source.contains("context.WalkSourceCohort(path_failure_0)"));
    assert!(!source.contains("context.IterateSource("));
    assert!(source.contains("finally { global::System.GC.KeepAlive(cohort_failure_0); }"));
    scalar.failure_rules[0].message = Some(101);
    retain_failure_host(
        "F542-02",
        &scalar,
        r#"Input=S("scalar");Declared=Array.Empty<FerruleDeclaredNamedSourceRoot>();var a=BridgeReach("F542-02",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"");Check(a.Actual.Nodes.SequenceEqual(new uint[]{101}),"empty text message is present and evaluated once");"#,
    );
    let mut ordered = failure_program(vec![
        expression(
            100,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(101, Expression::Raise { message: None }),
        expression(
            102,
            Expression::Const {
                value: Value::String("later".into()),
            },
        ),
    ]);
    ordered.failure_rules = vec![
        source_failure(&[], WhenTrue(100), Some(101)),
        source_failure(&[], All, None),
        source_failure(&[], All, Some(102)),
    ];
    retain_failure_host(
        "F542-03",
        &ordered,
        r#"var a=BridgeReach("F542-03",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,2,null);Check(a.Actual.Nodes.SequenceEqual(new uint[]{100}),"ordered rules/first selected rule/lazy message/no target");Check(a.Cohorts.Length==2,"third rule not walked");"#,
    );
    let mut first = failure_scores(All);
    retain_failure_host(
        "F542-04",
        &first,
        r#"var a=BridgeReach("F542-04",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"P1");ExpectCohort(a,0,3,new[]{"Rows"},new long[]{1,2,3});Check(a.Actual.Nodes.SequenceEqual(new uint[]{103}),"first candidate only, all cohort rows retained first");"#,
    );
    first.failure_rules[0].selection = WhenTrue(100);
    first
        .expressions
        .iter_mut()
        .find(|node| node.id == 100)
        .unwrap()
        .expression = Expression::Const {
        value: Value::Bool(false),
    };
    first.failure_rules[0].message = Some(101);
    first
        .expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = Expression::Raise { message: None };
    retain_failure_host(
        "F542-07",
        &first,
        r#"var a=BridgeReach("F542-07",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectTree(a.Actual,"{\"Value\":\"P.root\"}");ExpectCohort(a,0,3,new[]{"Rows"},new long[]{1,2,3});Check(a.Actual.Nodes.SequenceEqual(new uint[]{100,100,100,1}),"all false skips Raise message and reaches target only after rules");"#,
    );
}

#[test]
fn private_source_failure_true_false_and_boolean_error_bridge_hosts() {
    let program = failure_scores(codegen::FailureSelection::WhenTrue(100));
    retain_failure_host(
        "F542-05",
        &program,
        r#"var a=BridgeReach("F542-05",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"P2");ExpectCohort(a,0,3,new[]{"Rows"},new long[]{1,2,3});Check(a.Actual.Nodes.SequenceEqual(new uint[]{100,101,102,100,101,102,103}),"WhenTrue scans raw rows1,2 then lazy selected message");"#,
    );
    let program = failure_scores(codegen::FailureSelection::WhenFalse(100));
    retain_failure_host(
        "F542-06",
        &program,
        r#"var a=BridgeReach("F542-06",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"P1");Check(a.Actual.Nodes.SequenceEqual(new uint[]{100,101,102,103}),"WhenFalse first raw candidate, no remaining predicate/message");"#,
    );
    let mut wrong = failure_scores(codegen::FailureSelection::WhenTrue(100));
    wrong
        .expressions
        .iter_mut()
        .find(|node| node.id == 100)
        .unwrap()
        .expression = Expression::Const {
        value: Value::Int(7),
    };
    retain_failure_host(
        "F542-23",
        &wrong,
        r#"var a=BridgeReach("F542-23",()=>ExecuteOwnedBasic(Input,Declared,Limits));var e=ExpectRuntime(a.Actual,FerruleRuntimeError.NotABool);Check(e.Node==100 && e.FoundKind==FerruleValueKind.Int64 && e.FailureRule is null && e.MappingFailureMessage is null && e.InnerException is null && a.Actual.Result is null,"predicate7 retains NotABool payload before message/target");Check(a.Actual.Nodes.SequenceEqual(new uint[]{100}),"no message or target after invalid predicate");"#,
    );
}

#[test]
fn private_source_failure_selected_routes_and_legacy_named_shadow_bridge_hosts() {
    let mut selected = failure_program(vec![]);
    selected
        .failure_rules
        .push(source_failure(&[], codegen::FailureSelection::All, None));
    add_target(
        &mut selected,
        "named",
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_failure_host(
        "F542-08",
        &selected,
        r#"var primarySelection=FerruleTargetSelection.Primary();var a=BridgeReach("F542-08.primary",()=>ExecuteSelectedOwnedBasic(Input,primarySelection,Declared,Limits),primarySelection);var namedSelection=FerruleTargetSelection.Named("named");var b=BridgeReach("F542-08.named",()=>ExecuteSelectedOwnedBasic(Input,namedSelection,Declared,Limits),namedSelection);var c=BridgeReach("F542-08.all",()=>ExecuteOwnedBasic(Input,Declared,Limits));Limits=new(0,0,0,0,0);var missingSelection=FerruleTargetSelection.Named("missing");var d=BridgeReach("F542-08.unknown",()=>ExecuteSelectedOwnedBasic(Input,missingSelection,Declared,Limits),missingSelection);ExpectFailure(a,1,null);ExpectFailure(b,1,null);ExpectFailure(c,1,null);var e=ExpectRuntime(d.Actual,FerruleRuntimeError.UnknownTarget);Check(e.Detail=="missing" && d.Cohorts.Length==0 && d.Actual.Nodes.Length==0,"unknown selection keeps prior priority over factory/failure");Check(a.Actual.Nodes.Length==0 && b.Actual.Nodes.Length==0 && c.Actual.Nodes.Length==0,"global failures before every selected/all target");"#,
    );
    let mut shadow = failure_program(vec![]);
    shadow.failure_rules.push(source_failure(
        &["Ref", "Rows"],
        codegen::FailureSelection::All,
        None,
    ));
    retain_failure_host(
        "F542-09",
        &shadow,
        r#"var a=BridgeReach("F542-09",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectTree(a.Actual,"{\"Value\":\"P.root\"}");ExpectCohort(a,0,0,new[]{"Ref","Rows"},Array.Empty<long>());Check(a.Actual.Nodes.SequenceEqual(new uint[]{1}),"primary Ref shadows named Ref without rescue");"#,
    );
    let mut named_rule = failure_scores(codegen::FailureSelection::WhenTrue(100));
    named_rule.failure_rules[0].iteration =
        codegen::FailureIteration::Source(codegen::SourceIteration::new(path(&["ref", "Rows"])));
    named_rule
        .expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = Expression::SourceField {
        path: path(&["Score"]),
        frame: Some(path(&["ref", "Rows"])),
    };
    named_rule
        .expressions
        .iter_mut()
        .find(|node| node.id == 103)
        .unwrap()
        .expression = Expression::SourceField {
        path: path(&["Value"]),
        frame: Some(path(&["ref", "Rows"])),
    };
    retain_failure_host(
        "F542-10",
        &named_rule,
        r#"Declared[3]=new FerruleDeclaredNamedSourceRoot("ref",G(("Rows",R(G(("Value",S("N1")),("Score",I(30))),G(("Value",S("N2")),("Score",I(10)))))));var a=BridgeReach("F542-10",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"N2");ExpectCohort(a,0,2,new[]{"ref","Rows"},new long[]{1,2});Check(a.Actual.Nodes.SequenceEqual(new uint[]{100,101,102,100,101,102,103}),"exact declared lowercase ref parent context and second raw candidate");"#,
    );
}

#[test]
fn private_source_failure_nested_empty_and_real_empty_root_frame_bridge_hosts() {
    let mut nested = failure_program(vec![
        expression(
            100,
            Expression::Call {
                function: ScalarFunction::GreaterThan,
                args: vec![101, 102],
            },
        ),
        failure_legacy(101, &["Score"], Some(&["Rows", "Kids"])),
        expression(
            102,
            Expression::Const {
                value: Value::Int(1),
            },
        ),
        expression(
            110,
            Expression::Call {
                function: ScalarFunction::Concat,
                args: vec![111, 112, 113, 112, 114],
            },
        ),
        expression(
            111,
            Expression::Position {
                collection: path(&["Rows"]),
            },
        ),
        expression(
            112,
            Expression::Const {
                value: Value::String(":".into()),
            },
        ),
        expression(
            113,
            Expression::Position {
                collection: path(&["Rows", "Kids"]),
            },
        ),
        failure_legacy(114, &["Value"], Some(&["Rows", "Kids"])),
    ]);
    nested.source = SchemaNode::group(
        "Primary",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::group(
                        "Kids",
                        vec![
                            SchemaNode::scalar("Value", ScalarType::String),
                            SchemaNode::scalar("Score", ScalarType::Int),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    );
    nested.failure_rules.push(source_failure(
        &["Rows", "Kids"],
        codegen::FailureSelection::WhenTrue(100),
        Some(110),
    ));
    retain_failure_host(
        "F542-11",
        &nested,
        r#"Input=G(("Value",S("P.root")),("Rows",R(G(("Kids",R(G(("Value",S("C1")),("Score",I(0))),G(("Value",S("C2")),("Score",I(2)))))),G(("Kids",R(G(("Value",S("C3")),("Score",I(3)))))))));ObservedFailureCollections=new[]{new[]{"Rows"},new[]{"Rows","Kids"}};var a=BridgeReach("F542-11",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"1:2:C2");ExpectCohort(a,0,3,new[]{"Rows","Kids"},new long[]{1,2,1});Check(a.Cohorts[0].Collections[0].SequenceEqual(new long[]{1,1,2}) && a.Cohorts[0].Collections[1].SequenceEqual(new long[]{1,2,1}),"uncompacted parent and child raw positions");Check(a.Actual.Nodes.SequenceEqual(new uint[]{100,101,102,100,101,102,110,111,112,113,112,114}),"first selected nested item, lazy concat arguments, no target");"#,
    );
    let empty = failure_scores(codegen::FailureSelection::All);
    retain_failure_host(
        "F542-12",
        &empty,
        r#"Input=G(("Value",S("P.root")),("Rows",R()));var a=BridgeReach("F542-12",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectTree(a.Actual,"{\"Value\":\"P.root\"}");ExpectCohort(a,0,0,new[]{"Rows"},Array.Empty<long>());Check(a.Actual.Nodes.SequenceEqual(new uint[]{1}),"empty collection does not evaluate message");"#,
    );
    let mut flat = failure_program(vec![
        expression(
            100,
            Expression::Call {
                function: ScalarFunction::GreaterThan,
                args: vec![101, 102],
            },
        ),
        expression(101, Expression::Position { collection: vec![] }),
        expression(
            102,
            Expression::Const {
                value: Value::Int(1),
            },
        ),
        failure_legacy(103, &["Value"], Some(&[])),
    ]);
    flat.failure_rules.push(source_failure(
        &[],
        codegen::FailureSelection::WhenTrue(100),
        Some(103),
    ));
    retain_failure_host(
        "F542-13",
        &flat,
        r#"Input=R(G(("Value",S("flat1"))),G(("Value",S("flat2"))));var a=BridgeReach("F542-13",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectFailure(a,1,"flat2");ExpectCohort(a,0,2,Array.Empty<string>(),new long[]{1,2});Check(a.Actual.Nodes.SequenceEqual(new uint[]{100,101,102,100,101,102,103}),"Some[] message pin has actual repeated empty-root frame and second raw position");"#,
    );
}

#[test]
fn private_source_failure_parent_execution_and_parameter_bridge_hosts() {
    let mut runtime = failure_program(vec![expression(
        101,
        Expression::RuntimeValue {
            value: RuntimeValue::MappingFilePath,
        },
    )]);
    runtime.failure_rules.push(source_failure(
        &[],
        codegen::FailureSelection::All,
        Some(101),
    ));
    retain_failure_host(
        "F542-14",
        &runtime,
        r#"Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(Array.Empty<KeyValuePair<string,FerruleValue>>()));var a=BridgeReach("F542-14",()=>ExecuteOwnedBasic(Input,Declared,Limits,Execution));ExpectFailure(a,1,"mapping.test");Check(a.Actual.Nodes.SequenceEqual(new uint[]{101}),"failure candidate retains parent execution");"#,
    );
    runtime
        .expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = Expression::RuntimeValue {
        value: RuntimeValue::CurrentDateTime,
    };
    retain_failure_host(
        "F542-15",
        &runtime,
        r#"var a=BridgeReach("F542-15",()=>ExecuteOwnedBasic(Input,Declared,Limits));var e=ExpectRuntime(a.Actual,FerruleRuntimeError.MissingRuntimeValue);Check(e.RuntimeValue==FerruleRuntimeValue.CurrentDateTime && e.Node is null && e.FailureRule is null && e.MappingFailureMessage is null && e.InnerException is null && a.Actual.Result is null,"missing clock exact typed cause before mapping failure");Check(a.Actual.Nodes.SequenceEqual(new uint[]{101}),"clock called before target");"#,
    );
    let mut parameter = failure_program(vec![expression(
        103,
        Expression::RuntimeParameter {
            name: "failure".into(),
            ty: ScalarType::Int,
        },
    )]);
    parameter.failure_rules.push(source_failure(
        &[],
        codegen::FailureSelection::All,
        Some(103),
    ));
    retain_failure_host(
        "F542-16",
        &parameter,
        r#"ParameterValues=new[]{new KeyValuePair<string,FerruleValue>("failure",FerruleValue.FromInt64(42))};Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(ParameterValues));var a=BridgeReach("F542-16.provided",()=>ExecuteOwnedBasic(Input,Declared,Limits,Execution));ParameterValues=Array.Empty<KeyValuePair<string,FerruleValue>>();Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(ParameterValues));var b=BridgeReach("F542-16.missing",()=>ExecuteOwnedBasic(Input,Declared,Limits,Execution));ParameterValues=new[]{new KeyValuePair<string,FerruleValue>("failure",FerruleValue.FromString("bad"))};Execution=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(ParameterValues));var c=BridgeReach("F542-16.type",()=>ExecuteOwnedBasic(Input,Declared,Limits,Execution));ExpectFailure(a,1,"42");var m=ExpectRuntime(b.Actual,FerruleRuntimeError.MissingRuntimeParameter);Check(m.Node==103 && m.RuntimeParameter=="failure" && m.FoundKind is null && m.ExpectedScalarType is null && m.InnerException is null && m.FailureRule is null && b.Actual.Result is null,"missing parameter exact payload");var t=ExpectRuntime(c.Actual,FerruleRuntimeError.RuntimeParameterType);Check(t.Node==103 && t.RuntimeParameter=="failure" && t.FoundKind==FerruleValueKind.String && t.ExpectedScalarType==FerruleScalarType.Int64 && t.FailureRule is null && t.InnerException is null && c.Actual.Result is null,"invalid String cannot coerce to declared Int64");Check(a.Actual.Nodes.SequenceEqual(new uint[]{103}) && b.Actual.Nodes.SequenceEqual(new uint[]{103}) && c.Actual.Nodes.SequenceEqual(new uint[]{103}),"parameter access before target in every route");"#,
    );
}

#[test]
fn private_source_failure_complete_cohort_five_limits_bridge_host() {
    let mut program = failure_program(vec![expression(
        100,
        Expression::Const {
            value: Value::Bool(true),
        },
    )]);
    program.extra_sources.clear();
    program.failure_rules.push(source_failure(
        &["Rows"],
        codegen::FailureSelection::WhenTrue(100),
        None,
    ));
    retain_failure_host(
        "F542-17-22",
        &program,
        r#"Declared=Array.Empty<FerruleDeclaredNamedSourceRoot>();Limits=new(0,1,1,65536,3);var roots=BridgeReach("F542-17",()=>ExecuteOwnedBasic(Input,Declared,Limits));Limits=new(1,1,0,65536,3);var segments=BridgeReach("F542-18",()=>ExecuteOwnedBasic(Input,Declared,Limits));Limits=new(1,1,1,0,3);var bytes=BridgeReach("F542-19",()=>ExecuteOwnedBasic(Input,Declared,Limits));Limits=new(1,0,1,65536,3);var frames=BridgeReach("F542-20",()=>ExecuteOwnedBasic(Input,Declared,Limits));Limits=new(1,1,1,65536,2);var contexts=BridgeReach("F542-21",()=>ExecuteOwnedBasic(Input,Declared,Limits));Limits=new(1,1,1,65536,3);var admitted=BridgeReach("F542-22",()=>ExecuteOwnedBasic(Input,Declared,Limits));ExpectExactOwnedLimit(roots,FerruleOwnedSourceLimitKind.Roots,0,1,0);ExpectExactOwnedLimit(segments,FerruleOwnedSourceLimitKind.PathSegments,0,1,0);ExpectExactOwnedLimit(bytes,FerruleOwnedSourceLimitKind.MetadataBytes,0,4,0);ExpectExactBridgeLimit(frames,FerruleBasicSourceBridgeComponent.ActiveFrames,0,1,0);ExpectExactBridgeLimit(contexts,FerruleBasicSourceBridgeComponent.WalkContexts,0,3,2);ExpectFailure(admitted,1,null);ExpectCohort(admitted,0,3,new[]{"Rows"},new long[]{1,2,3});Check(admitted.Actual.Nodes.SequenceEqual(new uint[]{100}),"complete cohort before first selected predicate");Check(roots.Cohorts.Length==0 && segments.Cohorts.Length==0 && bytes.Cohorts.Length==0 && frames.Cohorts.Length==0 && contexts.Cohorts.Length==0,"no partial cohort published on refusal");"#,
    );
}

pub(super) fn expected_failure_diagnostic(
    index: usize,
    node: Option<u32>,
    reason: codegen::OwnedSourceCapabilityReason,
) -> ProgramValidationError {
    let location = match node {
        Some(node) => codegen::OwnedSourceDiagnosticLocation::Expression {
            namespace: codegen::OwnedSourceNamespace::Main,
            node,
            scope: None,
        },
        None => codegen::OwnedSourceDiagnosticLocation::Iteration {
            scope: ::mapping::SourceScopeLocation::root(::mapping::SourceScopeTarget::Primary),
        },
    };
    ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location,
            request: None,
            reason,
            origin: codegen::OwnedSourceCapabilityOrigin::Failure { index },
        }),
    }
}
#[test]
fn private_source_failure_dynamic_owned_generated_and_specialized_admission() {
    use codegen::{FailureSelection, OwnedSourceCapabilityReason as Reason};
    use std::error::Error as _;
    let mut base = failure_program(vec![
        expression(
            100,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        failure_legacy(101, &["D", "Value"], None),
        expression(
            102,
            Expression::Const {
                value: Value::String("d.json".into()),
            },
        ),
    ]);
    base.source = SchemaNode::group(
        "Primary",
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Score", ScalarType::Int),
            SchemaNode::group(
                "Rows",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            )
            .repeating(),
            SchemaNode::group("D", vec![SchemaNode::scalar("Value", ScalarType::String)]),
        ],
    );
    base.extra_sources.push(NamedSourceProgram {
        name: "D".into(),
        source: source_schema("Dynamic"),
        dynamic: Some(codegen::DynamicSourceProgram {
            path: 102,
            driver: codegen::SourceIteration::new(path(&["Rows"])).into(),
        }),
    });
    base.failure_rules
        .push(source_failure(&[], FailureSelection::WhenTrue(100), None));
    let mut cases: Vec<(&str, Program, Option<ProgramValidationError>)> =
        vec![("S542-07 dead/unused dynamic metadata", base.clone(), None)];
    let mut iteration = base.clone();
    iteration.failure_rules[0].iteration =
        codegen::FailureIteration::Source(codegen::SourceIteration::new(path(&["D", "Rows"])));
    cases.push((
        "S542-03 dynamic source iteration",
        iteration,
        Some(expected_failure_diagnostic(0, None, Reason::DynamicLoader)),
    ));
    let mut second_iteration = base.clone();
    second_iteration.failure_rules.push(source_failure(
        &["D", "Rows"],
        FailureSelection::All,
        None,
    ));
    cases.push((
        "S542-03 second rule exact failure index",
        second_iteration,
        Some(expected_failure_diagnostic(1, None, Reason::DynamicLoader)),
    ));
    let mut predicate = base.clone();
    predicate
        .expressions
        .iter_mut()
        .find(|node| node.id == 100)
        .unwrap()
        .expression = Expression::SourceField {
        path: path(&["D", "Value"]),
        frame: None,
    };
    cases.push((
        "S542-04 dynamic predicate path",
        predicate,
        Some(expected_failure_diagnostic(
            0,
            Some(100),
            Reason::DynamicLoader,
        )),
    ));
    let mut message = base.clone();
    message
        .expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = Expression::SourceField {
        path: path(&["Value"]),
        frame: Some(path(&["D", "Rows"])),
    };
    message.failure_rules[0].message = Some(101);
    cases.push((
        "S542-05 dynamic message frame",
        message,
        Some(expected_failure_diagnostic(
            0,
            Some(101),
            Reason::DynamicLoader,
        )),
    ));
    let mut lazy = base.clone();
    lazy.expressions.extend([
        expression(
            103,
            Expression::Const {
                value: Value::Bool(true),
            },
        ),
        expression(
            104,
            Expression::If {
                condition: 103,
                then: 100,
                else_: 101,
            },
        ),
    ]);
    lazy.failure_rules[0].selection = FailureSelection::WhenTrue(104);
    cases.push((
        "S542-06 potential unchosen dynamic dependency",
        lazy,
        Some(expected_failure_diagnostic(
            0,
            Some(101),
            Reason::DynamicLoader,
        )),
    ));
    let mut static_source = base.clone();
    static_source.extra_sources.last_mut().unwrap().dynamic = None;
    static_source.failure_rules[0].message = Some(101);
    cases.push(("S542-07 static same-name metadata", static_source, None));
    let mut empty_frame = base.clone();
    empty_frame
        .expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = Expression::SourceField {
        path: path(&["D", "Value"]),
        frame: Some(vec![]),
    };
    empty_frame.failure_rules[0].message = Some(101);
    cases.push((
        "S542-08 empty frame replaces path admission",
        empty_frame,
        None,
    ));
    let mut document = base.clone();
    document
        .expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = Expression::SourceDocumentPath;
    document.failure_rules[0].message = Some(101);
    cases.push((
        "S542-09 specialized document failure remains closed",
        document,
        Some(expected_failure_diagnostic(0, None, Reason::FailureRule)),
    ));
    let mut generated = base.clone();
    generated.expressions.extend([
        expression(
            110,
            Expression::Const {
                value: Value::Int(1),
            },
        ),
        expression(
            111,
            Expression::Const {
                value: Value::Int(2),
            },
        ),
        failure_legacy(112, &[], None),
    ]);
    generated.failure_rules[0] = codegen::FailureRule {
        iteration: codegen::FailureIteration::Generated(codegen::GeneratedSequence::Range {
            from: Some(110),
            to: 111,
            item: 112,
        }),
        selection: FailureSelection::All,
        message: None,
    };
    cases.push((
        "S542-01 generated failure remains closed",
        generated,
        Some(expected_failure_diagnostic(0, None, Reason::FailureRule)),
    ));
    for (id, program, expected) in cases {
        eprintln!("INPUT {id} complete Program={program:#?}");
        let actual = render_basic_owned_mapping(&program);
        eprintln!("RAW {id} complete Result={actual:#?}");
        if let Err(error) = &actual {
            eprintln!(
                "RAW {id} complete cause={:?}",
                error.source().map(|value| format!("{value:#?}"))
            );
        }
        eprintln!("EXPECTED {id} independent literal={expected:#?}");
        match expected {
            Some(expected) => match actual {
                Err(EmitError::ProgramValidation(actual)) => assert_eq!(actual, expected, "{id}"),
                other => panic!("{id} full retained result differs: {other:#?}"),
            },
            None => assert!(actual.is_ok(), "{id} full actual retained before assertion"),
        }
    }
    for message in [false, true] {
        let mut owned = failure_program(vec![expression(
            100,
            field(SourceOwner::Primary, &["Score"], None),
        )]);
        owned.failure_rules.push(source_failure(
            &[],
            if message {
                FailureSelection::All
            } else {
                FailureSelection::WhenTrue(100)
            },
            if message { Some(100) } else { None },
        ));
        let expected = ProgramValidationError::OwnedSource {
            diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
                location: codegen::OwnedSourceDiagnosticLocation::Expression {
                    namespace: codegen::OwnedSourceNamespace::Main,
                    node: 100,
                    scope: None,
                },
                request: Some(codegen::OwnedSourceDiagnosticRequest::Field {
                    owner: codegen::OwnedSourceDiagnosticOwner::Primary,
                    path: path(&["Score"]),
                    frame: None,
                }),
                reason: Reason::FailureRule,
                origin: codegen::OwnedSourceCapabilityOrigin::Failure { index: 0 },
            }),
        };
        let actual = render_basic_owned_mapping(&owned);
        eprintln!(
            "INPUT S542-02 ownedfailure message={message} full Program={owned:#?} RAW Result={actual:#?} EXPECTED={expected:#?}"
        );
        if let Err(error) = &actual {
            eprintln!(
                "RAW S542-02 complete cause={:?}",
                error.source().map(|value| format!("{value:#?}"))
            );
        }
        match actual {
            Err(EmitError::ProgramValidation(actual)) => assert_eq!(actual, expected),
            other => panic!("full owned failure refusal differs: {other:#?}"),
        }
    }
}

#[test]
fn private_source_failure_preserves_ordinary_bytes_and_public_owned_gates() {
    let mut program = failure_program(vec![expression(
        100,
        Expression::Const {
            value: Value::Bool(false),
        },
    )]);
    program.failure_rules.push(source_failure(
        &[],
        codegen::FailureSelection::WhenTrue(100),
        None,
    ));
    let ordinary = emit(&program);
    let typed = emit_typed(&program);
    let private = render_basic_owned_mapping(&program);
    eprintln!(
        "INPUT S542-10 complete Program={program:#?} RAW ordinary={ordinary:#?} typed={typed:#?} private={private:#?}"
    );
    assert!(matches!(
        ordinary,
        Err(EmitError::ProgramValidation(
            ProgramValidationError::OwnedSource { .. }
        ))
    ));
    assert!(matches!(
        typed,
        Err(EmitError::ProgramValidation(
            ProgramValidationError::OwnedSource { .. }
        ))
    ));
    let private = private.expect("complete private source result retained");
    assert!(private.contains("EvaluateBasicFailureRules(context);"));
    assert!(!private.contains("Reindex") && !private.contains("IterateGenerated"));
    program.expressions[0].expression = Expression::SourceField {
        path: path(&["Value"]),
        frame: None,
    };
    let ordinary = emit(&program);
    let typed = emit_typed(&program);
    let untyped_source = crate::mapping::render_complete(&program);
    let typed_source = crate::mapping::render_typed(&program);
    eprintln!(
        "INPUT ordinary legacy failure complete Program={program:#?} RAW ordinary={ordinary:#?} typed={typed:#?} ordinary_source={untyped_source:#?} typed_source={typed_source:#?}"
    );
    assert!(ordinary.is_ok() && typed.is_ok());
    assert!(
        untyped_source
            .expect("complete ordinary source recorded")
            .members
            .contains("FailureRule_0(context);")
    );
    assert!(
        typed_source
            .expect("complete typed source recorded")
            .members
            .contains("FailureRule_0(context);")
    );
}

const FAILURE_HOST_HELPERS: &str = r#"
    // Private observation only: these are the exact public same-walk cohort
    // and context APIs. No private ordinal or owner is inferred from a path.
    private sealed record FailureCohortObservation(int Rule,string[] Path,int Count,object[] Frames,long[] Positions,long[][] Collections);
    private sealed record BridgeReached(Reached Actual,FailureCohortObservation[] Cohorts);
    private static readonly List<FailureCohortObservation> FailureCohorts=new();
    private static string[][] ObservedFailureCollections=Array.Empty<string[]>();
    static partial void ObserveBasicFailureCohort(int rule,IReadOnlyList<string> path,FerruleRetainedSourceCohort cohort)
    {
        var frames=new object[cohort.Count];var positions=new long[cohort.Count];
        var collections=new long[ObservedFailureCollections.Length][];
        for(var p=0;p<collections.Length;p++)collections[p]=new long[cohort.Count];
        for(var i=0;i<cohort.Count;i++)
        {
            var candidate=cohort[i];frames[i]=new {candidateEnumerationIndex=i,legacyFrames=candidate.Context.LegacyView.Frames.Select(InstanceSnapshot).ToArray()};
            positions[i]=candidate.Context.LegacyView.Position(path);
            for(var p=0;p<collections.Length;p++)collections[p][i]=candidate.Context.LegacyView.Position(ObservedFailureCollections[p]);
        }
        FailureCohorts.Add(new(rule,path.ToArray(),cohort.Count,frames,positions,collections));
    }
    private static BridgeReached BridgeReach(string id,Func<object?> subject,FerruleTargetSelection? selection=null)
    {
        FailureCohorts.Clear();
        Console.WriteLine(Json(new {id,classification="bridge_input_before_subject",observedCollections=ObservedFailureCollections.Select(path=>path.ToArray()).ToArray()}));
        var actual=Reach(id,subject,selection);var cohorts=FailureCohorts.ToArray();
        Console.WriteLine(Json(new {id,classification="complete_failure_cohorts_before_comparisons",completeInput=actual.CompleteInput,completeActual=actual.CompleteActual,cohorts}));
        return new(actual,cohorts);
    }
    private static void ExpectCohort(BridgeReached reached,int ordinal,int count,string[] path,long[] positions)
    {
        Check(reached.Cohorts.Length>ordinal,"complete cohort observation exists");var cohort=reached.Cohorts[ordinal];
        Check(cohort.Count==count && cohort.Path.SequenceEqual(path) && cohort.Frames.Length==count && cohort.Positions.SequenceEqual(positions),"literal complete cohort/path/raw legacy position sequence");
    }
    private static void ExpectFailure(BridgeReached reached,int rule,string? message)
    {
        var e=ExpectRuntime(reached.Actual,FerruleRuntimeError.MappingFailure);
        Check(e.FailureRule==rule && e.MappingFailureMessage==message && e.Message==(message is null?$"mapping failure rule {rule}: mapping exception was raised":$"mapping failure rule {rule}: {message}"),"full ordinary failure rule/message/display");
        Check(e.Node is null && e.Function is null && e.ExpectedArity is null && e.ActualArity is null && e.FoundKind is null && e.AggregateOperation is null && e.Detail is null && e.RequestedItems is null && e.MaximumItems is null && e.MaximumDepth is null && e.RuntimeValue is null && e.MappingExceptionMessage is null && e.Join is null && e.UserFunction is null && e.FunctionParameter is null && e.ExpectedScalarType is null && e.RuntimeParameter is null && e.SourceField is null && e.FoundInstance is null && e.PrimaryRoot is null && e.OwnedSourceFailure is null && e.BasicSourceBridgeFailure is null && e.InnerException is null && reached.Actual.Result is null,"complete untouched typed failure envelope/no partial outcome");
    }
    private static void ExpectExactOwnedLimit(BridgeReached reached,FerruleOwnedSourceLimitKind kind,nuint used,nuint requested,nuint maximum)
    {
        var e=ExpectOwned(reached.Actual,FerruleOwnedSourceErrorKind.MetadataLimit,null,null,null);
        Check(e.LimitKind==kind && e.Used==used && e.Requested==requested && e.Maximum==maximum,"literal complete core resource cause");
        Check(reached.Actual.Nodes.Length==0 && reached.Cohorts.Length==0,"preallocation refusal publishes no cohort or predicate");
    }
    private static void ExpectExactBridgeLimit(BridgeReached reached,FerruleBasicSourceBridgeComponent component,nuint used,nuint requested,nuint maximum)
    {
        var outer=ExpectRuntime(reached.Actual,FerruleRuntimeError.BasicSourceBridge);var e=outer.BasicSourceBridgeFailure;
        Check(e is not null && ReferenceEquals(e,outer.InnerException),"bare paired bridge cause");var cause=e!;
        Check(cause.Error==FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit && cause.Component==component && cause.Used==used && cause.Requested==requested && cause.Maximum==maximum,"literal complete paired resource cause");
        Check(cause.InputOrdinal is null && cause.CandidateOrdinal is null && cause.DeclarationIndex is null && cause.DeclaredCount is null && cause.Found is null && cause.InnerException is null,"no invented bridge cause fields");
        Check(outer.Node is null && outer.FailureRule is null && outer.MappingFailureMessage is null && outer.OwnedSourceFailure is null && reached.Actual.Result is null && reached.Actual.Nodes.Length==0 && reached.Cohorts.Length==0,"whole cohort refused before predicates/messages/outputs");
    }
"#;

// Checked admission combined with ordinary global source failures (#519):
// catalogues include every target's legacy requirements, the factory runs
// first, and ordered global failures fire before any target output.
pub(super) fn checked_failure_program() -> Program {
    use codegen::FailureSelection;
    let mut p = failure_scores(FailureSelection::WhenFalse(100));
    p.expressions
        .iter_mut()
        .find(|node| node.id == 101)
        .unwrap()
        .expression = failure_legacy(101, &["Score"], Some(&["ref", "Rows"])).expression;
    p.expressions
        .iter_mut()
        .find(|node| node.id == 103)
        .unwrap()
        .expression = failure_legacy(103, &["Value"], Some(&["ref", "Rows"])).expression;
    p.failure_rules[0] = source_failure(
        &["ref", "Rows"],
        FailureSelection::WhenFalse(100),
        Some(103),
    );
    p.expressions.extend([
        expression(
            90,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(91, Expression::Raise { message: None }),
        expression(92, Expression::Raise { message: None }),
        expression(2, Expression::Raise { message: None }),
    ]);
    p.failure_rules.insert(
        0,
        source_failure(&["Rows"], FailureSelection::WhenTrue(90), Some(91)),
    );
    p.failure_rules
        .push(source_failure(&[], FailureSelection::All, Some(92)));
    add_target(
        &mut p,
        "named",
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    p
}
fn retain_checked_failure_host(id: &str, program: &Program, body: &str) -> String {
    let result = checked_result(id, program);
    if let Err(error) = &result {
        use std::error::Error as _;
        eprintln!(
            "RAW {id} complete typed cause={:?}",
            error.source().map(|value| format!("{value:#?}"))
        );
    }
    let source = result.expect("complete composed source Result captured before comparison");
    let mut host = complete_checked_host(program, body);
    let marker = "\n    public static void Main()\n    {\n";
    let entry_matches = host.matches(marker).count();
    eprintln!(
        "RAW {id} complete checked host before helper composition={host} entry_matches={entry_matches}"
    );
    assert_eq!(entry_matches, 1, "one literal existing host entry");
    let mut helpers = FAILURE_HOST_HELPERS.to_string();
    helpers.push_str(CHECKED_FAILURE_HOST_HELPERS);
    helpers.push_str(marker);
    host = host.replacen(marker, &helpers, 1);
    eprintln!(
        "INPUT {id} complete Program={program:#?} complete prospective composed checked/failure host={host}"
    );
    if let Some(directory) = std::env::var_os("FERRULE_PRIVATE_CHECKED_CSHARP_HOST_DIR") {
        let directory = std::path::PathBuf::from(directory).join(id);
        std::fs::create_dir_all(&directory).expect("root-supplied fresh composed host directory");
        std::fs::write(directory.join("GeneratedMapping.cs"), &source)
            .expect("retain composed mapping");
        std::fs::write(
            directory.join("GeneratedTargetBuilder.cs"),
            crate::runtime::TARGET_BUILDER,
        )
        .expect("unchanged target helper");
        std::fs::write(directory.join("Program.cs"), host)
            .expect("complete composed host retained");
        std::fs::write(directory.join("Program.ir.txt"), format!("{program:#?}\n"))
            .expect("complete typed Program retained");
    }
    source
}

#[test]
fn checked_source_failure_catalogues_include_global_legacy_requirements_for_every_target() {
    use codegen::NamedInputAvailability::{Optional as O, RequiredLegacy as R};
    let p = checked_failure_program();
    let plan = codegen::legacy_named_input_admission_plan(&p);
    let source = checked_result("S519-global-catalogues", &p);
    eprintln!(
        "INPUT S519 complete Program={p:#?} RAW complete catalogue Result={plan:#?} RAW complete source Result={source:#?}"
    );
    let plan = plan.expect("complete catalogue Result captured");
    let all = checked_rows(&plan.all_outputs);
    let primary = checked_rows(&plan.primary);
    let named = plan
        .named_targets
        .iter()
        .map(|target| {
            (
                target.target_index,
                target.name,
                checked_rows(&target.catalogue),
            )
        })
        .collect::<Vec<_>>();
    let expected = vec![
        (0, "Unused", O),
        (1, "Ref", O),
        (2, " Ref 雪 ", O),
        (3, "ref", R),
        (4, "primary", O),
    ];
    eprintln!(
        "RAW S519 all={all:#?} primary={primary:#?} named={named:#?} EXPECTED independent original-order rows={expected:#?}"
    );
    assert_eq!(all, expected);
    assert_eq!(primary, expected);
    assert_eq!(named, [(0, "named", expected)]);
    let source = source.expect("complete source Result captured");
    let seam = source
        .find("private static object ExecuteCheckedBasicContext(")
        .expect("checked context seam");
    let seam = &source[seam..];
    assert!(
        seam.find("EvaluateBasicFailureRules(context);").unwrap()
            < seam.find("if (selected.HasValue)").unwrap()
    );
}

#[test]
fn checked_source_failure_host_preserves_factory_first_and_ordered_global_failure_before_targets() {
    let p = checked_failure_program();
    retain_checked_failure_host(
        "C519-ordered-selection",
        &p,
        r#"
Input=G(("Value",S("P.root")),("Rows",R(G(("Value",S("P1")),("Score",I(10))),G(("Value",S("P2")),("Score",I(20))),G(("Value",S("P3")),("Score",I(30))))));
var namedRoot=G(("Rows",R(G(("Value",S("N1")),("Score",I(30))),G(("Value",S("N2")),("Score",I(10))))));
var probe=new CheckedProbe(new[]{new NamedInput("ref",namedRoot)});
var a=CheckedFailureReach("F519-01",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectReads(a.Checked,1,1,1,1);ExpectOrderedFailure(a);
var primarySelection=FerruleTargetSelection.Primary();probe=new CheckedProbe(new[]{new NamedInput("ref",namedRoot)});
var b=CheckedFailureReach("F519-02",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,primarySelection,probe,Limits),primarySelection);ExpectReads(b.Checked,1,1,1,1);ExpectOrderedFailure(b);
var namedSelection=FerruleTargetSelection.Named("named");probe=new CheckedProbe(new[]{new NamedInput("ref",namedRoot)});
var c=CheckedFailureReach("F519-03",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,namedSelection,probe,Limits),namedSelection);ExpectReads(c.Checked,1,1,1,1);ExpectOrderedFailure(c);
Limits=new(0,0,0,0,0);var missingSelection=FerruleTargetSelection.Named("missing");probe=new CheckedProbe(new[]{new NamedInput("X",namedRoot)});
var d=CheckedFailureReach("F519-04",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,missingSelection,probe,Limits),missingSelection);ExpectInputError(d.Checked.Value,FerruleRuntimeError.UnknownTarget,"missing","project has no named target "+(char)96+"missing"+(char)96);ExpectReads(d.Checked,0,0,0,0);Check(d.Bridge.Cohorts.Length==0 && d.Checked.Value.Nodes.Length==0,"target refusal precedes factory/global failures");
Limits=new(16,8,16,1048576,64);probe=new CheckedProbe(Array.Empty<NamedInput>());
var e=CheckedFailureReach("F519-05",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,primarySelection,probe,Limits),primarySelection);ExpectInputError(e.Checked.Value,FerruleRuntimeError.MissingNamedSource,"ref","named source is required by this mapping");ExpectReads(e.Checked,1,0,1,0);Check(e.Bridge.Cohorts.Length==0 && e.Checked.Value.Nodes.Length==0,"missing global legacy requirement precedes failure walk");
probe=new CheckedProbe(new[]{new NamedInput("X",namedRoot),new NamedInput("ref",namedRoot)});
var f=CheckedFailureReach("F519-06",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,primarySelection,probe,Limits),primarySelection);ExpectInputError(f.Checked.Value,FerruleRuntimeError.UnexpectedNamedSource,"X","named source is not declared by this mapping");ExpectReads(f.Checked,1,1,1,1);Check(f.Bridge.Cohorts.Length==0 && f.Checked.Value.Nodes.Length==0,"first bad input preserves unread suffix/global failure ordering");
"#,
    );
}

#[test]
fn checked_source_failure_hosts_preserve_false_rule_laziness_and_first_selected_raw_position() {
    use codegen::FailureSelection;
    let mut success = failure_program(vec![
        expression(
            90,
            Expression::Const {
                value: Value::Bool(false),
            },
        ),
        expression(91, Expression::Raise { message: None }),
    ]);
    success.failure_rules.push(source_failure(
        &["Rows"],
        FailureSelection::WhenTrue(90),
        Some(91),
    ));
    add_target(
        &mut success,
        "named",
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_checked_failure_host(
        "C519-false-lazy",
        &success,
        r#"
Input=G(("Value",S("P.root")),("Rows",R(G(("Value",S("P1")),("Score",I(10))),G(("Value",S("P2")),("Score",I(20))),G(("Value",S("P3")),("Score",I(30))))));
var probe=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedFailureReach("F519-07",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectReads(a.Checked,1,0,1,0);ExpectTree(a.Checked.Value,"{\"Value\":\"P.root\"}");var all=(ExecutionOutputs)a.Checked.Value.Result!;Check(all.Extras.Count==1 && all.Extras[0].Name=="named" && Json(Plain(all.Extras[0].Instance))=="{\"Value\":\"P.root\"}","all ordinary outputs after successful global rule");ExpectCohort(a.Bridge,0,3,new[]{"Rows"},new long[]{1,2,3});Check(a.Bridge.Cohorts.Length==1 && a.Checked.Value.Nodes.SequenceEqual(new uint[]{90,90,90,1,1}),"false predicate complete walk before both output nodes; lazy Raise message");
var primarySelection=FerruleTargetSelection.Primary();probe=new CheckedProbe(Array.Empty<NamedInput>());var b=CheckedFailureReach("F519-08",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,primarySelection,probe,Limits),primarySelection);ExpectReads(b.Checked,1,0,1,0);ExpectTree(b.Checked.Value,"{\"Value\":\"P.root\"}");ExpectCohort(b.Bridge,0,3,new[]{"Rows"},new long[]{1,2,3});Check(b.Bridge.Cohorts.Length==1 && b.Checked.Value.Nodes.SequenceEqual(new uint[]{90,90,90,1}),"selected primary follows whole false rule");
var namedSelection=FerruleTargetSelection.Named("named");probe=new CheckedProbe(Array.Empty<NamedInput>());var c=CheckedFailureReach("F519-09",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,namedSelection,probe,Limits),namedSelection);ExpectReads(c.Checked,1,0,1,0);ExpectTree(c.Checked.Value,"{\"Value\":\"P.root\"}");ExpectCohort(c.Bridge,0,3,new[]{"Rows"},new long[]{1,2,3});Check(c.Bridge.Cohorts.Length==1 && c.Checked.Value.Nodes.SequenceEqual(new uint[]{90,90,90,1}),"selected named follows same global rule; unused static names optional");
"#,
    );
    let mut first = failure_scores(FailureSelection::WhenFalse(100));
    first
        .expressions
        .push(expression(91, Expression::Raise { message: None }));
    first
        .failure_rules
        .push(source_failure(&[], FailureSelection::All, Some(91)));
    retain_checked_failure_host(
        "C519-first-selected",
        &first,
        r#"
Input=G(("Value",S("P.root")),("Rows",R(G(("Value",S("P1")),("Score",I(10))),G(("Value",S("P2")),("Score",I(20))),G(("Value",S("P3")),("Score",I(30))))));
var probe=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedFailureReach("F519-10",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectFirstSecondRow(a);ExpectReads(a.Checked,1,0,1,0);
var primarySelection=FerruleTargetSelection.Primary();probe=new CheckedProbe(Array.Empty<NamedInput>());var b=CheckedFailureReach("F519-11",probe,()=>ExecuteSelectedCheckedOwnedBasic(Input,primarySelection,probe,Limits),primarySelection);ExpectFirstSecondRow(b);ExpectReads(b.Checked,1,0,1,0);
"#,
    );
}

const CHECKED_FAILURE_HOST_HELPERS: &str = r#"
    private sealed record CheckedFailureReached(CheckedReached Checked,BridgeReached Bridge);
    private static CheckedFailureReached CheckedFailureReach(string id,CheckedProbe probe,Func<object?> subject,FerruleTargetSelection? selection=null)
    {
        FailureCohorts.Clear();
        Console.WriteLine(Json(new {id,classification="checked_failure_input_before_subject",observedCollections=ObservedFailureCollections.Select(path=>path.ToArray()).ToArray()}));
        var actual=CheckedReach(id,probe,subject,selection);var cohorts=FailureCohorts.ToArray();
        var complete=Json(new {classification="complete_checked_failure_actual",actual.Value.CompleteInput,actual.Value.CompleteActual,actual.Factory,actual.Accessor,actual.Count,actual.Index,cohorts});
        Console.WriteLine(Json(new {id,classification="before_comparisons",completeInput=actual.Value.CompleteInput,completeActual=complete}));
        return new(actual,new(actual.Value,cohorts));
    }
    private static void ExpectOrderedFailure(CheckedFailureReached actual)
    {
        ExpectFailure(actual.Bridge,2,"N1");
        Check(actual.Bridge.Cohorts.Length==2 && actual.Bridge.Cohorts[0].Rule==1 && actual.Bridge.Cohorts[1].Rule==2,"exact global rule order and stop before third rule");
        ExpectCohort(actual.Bridge,0,3,new[]{"Rows"},new long[]{1,2,3});
        ExpectCohort(actual.Bridge,1,2,new[]{"ref","Rows"},new long[]{1,2});
        Check(actual.Checked.Value.Nodes.SequenceEqual(new uint[]{90,90,90,100,101,102,103}),"first selected named row/lazy unselected message/no target scope entered");
    }
    private static void ExpectFirstSecondRow(CheckedFailureReached actual)
    {
        ExpectFailure(actual.Bridge,1,"P2");ExpectCohort(actual.Bridge,0,3,new[]{"Rows"},new long[]{1,2,3});
        Check(actual.Bridge.Cohorts.Length==1 && actual.Bridge.Cohorts[0].Rule==1,"second global rule never reached");
        Check(actual.Checked.Value.Nodes.SequenceEqual(new uint[]{100,101,102,100,101,102,103}),"predicate visits only first two raw items; message only on selected raw position2; no target output");
    }
"#;
