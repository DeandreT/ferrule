use super::*;

fn checked_program() -> Program {
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(
                2,
                Expression::SourceField {
                    path: path(&["Ref", "Value"]),
                    frame: None,
                },
            ),
            expression(3, field(named("ref"), &["Value"], None)),
            expression(
                99,
                Expression::Const {
                    value: Value::String("unused.json".into()),
                },
            ),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.extra_sources[0].dynamic = Some(codegen::DynamicSourceProgram {
        path: 99,
        driver: codegen::SourceIteration::new(Vec::new()).into(),
    });
    add_target(
        &mut p,
        "legacy",
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    add_target(
        &mut p,
        "owned",
        group(vec![binding("Value", 3, ScalarType::String)]),
    );
    p
}
pub(super) fn checked_rows<'a>(
    catalogue: &codegen::NamedInputCatalogue<'a>,
) -> Vec<(usize, &'a str, codegen::NamedInputAvailability)> {
    catalogue
        .entries
        .iter()
        .map(|entry| (entry.declaration_index, entry.name, entry.availability))
        .collect()
}
pub(super) fn checked_result(label: &str, program: &Program) -> Result<String, EmitError> {
    let actual = crate::render_checked_private_owned_basic_mapping(program);
    eprintln!(
        "{label} complete typed Program={program:#?} complete checked source Result={actual:#?}"
    );
    actual
}
pub(super) fn complete_checked_host(program: &Program, body: &str) -> String {
    let named=program.extra_targets.iter().enumerate().map(|(index,target)|format!("new {{ index={index},name={},entries=CheckedCatalogueSnapshot(CheckedNamedInputs_{index}) }}",crate::literal::string(&target.name))).collect::<Vec<_>>().join(",");
    let extension = CHECKED_HOST_EXTENSION.replace(
        "__NAMED_CATALOGUES__",
        &format!("new object[] {{ {named} }}"),
    );
    let mut host = HOST_PREFIX.to_string();
    host.push_str(HOST_ERROR_SNAPSHOT);
    host.push_str(&extension);
    host.push_str("\n    public static void Main()\n    {\n");
    host.push_str(body);
    host.push_str(HOST_SUFFIX);
    host
}
pub(super) fn retain_checked_host(id: &str, program: &Program, body: &str) -> String {
    let source = checked_result(id, program).expect("full checked source result captured");
    let host = complete_checked_host(program, body);
    eprintln!("{id} complete prospective checked host={host}");
    if let Some(directory) = std::env::var_os("FERRULE_PRIVATE_CHECKED_CSHARP_HOST_DIR") {
        let directory = std::path::PathBuf::from(directory).join(id);
        std::fs::create_dir_all(&directory).expect("root-supplied fresh checked host directory");
        std::fs::write(directory.join("GeneratedMapping.cs"), &source)
            .expect("retain checked mapping");
        std::fs::write(
            directory.join("GeneratedTargetBuilder.cs"),
            crate::runtime::TARGET_BUILDER,
        )
        .expect("unchanged target helper");
        std::fs::write(directory.join("Program.cs"), host).expect("full host retained");
        std::fs::write(directory.join("Program.ir.txt"), format!("{program:#?}\n"))
            .expect("full typed Program retained");
    }
    source
}

#[test]
fn checked_catalogues_source_and_prepared_entries_are_separate_and_complete() {
    use codegen::NamedInputAvailability::{
        Optional as O, RequiredLegacy as R, ReservedUnavailable as U,
    };
    let p = checked_program();
    let plan = codegen::legacy_named_input_admission_plan(&p);
    eprintln!("S01 complete Program={p:#?} complete plan={plan:#?}");
    let plan = plan.expect("raw plan captured");
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
    eprintln!("S01 all={all:#?} primary={primary:#?} named={named:#?}");
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
    let old = render_basic_owned_mapping(&p);
    eprintln!("S02 prepared full source={old:#?}");
    let old = old.expect("prepared captured");
    let checked = checked_result("S02_S03", &p).expect("checked captured");
    assert!(checked.starts_with(&old));
    let tail = &checked[old.len()..];
    assert!(tail.contains("new(\" Ref \\u96EA \", global::Ferrule.Runtime.FerruleBasicNamedSourceMode.Optional), // declaration 2"));
    assert!(tail.contains("private readonly struct CheckedNamedInputAccessor : global::Ferrule.Runtime.IFerruleNamedSourceInputAccessor<NamedInput>"));
    assert!(tail.contains("return item is null ? default : new(item.Name, item.Instance);"));
    assert!(tail.contains("source, catalogue, accessor.Supplied, accessor, limits"));
    assert!(!tail.contains(".ToArray("));
    assert!(!tail.contains(".ToList("));
    assert!(!tail.contains("new global::Ferrule.Runtime.FerruleOwnedSourceLimits"));
    let selected = tail
        .find("private static SelectedTargetOutput ExecuteSelectedCheckedOwnedBasic")
        .unwrap();
    let selected = &tail[selected..];
    assert!(
        selected.find("ResolveOwnedBasicTarget(selection)").unwrap()
            < selected.find("ObserveCheckedInputBoundary(true)").unwrap()
    );
    assert_eq!(
        tail.matches("ExecuteCheckedBasicContext(context,").count(),
        2
    );
}

