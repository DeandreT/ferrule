use super::*;

fn retain_public_sort_consumer(label: &str, program: &Program, body: &str) -> String {
    eprintln!("INPUT {label} complete sorting Program={program:#?}");
    let actual = crate::emit_with_basic_owned_sort(program);
    eprintln!("RAW {label} complete sorting artifact result={actual:#?}");
    let artifacts = actual.expect("complete artifact/refusal retained before propagation");
    let source = String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .expect("complete emitted mapping is UTF-8");
    let host = public_basic_consumer(body, false);
    eprintln!("RAW {label} complete independent public consumer={host}");
    if let Some(base) = std::env::var_os("FERRULE_PUBLIC_BASIC_CSHARP_HOST_DIR") {
        let base = std::path::PathBuf::from(base);
        assert!(
            base.is_absolute(),
            "root supplies a fresh absolute evidence namespace"
        );
        let root = base.join(label);
        std::fs::create_dir(&root).expect("fresh consumer namespace; preserve failure evidence");
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
            reached.sync_all().unwrap();
        }
        let input_original = format!("{program:#?}\n");
        for (name, bytes) in [
            (
                "consumer/PublicConsumer.csproj",
                PUBLIC_BASIC_CONSUMER_PROJECT.as_bytes(),
            ),
            ("consumer/Program.cs", host.as_bytes()),
            ("Program.ir.original.txt", input_original.as_bytes()),
        ] {
            let mut reached = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join(name))
                .unwrap();
            std::io::Write::write_all(&mut reached, bytes).unwrap();
            reached.sync_all().unwrap();
        }
    }
    source
}

