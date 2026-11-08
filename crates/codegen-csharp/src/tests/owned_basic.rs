//! Owned-source C# generation for Basic programs.
//!
//! Covers both the private renderer (`render_basic_owned_mapping`, with and
//! without checked catalogue admission) and the public emitters
//! (`emit_with_basic_owned_sources` and its filter and sort companions):
//! admission and typed refusals, source failures, target value maps, root pins,
//! leaf provenance, filtering and sorting. Ordinary emitter output stays
//! byte-for-byte unchanged.
//!
//! FERRULE_PRIVATE_OWNED_CSHARP_HOST_DIR and FERRULE_PRIVATE_CHECKED_CSHARP_HOST_DIR
//! retain the generated Mapping, TargetBuilder and complete typed host;
//! FERRULE_PUBLIC_BASIC_CSHARP_HOST_DIR retains the public consumers.
//! `owned_basic_consumers` builds and runs all of them.
use super::*;
use crate::owned_basic::render_basic_owned_mapping;
use ::mapping::{OwnedPosition, OwnedSourceField, OwnedSourceRef, SourceOwner};
use codegen::{NamedSourceProgram, NamedTargetProgram, ScalarTargetDomain, TargetConstruction};

#[path = "owned_basic/checked_catalogues.rs"]
mod checked_catalogues;
#[path = "owned_basic/failure_value_maps.rs"]
mod failure_value_maps;
#[path = "owned_basic/filtering.rs"]
mod filtering;
#[path = "owned_basic/node_errors.rs"]
mod node_errors;
#[path = "owned_basic/private_hosts.rs"]
mod private_hosts;
#[path = "owned_basic/public_consumers.rs"]
mod public_consumers;
#[path = "owned_basic/public_value_maps.rs"]
mod public_value_maps;
#[path = "owned_basic/retained_inventory.rs"]
mod retained_inventory;
#[path = "owned_basic/root_pins.rs"]
mod root_pins;
#[path = "owned_basic/sorting.rs"]
mod sorting;
#[path = "owned_basic/source_failures.rs"]
mod source_failures;
#[path = "owned_basic/target_value_maps.rs"]
mod target_value_maps;
use checked_catalogues::{
    checked_result, checked_rows, complete_checked_host, retain_checked_host,
};
use filtering::assert_public_basic_typed_artifacts_match;
use public_consumers::{
    PUBLIC_BASIC_CONSUMER_PROJECT, public_basic_consumer, public_basic_result,
    retain_public_basic_consumer,
};
use public_value_maps::public582_map;
use retained_inventory::{
    retained585_extract, retained585_function, retained585_function_expected,
    retained585_leaf_expected, retained585_result,
};
use sorting::{sort_669_program, sort_669_scope_control};
use source_failures::{
    checked_failure_program, expected_failure_diagnostic, failure_legacy, failure_program,
    failure_scores, source_failure,
};
use target_value_maps::{mapped_string, target_value_map};

