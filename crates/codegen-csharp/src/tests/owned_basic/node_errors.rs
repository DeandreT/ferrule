use super::*;

fn public_651_wrapper(node: u32, annotated: bool) -> String {
    let body = if annotated {
        format!(
            "        try {{ return NodeBody_{node}(context); }}\n        catch (global::Ferrule.Runtime.FerruleRuntimeException error)\n        {{\n            error.WithOwnedSourceNode({node}U);\n            throw;\n        }}\n"
        )
    } else {
        format!("        return NodeBody_{node}(context);\n")
    };
    format!(
        "\n    private static global::Ferrule.Runtime.FerruleValue Node_{node}(\n        global::Ferrule.Runtime.FerruleBasicScopeContext context)\n    {{\n        ObserveOwnedBasicNode({node}U, context);\n{body}    }}\n"
    )
}

#[test]
fn public_basic_owned_leaf_nodes_preserve_private_profiles_and_uint_width() {
    let p = basic(
        vec![
            expression(0, field(SourceOwner::Primary, &["Value"], None)),
            expression(1, field(named("Ref"), &["Value"], Some(&[]))),
            expression(
                2,
                field(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
            ),
            expression(3, position(named("Ref"), Some(&["Rows"]))),
            expression(
                4,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: None,
                },
            ),
            expression(
                5,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
            expression(
                6,
                Expression::If {
                    condition: 5,
                    then: 0,
                    else_: 1,
                },
            ),
            expression(u32::MAX, position(SourceOwner::Primary, Some(&[]))),
        ],
        group(vec![
            binding("Nearest", 0, ScalarType::String),
            binding("Root", 1, ScalarType::String),
            binding("Row", 2, ScalarType::String),
            binding("Position", 3, ScalarType::Int),
            binding("Legacy", 4, ScalarType::String),
            binding("Choice", 6, ScalarType::String),
            binding("High", u32::MAX, ScalarType::Int),
        ]),
    );
    let private = render_basic_owned_mapping(&p);
    let checked = crate::render_checked_private_owned_basic_mapping(&p);
    let public = crate::owned_basic::render_public_basic_owned_mapping(&p);
    let ordinary = crate::emit(&p);
    eprintln!("INPUT S651-CS01 complete_program={p:#?}");
    eprintln!(
        "RAW S651-CS01 private={private:#?} checked={checked:#?} public={public:#?} ordinary={ordinary:#?}"
    );
    let private = private.unwrap();
    let checked = checked.unwrap();
    let public = public.unwrap();
    assert!(checked.starts_with(&private));
    assert!(!private.contains("WithOwnedSourceNode"));
    assert!(!checked.contains("WithOwnedSourceNode"));
    assert_eq!(public.matches("error.WithOwnedSourceNode(").count(), 5);
    let mut restored = public.clone();
    for node in [0, 1, 2, 3, u32::MAX] {
        let annotated = public_651_wrapper(node, true);
        assert_eq!(public.matches(&annotated).count(), 1);
        assert!(private.contains(&public_651_wrapper(node, false)));
        restored = restored.replace(&annotated, &public_651_wrapper(node, false));
    }
    assert!(restored.starts_with(&checked));
    for node in [4, 5, 6] {
        assert!(public.contains(&public_651_wrapper(node, false)));
    }
    assert!(matches!(
        ordinary,
        Err(EmitError::ProgramValidation(
            ProgramValidationError::OwnedSource { .. }
        ))
    ));
}

fn public_651_nested_field_program() -> Program {
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::RuntimeParameter {
                    name: "flag".into(),
                    ty: ScalarType::Bool,
                },
            ),
            expression(2, field(named("Ref"), &["Rows", "Value"], Some(&["Rows"]))),
            expression(
                3,
                Expression::Const {
                    value: Value::String("!".into()),
                },
            ),
            expression(
                4,
                Expression::Call {
                    function: ScalarFunction::Concat,
                    args: vec![2, 3],
                },
            ),
            expression(
                5,
                public582_map(4, None, vec![], Some(Value::String("fallback".into()))),
            ),
            expression(
                6,
                Expression::If {
                    condition: 1,
                    then: 5,
                    else_: 7,
                },
            ),
            expression(
                7,
                Expression::Const {
                    value: Value::String("safe".into()),
                },
            ),
            expression(u32::MAX, field(named("Ref"), &["Details", "Counter"], None)),
            expression(91, Expression::Raise { message: None }),
        ],
        group(vec![binding("Value", 6, ScalarType::String)]),
    );
    add_target(
        &mut p,
        "High",
        group(vec![binding("Counter", u32::MAX, ScalarType::Int)]),
    );
    add_target(
        &mut p,
        "Raise",
        group(vec![binding("Value", 91, ScalarType::String)]),
    );
    p
}