#[test]
fn checked_selected_catalogues_host_keeps_unrelated_legacy_requirements_lazy() {
    retain_checked_host(
        "C538-selection",
        &checked_program(),
        r#"
var supplied=new[]{new NamedInput("ref",Declared[3].Instance!),new NamedInput("Ref",Declared[1].Instance!)};
var p=new CheckedProbe(supplied);var a=CheckedReach("H01",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(a.Value,"{\"Value\":\"P.root\"}");var all=(ExecutionOutputs)a.Value.Result!;Check(all.Extras.Count==2,"two exact target outputs");Check(all.Extras[0].Name=="legacy" && Json(Plain(all.Extras[0].Instance))=="{\"Value\":\"P.collision\"}" && all.Extras[1].Name=="owned" && Json(Plain(all.Extras[1].Instance))=="{\"Value\":\"N.lower\"}","all targets/order");ExpectReads(a,1,2,1,2);
p=new CheckedProbe(Array.Empty<NamedInput>());var b=CheckedReach("H02",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Primary(),p,Limits),FerruleTargetSelection.Primary());ExpectTree(b.Value,"{\"Value\":\"P.root\"}");ExpectReads(b,1,0,1,0);
p=new CheckedProbe(Array.Empty<NamedInput>());var c=CheckedReach("H03 absent",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Named("legacy"),p,Limits),FerruleTargetSelection.Named("legacy"));ExpectInputError(c.Value,FerruleRuntimeError.MissingNamedSource,"Ref","named source is required by this mapping");ExpectReads(c,1,0,1,0);
p=new CheckedProbe(new[]{supplied[1]});var d=CheckedReach("H03 present",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Named("legacy"),p,Limits),FerruleTargetSelection.Named("legacy"));ExpectTree(d.Value,"{\"Value\":\"P.collision\"}");ExpectReads(d,1,1,1,1);
p=new CheckedProbe(new[]{supplied[0]});var e=CheckedReach("H04",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Named("owned"),p,Limits),FerruleTargetSelection.Named("owned"));ExpectTree(e.Value,"{\"Value\":\"N.lower\"}");ExpectReads(e,1,1,1,1);
Limits=new(0,0,0,0,0);p=new CheckedProbe(new[]{new NamedInput("X",Input)});var f=CheckedReach("H05",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Named("missing"),p,Limits),FerruleTargetSelection.Named("missing"));ExpectInputError(f.Value,FerruleRuntimeError.UnknownTarget,"missing","project has no named target `missing`");ExpectReads(f,0,0,0,0);
Limits=new(16,8,16,1048576,64);p=new CheckedProbe(Array.Empty<NamedInput>());var g=CheckedReach("H08 absent",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Primary(),p,Limits),FerruleTargetSelection.Primary());ExpectTree(g.Value,"{\"Value\":\"P.root\"}");ExpectReads(g,1,0,1,0);
p=new CheckedProbe(new[]{new NamedInput("Unused",Input)});var h=CheckedReach("H08 supplied",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Primary(),p,Limits),FerruleTargetSelection.Primary());ExpectInputError(h.Value,FerruleRuntimeError.UnexpectedNamedSource,"Unused","named source is not declared by this mapping");ExpectReads(h,1,1,1,1);
"#,
    );
}

