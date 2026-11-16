use super::*;

pub(super) fn public_basic_result(
    label: &str,
    program: &Program,
) -> Result<codegen::ArtifactSet, EmitError> {
    eprintln!("INPUT {label} complete Program={program:#?}");
    let actual = crate::emit_with_basic_owned_sources(program);
    eprintln!("RAW {label} complete artifact/error Result={actual:#?}");
    actual
}
pub(super) fn retain_public_basic_consumer(label: &str, program: &Program, body: &str) -> String {
    let actual = public_basic_result(label, program);
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
    let host = public_basic_consumer(body, true);
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

pub(super) const PUBLIC_BASIC_CONSUMER_PROJECT: &str = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <TargetFramework>net10.0</TargetFramework>
    <OutputType>Exe</OutputType>
    <ImplicitUsings>enable</ImplicitUsings>
    <Nullable>enable</Nullable>
    <TreatWarningsAsErrors>true</TreatWarningsAsErrors>
    <Deterministic>true</Deterministic>
    <InvariantGlobalization>true</InvariantGlobalization>
    <AssemblyName>Ferrule.PublicBasicConsumer</AssemblyName>
  </PropertyGroup>
  <ItemGroup><ProjectReference Include="../generated/Ferrule.Generated.csproj" /></ItemGroup>
</Project>
"#;

pub(super) fn public_basic_consumer(body: &str, json: bool) -> String {
    let expected = if json {
        r#""ExecuteJsonBytesOutputsWithBasicOwnedSources","ExecuteJsonBytesSelectedTargetWithBasicOwnedSources","ExecuteJsonOutputsWithBasicOwnedSources","ExecuteJsonSelectedTargetWithBasicOwnedSources","ExecuteSelectedWithBasicOwnedSources","ExecuteWithBasicOwnedSources""#
    } else {
        r#""ExecuteSelectedWithBasicOwnedSources","ExecuteWithBasicOwnedSources""#
    };
    PUBLIC_BASIC_CONSUMER
        .replace("__ERROR_SNAPSHOT__", HOST_ERROR_SNAPSHOT)
        .replace("__EXPECTED_PUBLIC_METHODS__", expected)
        .replace(
            "__JSON_API_CHECKS__",
            if json { PUBLIC_BASIC_JSON_API } else { "" },
        )
        .replace("__BODY__", body)
}

const PUBLIC_BASIC_JSON_API: &str = r#"
        void JsonMethod(string name,Type result,params Type[] parameters)
        {
            var method=methods.Single(m=>m.Name==name);
            Check(method.ReturnType==result && method.GetParameters().Select(p=>p.ParameterType).SequenceEqual(parameters),"exact JSON entry "+name);
        }
        JsonMethod("ExecuteJsonOutputsWithBasicOwnedSources",typeof(JsonExecutionOutputs),typeof(string),typeof(IReadOnlyList<NamedJsonInput>),typeof(FerruleOwnedSourceLimits),typeof(FerruleExecutionContext));
        JsonMethod("ExecuteJsonBytesOutputsWithBasicOwnedSources",typeof(JsonBytesExecutionOutputs),typeof(byte[]),typeof(IReadOnlyList<NamedJsonBytesInput>),typeof(FerruleOwnedSourceLimits),typeof(FerruleExecutionContext));
        JsonMethod("ExecuteJsonSelectedTargetWithBasicOwnedSources",typeof(SelectedJsonTargetOutput),typeof(string),typeof(FerruleTargetSelection),typeof(IReadOnlyList<NamedJsonInput>),typeof(FerruleOwnedSourceLimits),typeof(FerruleExecutionContext));
        JsonMethod("ExecuteJsonBytesSelectedTargetWithBasicOwnedSources",typeof(SelectedJsonBytesTargetOutput),typeof(byte[]),typeof(FerruleTargetSelection),typeof(IReadOnlyList<NamedJsonBytesInput>),typeof(FerruleOwnedSourceLimits),typeof(FerruleExecutionContext));
"#;

const PUBLIC_BASIC_CONSUMER: &str = r#"using System;
using System.Linq;
using System.Collections.Generic;
using System.Reflection;
using System.Text.Json;
using Ferrule.Generated;
using Ferrule.Runtime;
namespace Ferrule.PublicConsumer;
internal static class Program
{
    private sealed class Probe : IReadOnlyList<NamedInput>
    {
        internal readonly NamedInput[] Items;
        internal readonly bool ThrowCount, ThrowIndex;
        internal readonly Exception Failure = new InvalidOperationException("public caller sentinel");
        internal int CountReads, IndexReads;
        internal Probe(NamedInput[] items,bool throwCount=false,bool throwIndex=false)
        { Items=items;ThrowCount=throwCount;ThrowIndex=throwIndex; }
        public int Count { get { CountReads++;if(ThrowCount)throw Failure;return Items.Length; } }
        public NamedInput this[int index] { get { IndexReads++;if(ThrowIndex)throw Failure;return Items[index]; } }
        public IEnumerator<NamedInput> GetEnumerator()=>throw new InvalidOperationException("input enumeration forbidden");
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator()=>GetEnumerator();
    }
    private sealed record Actual(object? Result,Exception? Error,int CountReads,int IndexReads,string Input,string Raw,bool CallerIdentity);
    private static string Json(object? value)=>JsonSerializer.Serialize(value);
    private static void Check(bool value,string expected)
    { if(!value)throw new InvalidOperationException("Frozen expected: "+expected); }
    private static object ValueSnapshot(FerruleValue value)=>new {kind=value.Kind.ToString(),value=value.Kind switch
    {FerruleValueKind.String=>(object?)value.StringValue,FerruleValueKind.Int64=>value.Int64Value,FerruleValueKind.Bool=>value.BooleanValue,FerruleValueKind.Double=>BitConverter.DoubleToInt64Bits(value.DoubleValue),_=>null}};
    private static object? InstanceSnapshot(FerruleInstance? value)=>value switch
    {
        null=>null,FerruleScalar scalar=>new {kind="Scalar",value=ValueSnapshot(scalar.Value)},
        FerruleGroup group=>new {kind="Group",fields=group.Fields.Select(f=>new {f.Name,value=InstanceSnapshot(f.Value)}).ToArray()},
        FerruleRepeated repeated=>new {kind="Repeated",items=repeated.Items.Select(InstanceSnapshot).ToArray()},
        _=>throw new InvalidOperationException("Unrecorded instance kind")
    };
    private static object? ResultSnapshot(object? value)=>value switch
    {
        ExecutionOutputs all=>new {kind="ExecutionOutputs",primary=InstanceSnapshot(all.Primary),extras=all.Extras.Select(e=>new {e.Name,instance=InstanceSnapshot(e.Instance)}).ToArray()},
        SelectedTargetOutput.Primary primary=>new {kind="Primary",instance=InstanceSnapshot(primary.Instance)},
        SelectedTargetOutput.Named named=>new {kind="Named",named.Output.Name,instance=InstanceSnapshot(named.Output.Instance)},
        null=>null,_=>throw new InvalidOperationException("Unrecorded public result shape")
    };
    private static Actual Reach(string id,FerruleInstance source,Probe supplied,FerruleOwnedSourceLimits limits,
        FerruleExecutionContext? execution,FerruleTargetSelection? selection,Func<object?> subject,
        KeyValuePair<string,FerruleValue>[]? parameters=null)
    {
        supplied.CountReads=0;supplied.IndexReads=0;
        var input=Json(new {source=InstanceSnapshot(source),supplied=supplied.Items.Select(item=>item is null?null:new {item.Name,units=item.Name?.Select(c=>(int)c).ToArray(),instance=InstanceSnapshot(item.Instance)}).ToArray(),
            limits=new {roots=(ulong)limits.MaxRoots,frames=(ulong)limits.MaxActiveFrames,segments=(ulong)limits.MaxPathSegments,bytes=(ulong)limits.MaxMetadataBytes,contexts=(ulong)limits.MaxWalkContexts},
            selection=selection is null?null:new {selection.IsPrimary,selection.Name},
            execution=execution is null?null:new {execution.MappingFilePath,execution.MainMappingFilePath,execution.CurrentDateTime,parameterCount=execution.RuntimeParameters.Count,parameters=(parameters??Array.Empty<KeyValuePair<string,FerruleValue>>()).Select(p=>new {p.Key,value=ValueSnapshot(p.Value)}).ToArray()},
            supplied.ThrowCount,supplied.ThrowIndex});
        Console.WriteLine(Json(new {id,classification="complete_input_before_subject",input}));
        object? result=null;Exception? error=null;try{result=subject();}catch(Exception reached){error=reached;}
        var count=supplied.CountReads;var index=supplied.IndexReads;var identity=ReferenceEquals(error,supplied.Failure);
        var raw=Json(new {result=ResultSnapshot(result),error=error is null?null:ErrorSnapshot(error),count,index,callerIdentity=identity});
        Console.WriteLine(Json(new {id,classification="complete_actual_before_comparison",input,raw}));
        return new(result,error,count,index,input,raw,identity);
    }
    private static object? Plain(FerruleInstance value)=>value switch
    {
        FerruleScalar scalar=>scalar.Value.Kind switch {FerruleValueKind.String=>(object?)scalar.Value.StringValue,FerruleValueKind.Int64=>scalar.Value.Int64Value,FerruleValueKind.Bool=>scalar.Value.BooleanValue,FerruleValueKind.Null=>"Null",FerruleValueKind.JsonNull=>"JsonNull",FerruleValueKind.XmlNil=>"XmlNil",FerruleValueKind.Double=>scalar.Value.DoubleValue,_=>throw new InvalidOperationException()},
        FerruleGroup group=>group.Fields.ToDictionary(f=>f.Name,f=>Plain(f.Value),StringComparer.Ordinal),
        FerruleRepeated repeated=>repeated.Items.Select(Plain).ToArray(),_=>throw new InvalidOperationException()
    };
    private static void Tree(Actual actual,string expected)
    {
        Check(actual.Error is null,"typed successful result");
        var root=actual.Result switch {ExecutionOutputs all=>all.Primary,SelectedTargetOutput.Primary primary=>primary.Instance,SelectedTargetOutput.Named named=>named.Output.Instance,_=>throw new InvalidOperationException("Missing public tree")};
        var found=Json(Plain(root));Console.WriteLine(Json(new {classification="comparison",actual=found,expected}));Check(found==expected,expected);
    }
    private static void Reads(Actual actual,int count,int index)
    { Check(actual.CountReads==count && actual.IndexReads==index,"actual caller-list reads only, not factory allocation"); }
    private static FerruleRuntimeException Runtime(Actual actual,FerruleRuntimeError category)
    { Check(actual.Error is FerruleRuntimeException e && e.Error==category && actual.Result is null,"exact typed outer/no partial returned output");return (FerruleRuntimeException)actual.Error!; }
    private static void NullMetadata(FerruleRuntimeException error,params string[] permitted)
    {
        var names=new[]{"Node","Function","ExpectedArity","ActualArity","FoundKind","AggregateOperation","Detail","RequestedItems","MaximumItems","MaximumDepth","RuntimeValue","FailureRule","MappingFailureMessage","MappingExceptionMessage","Join","UserFunction","FunctionParameter","ExpectedScalarType","RuntimeParameter","SourceField","FoundInstance","PrimaryRoot","OwnedSourceFailure","BasicSourceBridgeFailure"};
        foreach(var name in names)if(!permitted.Contains(name,StringComparer.Ordinal))
            Check(typeof(FerruleRuntimeException).GetProperty(name)!.GetValue(error) is null,"unchanged absent "+name);
    }
    private static void InputError(Actual actual,FerruleRuntimeError category,string detail,string message)
    {var e=Runtime(actual,category);Check(e.Detail==detail && e.Message==message && e.InnerException is null,"full input error");NullMetadata(e,"Detail");}
    private static void Failure(Actual actual,int rule,string message)
    {var e=Runtime(actual,FerruleRuntimeError.MappingFailure);Check(e.FailureRule==rule && e.MappingFailureMessage==message && e.Message==$"mapping failure rule {rule}: {message}" && e.InnerException is null,"full ordered global failure");NullMetadata(e,"FailureRule","MappingFailureMessage");}
    private static void OwnedAbsent(Actual actual,string name,uint node)
    {
        var outer=Runtime(actual,FerruleRuntimeError.OwnedSource);var cause=outer.OwnedSourceFailure;
        Check(cause is not null && ReferenceEquals(cause,outer.InnerException) && cause.Error==FerruleOwnedSourceErrorKind.SourceUnavailable,"exact bare owned inner cause");
        var c=cause!;Check(c.Owner?.IsPrimary==false && c.Owner?.Name==name && c.RequestedName==name && c.Message=="Owned source operation failed." && outer.Message=="owned source access failed","exact owner, requested name and display");
        Check(c.DeclarationIndices is null && c.Path is null && c.Frame is null && c.Collection is null && c.Segment is null && c.FoundInstance is null && c.RequestedIndex is null && c.LimitKind is null && c.Used is null && c.Requested is null && c.Maximum is null && c.InnerException is null,"complete absent owned metadata");Check(outer.Node==node,"exact owned leaf node");NullMetadata(outer,"Node","OwnedSourceFailure");
    }
    private static FerruleInstance S(string text)=>new FerruleScalar(FerruleValue.FromString(text));
    private static FerruleInstance I(long value)=>new FerruleScalar(FerruleValue.FromInt64(value));
    private static FerruleInstance G(params (string Name,FerruleInstance Value)[] fields)=>new FerruleGroup(fields.Select(f=>new FerruleField(f.Name,f.Value)));
    private static FerruleInstance R(params FerruleInstance[] items)=>new FerruleRepeated(items);
    private static FerruleInstance Primary()=>G(("Value",S("P.root")),("Ref",G(("Value",S("P.collision")))),("Rows",R(G(("Value",S("P1")),("Score",I(10))),G(("Value",S("P2")),("Score",I(20))),G(("Value",S("P3")),("Score",I(30))))));
    private static FerruleOwnedSourceLimits Policy()=>new(16,8,16,1048576,64);
    private static void Api()
    {
        var methods=typeof(GeneratedMapping).GetMethods(BindingFlags.Public|BindingFlags.Static|BindingFlags.DeclaredOnly).OrderBy(m=>m.Name,StringComparer.Ordinal).ToArray();
        var properties=typeof(FerruleOwnedSourceLimits).GetProperties(BindingFlags.Public|BindingFlags.Instance|BindingFlags.DeclaredOnly).OrderBy(p=>p.Name,StringComparer.Ordinal).ToArray();
        Console.WriteLine(Json(new {classification="complete_public_API_before_comparison",methods=methods.Select(m=>new {m.Name,result=m.ReturnType.FullName,parameters=m.GetParameters().Select(p=>new {p.Name,type=p.ParameterType.FullName,p.IsOptional,p.HasDefaultValue}).ToArray()}).ToArray(),limits=properties.Select(p=>new {p.Name,type=p.PropertyType.FullName}).ToArray()}));
        Check(methods.Select(m=>m.Name).SequenceEqual(new[]{__EXPECTED_PUBLIC_METHODS__}),"exact public API for the selected emitter profile");
        var all=methods.Single(m=>m.Name=="ExecuteWithBasicOwnedSources");var selected=methods.Single(m=>m.Name=="ExecuteSelectedWithBasicOwnedSources");
        Check(all.ReturnType==typeof(ExecutionOutputs) && all.GetParameters().Select(p=>p.ParameterType).SequenceEqual(new[]{typeof(FerruleInstance),typeof(IReadOnlyList<NamedInput>),typeof(FerruleOwnedSourceLimits),typeof(FerruleExecutionContext)}),"exact all entry");
        Check(selected.ReturnType==typeof(SelectedTargetOutput) && selected.GetParameters().Select(p=>p.ParameterType).SequenceEqual(new[]{typeof(FerruleInstance),typeof(FerruleTargetSelection),typeof(IReadOnlyList<NamedInput>),typeof(FerruleOwnedSourceLimits),typeof(FerruleExecutionContext)}),"exact selected entry");
__JSON_API_CHECKS__
        Check(methods.SelectMany(m=>m.GetParameters()).All(p=>!p.IsOptional && !p.HasDefaultValue),"every public argument mandatory");
        Check(properties.Select(p=>p.Name).SequenceEqual(new[]{"MaxActiveFrames","MaxMetadataBytes","MaxPathSegments","MaxRoots","MaxWalkContexts"}) && properties.All(p=>p.PropertyType==typeof(nuint)),"all five native policy fields unchanged");
    }
__ERROR_SNAPSHOT__
    public static void Main()
    {
        Api();
__BODY__
    }
}
"#;

#[test]
fn public_basic_external_consumer_has_mandatory_policy_and_input_order() {
    let p = basic(
        vec![expression(1, field(SourceOwner::Primary, &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let source = retain_public_basic_consumer("public-input-order", &p, PUBLIC_INPUT_ORDER);
    assert!(source.contains("public static ExecutionOutputs ExecuteWithBasicOwnedSources("));
    assert!(
        source.contains("public static SelectedTargetOutput ExecuteSelectedWithBasicOwnedSources(")
    );
    assert!(source.contains("=> ExecuteCheckedOwnedBasic(source, supplied, limits, execution);"));
    assert!(source.contains(
        "=> ExecuteSelectedCheckedOwnedBasic(source, selection, supplied, limits, execution);"
    ));
    let selected = source
        .find("private static SelectedTargetOutput ExecuteSelectedCheckedOwnedBasic(")
        .unwrap();
    let selected = &source[selected..];
    assert!(
        selected.find("ResolveOwnedBasicTarget(selection)").unwrap()
            < selected
                .find("new CheckedNamedInputAccessor(supplied)")
                .unwrap()
    );
    assert!(
        selected
            .find("new CheckedNamedInputAccessor(supplied)")
            .unwrap()
            < selected.find("CreateFromNamedInputs(").unwrap()
    );
}

#[test]
fn public_basic_external_consumer_keeps_lazy_optional_named_owner() {
    let p = basic(
        vec![
            expression(
                1,
                Expression::RuntimeParameter {
                    name: "flag".into(),
                    ty: ScalarType::Bool,
                },
            ),
            expression(2, field(SourceOwner::Primary, &["Value"], None)),
            expression(3, field(named("Ref"), &["Value"], None)),
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
    let source = retain_public_basic_consumer("public-lazy-owner", &p, PUBLIC_LAZY_OWNER);
    assert!(source.contains("FerruleOwnedSourceOwner.Named(\"Ref\")"));
    assert!(source.contains("FerruleBasicNamedSourceMode.Optional"));
}

#[test]
fn public_basic_external_consumer_preserves_selected_target_and_ordinary_raise() {
    let mut p = basic(
        vec![
            expression(1, field(SourceOwner::Primary, &["Value"], None)),
            expression(2, Expression::Raise { message: None }),
        ],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    add_target(
        &mut p,
        "Bad",
        TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Scalar {
                expression: 2,
                target_domain: ScalarType::String.into(),
            },
            bindings: vec![],
            children: vec![],
        },
    );
    let source = retain_public_basic_consumer("public-P19-selected", &p, PUBLIC_P19_SELECTED);
    assert!(source.contains("FerruleFailures.MappingException(2U, null)"));
    assert!(source.contains("if (selected.HasValue)"));
}

#[test]
fn public_basic_external_consumer_runs_ordered_global_failures_for_every_selection() {
    let p = checked_failure_program();
    let source = retain_public_basic_consumer("public-global-order", &p, PUBLIC_GLOBAL_ORDER);
    let context = source
        .find("private static object ExecuteCheckedBasicContext(")
        .unwrap();
    let context = &source[context..];
    assert!(
        context.find("EvaluateBasicFailureRules(context);").unwrap()
            < context.find("if (selected.HasValue)").unwrap()
    );
    assert_eq!(
        source
            .matches("FerruleBasicNamedSourceMode.RequiredLegacy")
            .count(),
        3
    );
    assert!(source.contains("finally { global::System.GC.KeepAlive(cohort_failure_1); }"));
}

#[test]
fn public_basic_external_consumer_keeps_complete_raw_position_before_failure() {
    use codegen::FailureSelection;
    let mut p = failure_scores(FailureSelection::WhenFalse(100));
    p.expressions
        .iter_mut()
        .find(|node| node.id == 103)
        .unwrap()
        .expression = Expression::Position {
        collection: path(&["Rows"]),
    };
    p.expressions.extend([
        expression(91, Expression::Raise { message: None }),
        expression(2, Expression::Raise { message: None }),
    ]);
    p.failure_rules
        .push(source_failure(&[], FailureSelection::All, Some(91)));
    add_target(
        &mut p,
        "named",
        group(vec![binding("Value", 2, ScalarType::String)]),
    );
    let source = retain_public_basic_consumer("public-global-position", &p, PUBLIC_GLOBAL_POSITION);
    assert!(source.contains("var path_failure_0 = new string[] { \"Rows\" };"));
    assert!(source.contains("cohort_failure_0 = context.WalkSourceCohort(path_failure_0)"));
    assert!(source.contains("finally { global::System.GC.KeepAlive(cohort_failure_0); }"));
    assert!(source.contains("LegacyPosition(new string[] { \"Rows\" })"));
}

#[test]
fn public_basic_external_consumer_keeps_named_occurrence_identity_and_positions() {
    let p = basic(
        vec![
            expression(1, field(named("Ref"), &["Rows", "Value"], Some(&["Rows"]))),
            expression(2, position(named("Ref"), Some(&["Rows"]))),
        ],
        owned_iteration(
            named("Ref"),
            &["Rows"],
            vec![
                binding("Value", 1, ScalarType::String),
                binding("Position", 2, ScalarType::Int),
            ],
        ),
    );
    let source = retain_public_basic_consumer("public-owned-walk", &p, PUBLIC_OWNED_WALK);
    assert!(source.contains("WalkOwnedSourceCohort(global::Ferrule.Runtime.FerruleOwnedSourceOwner.Named(\"Ref\"), new string[] { \"Rows\" })"));
    assert!(source.contains("global::System.GC.KeepAlive("));
}

#[test]
fn public_basic_external_consumer_preserves_scalar_root_empty_catalogue_and_zero_text() {
    let mut p = basic(
        vec![expression(1, field(SourceOwner::Primary, &[], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    p.source = SchemaNode::scalar("Input", ScalarType::String);
    p.extra_sources.clear();
    let source = retain_public_basic_consumer("public-scalar-zero", &p, PUBLIC_SCALAR_ZERO);
    assert!(
        source.contains(
            "FerruleOwnedSourceOwner.Primary, global::System.Array.Empty<string>(), null"
        )
    );
    assert!(source.contains("CheckedAllInputs"));
}

#[test]
fn public_basic_compiler_preserves_structure_closed_controls_and_ordinary_family() {
    use codegen::{
        OwnedSourceCapabilityOrigin as O, OwnedSourceCapabilityReason as R,
        OwnedSourceDiagnostic as D, OwnedSourceDiagnosticLocation as L,
        OwnedSourceDiagnosticOwner as W, OwnedSourceDiagnosticRequest as Q,
        OwnedSourceNamespace as N,
    };
    fn extract(actual: Result<codegen::ArtifactSet, EmitError>) -> ProgramValidationError {
        match actual {
            Err(EmitError::ProgramValidation(error)) => error,
            other => panic!("complete retained refusal expected: {other:#?}"),
        }
    }
    let mut unknown = basic(
        vec![expression(1, field(named("Absent"), &["Value"], None))],
        group(vec![binding("Value", 1, ScalarType::String)]),
    );
    let unknown_actual = public_basic_result("S519-C01", &unknown);
    let unknown_expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(D::Structure {
            location: L::Expression {
                namespace: N::Main,
                node: 1,
                scope: None,
            },
            request: Q::Field {
                owner: W::Named {
                    name: "Absent".into(),
                },
                path: path(&["Value"]),
                frame: None,
            },
            reason: codegen::OwnedSourceStructureReason::UnknownOwner,
        }),
    };
    eprintln!("EXPECTED S519-C01 complete independent diagnostic={unknown_expected:#?}");
    assert_eq!(extract(unknown_actual), unknown_expected);
    unknown.expressions[0].expression = field(SourceOwner::Primary, &["Value"], None);
    let mut value_map = unknown.clone();
    value_map.expressions.push(expression(
        2,
        Expression::ValueMap {
            input: 1,
            input_type: None,
            table: vec![(
                Value::String("P.root".into()),
                Value::String("mapped".into()),
            )],
            default: None,
        },
    ));
    value_map.root.bindings[0].expression = 2;
    let value_actual = public_basic_result("S519-C02", &value_map);
    let value_files =
        value_actual.expect("complete successful target ValueMap artifact result retained");
    let value_source = std::str::from_utf8(
        &value_files
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .expect("successful public artifact contains mapping")
            .contents,
    )
    .expect("complete emitted mapping is UTF-8");
    assert_eq!(
        value_source
            .matches("var input_2 = Node_1(context);")
            .count(),
        1
    );
    assert!(value_source.contains("FerruleValueMaps.Apply("));
    assert!(value_source.contains("public static ExecutionOutputs ExecuteWithBasicOwnedSources("));
    let mut controlled = basic(
        vec![
            expression(
                1,
                field(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
            ),
            expression(
                2,
                Expression::Const {
                    value: Value::Bool(true),
                },
            ),
        ],
        owned_iteration(
            SourceOwner::Primary,
            &["Rows"],
            vec![binding("Value", 1, ScalarType::String)],
        ),
    );
    controlled.root.iteration = Some(IterationPlan::new(
        codegen::OwnedSourceIteration::new(OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        }),
        Some(2),
        None,
        vec![],
        codegen::IterationOutput::Repeated,
    ));
    let control_actual = public_basic_result("S519-C03", &controlled);
    let route = ::mapping::SourceScopeLocation::root(::mapping::SourceScopeTarget::Primary);
    let control_expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(D::Capability {
            location: L::Iteration {
                scope: route.clone(),
            },
            request: Some(Q::Iteration {
                owner: W::Primary,
                path: path(&["Rows"]),
            }),
            reason: R::ScopeControl,
            origin: O::Scope { scope: route },
        }),
    };
    eprintln!("EXPECTED S519-C03 complete independent diagnostic={control_expected:#?}");
    assert_eq!(extract(control_actual), control_expected);
    eprintln!("INPUT S519-C04 ordinary family complete Program={unknown:#?}");
    let old = crate::emit(&unknown);
    eprintln!("RAW S519-C04 ordinary family full Result={old:#?}");
    let old_expected = ProgramValidationError::OwnedSource {
        diagnostic: Box::new(D::Capability {
            location: L::Expression {
                namespace: N::Main,
                node: 1,
                scope: None,
            },
            request: Some(Q::Field {
                owner: W::Primary,
                path: path(&["Value"]),
                frame: None,
            }),
            reason: R::UnsupportedBasicContext,
            origin: O::RetainedExpression {
                namespace: N::Main,
                node: 1,
            },
        }),
    };
    assert_eq!(extract(old), old_expected);
    let mut legacy = unknown;
    legacy.expressions[0].expression = Expression::SourceField {
        frame: None,
        path: path(&["Value"]),
    };
    eprintln!("INPUT S519-C04 legacy complete Program={legacy:#?}");
    let ordinary = crate::emit(&legacy);
    eprintln!("RAW S519-C04 legacy full artifact Result={ordinary:#?}");
    let ordinary = ordinary.expect("complete old legacy output retained");
    let mapping = String::from_utf8(
        ordinary
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .unwrap();
    assert!(mapping.contains("public static ExecutionOutputs ExecuteOutputs("));
    assert!(!mapping.contains("ExecuteWithBasicOwnedSources("));
}

const PUBLIC_INPUT_ORDER: &str = r#"
var input=Primary();var limits=Policy();var selection=FerruleTargetSelection.Primary();
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("C01.1",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("C01.2",input,p1,limits,null,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p1,limits,null));
var missing=FerruleTargetSelection.Named("missing");var zero=new FerruleOwnedSourceLimits(0,0,0,0,0);var p2=new Probe(new[]{new NamedInput("X",S("x"))},throwCount:true);var c=Reach("C01.3",input,p2,zero,null,missing,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,missing,p2,zero,null));
var p3=new Probe(new[]{new NamedInput("X",S("x"))});var d=Reach("C01.4",input,p3,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p3,limits,null));
var p4=new Probe(new[]{new NamedInput("Ref",S("a")),new NamedInput("Ref",S("b"))});var e=Reach("C01.5",input,p4,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p4,limits,null));
var p5=new Probe(new NamedInput[]{null!});var f=Reach("C01.6",input,p5,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p5,limits,null));
var p6=new Probe(new[]{new NamedInput("Ref",S("a"))},throwCount:true);var g=Reach("C01.7",input,p6,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p6,limits,null));
var p7=new Probe(new[]{new NamedInput("Ref",S("a"))},throwIndex:true);var h=Reach("C01.8",input,p7,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p7,limits,null));
Tree(a,"{\"Value\":\"P.root\"}");Tree(b,"{\"Value\":\"P.root\"}");Check(((ExecutionOutputs)a.Result!).Extras.Count==0,"all extras empty");
InputError(c,FerruleRuntimeError.UnknownTarget,"missing","project has no named target `missing`");Check(!c.CallerIdentity,"selected validation precedes caller sentinel");
InputError(d,FerruleRuntimeError.UnexpectedNamedSource,"X","named source is not declared by this mapping");InputError(e,FerruleRuntimeError.DuplicateNamedSource,"Ref","named source was supplied more than once");
Check(f.Error is ArgumentNullException n && n.ParamName=="supplied" && n.InnerException is null && f.Result is null,"null record retains ArgumentNullException supplied");
Check(g.CallerIdentity && ReferenceEquals(g.Error,p6.Failure) && g.Result is null,"exact Count caller exception, no partial output");Check(h.CallerIdentity && ReferenceEquals(h.Error,p7.Failure) && h.Result is null,"exact Index caller exception, no partial output");
Reads(a,1,0);Reads(b,1,0);Reads(c,0,0);Reads(d,1,1);Reads(e,1,2);Reads(f,1,1);Reads(g,1,0);Reads(h,1,1);
"#;

const PUBLIC_LAZY_OWNER: &str = r#"
var input=Primary();var limits=Policy();var t=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(true))};var f=new[]{new KeyValuePair<string,FerruleValue>("flag",FerruleValue.FromBoolean(false))};
var yes=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(t));var no=FerruleExecutionContext.WithParameters("mapping.test",new FerruleRuntimeParameters(f));
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("C02.1",input,p0,limits,yes,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,yes),t);
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("C02.2",input,p1,limits,no,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p1,limits,no),f);
var p2=new Probe(new[]{new NamedInput("Ref",G(("Value",S("N.root"))))});var c=Reach("C02.3",input,p2,limits,no,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p2,limits,no),f);
Tree(a,"{\"Value\":\"P.root\"}");OwnedAbsent(b,"Ref",3U);Tree(c,"{\"Value\":\"N.root\"}");Reads(a,1,0);Reads(b,1,0);Reads(c,1,1);
"#;