#[test]
fn public_basic_owned_field_errors_keep_leaf_node_through_call_map_and_lazy_if() {
    let p = public_651_nested_field_program();
    let source = retain_public_basic_consumer("public-node-651-field", &p, PUBLIC651_FIELDS);
    assert_eq!(source.matches("error.WithOwnedSourceNode(").count(), 2);
    assert!(source.contains(&public_651_wrapper(2, true)));
    assert!(source.contains(&public_651_wrapper(u32::MAX, true)));
    for node in [1, 3, 4, 5, 6, 7, 91] {
        assert!(source.contains(&public_651_wrapper(node, false)));
    }
}

const PUBLIC651_FIELDS: &str = r#"
void Inactive(Actual actual,uint node)
{
    var outer=Runtime(actual,FerruleRuntimeError.OwnedSource);var cause=outer.OwnedSourceFailure!;
    Check(outer.Node==node && outer.Message=="owned source access failed" && ReferenceEquals(cause,outer.InnerException),"exact owned leaf wrapper");
    Check(cause.Error==FerruleOwnedSourceErrorKind.InactiveSourceFrame && cause.Owner?.IsPrimary==false && cause.Owner?.Name=="Ref" && cause.RequestedName=="Ref" && cause.Collection!.SequenceEqual(new[]{"Rows"}),"exact inactive owner and collection");
    Check(cause.Message=="Owned source operation failed." && cause.DeclarationIndices is null && cause.Path is null && cause.Frame is null && cause.Segment is null && cause.FoundInstance is null && cause.RequestedIndex is null && cause.LimitKind is null && cause.Used is null && cause.Requested is null && cause.Maximum is null && cause.InnerException is null,"complete unchanged inactive cause");
    NullMetadata(outer,"Node","OwnedSourceFailure");
}
var input=Primary();var limits=Policy();var primary=FerruleTargetSelection.Primary();
var t=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(true))};var f=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(false))};
var yes=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(t));var no=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(f));
var refRoot=G(("Value",S("N.root")),("Rows",R(G(("Value",S("N1"))))),("Details",S("blocked")));
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("N651-F01",input,p0,limits,no,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p0,limits,no),f);
var p1=new Probe(new[]{new NamedInput("Ref",refRoot)});var b=Reach("N651-F02",input,p1,limits,yes,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p1,limits,yes),t);
var p2=new Probe(new[]{new NamedInput("Ref",refRoot)});var c=Reach("N651-F03",input,p2,limits,yes,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p2,limits,yes),t);
var high=FerruleTargetSelection.Named("High");var p3=new Probe(new[]{new NamedInput("Ref",refRoot)});var d=Reach("N651-F04",input,p3,limits,null,high,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,high,p3,limits,null));
var p4=new Probe(Array.Empty<NamedInput>());var e=Reach("N651-F05",input,p4,limits,null,high,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,high,p4,limits,null));
var raised=FerruleTargetSelection.Named("Raise");var p5=new Probe(Array.Empty<NamedInput>());var f0=Reach("N651-F06",input,p5,limits,null,raised,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,raised,p5,limits,null));
var missing=FerruleTargetSelection.Named("missing");var p6=new Probe(Array.Empty<NamedInput>(),throwCount:true);var g=Reach("N651-F07",input,p6,limits,null,missing,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,missing,p6,limits,null));
Tree(a,"{\"Value\":\"safe\"}");Inactive(b,2U);Inactive(c,2U);
var outer=Runtime(d,FerruleRuntimeError.OwnedSource);var cause=outer.OwnedSourceFailure!;var traversal=cause.InnerException as FerruleOwnedSourceTraversalException;
Check(outer.Node==uint.MaxValue && outer.Message=="owned source access failed" && ReferenceEquals(cause,outer.InnerException),"maximum node stays unsigned and cause identity survives");
Check(cause.Error==FerruleOwnedSourceErrorKind.Traversal && cause.Owner?.IsPrimary==false && cause.Owner?.Name=="Ref" && cause.RequestedName=="Ref" && cause.Path!.SequenceEqual(new[]{"Details","Counter"}) && cause.Segment==(nuint)1 && cause.FoundInstance==FerruleOwnedSourceInstanceKind.Scalar,"complete malformed owner/path/segment");
Check(traversal is not null && traversal.Error==FerruleOwnedSourceTraversalKind.CannotTraverse && traversal.Found==FerruleOwnedSourceInstanceKind.Scalar && traversal.Segment==(nuint)1 && ReferenceEquals(cause.Path,traversal.Path) && traversal.InnerException is null,"exact nested traversal and shared path");
Check(cause.DeclarationIndices is null && cause.Frame is null && cause.Collection is null && cause.RequestedIndex is null && cause.LimitKind is null && cause.Used is null && cause.Requested is null && cause.Maximum is null,"unchanged absent traversal metadata");NullMetadata(outer,"Node","OwnedSourceFailure");
OwnedAbsent(e,"Ref",uint.MaxValue);
outer=Runtime(f0,FerruleRuntimeError.MappingException);Check(outer.Node==91U && outer.MappingExceptionMessage is null && outer.InnerException is null && outer.Message=="node 91: mapping exception: mapping exception was raised","ordinary error is not reannotated");NullMetadata(outer,"Node");
InputError(g,FerruleRuntimeError.UnknownTarget,"missing","project has no named target `missing`");
Reads(a,1,0);Reads(b,1,1);Reads(c,1,1);Reads(d,1,1);Reads(e,1,0);Reads(f0,1,0);Reads(g,0,0);
"#;