fn path(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|part| (*part).into()).collect()
}
fn named(name: &str) -> SourceOwner {
    SourceOwner::Named { name: name.into() }
}
fn field(owner: SourceOwner, parts: &[&str], frame: Option<&[&str]>) -> Expression {
    Expression::OwnedSourceField(OwnedSourceField {
        source: OwnedSourceRef {
            owner,
            path: path(parts),
        },
        frame: frame.map(path),
    })
}
fn position(owner: SourceOwner, collection: Option<&[&str]>) -> Expression {
    Expression::OwnedPosition(OwnedPosition {
        owner,
        collection: collection.map(path),
    })
}
fn expression(id: u32, expression: Expression) -> ExpressionNode {
    ExpressionNode { id, expression }
}
fn binding(name: &str, id: u32, ty: ScalarType) -> Binding {
    Binding {
        target_field: name.into(),
        expression: id,
        target_domain: ty.into(),
        repeating: false,
    }
}
fn group(bindings: Vec<Binding>) -> TargetScope {
    TargetScope {
        target_field: String::new(),
        repeating: false,
        iteration: None,
        construction: TargetConstruction::Group,
        bindings,
        children: Vec::new(),
    }
}
fn owned_iteration(owner: SourceOwner, parts: &[&str], bindings: Vec<Binding>) -> TargetScope {
    let mut scope = group(bindings);
    scope.repeating = true;
    scope.iteration = Some(IterationPlan::owned_source(OwnedSourceRef {
        owner,
        path: path(parts),
    }));
    scope
}
fn plain_iteration(parts: &[&str], bindings: Vec<Binding>) -> TargetScope {
    let mut scope = group(bindings);
    scope.repeating = true;
    scope.iteration = Some(IterationPlan::source(path(parts)));
    scope
}
fn source_schema(name: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Value", ScalarType::String),
            SchemaNode::scalar("Score", ScalarType::Int),
            SchemaNode::group(
                "Details",
                vec![SchemaNode::scalar("Counter", ScalarType::Int)],
            ),
            SchemaNode::group("Ref", vec![SchemaNode::scalar("Value", ScalarType::String)]),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::scalar("Value", ScalarType::String),
                    SchemaNode::scalar("Score", ScalarType::Int),
                    SchemaNode::group(
                        "Details",
                        vec![SchemaNode::scalar("Counter", ScalarType::Int)],
                    ),
                    SchemaNode::group(
                        "Rows",
                        vec![SchemaNode::scalar("Value", ScalarType::String)],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    )
}
fn target_schema(scope: &TargetScope, name: &str) -> SchemaNode {
    let mut node = match scope.construction {
        TargetConstruction::Scalar {
            target_domain: ScalarTargetDomain::Single(ty),
            ..
        } => SchemaNode::scalar(name, ty),
        TargetConstruction::Group => {
            let mut children = scope
                .bindings
                .iter()
                .map(|binding| {
                    let ScalarTargetDomain::Single(ty) = binding.target_domain else {
                        panic!("literal fixture domain")
                    };
                    let node = SchemaNode::scalar(&binding.target_field, ty);
                    if binding.repeating {
                        node.repeating()
                    } else {
                        node
                    }
                })
                .collect::<Vec<_>>();
            children.extend(
                scope
                    .children
                    .iter()
                    .map(|child| target_schema(child, &child.target_field)),
            );
            SchemaNode::group(name, children)
        }
        _ => panic!("private fixture construction"),
    };
    if scope.repeating {
        node = node.repeating();
    }
    node
}
fn basic(expressions: Vec<ExpressionNode>, root: TargetScope) -> Program {
    Program {
        execution_contract: None,
        xml_boundary: None,
        source: source_schema("Primary"),
        extra_sources: ["Unused", "Ref", " Ref 雪 ", "ref", "primary"]
            .iter()
            .map(|name| NamedSourceProgram {
                name: (*name).into(),
                source: source_schema("Static"),
                dynamic: None,
            })
            .collect(),
        target: target_schema(&root, "Output"),
        root,
        expressions,
        user_functions: vec![],
        failure_rules: vec![],
        extra_targets: vec![],
    }
}
fn add_target(program: &mut Program, name: &str, root: TargetScope) {
    program.extra_targets.push(NamedTargetProgram {
        name: name.into(),
        target: target_schema(&root, "NamedOutput"),
        root,
    });
}
fn complete_host(body: &str) -> String {
    let mut host = HOST_PREFIX.to_string();
    host.push_str(HOST_ERROR_SNAPSHOT);
    host.push_str("\n    public static void Main()\n    {\n");
    host.push_str(body);
    host.push_str(HOST_SUFFIX);
    host
}
fn retain_host(id: &str, program: &Program, body: &str) -> String {
    let result = render_basic_owned_mapping(program);
    eprintln!(
        "{id} complete typed Program={program:#?} private renderer Result={result:#?} complete host body={body}"
    );
    let mapping = result.expect("valid frozen Basic fixture renders privately");
    let host = complete_host(body);
    eprintln!("{id} complete generated typed host={host}");
    if let Some(directory) = std::env::var_os("FERRULE_PRIVATE_OWNED_CSHARP_HOST_DIR") {
        let directory = std::path::PathBuf::from(directory).join(id);
        std::fs::create_dir_all(&directory).expect("protected actor supplied output directory");
        std::fs::write(directory.join("GeneratedMapping.cs"), &mapping)
            .expect("retain private mapping");
        std::fs::write(
            directory.join("GeneratedTargetBuilder.cs"),
            crate::runtime::TARGET_BUILDER,
        )
        .expect("retain unchanged target helper");
        std::fs::write(directory.join("Program.cs"), host).expect("retain complete typed host");
        std::fs::write(directory.join("Program.ir.txt"), format!("{program:#?}\n"))
            .expect("retain complete typed input");
    }
    mapping
}