const PUBLIC_P19_SELECTED: &str = r#"
var input=Primary();var limits=Policy();var selection=FerruleTargetSelection.Primary();var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("C03.1",input,p0,limits,null,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p0,limits,null));
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("C03.2",input,p1,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p1,limits,null));
Tree(a,"{\"Value\":\"P.root\"}");var e=Runtime(b,FerruleRuntimeError.MappingException);Check(e.Node==2u && e.MappingExceptionMessage is null && e.Message=="node 2: mapping exception: mapping exception was raised" && e.InnerException is null,"complete ordinary Raise");NullMetadata(e,"Node");Reads(a,1,0);Reads(b,1,0);
"#;

const PUBLIC_GLOBAL_ORDER: &str = r#"
var input=Primary();var limits=Policy();var namedRoot=G(("Rows",R(G(("Value",S("N1")),("Score",I(30))),G(("Value",S("N2")),("Score",I(10))))));var primary=FerruleTargetSelection.Primary();var named=FerruleTargetSelection.Named("named");
var p0=new Probe(new[]{new NamedInput("ref",namedRoot)});var a=Reach("C04.1",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(new[]{new NamedInput("ref",namedRoot)});var b=Reach("C04.2",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(new[]{new NamedInput("ref",namedRoot)});var c=Reach("C04.3",input,p2,limits,null,named,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,named,p2,limits,null));
var p3=new Probe(Array.Empty<NamedInput>());var d=Reach("C04.4",input,p3,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p3,limits,null));
var p4=new Probe(new[]{new NamedInput("X",S("x")),new NamedInput("ref",namedRoot)});var e=Reach("C04.5",input,p4,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p4,limits,null));
Failure(a,2,"N1");Failure(b,2,"N1");Failure(c,2,"N1");InputError(d,FerruleRuntimeError.MissingNamedSource,"ref","named source is required by this mapping");InputError(e,FerruleRuntimeError.UnexpectedNamedSource,"X","named source is not declared by this mapping");Reads(a,1,1);Reads(b,1,1);Reads(c,1,1);Reads(d,1,0);Reads(e,1,1);
"#;