#[test]
fn public_basic_owned_position_errors_keep_complete_owner_and_empty_none_nonempty_frames() {
    let mut p = basic(
        vec![
            expression(2, position(SourceOwner::Primary, Some(&[]))),
            expression(u32::MAX, position(named("Ref"), Some(&[]))),
            expression(0, position(SourceOwner::Primary, Some(&["Rows"]))),
            expression(3, position(named("Ref"), None)),
            expression(4, position(named("Ref"), Some(&["Rows"]))),
        ],
        group(vec![binding("Value", 2, ScalarType::Int)]),
    );
    add_target(
        &mut p,
        "Named",
        group(vec![binding("Value", u32::MAX, ScalarType::Int)]),
    );
    add_target(
        &mut p,
        "Zero",
        group(vec![binding("Value", 0, ScalarType::Int)]),
    );
    add_target(
        &mut p,
        "Nearest",
        group(vec![binding("Value", 3, ScalarType::Int)]),
    );
    add_target(
        &mut p,
        "Active",
        owned_iteration(
            named("Ref"),
            &["Rows"],
            vec![binding("Value", 4, ScalarType::Int)],
        ),
    );
    let source = retain_public_basic_consumer("public-node-651-position", &p, PUBLIC651_POSITIONS);
    assert_eq!(source.matches("error.WithOwnedSourceNode(").count(), 5);
    for node in [0, 2, 3, 4, u32::MAX] {
        assert!(source.contains(&public_651_wrapper(node, true)));
    }
    assert!(!source.contains("OwnedRootIsDeclaredNonrepeating"));
}