fn sort_669(
    scope: &mut TargetScope,
    predicate: Option<u32>,
    primary: codegen::SortKey,
    secondary: Vec<codegen::SortKey>,
    order: codegen::SortFilterOrder,
) {
    let input = scope.iteration.as_ref().unwrap().input().clone();
    scope.iteration = Some(IterationPlan::new(
        input,
        predicate,
        Some(codegen::SortPlan::new(primary, secondary, order)),
        vec![],
        IterationOutput::Repeated,
    ));
}
pub(super) fn sort_669_scope_control() -> EmitError {
    let scope = ::mapping::SourceScopeLocation::root(::mapping::SourceScopeTarget::Primary);
    EmitError::ProgramValidation(ProgramValidationError::OwnedSource {
        diagnostic: Box::new(codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Iteration {
                scope: scope.clone(),
            },
            request: Some(codegen::OwnedSourceDiagnosticRequest::Iteration {
                owner: (&SourceOwner::Primary).into(),
                path: path(&["Rows"]),
            }),
            reason: codegen::OwnedSourceCapabilityReason::ScopeControl,
            origin: codegen::OwnedSourceCapabilityOrigin::Scope { scope },
        }),
    })
}
pub(super) fn sort_669_program(order: codegen::SortFilterOrder, predicate: Option<u32>) -> Program {
    let mut root = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![
            binding("Value", 3, ScalarType::String),
            binding("OwnedPosition", 1, ScalarType::Int),
            binding("LegacyPosition", 4, ScalarType::Int),
        ],
    );
    sort_669(
        &mut root,
        predicate,
        codegen::SortKey {
            expression: 20,
            descending: false,
        },
        vec![],
        order,
    );
    let mut p = basic(
        vec![
            expression(1, position(SourceOwner::Primary, Some(&["Rows"]))),
            expression(
                2,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(
                3,
                field(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
            ),
            expression(
                4,
                Expression::Position {
                    collection: path(&[]),
                },
            ),
            expression(
                20,
                field(SourceOwner::Primary, &["Rows", "Score"], Some(&["Rows"])),
            ),
        ],
        root.clone(),
    );
    p.extra_sources.clear();
    add_target(&mut p, "Sorted", root);
    p
}
const PUBLIC669_ORDERED: &str = r#"
var input=G(("Value",S("P.root")),("Rows",R(G(("Value",S("A")),("Score",I(30))),G(("Value",S("B")),("Score",I(10))),G(("Value",S("C")),("Score",I(20))))));
var limits=Policy();var supplied=Array.Empty<NamedInput>();var primary=FerruleTargetSelection.Primary();var named=FerruleTargetSelection.Named("Sorted");
var p0=new Probe(supplied);var a=Reach("N669-G01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(supplied);var b=Reach("N669-G02",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(supplied);var c=Reach("N669-G03",input,p2,limits,null,named,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,named,p2,limits,null));
var expected="[{\"Value\":\"B\",\"OwnedPosition\":1,\"LegacyPosition\":1},{\"Value\":\"C\",\"OwnedPosition\":2,\"LegacyPosition\":2},{\"Value\":\"A\",\"OwnedPosition\":3,\"LegacyPosition\":3}]";
Tree(a,expected);Tree(b,expected);Tree(c,expected);
var all=(ExecutionOutputs)a.Result!;Check(all.Extras.Count==1 && all.Extras[0].Name=="Sorted" && Json(Plain(all.Extras[0].Instance))==expected,"complete extra sorted tree");Reads(a,1,0);Reads(b,1,0);Reads(c,1,0);
"#;
const PUBLIC669_TYPED_ERRORS: &str = r#"
void Clr(Exception error,string type)
{
    Check(error.GetType().FullName==type && error.HResult==unchecked((int)0x80131500) && error.ToString().StartsWith(type+": "+error.Message,StringComparison.Ordinal),"exact CLR type/HResult/native prefix; full stack retained in raw snapshot");
}
void MappingRaise(Actual actual,uint node,string message)
{
    var error=Runtime(actual,FerruleRuntimeError.MappingException);
    Check(error.Node==node && error.MappingExceptionMessage==message && error.Message==$"node {node}: mapping exception: {message}" && error.InnerException is null,"complete sorting MappingException payload");
    NullMetadata(error,"Node","MappingExceptionMessage");Clr(error,"Ferrule.Runtime.FerruleRuntimeException");
}
void MissingOwnedKey(Actual actual,uint node)
{
    OwnedAbsent(actual,"Ref",node);var error=(FerruleRuntimeException)actual.Error!;
    Clr(error,"Ferrule.Runtime.FerruleRuntimeException");Clr(error.OwnedSourceFailure!,"Ferrule.Runtime.FerruleOwnedSourceException");
    Check(error.ToString().Contains(" ---> Ferrule.Runtime.FerruleOwnedSourceException: Owned source operation failed.",StringComparison.Ordinal),"full CLR native inner cause retained");
}
"#;
#[test]
fn public_basic_sort_companion_preserves_existing_bytes_and_closed_routes() {
    let plain = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let old = crate::emit_with_basic_owned_sources(&plain);
    let filtered = crate::emit_with_basic_owned_filters(&plain);
    let sorted = crate::emit_with_basic_owned_sort(&plain);
    let private_plain = render_basic_owned_mapping(&plain);
    let public_plain = crate::owned_basic::render_public_basic_owned_mapping(&plain);
    let public_filter_plain = crate::owned_basic::render_public_basic_owned_capability_mapping(
        &plain,
        codegen::OwnedPreset::Filters.capabilities(),
    );
    let public_sort_plain = crate::owned_basic::render_public_basic_owned_capability_mapping(
        &plain,
        codegen::OwnedPreset::Sort.capabilities(),
    );
    eprintln!(
        "N669 compatibility complete input={plain:#?} old={old:#?} filter={filtered:#?} sort={sorted:#?} private={private_plain:#?} public={public_plain:#?} public_filter={public_filter_plain:#?} public_sort={public_sort_plain:#?}"
    );
    let old = old.unwrap();
    let filtered = filtered.unwrap();
    let sorted = sorted.unwrap();
    assert_public_basic_typed_artifacts_match(&plain, &old, &filtered);
    assert_public_basic_typed_artifacts_match(&plain, &old, &sorted);
    assert_eq!(filtered, sorted);
    assert_eq!(public_plain, public_filter_plain);
    assert_eq!(public_plain, public_sort_plain);
    assert!(private_plain.is_ok());
    let p = sort_669_program(codegen::SortFilterOrder::FilterThenSort, Some(2));
    let old = crate::emit_with_basic_owned_sources(&p);
    let filter = crate::emit_with_basic_owned_filters(&p);
    let private = render_basic_owned_mapping(&p);
    eprintln!(
        "N669 sorted complete input={p:#?} old={old:#?} filter={filter:#?} private={private:#?}"
    );
    for value in [old.map(|_| ()), filter.map(|_| ()), private.map(|_| ())] {
        assert_eq!(value, Err(sort_669_scope_control()));
    }
}
#[test]
fn public_basic_sort_filter_then_sort_finishes_original_predicates_before_keys() {
    let p = sort_669_program(codegen::SortFilterOrder::FilterThenSort, Some(2));
    let source = retain_public_sort_consumer("public-sort-669-fts", &p, PUBLIC669_ORDERED);
    assert_eq!(source.matches(".SortRetainedSource(").count(), 2);
    assert!(source.contains("sort_keys_0, true,"));
    assert!(source.contains("candidate => global::Ferrule.Runtime.FerruleFunctions.RequireBoolean(Node_2(candidate), 2U)"));
    assert!(source.contains("new(candidate => Node_20(candidate), false)"));
    assert!(!source.contains(".WithPhasePosition("));
}
#[test]
fn public_basic_sort_sort_then_filter_finishes_candidate_major_keys_before_targets() {
    let p = sort_669_program(codegen::SortFilterOrder::SortThenFilter, Some(2));
    let source = retain_public_sort_consumer("public-sort-669-stf", &p, PUBLIC669_ORDERED);
    assert!(source.contains("sort_keys_0, false,"));
    assert!(source.contains("candidate => ScopeItem_0(candidate)"));
    assert!(source.contains("private static global::Ferrule.Runtime.FerruleValue Node_20("));
}
#[test]
fn public_basic_sort_stable_secondary_descending_total_scalar_order() {
    let mut p = sort_669_program(codegen::SortFilterOrder::FilterThenSort, None);
    p.expressions.push(expression(
        21,
        Expression::Const {
            value: Value::Float(f64::NAN),
        },
    ));
    p.expressions.push(expression(
        22,
        Expression::Const {
            value: Value::Int(i64::MAX),
        },
    ));
    p.expressions.push(expression(
        23,
        Expression::Const {
            value: Value::Float(9223372036854775808.0),
        },
    ));
    p.expressions.push(expression(
        99,
        Expression::Const {
            value: Value::String("dead-key".into()),
        },
    ));
    let keys = vec![
        codegen::SortKey {
            expression: 20,
            descending: true,
        },
        codegen::SortKey {
            expression: 22,
            descending: true,
        },
        codegen::SortKey {
            expression: 23,
            descending: false,
        },
    ];
    sort_669(
        &mut p.root,
        None,
        codegen::SortKey {
            expression: 21,
            descending: false,
        },
        keys.clone(),
        codegen::SortFilterOrder::FilterThenSort,
    );
    sort_669(
        &mut p.extra_targets[0].root,
        None,
        codegen::SortKey {
            expression: 21,
            descending: false,
        },
        keys,
        codegen::SortFilterOrder::FilterThenSort,
    );
    let body = r#"
// Unequal stored secondary scores descend; equal complete key tuples retain
// A-first before A-second. NaN and the exact integer/float boundary literals
// exercise generated scalar payloads; RuntimeSmoke40 owns the total-order matrix.
var input=G(("Rows",R(G(("Value",S("A-first")),("Score",I(30))),G(("Value",S("B")),("Score",I(10))),G(("Value",S("A-second")),("Score",I(30))),G(("Value",S("C")),("Score",I(20))))));
var limits=Policy();var supplied=Array.Empty<NamedInput>();var primary=FerruleTargetSelection.Primary();var named=FerruleTargetSelection.Named("Sorted");
var p0=new Probe(supplied);var a=Reach("N669-T01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(supplied);var b=Reach("N669-T02",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(supplied);var c=Reach("N669-T03",input,p2,limits,null,named,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,named,p2,limits,null));
var expected="[{\"Value\":\"A-first\",\"OwnedPosition\":1,\"LegacyPosition\":1},{\"Value\":\"A-second\",\"OwnedPosition\":2,\"LegacyPosition\":2},{\"Value\":\"C\",\"OwnedPosition\":3,\"LegacyPosition\":3},{\"Value\":\"B\",\"OwnedPosition\":4,\"LegacyPosition\":4}]";
Tree(a,expected);Tree(b,expected);Tree(c,expected);var all=(ExecutionOutputs)a.Result!;Check(all.Extras.Count==1&&all.Extras[0].Name=="Sorted"&&Json(Plain(all.Extras[0].Instance))==expected,"complete stable descending extra");Reads(a,1,0);Reads(b,1,0);Reads(c,1,0);
"#;
    let source = retain_public_sort_consumer("public-sort-669-secondary", &p, body);
    for node in [20, 21, 22, 23] {
        assert!(source.contains(&format!("Node_{node}(candidate)")));
    }
    assert!(!source.contains("Node_99("));
    assert!(source.contains(
        "global::Ferrule.Runtime.FerruleValue.FromDouble(global::System.BitConverter.Int64BitsToDouble(unchecked((long)0x7FF8000000000000UL)));"
    ));
    assert!(source.contains("Node_20(candidate), true"));
    assert!(source.contains("Node_22(candidate), true"));
}
#[test]
fn public_basic_sort_raw_selection_sorted_predicate_compact_output_phases() {
    let mut p = sort_669_program(codegen::SortFilterOrder::FilterThenSort, Some(2));
    p.expressions
        .iter_mut()
        .find(|n| n.id == 2)
        .unwrap()
        .expression = public582_map(
        1,
        None,
        vec![
            (Value::Int(2), Value::Bool(true)),
            (Value::Int(4), Value::Bool(true)),
        ],
        Some(Value::Bool(false)),
    );
    let body = r#"
var input=G(("Rows",R(new FerruleScalar(FerruleValue.Null),G(("Value",S("A")),("Score",I(30))),new FerruleScalar(FerruleValue.JsonNull),G(("Value",S("B")),("Score",I(10))),G(("Value",S("C")),("Score",I(20))))));var limits=Policy();var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("N669-R01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));Tree(a,"[{\"Value\":\"B\",\"OwnedPosition\":1,\"LegacyPosition\":1},{\"Value\":\"A\",\"OwnedPosition\":2,\"LegacyPosition\":2}]");var all=(ExecutionOutputs)a.Result!;Check(all.Extras.Count==1&&all.Extras[0].Name=="Sorted"&&Json(Plain(all.Extras[0].Instance))==Json(Plain(all.Primary)),"full filtered sorted extra");Reads(a,1,0);
"#;
    let source = retain_public_sort_consumer("public-sort-669-raw", &p, body);
    assert!(source.contains("Node_1(context)"));
    assert!(!source.contains("BeginRetainedFilterPhase("));
    for scope in [&mut p.root, &mut p.extra_targets[0].root] {
        sort_669(
            scope,
            Some(2),
            codegen::SortKey {
                expression: 20,
                descending: false,
            },
            vec![],
            codegen::SortFilterOrder::SortThenFilter,
        );
    }
    let body = r#"
// The same raw2/raw4 predicate now sees sorted positions B1/C2/A3. Only C2
// survives, and its target immediately sees compact owned/legacy position1.
var input=G(("Rows",R(new FerruleScalar(FerruleValue.Null),G(("Value",S("A")),("Score",I(30))),new FerruleScalar(FerruleValue.JsonNull),G(("Value",S("B")),("Score",I(10))),G(("Value",S("C")),("Score",I(20))))));var limits=Policy();var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("N669-R02",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));Tree(a,"[{\"Value\":\"C\",\"OwnedPosition\":1,\"LegacyPosition\":1}]");var all=(ExecutionOutputs)a.Result!;Check(all.Extras.Count==1&&all.Extras[0].Name=="Sorted"&&Json(Plain(all.Extras[0].Instance))==Json(Plain(all.Primary)),"full sorted-predicate compact extra");Reads(a,1,0);
"#;
    let source = retain_public_sort_consumer("public-sort-669-raw-stf", &p, body);
    assert!(source.contains("sort_keys_0, false,"));
    assert!(!source.contains("BeginRetainedFilterPhase("));
}
#[test]
fn public_basic_sort_keeps_named_collision_nested_plain_and_identity_frames() {
    let mut p = sort_669_program(codegen::SortFilterOrder::SortThenFilter, None);
    let input = codegen::IterationSource::Source(codegen::SourceIteration::new(path(&["Rows"])));
    for scope in [&mut p.root, &mut p.extra_targets[0].root] {
        scope.iteration = Some(IterationPlan::new(
            input.clone(),
            None,
            Some(codegen::SortPlan::new(
                codegen::SortKey {
                    expression: 20,
                    descending: false,
                },
                vec![],
                codegen::SortFilterOrder::SortThenFilter,
            )),
            vec![],
            IterationOutput::Repeated,
        ));
    }
    let source = retain_public_sort_consumer("public-sort-669-plain", &p, PUBLIC669_ORDERED);
    assert_eq!(
        source
            .matches("WalkSourceCohort(new string[] { \"Rows\" })")
            .count(),
        2
    );
    assert!(source.contains("context.SortRetainedSource("));
    let mut p = sort_669_program(codegen::SortFilterOrder::SortThenFilter, None);
    p.extra_sources.push(NamedSourceProgram {
        name: "Ref".into(),
        source: source_schema("Static"),
        dynamic: None,
    });
    for node in &mut p.expressions {
        if let Expression::OwnedSourceField(field) = &mut node.expression {
            field.source.owner = named("Ref");
        }
        if let Expression::OwnedPosition(position) = &mut node.expression {
            position.owner = named("Ref");
        }
    }
    p.expressions.push(expression(
        6,
        field(SourceOwner::Primary, &["Value"], Some(&[])),
    ));
    p.expressions
        .push(expression(7, field(named("Ref"), &["Value"], Some(&[]))));
    for scope in [&mut p.root, &mut p.extra_targets[0].root] {
        scope.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
            owner: named("Ref"),
            path: path(&["Rows"]),
        }));
        scope
            .bindings
            .push(binding("PrimaryRoot", 6, ScalarType::String));
        scope
            .bindings
            .push(binding("NamedRoot", 7, ScalarType::String));
        sort_669(
            scope,
            None,
            codegen::SortKey {
                expression: 20,
                descending: false,
            },
            vec![],
            codegen::SortFilterOrder::SortThenFilter,
        );
    }
    p.target = target_schema(&p.root, "Output");
    p.extra_targets[0].target = target_schema(&p.extra_targets[0].root, "Sorted");
    let body = r#"
// The primary's real Ref field collides with the named owner. Sorting must
// retain the authenticated Named Ref frame and its generic legacy position.
// Owned Named Ref/Rows publishes canonical Rows; a prefixed Ref/Rows legacy
// lookup would remain1 under #681 and is not this paired-positive control.
var input=G(("Value",S("P.root")),("Ref",G(("Value",S("P.collision")),("Rows",R(G(("Value",S("wrong")),("Score",I(-1))))))));
var namedInput=G(("Value",S("N.root")),("Rows",R(G(("Value",S("A")),("Score",I(30))),G(("Value",S("B")),("Score",I(10))),G(("Value",S("C")),("Score",I(20))))));
var limits=Policy();var supplied=new[]{new NamedInput("Ref",namedInput)};var primary=FerruleTargetSelection.Primary();var selected=FerruleTargetSelection.Named("Sorted");
var p0=new Probe(supplied);var a=Reach("N669-C01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(supplied);var b=Reach("N669-C02",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(supplied);var c=Reach("N669-C03",input,p2,limits,null,selected,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selected,p2,limits,null));
var expected="[{\"Value\":\"B\",\"OwnedPosition\":1,\"LegacyPosition\":1,\"PrimaryRoot\":\"P.root\",\"NamedRoot\":\"N.root\"},{\"Value\":\"C\",\"OwnedPosition\":2,\"LegacyPosition\":2,\"PrimaryRoot\":\"P.root\",\"NamedRoot\":\"N.root\"},{\"Value\":\"A\",\"OwnedPosition\":3,\"LegacyPosition\":3,\"PrimaryRoot\":\"P.root\",\"NamedRoot\":\"N.root\"}]";
Tree(a,expected);Tree(b,expected);Tree(c,expected);var all=(ExecutionOutputs)a.Result!;Check(all.Extras.Count==1&&all.Extras[0].Name=="Sorted"&&Json(Plain(all.Extras[0].Instance))==expected,"complete named-collision extra");Reads(a,1,1);Reads(b,1,1);Reads(c,1,1);
"#;
    let source = retain_public_sort_consumer("public-sort-669-named-collision", &p, body);
    assert_eq!(source.matches("WalkOwnedSourceCohort(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"), new string[] { \"Rows\" })").count(),2);
    let mut identity = owned_iteration(
        SourceOwner::Primary,
        &["Rows"],
        vec![
            binding("Outer", 5, ScalarType::String),
            binding("Inner", 3, ScalarType::String),
            binding("ParentPosition", 1, ScalarType::Int),
            binding("Position", 4, ScalarType::Int),
        ],
    );
    identity.target_field = "Identity".into();
    sort_669(
        &mut identity,
        None,
        codegen::SortKey {
            expression: 20,
            descending: false,
        },
        vec![],
        codegen::SortFilterOrder::FilterThenSort,
    );
    let mut kids = owned_iteration(SourceOwner::Primary, &["Rows", "Rows"], vec![]);
    kids.target_field = "Kids".into();
    kids.children.push(identity);
    sort_669(
        &mut kids,
        Some(2),
        codegen::SortKey {
            expression: 3,
            descending: false,
        },
        vec![],
        codegen::SortFilterOrder::SortThenFilter,
    );
    let mut root = owned_iteration(SourceOwner::Primary, &["Rows"], vec![]);
    root.children.push(kids);
    sort_669(
        &mut root,
        None,
        codegen::SortKey {
            expression: 20,
            descending: false,
        },
        vec![],
        codegen::SortFilterOrder::FilterThenSort,
    );
    let mut p = basic(
        vec![
            expression(1, position(SourceOwner::Primary, Some(&["Rows"]))),
            expression(
                2,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(
                3,
                field(SourceOwner::Primary, &["Rows", "Rows", "Value"], None),
            ),
            expression(4, position(SourceOwner::Primary, None)),
            expression(
                5,
                field(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
            ),
            expression(
                20,
                field(SourceOwner::Primary, &["Rows", "Score"], Some(&["Rows"])),
            ),
        ],
        root,
    );
    p.extra_sources.clear();
    let body = r#"
// The identity walk back to Rows introduces no repeated cursor. Its sort
// preserves both the compact parent phase and the child's sorted phase.
var input=G(("Rows",R(new FerruleScalar(FerruleValue.Null),G(("Value",S("A")),("Score",I(30)),("Rows",R(G(("Value",S("K3"))),new FerruleScalar(FerruleValue.JsonNull),G(("Value",S("K1"))),G(("Value",S("K2")))))),G(("Value",S("B")),("Score",I(10)),("Rows",R(G(("Value",S("B2"))),G(("Value",S("B1")))))))));
var limits=Policy();var probe=new Probe(Array.Empty<NamedInput>());var a=Reach("N669-N01",input,probe,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,probe,limits,null));
Tree(a,"[{\"Kids\":[{\"Identity\":[{\"Outer\":\"B\",\"Inner\":\"B1\",\"ParentPosition\":1,\"Position\":1}]},{\"Identity\":[{\"Outer\":\"B\",\"Inner\":\"B2\",\"ParentPosition\":1,\"Position\":2}]}]},{\"Kids\":[{\"Identity\":[{\"Outer\":\"A\",\"Inner\":\"K1\",\"ParentPosition\":2,\"Position\":1}]},{\"Identity\":[{\"Outer\":\"A\",\"Inner\":\"K2\",\"ParentPosition\":2,\"Position\":2}]},{\"Identity\":[{\"Outer\":\"A\",\"Inner\":\"K3\",\"ParentPosition\":2,\"Position\":3}]}]}]");Check(((ExecutionOutputs)a.Result!).Extras.Count==0,"complete nested identity output");Reads(a,1,0);
"#;
    let source = retain_public_sort_consumer("public-sort-669-nested-identity", &p, body);
    assert_eq!(source.matches(".SortRetainedSource(").count(), 3);
    assert!(!source.contains(".WithPhasePosition("));
}
#[test]
fn public_basic_sort_sorted_scalar_and_union_outputs_keep_full_shapes() {
    let mut p = sort_669_program(codegen::SortFilterOrder::FilterThenSort, None);
    p.extra_targets.clear();
    p.root.construction = TargetConstruction::Scalar {
        expression: 20,
        target_domain: ScalarType::Int.into(),
    };
    p.root.bindings.clear();
    p.target = target_schema(&p.root, "Output");
    let body = r#"var input=G(("Rows",R(G(("Value",S("A")),("Score",I(30))),G(("Value",S("B")),("Score",I(10))),G(("Value",S("C")),("Score",I(20))))));var limits=Policy();var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("N669-S01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));Tree(a,"[10,20,30]");Check(((ExecutionOutputs)a.Result!).Extras.Count==0,"complete scalar all output");Reads(a,1,0);"#;
    let source = retain_public_sort_consumer("public-sort-669-scalar", &p, body);
    assert!(source.contains("TargetBuilder.Scalar(Node_20(context)"));
    let union = ir::ScalarTypeSet::new([ScalarType::Int, ScalarType::Float]).unwrap();
    p.root.construction = TargetConstruction::Scalar {
        expression: 20,
        target_domain: ScalarTargetDomain::Union(union),
    };
    p.target = SchemaNode::scalar_union("Output", union).repeating();
    let source = retain_public_sort_consumer("public-sort-669-union", &p, body);
    assert!(source.contains("TargetBuilder.Scalar(Node_20(context)"));
}
#[test]
fn public_basic_sort_potential_dependencies_selected_targets_and_leaf_causes() {
    let mut p = sort_669_program(codegen::SortFilterOrder::FilterThenSort, None);
    p.expressions.push(expression(
        24,
        Expression::Const {
            value: Value::Bool(false),
        },
    ));
    p.expressions.push(expression(
        25,
        Expression::If {
            condition: 24,
            then: 29,
            else_: 20,
        },
    ));
    p.expressions
        .push(expression(29, Expression::SourceDocumentPath));
    sort_669(
        &mut p.root,
        None,
        codegen::SortKey {
            expression: 20,
            descending: false,
        },
        vec![codegen::SortKey {
            expression: 25,
            descending: false,
        }],
        codegen::SortFilterOrder::FilterThenSort,
    );
    let actual = crate::emit_with_basic_owned_sort(&p);
    eprintln!("N669 potential complete program={p:#?} complete result={actual:#?}");
    let Err(EmitError::ProgramValidation(ProgramValidationError::OwnedSource { diagnostic })) =
        actual
    else {
        panic!("potential secondary dependency must remain closed")
    };
    assert_eq!(
        *diagnostic,
        codegen::OwnedSourceDiagnostic::Capability {
            location: codegen::OwnedSourceDiagnosticLocation::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 29,
                scope: Some(::mapping::SourceScopeLocation::root(
                    ::mapping::SourceScopeTarget::Primary
                ))
            },
            request: None,
            reason: codegen::OwnedSourceCapabilityReason::XmlOperator,
            origin: codegen::OwnedSourceCapabilityOrigin::Expression {
                namespace: codegen::OwnedSourceNamespace::Main,
                node: 29
            }
        }
    );
    let mut p = sort_669_program(codegen::SortFilterOrder::FilterThenSort, Some(2));
    p.extra_targets.clear();
    p.extra_sources.push(NamedSourceProgram {
        name: "Ref".into(),
        source: source_schema("Static"),
        dynamic: None,
    });
    p.expressions
        .iter_mut()
        .find(|node| node.id == 2)
        .unwrap()
        .expression = Expression::If {
        condition: 21,
        then: 22,
        else_: 24,
    };
    p.expressions
        .iter_mut()
        .find(|node| node.id == 3)
        .unwrap()
        .expression = Expression::Raise { message: Some(25) };
    p.expressions.extend([
        expression(0, field(named("Ref"), &["Value"], None)),
        expression(
            21,
            public582_map(
                1,
                None,
                vec![(Value::Int(2), Value::Bool(true))],
                Some(Value::Bool(false)),
            ),
        ),
        expression(22, Expression::Raise { message: Some(23) }),
        expression(
            23,
            Expression::Const {
                value: Value::String("later predicate".into()),
            },
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
                value: Value::String("target after keys".into()),
            },
        ),
    ]);
    sort_669(
        &mut p.root,
        Some(2),
        codegen::SortKey {
            expression: 0,
            descending: false,
        },
        vec![],
        codegen::SortFilterOrder::FilterThenSort,
    );
    let body = format!(
        "{PUBLIC669_TYPED_ERRORS}{}",
        r#"
var limits=Policy();var primary=FerruleTargetSelection.Primary();var two=G(("Rows",R(G(("Value",S("A")),("Score",I(30))),G(("Value",S("B")),("Score",I(10))))));var one=G(("Rows",R(G(("Value",S("A")),("Score",I(30))))));
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("N669-E01",two,p0,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(two,primary,p0,limits,null));
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("N669-E02",one,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(one,primary,p1,limits,null));
var p2=new Probe(new[]{new NamedInput("Ref",G(("Value",S("key"))))});var c=Reach("N669-E03",one,p2,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(one,primary,p2,limits,null));
MappingRaise(a,22U,"later predicate");MissingOwnedKey(b,0U);MappingRaise(c,3U,"target after keys");Reads(a,1,0);Reads(b,1,0);Reads(c,1,1);
"#
    );
    let source = retain_public_sort_consumer("public-sort-669-fts-errors", &p, &body);
    assert!(source.contains("error.WithOwnedSourceNode(0U)"));
    assert!(source.contains("sort_keys_0, true,"));
    let mut p = sort_669_program(codegen::SortFilterOrder::SortThenFilter, Some(2));
    p.extra_targets.clear();
    p.extra_sources.push(NamedSourceProgram {
        name: "Ref".into(),
        source: source_schema("Static"),
        dynamic: None,
    });
    p.expressions
        .iter_mut()
        .find(|node| node.id == 20)
        .unwrap()
        .expression = Expression::If {
        condition: 21,
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
            public582_map(
                1,
                None,
                vec![(Value::Int(2), Value::Bool(true))],
                Some(Value::Bool(false)),
            ),
        ),
        expression(22, Expression::Raise { message: Some(23) }),
        expression(
            23,
            Expression::Const {
                value: Value::String("second primary key".into()),
            },
        ),
        expression(
            24,
            field(SourceOwner::Primary, &["Rows", "Score"], Some(&["Rows"])),
        ),
        expression(
            25,
            Expression::Const {
                value: Value::String("target after keys".into()),
            },
        ),
        expression(u32::MAX, field(named("Ref"), &["Value"], None)),
    ]);
    sort_669(
        &mut p.root,
        Some(2),
        codegen::SortKey {
            expression: 20,
            descending: false,
        },
        vec![codegen::SortKey {
            expression: u32::MAX,
            descending: false,
        }],
        codegen::SortFilterOrder::SortThenFilter,
    );
    let body = format!(
        "{PUBLIC669_TYPED_ERRORS}{}",
        r#"
var limits=Policy();var primary=FerruleTargetSelection.Primary();var input=G(("Rows",R(G(("Value",S("A")),("Score",I(30))),G(("Value",S("B")),("Score",I(10))))));
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("N669-E04",input,p0,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p0,limits,null));
var p1=new Probe(new[]{new NamedInput("Ref",G(("Value",S("key"))))});var b=Reach("N669-E05",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
MissingOwnedKey(a,4294967295U);MappingRaise(b,22U,"second primary key");Reads(a,1,0);Reads(b,1,1);
"#
    );
    let source = retain_public_sort_consumer("public-sort-669-stf-errors", &p, &body);
    assert!(source.contains("error.WithOwnedSourceNode(4294967295U)"));
    assert!(source.contains("new(candidate => Node_4294967295(candidate), false)"));
    assert!(source.contains("sort_keys_0, false,"));
}
#[test]
fn public_basic_sort_keeps_first_windows_other_families_and_uint_boundaries() {
    let base = sort_669_program(codegen::SortFilterOrder::FilterThenSort, None);
    for first in [false, true] {
        let mut p = base.clone();
        p.extra_targets.clear();
        p.expressions.push(expression(
            30,
            Expression::Const {
                value: Value::Int(1),
            },
        ));
        let iteration = p.root.iteration.as_ref().unwrap();
        let input = iteration.input().clone();
        let sort = iteration.sort().cloned();
        p.root.iteration = Some(IterationPlan::new(
            input,
            None,
            sort,
            if first {
                vec![]
            } else {
                vec![codegen::SequenceWindow::First { count: 30 }]
            },
            if first {
                IterationOutput::First
            } else {
                IterationOutput::Repeated
            },
        ));
        if first {
            p.root.repeating = false;
            p.target = target_schema(&p.root, "Output");
        }
        let actual = crate::emit_with_basic_owned_sort(&p);
        eprintln!("N669 closed complete input={p:#?} result={actual:#?}");
        assert_eq!(actual.map(|_| ()), Err(sort_669_scope_control()));
    }
    let mut boundary = sort_669_program(codegen::SortFilterOrder::FilterThenSort, None);
    boundary
        .expressions
        .iter_mut()
        .find(|node| node.id == 1)
        .unwrap()
        .id = 0;
    boundary
        .expressions
        .iter_mut()
        .find(|node| node.id == 20)
        .unwrap()
        .id = u32::MAX;
    for scope in [&mut boundary.root, &mut boundary.extra_targets[0].root] {
        scope
            .bindings
            .iter_mut()
            .find(|binding| binding.expression == 1)
            .unwrap()
            .expression = 0;
        sort_669(
            scope,
            None,
            codegen::SortKey {
                expression: u32::MAX,
                descending: false,
            },
            vec![],
            codegen::SortFilterOrder::FilterThenSort,
        );
    }
    let source = retain_public_sort_consumer("public-sort-669-uint", &boundary, PUBLIC669_ORDERED);
    assert!(source.contains("error.WithOwnedSourceNode(0U)"));
    assert!(source.contains("error.WithOwnedSourceNode(4294967295U)"));
    assert!(source.contains("Node_4294967295(candidate)"));
}