#[test]
fn checked_optional_and_required_input_hosts_keep_exact_error_classes() {
    let owned = basic(
        vec![expression(1, field(named("Ref"), &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_checked_host(
        "C538-optional",
        &owned,
        r#"var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H06",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectOwned(a.Value,FerruleOwnedSourceErrorKind.SourceUnavailable,false,"Ref",null);ExpectReads(a,1,0,1,0);p=new CheckedProbe(new[]{new NamedInput("Ref",Declared[1].Instance!)});var b=CheckedReach("H07",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(b.Value,"{\"Value\":\"N.root\"}");ExpectReads(b,1,1,1,1);"#,
    );
    let legacy = basic(
        vec![expression(
            1,
            Expression::SourceField {
                path: path(&["Ref", "Value"]),
                frame: None,
            },
        )],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_checked_host(
        "C538-required",
        &legacy,
        r#"var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H03 required",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectInputError(a.Value,FerruleRuntimeError.MissingNamedSource,"Ref","named source is required by this mapping");ExpectReads(a,1,0,1,0);p=new CheckedProbe(new[]{new NamedInput("Ref",Declared[1].Instance!)});var b=CheckedReach("H03 supplied",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(b.Value,"{\"Value\":\"P.collision\"}");ExpectReads(b,1,1,1,1);"#,
    );
}

#[test]
fn checked_supplied_records_host_captures_order_null_and_caller_failures() {
    let p = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_checked_host(
        "C538-supplied",
        &p,
        r#"
var r=Declared[1].Instance!;var different=G(("Value",S("different")));
var scenarios=new[]{
("H09",new[]{new NamedInput("X",r),new NamedInput("Ref",r)},FerruleRuntimeError.UnexpectedNamedSource,"X","named source is not declared by this mapping",1),
("H10",new[]{new NamedInput("Ref",r),new NamedInput("Ref",r)},FerruleRuntimeError.DuplicateNamedSource,"Ref","named source was supplied more than once",2),
("H11",new[]{new NamedInput("Ref",r),new NamedInput("Ref",different)},FerruleRuntimeError.DuplicateNamedSource,"Ref","named source was supplied more than once",2),
("H12",new[]{new NamedInput("",r)},FerruleRuntimeError.UnexpectedNamedSource,"","named source is not declared by this mapping",1),
("H13 unknown",new[]{new NamedInput("X",r),new NamedInput("Ref",r),new NamedInput("Ref",r)},FerruleRuntimeError.UnexpectedNamedSource,"X","named source is not declared by this mapping",1),
("H13 duplicate",new[]{new NamedInput("Ref",r),new NamedInput("Ref",r),new NamedInput("X",r)},FerruleRuntimeError.DuplicateNamedSource,"Ref","named source was supplied more than once",2)};
foreach(var row in scenarios){var probe=new CheckedProbe(row.Item2);var actual=CheckedReach(row.Item1,probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectInputError(actual.Value,row.Item3,row.Item4,row.Item5);ExpectReads(actual,1,row.Item6,1,row.Item6);}
foreach(var row in new[]{("H26",new NamedInput[]{null!}),("H29",new[]{new NamedInput(null!,r)}),("H30",new[]{new NamedInput("Ref",null!)})}){var probe=new CheckedProbe(row.Item2);var actual=CheckedReach(row.Item1,probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));Check(actual.Value.Error is ArgumentNullException e && e.ParamName=="supplied" && e.InnerException is null,"exact null argument, no NullReferenceException");ExpectReads(actual,1,1,1,1);}
var invalid=new CheckedProbe(new[]{new NamedInput("\uD800",r)});var bad=CheckedReach("H31",invalid,()=>ExecuteCheckedOwnedBasic(Input,invalid,Limits));Check(bad.Value.Error is ArgumentException unicode && unicode.ParamName=="supplied" && unicode.InnerException is null && bad.Value.Error is not ArgumentNullException,"well-formed UTF16 refusal");ExpectReads(bad,1,1,1,1);
var count=new CheckedProbe(new[]{new NamedInput("Ref",r)},throwCount:true);var ca=CheckedReach("H27",count,()=>ExecuteCheckedOwnedBasic(Input,count,Limits));Check(ReferenceEquals(ca.Value.Error,count.Failure),"caller Count exception exact reference");ExpectReads(ca,1,0,1,0);
var index=new CheckedProbe(new[]{new NamedInput("Ref",r)},throwIndex:true);var ix=CheckedReach("H28",index,()=>ExecuteCheckedOwnedBasic(Input,index,Limits));Check(ReferenceEquals(ix.Value.Error,index.Failure),"caller item exception exact reference");ExpectReads(ix,1,0,1,1);
"#,
    );
}

#[test]
fn checked_exact_name_host_preserves_catalogue_order_and_unicode() {
    let p = basic(
        vec![
            expression(1, field(named(" Ref 雪 "), &["Value"], None)),
            expression(2, field(named("ref"), &["Value"], None)),
            expression(3, field(named("primary"), &["Value"], None)),
        ],
        group(vec![
            binding("Exact", 1, ScalarType::String),
            binding("Lower", 2, ScalarType::String),
            binding("LiteralPrimary", 3, ScalarType::String),
        ]),
    );
    retain_checked_host(
        "C538-exact",
        &p,
        r#"var p=new CheckedProbe(new[]{new NamedInput("primary",Declared[4].Instance!),new NamedInput("ref",Declared[3].Instance!),new NamedInput(" Ref 雪 ",Declared[2].Instance!)});var a=CheckedReach("H14",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(a.Value,"{\"Exact\":\"N.exact\",\"Lower\":\"N.lower\",\"LiteralPrimary\":\"N.literal.primary\"}");ExpectReads(a,1,3,1,3);Check(CheckedAllInputs.Select(d=>d.Name).SequenceEqual(new[]{"Unused","Ref"," Ref 雪 ","ref","primary"}),"complete original catalogue/order/ordinals");"#,
    );
}

#[test]
fn checked_five_limit_hosts_preserve_policy_and_typed_causes() {
    let p = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    retain_checked_host(
        "C538-limits",
        &p,
        r#"
var probe=new CheckedProbe(new[]{new NamedInput("Ref",Declared[1].Instance!)});Limits=new(6,8,16,1048576,64);var a=CheckedReach("H15",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectTree(a.Value,"{\"Value\":\"P.root\"}");ExpectReads(a,1,1,1,1);
probe=new CheckedProbe(new[]{new NamedInput("Ref",Declared[1].Instance!)});Limits=new(5,8,16,1048576,64);var b=CheckedReach("H16",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectExactLimit(b.Value,FerruleOwnedSourceLimitKind.Roots,0,6,5);ExpectReads(b,1,0,0,0);
probe=new CheckedProbe(new[]{new NamedInput("Ref",Declared[1].Instance!)});Limits=new(16,8,16,0,64);var c=CheckedReach("H17",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectExactLimit(c.Value,FerruleOwnedSourceLimitKind.MetadataBytes,0,6,0);ExpectReads(c,1,0,0,0);
probe=new CheckedProbe(Array.Empty<NamedInput>());Limits=new(16,8,0,1048576,64);var d=CheckedReach("H18",probe,()=>ExecuteCheckedOwnedBasic(Input,probe,Limits));ExpectExactLimit(d.Value,FerruleOwnedSourceLimitKind.PathSegments,0,1,0);ExpectReads(d,1,0,1,0);
"#,
    );
    let mut walk = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], Some(&[]))),
            expression(2, position(SourceOwner::Primary, Some(&[]))),
        ],
        owned_iteration(
            SourceOwner::Primary,
            &[],
            vec![
                binding("Value", 1, ScalarType::String),
                binding("Position", 2, ScalarType::Int),
            ],
        ),
    );
    walk.extra_sources.clear();
    retain_checked_host(
        "C538-walk",
        &walk,
        r#"Input=R(G(("Value",S("flat1"))),G(("Value",S("flat2"))));var p=new CheckedProbe(Array.Empty<NamedInput>());Limits=new(1,1,1,4096,2);var a=CheckedReach("H20 inclusive",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(a.Value,"[{\"Value\":\"flat1\",\"Position\":1},{\"Value\":\"flat2\",\"Position\":2}]");ExpectReads(a,1,0,1,0);p=new CheckedProbe(Array.Empty<NamedInput>());Limits=new(1,0,1,4096,2);var b=CheckedReach("H19",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectExactLimit(b.Value,FerruleOwnedSourceLimitKind.Frames,0,1,0);ExpectReads(b,1,0,1,0);p=new CheckedProbe(Array.Empty<NamedInput>());Limits=new(1,1,1,4096,1);var c=CheckedReach("H20 refused",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectPairedWalkLimit(c.Value);ExpectReads(c,1,0,1,0);Check(b.Value.Result is null && c.Value.Result is null,"no partial output");"#,
    );
}

#[test]
fn checked_lazy_and_raise_hosts_keep_legacy_eager_admission_and_selected_targets() {
    for (name, flag, body) in [
        (
            "C538-lazy-true",
            true,
            r#"var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H21",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(a.Value,"{\"Value\":\"chosen\"}");ExpectReads(a,1,0,1,0);Check(!a.Value.Nodes.Contains(3u),"unchosen owned branch not reached");"#,
        ),
        (
            "C538-lazy-false",
            false,
            r#"var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H22",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectOwned(a.Value,FerruleOwnedSourceErrorKind.SourceUnavailable,false,"Unused",null);ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.Count(n=>n==3u)==1,"chosen owned branch reached once");"#,
        ),
    ] {
        let p = basic(
            vec![
                expression(
                    1,
                    Expression::Const {
                        value: Value::Bool(flag),
                    },
                ),
                expression(
                    2,
                    Expression::Const {
                        value: Value::String("chosen".into()),
                    },
                ),
                expression(3, field(named("Unused"), &["Value"], None)),
                expression(
                    4,
                    Expression::If {
                        condition: 1,
                        then: 2,
                        else_: 3,
                    },
                ),
            ],
            group(vec![binding("Value", 4, ScalarType::String)]),
        );
        retain_checked_host(name, &p, body);
    }
    let eager = basic(
        vec![
            expression(
                1,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(
                2,
                Expression::Const {
                    value: Value::String("chosen".into()),
                },
            ),
            expression(
                3,
                Expression::SourceField {
                    path: path(&["Ref", "Value"]),
                    frame: None,
                },
            ),
            expression(
                4,
                Expression::If {
                    condition: 1,
                    then: 2,
                    else_: 3,
                },
            ),
        ],
        group(vec![binding("Value", 4, ScalarType::String)]),
    );
    retain_checked_host(
        "C538-potential-legacy",
        &eager,
        r#"var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H23",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectInputError(a.Value,FerruleRuntimeError.MissingNamedSource,"Ref","named source is required by this mapping");ExpectReads(a,1,0,1,0);Check(a.Value.Nodes.Length==0,"admission before any expression");"#,
    );
    let mut raise = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(71, Expression::Raise { message: None }),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    add_target(
        &mut raise,
        "fail",
        group(vec![binding("Value", 71, ScalarType::String)]),
    );
    retain_checked_host(
        "C538-raise",
        &raise,
        r#"var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H24 primary",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Primary(),p,Limits),FerruleTargetSelection.Primary());ExpectTree(a.Value,"{\"Value\":\"P.root\"}");Check(!a.Value.Nodes.Contains(71u),"unselected ordinary Raise stays inert");ExpectReads(a,1,0,1,0);p=new CheckedProbe(Array.Empty<NamedInput>());var b=CheckedReach("H24 all",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));var e=ExpectRuntime(b.Value,FerruleRuntimeError.MappingException);Check(e.Node==71 && e.MappingExceptionMessage is null && b.Value.Result is null,"exact ordinary Raise payload/no partial outcome");ExpectReads(b,1,0,1,0);p=new CheckedProbe(Array.Empty<NamedInput>());var c=CheckedReach("H24 named",p,()=>ExecuteSelectedCheckedOwnedBasic(Input,FerruleTargetSelection.Named("fail"),p,Limits),FerruleTargetSelection.Named("fail"));var f=ExpectRuntime(c.Value,FerruleRuntimeError.MappingException);Check(f.Node==71 && f.MappingExceptionMessage is null && c.Value.Nodes.SequenceEqual(new uint[]{71}),"selected ordinary Raise only");ExpectReads(c,1,0,1,0);"#,
    );
}

#[test]
fn checked_empty_catalogue_scalar_root_has_no_implicit_policy() {
    let mut p = basic(
        vec![expression(1, field(SourceOwner::Primary, &[], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.source = SchemaNode::scalar("Input", ScalarType::String);
    p.extra_sources.clear();
    retain_checked_host(
        "C538-zero",
        &p,
        r#"Input=S("P.root");Limits=new(1,0,0,0,0);var p=new CheckedProbe(Array.Empty<NamedInput>());var a=CheckedReach("H25",p,()=>ExecuteCheckedOwnedBasic(Input,p,Limits));ExpectTree(a.Value,"{\"Value\":\"P.root\"}");ExpectReads(a,1,0,1,0);"#,
    );
}

const CHECKED_HOST_EXTENSION: &str = r#"
    private static int CheckedFactoryCalls,CheckedAccessorReads;
    static partial void ObserveCheckedInputBoundary(bool factory) { if(factory)CheckedFactoryCalls++;else CheckedAccessorReads++; }
    private sealed class CheckedProbe : IReadOnlyList<NamedInput>
    {
        private readonly NamedInput[] _items;private readonly bool _throwCount,_throwIndex;
        internal CheckedProbe(NamedInput[] items,bool throwCount=false,bool throwIndex=false) { _items=items;_throwCount=throwCount;_throwIndex=throwIndex; }
        internal NamedInput[] CapturedItems=>_items;
        internal bool ThrowsCount=>_throwCount;internal bool ThrowsIndex=>_throwIndex;
        internal readonly Exception Failure=new InvalidOperationException("checked caller sentinel");
        internal int CountReads,IndexReads;
        public int Count {get{CountReads++;if(_throwCount)throw Failure;return _items.Length;}}
        public NamedInput this[int index]{get{IndexReads++;if(_throwIndex)throw Failure;return _items[index];}}
        public IEnumerator<NamedInput> GetEnumerator()=>throw new InvalidOperationException("input enumeration forbidden");
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator()=>GetEnumerator();
    }
    private sealed record CheckedReached(Reached Value,int Factory,int Accessor,int Count,int Index);
    private static object CheckedCatalogueSnapshot(IReadOnlyList<FerruleBasicNamedSourceDeclaration> catalogue)=>catalogue.Select((d,index)=>new {index,d.Name,units=d.Name.Select(c=>(int)c).ToArray(),mode=d.Mode.ToString()}).ToArray();
    private static CheckedReached CheckedReach(string id,CheckedProbe probe,Func<object?> subject,FerruleTargetSelection? selection=null)
    {
        NodeTrace.Clear();FrameTrace.Clear();CheckedFactoryCalls=0;CheckedAccessorReads=0;probe.CountReads=0;probe.IndexReads=0;
        var input=Json(new {classification="complete_checked_typed_input",source=InstanceSnapshot(Input),supplied=probe.CapturedItems.Select(item=>item is null?null:new {item.Name,units=item.Name?.Select(c=>(int)c).ToArray(),instance=InstanceSnapshot(item.Instance)}).ToArray(),
            selection=selection is null?null:new {selection.IsPrimary,selection.Name},limits=new {roots=(ulong)Limits.MaxRoots,frames=(ulong)Limits.MaxActiveFrames,segments=(ulong)Limits.MaxPathSegments,bytes=(ulong)Limits.MaxMetadataBytes,contexts=(ulong)Limits.MaxWalkContexts},
            execution=Execution is null?null:new {Execution.MappingFilePath,Execution.MainMappingFilePath,Execution.CurrentDateTime,parameterCount=Execution.RuntimeParameters.Count,parameters=ParameterValues.Select(p=>new {p.Key,value=ValueSnapshot(p.Value)}).ToArray()},
            catalogueAll=CheckedCatalogueSnapshot(CheckedAllInputs),cataloguePrimary=CheckedCatalogueSnapshot(CheckedPrimaryInputs),catalogueNamed=__NAMED_CATALOGUES__,probeCountThrows=probe.ThrowsCount,probeIndexThrows=probe.ThrowsIndex });
        Console.WriteLine(Json(new {id,classification="before_subject",completeInput=input}));object? result=null;Exception? error=null;
        try {result=subject();}catch(Exception actual){error=actual;}
        var nodes=NodeTrace.ToArray();var frames=FrameTrace.ToArray();var factory=CheckedFactoryCalls;var accessor=CheckedAccessorReads;var count=probe.CountReads;var index=probe.IndexReads;
        var complete=Json(new {classification="complete_checked_actual",result=ResultSnapshot(result),error=error is null?null:ErrorSnapshot(error),nodes,frames,factoryCalls=factory,accessorReads=accessor,countReads=count,indexReads=index});
        Console.WriteLine(Json(new {id,classification="before_comparisons",completeInput=input,completeActual=complete}));return new(new(result,error,nodes,frames,input,complete),factory,accessor,count,index);
    }
    private static void ExpectReads(CheckedReached actual,int factory,int accessor,int count,int index)
    { Check(actual.Factory==factory && actual.Accessor==accessor && actual.Count==count && actual.Index==index,"exact factory/accessor/list reached calls, not allocations"); }
    private static void ExpectInputError(Reached actual,FerruleRuntimeError category,string detail,string message)
    {
        var e=ExpectRuntime(actual,category);Check(e.Detail==detail && e.Message==message && e.InnerException is null,"exact category/detail/message/chain");
        Check(e.Node is null && e.Function is null && e.ExpectedArity is null && e.ActualArity is null && e.FoundKind is null && e.AggregateOperation is null && e.RequestedItems is null && e.MaximumItems is null && e.MaximumDepth is null && e.RuntimeValue is null && e.FailureRule is null && e.MappingFailureMessage is null && e.MappingExceptionMessage is null && e.Join is null && e.UserFunction is null && e.FunctionParameter is null && e.ExpectedScalarType is null && e.RuntimeParameter is null && e.SourceField is null && e.FoundInstance is null && e.PrimaryRoot is null && e.OwnedSourceFailure is null && e.BasicSourceBridgeFailure is null && actual.Result is null,"full untouched typed envelope/no partial output");
    }
    private static void ExpectPairedWalkLimit(Reached actual)
    {
        var outer=ExpectRuntime(actual,FerruleRuntimeError.BasicSourceBridge);var cause=outer.BasicSourceBridgeFailure;
        Check(cause is not null && ReferenceEquals(cause,outer.InnerException),"exact bare paired inner cause");var error=cause!;
        Check(error.Error==FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit && error.Component==FerruleBasicSourceBridgeComponent.WalkContexts && error.Used==0 && error.Requested==2 && error.Maximum==1,"full paired0/2/1 WalkContexts payload");
        Check(error.InputOrdinal is null && error.CandidateOrdinal is null && error.DeclarationIndex is null && error.DeclaredCount is null && error.Found is null && error.InnerException is null,"unchanged absent paired witness fields");
        Check(outer.Node is null && outer.Function is null && outer.ExpectedArity is null && outer.ActualArity is null && outer.FoundKind is null && outer.AggregateOperation is null && outer.Detail is null && outer.RequestedItems is null && outer.MaximumItems is null && outer.MaximumDepth is null && outer.RuntimeValue is null && outer.FailureRule is null && outer.MappingFailureMessage is null && outer.MappingExceptionMessage is null && outer.Join is null && outer.UserFunction is null && outer.FunctionParameter is null && outer.ExpectedScalarType is null && outer.RuntimeParameter is null && outer.SourceField is null && outer.FoundInstance is null && outer.PrimaryRoot is null && outer.OwnedSourceFailure is null,"complete untouched outer paired envelope");
        Check(actual.Result is null && actual.Nodes.Length==0,"paired refusal before any expression or partial outcome");
    }
    private static void ExpectExactLimit(Reached actual,FerruleOwnedSourceLimitKind kind,nuint used,nuint requested,nuint maximum)
    {var error=ExpectOwned(actual,FerruleOwnedSourceErrorKind.MetadataLimit,null,null,null);Check(error.LimitKind==kind && error.Used==used && error.Requested==requested && error.Maximum==maximum,"complete exact limit payload");}
"#;

#[test]
fn checked_inventory_preserves_full_dynamic_refusal_and_present_empty_frame() {
    let mut p = checked_program();
    p.expressions[0].expression = Expression::SourceField {
        path: path(&["Unused", "Value"]),
        frame: None,
    };
    let actual = checked_result("S04 retained dynamic path", &p);
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
    match actual {
        Err(EmitError::ProgramValidation(error)) => assert_eq!(error, expected),
        other => panic!("complete raw refusal captured: {other:#?}"),
    }
    p.expressions[0].expression = Expression::SourceField {
        path: path(&["Unused", "Value"]),
        frame: Some(Vec::new()),
    };
    let actual = checked_result("S04 present empty frame", &p);
    assert!(actual.is_ok());
    let source = actual.expect("full source captured");
    let prepared = render_basic_owned_mapping(&p);
    eprintln!("S04 prepared present-empty source={prepared:#?}");
    assert!(source.starts_with(&prepared.expect("prepared result captured")));
}

// Private renderer with ordinary source failures (#542): rule selection, lazy
// messages, selected targets and bridge errors in generated hosts. The
// control-free renders above stay byte-for-byte unchanged.