const PUBLIC651_POSITIONS: &str = r#"
void Inactive(Actual actual,uint node,bool primary,string[]? collection)
{
    var outer=Runtime(actual,FerruleRuntimeError.OwnedSource);var cause=outer.OwnedSourceFailure!;
    Check(outer.Node==node && outer.Message=="owned source access failed" && ReferenceEquals(cause,outer.InnerException),"complete annotated position wrapper");
    Check(cause.Error==FerruleOwnedSourceErrorKind.InactiveSourceFrame && cause.Owner?.IsPrimary==primary && cause.Owner?.Name==(primary?null:"Ref") && cause.RequestedName==(primary?null:"Ref"),"exact owner-local inactive cause");
    Check(collection is null?cause.Collection is null:cause.Collection is not null && cause.Collection.SequenceEqual(collection),"null/empty/nonempty collection remains distinct");
    Check(cause.Message=="Owned source operation failed." && cause.DeclarationIndices is null && cause.Path is null && cause.Frame is null && cause.Segment is null && cause.FoundInstance is null && cause.RequestedIndex is null && cause.LimitKind is null && cause.Used is null && cause.Requested is null && cause.Maximum is null && cause.InnerException is null,"complete unchanged inactive position metadata");
    NullMetadata(outer,"Node","OwnedSourceFailure");
}
var input=Primary();var limits=Policy();var refRoot=G(("Value",S("N.root")),("Rows",R(G(("Value",S("N1"))),G(("Value",S("N2"))))));
var p0=new Probe(new[]{new NamedInput("Ref",refRoot)});var a=Reach("N651-P01",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var primary=FerruleTargetSelection.Primary();var p1=new Probe(new[]{new NamedInput("Ref",refRoot)});var b=Reach("N651-P02",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var named=FerruleTargetSelection.Named("Named");var p2=new Probe(new[]{new NamedInput("Ref",refRoot)});var c=Reach("N651-P03",input,p2,limits,null,named,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,named,p2,limits,null));
var zero=FerruleTargetSelection.Named("Zero");var p3=new Probe(new[]{new NamedInput("Ref",refRoot)});var d=Reach("N651-P04",input,p3,limits,null,zero,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,zero,p3,limits,null));
var nearest=FerruleTargetSelection.Named("Nearest");var p4=new Probe(new[]{new NamedInput("Ref",refRoot)});var e=Reach("N651-P05",input,p4,limits,null,nearest,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,nearest,p4,limits,null));
var active=FerruleTargetSelection.Named("Active");var p5=new Probe(new[]{new NamedInput("Ref",refRoot)});var f0=Reach("N651-P06",input,p5,limits,null,active,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,active,p5,limits,null));
Inactive(a,2U,true,Array.Empty<string>());Inactive(b,2U,true,Array.Empty<string>());Inactive(c,uint.MaxValue,false,Array.Empty<string>());Inactive(d,0U,true,new[]{"Rows"});Inactive(e,3U,false,null);
Tree(f0,"[{\"Value\":1},{\"Value\":2}]");
Reads(a,1,1);Reads(b,1,1);Reads(c,1,1);Reads(d,1,1);Reads(e,1,1);Reads(f0,1,1);
"#;

#[test]
fn public_basic_node_annotation_does_not_cover_legacy_leaves_or_setup() {
    let mut p = basic(
        vec![
            expression(
                1,
                Expression::SourceField {
                    path: path(&["Value"]),
                    frame: None,
                },
            ),
            expression(
                2,
                Expression::Const {
                    value: Value::String("ordinary".into()),
                },
            ),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.extra_sources.clear();
    let source = crate::owned_basic::render_public_basic_owned_mapping(&p);
    let private = crate::render_checked_private_owned_basic_mapping(&p);
    eprintln!("INPUT S651-CS04 complete_program={p:#?}");
    eprintln!("RAW S651-CS04 public={source:#?} private={private:#?}");
    let source = source.unwrap();
    let private = private.unwrap();
    assert!(source.starts_with(&private));
    assert!(!source.contains("WithOwnedSourceNode"));
    assert!(source.contains("context.ResolveLegacyField("));
    assert!(source.contains("var selected = ResolveOwnedBasicTarget(selection);"));
    assert_eq!(
        source
            .matches("catch (global::Ferrule.Runtime.FerruleRuntimeException error)")
            .count(),
        0
    );
}

// #645 is a separate public compiler profile. Existing emitters and hosts above
// retain their exact control-free admission and capture behavior.