const HOST_PREFIX: &str = r#"using System;
using System.Linq;
using System.Collections.Generic;
using System.Text.Json;
using Ferrule.Runtime;
namespace Ferrule.Generated;
public static partial class GeneratedMapping
{
    private static readonly List<uint> NodeTrace=new();
    private static readonly List<object> FrameTrace=new();
    static partial void ObserveOwnedBasicNode(uint node,FerruleBasicScopeContext context)
    {
        NodeTrace.Add(node);
        FrameTrace.Add(new { node, legacyFrames=context.LegacyView.Frames.Select(InstanceSnapshot).ToArray() });
    }
    private sealed record DirectPlainResult(FerruleInstance Tree,FerruleBasicSourceCandidate Candidate);
    private sealed record Reached(object? Result,Exception? Error,uint[] Nodes,object[] Frames,string CompleteInput,string CompleteActual);
    private static FerruleInstance Input=PrimaryFixture();
    private static FerruleDeclaredNamedSourceRoot[] Declared=Declarations();
    private static FerruleOwnedSourceLimits Limits=new(16,8,16,1048576,64);
    private static FerruleExecutionContext? Execution=null;
    private static KeyValuePair<string,FerruleValue>[] ParameterValues=Array.Empty<KeyValuePair<string,FerruleValue>>();
    private static string Json(object? value)=>JsonSerializer.Serialize(value);
    private static Reached ReachWithLimits(string id,FerruleOwnedSourceLimits limits)
    { Limits=limits;return Reach(id,()=>ExecuteOwnedBasic(Input,Declared,Limits)); }
    private static Reached ReachSelected(string id,FerruleTargetSelection selection)
    { return Reach(id,()=>ExecuteSelectedOwnedBasic(Input,selection,Declared,Limits,Execution),selection); }
    private static Reached Reach(string id,Func<object?> subject,FerruleTargetSelection? selection=null)
    {
        NodeTrace.Clear();FrameTrace.Clear();
        var input=Json(new { classification="complete_typed_input", selection=selection is null?null:new {selection.IsPrimary,selection.Name},source=InstanceSnapshot(Input),
            declarations=Declared.Select((d,i)=>new { index=i,d.Name,source=InstanceSnapshot(d.Instance) }).ToArray(),
            limits=new { roots=(ulong)Limits.MaxRoots,frames=(ulong)Limits.MaxActiveFrames,segments=(ulong)Limits.MaxPathSegments,bytes=(ulong)Limits.MaxMetadataBytes,contexts=(ulong)Limits.MaxWalkContexts },
            execution=Execution is null?null:new { Execution.MappingFilePath,Execution.MainMappingFilePath,Execution.CurrentDateTime,parameterCount=Execution.RuntimeParameters.Count,parameters=ParameterValues.Select(p=>new {p.Key,value=ValueSnapshot(p.Value)}).ToArray() } });
        Console.WriteLine(Json(new {id,classification="before_subject",completeInput=input}));
        object? result=null;Exception? error=null;
        try { result=subject(); } catch(Exception reached) { error=reached; }
        var nodes=NodeTrace.ToArray();var frames=FrameTrace.ToArray();
        var complete=Json(new { classification="actual_reached_result", result=ResultSnapshot(result),error=error is null?null:ErrorSnapshot(error),nodes,frames });
        Console.WriteLine(Json(new {id,classification="before_comparisons",completeInput=input,completeActual=complete}));
        return new Reached(result,error,nodes,frames,input,complete);
    }
    private static object? ResultSnapshot(object? result)=>result switch
    {
        ExecutionOutputs all=>new {kind="ExecutionOutputs",primary=InstanceSnapshot(all.Primary),extras=all.Extras.Select(e=>new {e.Name,instance=InstanceSnapshot(e.Instance)}).ToArray()},
        SelectedTargetOutput.Primary primary=>new {kind="Primary",instance=InstanceSnapshot(primary.Instance)},
        SelectedTargetOutput.Named named=>new {kind="Named",named.Output.Name,instance=InstanceSnapshot(named.Output.Instance)},
        DirectPlainResult plain=>new {kind="direct_owned_plain_runtime_control",tree=InstanceSnapshot(plain.Tree),sourceId=(ulong)plain.Candidate.SourceId,raw=plain.Candidate.RawSourceOrdinal.HasValue?(ulong?)plain.Candidate.RawSourceOrdinal.Value:null,newCursor=plain.Candidate.HasNewCursor,legacyFrames=plain.Candidate.Context.LegacyView.Frames.Select(InstanceSnapshot).ToArray()},
        FerruleInstance instance=>InstanceSnapshot(instance),
        FerruleValue value=>ValueSnapshot(value),
        Exception error=>ErrorSnapshot(error),
        null=>null,
        _=>throw new InvalidOperationException("Unrecorded returned result type")
    };
    private static object ValueSnapshot(FerruleValue value)=>new {kind=value.Kind.ToString(),value=value.Kind switch
    {FerruleValueKind.String=>(object?)value.StringValue,FerruleValueKind.Int64=>value.Int64Value,FerruleValueKind.Bool=>value.BooleanValue,FerruleValueKind.Double=>BitConverter.DoubleToInt64Bits(value.DoubleValue),_=>null}};
    private static object? InstanceSnapshot(FerruleInstance? value)=>value switch
    {
        null=>null,FerruleScalar scalar=>new {kind="Scalar",value=ValueSnapshot(scalar.Value)},
        FerruleGroup group=>new {kind="Group",fields=group.Fields.Select(field=>new {field.Name,value=InstanceSnapshot(field.Value)}).ToArray()},
        FerruleRepeated repeated=>new {kind="Repeated",items=repeated.Items.Select(InstanceSnapshot).ToArray()},
        _=>throw new InvalidOperationException("Unrecorded input instance kind")
    };
    private static object? Plain(FerruleInstance value)=>value switch
    {
        FerruleScalar scalar=>scalar.Value.Kind switch {FerruleValueKind.String=>(object?)scalar.Value.StringValue,FerruleValueKind.Int64=>scalar.Value.Int64Value,FerruleValueKind.Bool=>scalar.Value.BooleanValue,FerruleValueKind.Null=>"Null",FerruleValueKind.JsonNull=>"JsonNull",FerruleValueKind.XmlNil=>"XmlNil",FerruleValueKind.Double=>scalar.Value.DoubleValue,_=>throw new InvalidOperationException()},
        FerruleGroup group=>group.Fields.ToDictionary(field=>field.Name,field=>Plain(field.Value),StringComparer.Ordinal),
        FerruleRepeated repeated=>repeated.Items.Select(Plain).ToArray(),_=>throw new InvalidOperationException()
    };
    private static void Check(bool condition,string expected) { if(!condition)throw new InvalidOperationException("Frozen expected: "+expected); }
    private static void ExpectTree(Reached actual,string expected)
    {
        Check(actual.Error is null,"successful typed return");
        var instance=actual.Result switch {ExecutionOutputs all=>all.Primary,SelectedTargetOutput.Primary primary=>primary.Instance,SelectedTargetOutput.Named named=>named.Output.Instance,DirectPlainResult plain=>plain.Tree,FerruleInstance root=>root,_=>throw new InvalidOperationException("Missing tree")};
        var reached=Json(Plain(instance));Console.WriteLine(Json(new {classification="comparison",actual=reached,expected}));Check(reached==expected,expected);
    }
    private static FerruleRuntimeException ExpectRuntime(Reached actual,FerruleRuntimeError expected)
    {var error=actual.Error as FerruleRuntimeException;Check(error is not null && error.Error==expected,"outer "+expected);return error!;}
    private static FerruleOwnedSourceException ExpectOwned(Reached actual,FerruleOwnedSourceErrorKind expected,bool? primary,string? name,string[]? collection)
    {
        var outer=ExpectRuntime(actual,FerruleRuntimeError.OwnedSource);var error=outer.OwnedSourceFailure;
        Check(error is not null && ReferenceEquals(error,outer.InnerException) && error.Error==expected,"bare owned cause "+expected);
        var found=error!;
        if(primary.HasValue)Check(found.Owner?.IsPrimary==primary && found.Owner?.Name==name && found.RequestedName==name,"exact owner/name");
        Check(collection is null?found.Collection is null:found.Collection is not null && found.Collection.SequenceEqual(collection),"exact None or Some collection");
        Check(found.DeclarationIndices is null && found.Frame is null && found.RequestedIndex is null,"unchanged absent declaration/frame/index fields");
        if(expected!=FerruleOwnedSourceErrorKind.Traversal && expected!=FerruleOwnedSourceErrorKind.UniteratedRepetition)
            Check(found.Path is null && found.Segment is null && found.FoundInstance is null && found.InnerException is null,"no invented traversal payload");
        if(expected!=FerruleOwnedSourceErrorKind.MetadataLimit && expected!=FerruleOwnedSourceErrorKind.MetadataArithmeticOverflow)
            Check(found.LimitKind is null && found.Used is null && found.Requested is null && found.Maximum is null,"no invented resource payload");
        Check(outer.Node is null && outer.Function is null && outer.ExpectedArity is null && outer.ActualArity is null && outer.FoundKind is null && outer.AggregateOperation is null && outer.Detail is null && outer.RequestedItems is null && outer.MaximumItems is null && outer.MaximumDepth is null && outer.RuntimeValue is null && outer.FailureRule is null && outer.MappingFailureMessage is null && outer.MappingExceptionMessage is null && outer.Join is null && outer.UserFunction is null && outer.FunctionParameter is null && outer.ExpectedScalarType is null && outer.RuntimeParameter is null && outer.SourceField is null && outer.FoundInstance is null && outer.PrimaryRoot is null && outer.BasicSourceBridgeFailure is null,"complete untouched runtime cause envelope");
        Check(actual.Result is null,"no partial returned outcome");return found;
    }
    private static void ExpectLimit(Reached actual,FerruleOwnedSourceLimitKind expected,nuint maximum)
    {var error=ExpectOwned(actual,FerruleOwnedSourceErrorKind.MetadataLimit,null,null,null);Check(error.LimitKind==expected && error.Maximum==maximum,"exact limit kind and maximum");}
    private static FerruleInstance S(string value)=>new FerruleScalar(FerruleValue.FromString(value));
    private static FerruleInstance I(long value)=>new FerruleScalar(FerruleValue.FromInt64(value));
    private static FerruleInstance G(params (string Name,FerruleInstance Value)[] fields)=>new FerruleGroup(fields.Select(field=>new FerruleField(field.Name,field.Value)));
    private static FerruleInstance R(params FerruleInstance[] values)=>new FerruleRepeated(values);
    private static FerruleInstance PrimaryFixture()=>G(("Value",S("P.root")),("Ref",G(("Value",S("P.collision")))),("Details",G(("Counter",I(99)))),("Rows",R(G(("Value",S("P1")),("Score",I(30)),("Rows",R(G(("Value",S("P1.1"))),G(("Value",S("P1.2")))))),G(("Value",S("P2")),("Score",I(10))),G(("Value",S("P3")),("Score",I(20)),("Details",G())))));
    private static FerruleDeclaredNamedSourceRoot[] Declarations()=>new[]{new FerruleDeclaredNamedSourceRoot("Unused",null),new FerruleDeclaredNamedSourceRoot("Ref",G(("Value",S("N.root")),("Rows",R(G(("Value",S("N1")),("Score",I(30)),("Details",new FerruleScalar(FerruleValue.JsonNull))),G(("Value",S("N2")),("Score",I(10)),("Details",G(("Counter",I(7))))))))),new FerruleDeclaredNamedSourceRoot(" Ref 雪 ",G(("Value",S("N.exact")))),new FerruleDeclaredNamedSourceRoot("ref",G(("Value",S("N.lower")))),new FerruleDeclaredNamedSourceRoot("primary",G(("Value",S("N.literal.primary"))))};
"#;
const HOST_SUFFIX: &str = r#"
    }
}
"#;