const PUBLIC_GLOBAL_POSITION: &str = r#"
var input=Primary();var limits=Policy();var primary=FerruleTargetSelection.Primary();var named=FerruleTargetSelection.Named("named");
var p0=new Probe(Array.Empty<NamedInput>());var a=Reach("C05.1",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(Array.Empty<NamedInput>());var b=Reach("C05.2",input,p1,limits,null,primary,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,primary,p1,limits,null));
var p2=new Probe(Array.Empty<NamedInput>());var c=Reach("C05.3",input,p2,limits,null,named,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,named,p2,limits,null));
Failure(a,1,"2");Failure(b,1,"2");Failure(c,1,"2");Reads(a,1,0);Reads(b,1,0);Reads(c,1,0);
"#;

const PUBLIC_OWNED_WALK: &str = r#"
var input=Primary();var limits=Policy();var root=G(("Rows",R(G(("Value",S("N1"))),G(("Value",S("N2"))))));var selection=FerruleTargetSelection.Primary();
var p0=new Probe(new[]{new NamedInput("Ref",root)});var a=Reach("C06.1",input,p0,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,p0,limits,null));
var p1=new Probe(new[]{new NamedInput("Ref",root)});var b=Reach("C06.2",input,p1,limits,null,selection,()=>GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(input,selection,p1,limits,null));
Tree(a,"[{\"Value\":\"N1\",\"Position\":1},{\"Value\":\"N2\",\"Position\":2}]");Tree(b,"[{\"Value\":\"N1\",\"Position\":1},{\"Value\":\"N2\",\"Position\":2}]");Check(((ExecutionOutputs)a.Result!).Extras.Count==0,"complete all extras empty");Reads(a,1,1);Reads(b,1,1);
"#;

const PUBLIC_SCALAR_ZERO: &str = r#"
var input=S("P.root");var supplied=new Probe(Array.Empty<NamedInput>());var limits=new FerruleOwnedSourceLimits(1,0,0,0,0);var a=Reach("C07.1",input,supplied,limits,null,null,()=>GeneratedMapping.ExecuteWithBasicOwnedSources(input,supplied,limits,null));
Tree(a,"{\"Value\":\"P.root\"}");Check(((ExecutionOutputs)a.Result!).Extras.Count==0,"empty catalogue/all extras");Reads(a,1,0);
"#;

// #566 target ValueMap only: the shared Basic-aware ordinary renderer remains
// authoritative. Defaults/table entries are literal Values, never graph nodes.