const HOST_ERROR_SNAPSHOT: &str = r#"    private static object ErrorSnapshot(Exception error)
    {
        var outer = error as FerruleRuntimeException;
        var owned = error as FerruleOwnedSourceException;
        var bridge = error as FerruleBasicSourceBridgeException;
        var traversal = error as FerruleOwnedSourceTraversalException;
        return new
        {
            type = error.GetType().FullName, error.Message, error.HResult, fullNative = error.ToString(),
            argument = (error as ArgumentException)?.ParamName,
            runtime = outer is null ? null : new
            {
                category = outer.Error.ToString(), outer.Node, outer.Function, outer.ExpectedArity, outer.ActualArity,
                foundKind = outer.FoundKind?.ToString(), aggregate = outer.AggregateOperation?.ToString(), outer.Detail,
                requestedItems = outer.RequestedItems?.ToString(), maximumItems = outer.MaximumItems?.ToString(), outer.MaximumDepth,
                runtimeValue = outer.RuntimeValue?.ToString(), outer.FailureRule, outer.MappingFailureMessage,
                outer.MappingExceptionMessage, outer.Join, outer.UserFunction, outer.FunctionParameter,
                expectedScalar = outer.ExpectedScalarType?.ToString(), outer.RuntimeParameter, outer.SourceField,
                outer.FoundInstance, outer.PrimaryRoot,
                ownedSameInner = outer.OwnedSourceFailure is not null && ReferenceEquals(outer.OwnedSourceFailure, outer.InnerException),
                bridgeSameInner = outer.BasicSourceBridgeFailure is not null && ReferenceEquals(outer.BasicSourceBridgeFailure, outer.InnerException),
                owned = outer.OwnedSourceFailure is null ? null : ErrorSnapshot(outer.OwnedSourceFailure),
                bridge = outer.BasicSourceBridgeFailure is null ? null : ErrorSnapshot(outer.BasicSourceBridgeFailure),
            },
            owned = owned is null ? null : new
            {
                kind = owned.Error.ToString(), owner = owned.Owner?.Name, primary = owned.Owner?.IsPrimary,
                name = owned.RequestedName, declarations = owned.DeclarationIndices?.ToArray(),
                path = owned.Path?.ToArray(), frame = owned.Frame?.ToArray(), collection = owned.Collection?.ToArray(),
                segment = owned.Segment.HasValue ? (ulong?)owned.Segment.Value : null,
                found = owned.FoundInstance?.ToString(), index = owned.RequestedIndex.HasValue ? (ulong?)owned.RequestedIndex.Value : null,
                limit = owned.LimitKind?.ToString(), used = owned.Used.HasValue ? (ulong?)owned.Used.Value : null,
                requested = owned.Requested.HasValue ? (ulong?)owned.Requested.Value : null,
                maximum = owned.Maximum.HasValue ? (ulong?)owned.Maximum.Value : null,
            },
            bridge = bridge is null ? null : new
            {
                kind = bridge.Error.ToString(), component = bridge.Component?.ToString(),
                used = bridge.Used.HasValue ? (ulong?)bridge.Used.Value : null,
                requested = bridge.Requested.HasValue ? (ulong?)bridge.Requested.Value : null,
                maximum = bridge.Maximum.HasValue ? (ulong?)bridge.Maximum.Value : null,
                input = bridge.InputOrdinal.HasValue ? (ulong?)bridge.InputOrdinal.Value : null,
                candidate = bridge.CandidateOrdinal.HasValue ? (ulong?)bridge.CandidateOrdinal.Value : null,
                declaration = bridge.DeclarationIndex.HasValue ? (ulong?)bridge.DeclarationIndex.Value : null,
                declared = bridge.DeclaredCount.HasValue ? (ulong?)bridge.DeclaredCount.Value : null,
                found = bridge.Found?.ToString(),
            },
            traversal = traversal is null ? null : new
            { kind = traversal.Error.ToString(), path = traversal.Path.ToArray(),
              segment = traversal.Segment.HasValue ? (ulong?)traversal.Segment.Value : null, found = traversal.Found.ToString() },
            inner = error.InnerException is null ? null : ErrorSnapshot(error.InnerException),
        };
    }
"#;

include!("owned_windows.rs");
include!("owned_row_failures.rs");
include!("owned_group_by.rs");
include!("owned_group_adjacent_by.rs");
include!("owned_group_starting_with.rs");
include!("owned_group_ending_with.rs");
include!("owned_group_into_blocks.rs");
include!("owned_sequence_item_at.rs");
