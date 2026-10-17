using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text.Json;
using Ferrule.Generated;
using Ferrule.Runtime;

namespace Ferrule.Conformance.BasicOwnedWindows;

internal static class Program
{
    private const string ProgramName = "__PROGRAM__";
    private const string ProgramsSha256 = "__PROGRAMS_SHA256__";
    private const string ExpectedSha256 = "__EXPECTED_SHA256__";
    private static readonly Dictionary<string, string[]> ModelCases = new(StringComparer.Ordinal)
    {
        ["operators"] = new[] { "W01", "W11", "W12" },
        ["filter_then_sort"] = new[] { "W02" },
        ["sort_then_filter"] = new[] { "W03" },
        ["nested_parent_bounds"] = new[] { "W04" },
        ["empty_bounds_eager"] = new[] { "W05" },
        ["zero_window_non_bool"] = new[] { "W06" },
        ["invalid_bound"] = new[] { "W07" },
        ["fts_predicate_priority"] = new[] { "W08" },
        ["stf_key_priority"] = new[] { "W09" },
        ["first_predicate_priority"] = new[] { "W10" },
    };

    private sealed record Actual(
        string Id,
        JsonElement Call,
        FerruleInstance Source,
        NamedInput[] Supplied,
        JsonElement Input,
        object? Result,
        Exception? Error,
        JsonElement PublicResult,
        JsonElement CompleteError,
        JsonElement Semantic,
        JsonElement TypedError);

    private static string Json(object? value) => JsonSerializer.Serialize(value);
    private static void Check(bool value, string message)
    {
        if (!value) throw new InvalidOperationException(message);
    }

    private static JsonDocument FrozenDocument(string path, string sha256)
    {
        var bytes = File.ReadAllBytes(path);
        var actual = Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();
        if (!StringComparer.Ordinal.Equals(actual, sha256))
            throw new InvalidOperationException("Frozen fixture bytes differ: " + path);
        return JsonDocument.Parse(bytes);
    }

    private static string Text(JsonElement value) =>
        value.GetString() ?? throw new InvalidOperationException("Frozen string is null.");

    private static FerruleValue ReadValue(JsonElement instance)
    {
        var payload = instance.GetProperty("value");
        return Text(instance.GetProperty("type")) switch
        {
            "Null" when payload.ValueKind == JsonValueKind.Null => FerruleValue.Null,
            "JsonNull" when payload.ValueKind == JsonValueKind.Null => FerruleValue.JsonNull,
            "XmlNil" when payload.ValueKind == JsonValueKind.Null => FerruleValue.XmlNil,
            "Bool" => FerruleValue.FromBoolean(payload.GetBoolean()),
            "Float" => FerruleValue.FromDouble(BitConverter.Int64BitsToDouble(unchecked((long)Convert.ToUInt64(Text(payload),16)))),
            "Int" => FerruleValue.FromInt64(payload.GetInt64()),
            "String" => FerruleValue.FromString(Text(payload)),
            _ => throw new InvalidOperationException("Unrecorded frozen scalar tag or payload."),
        };
    }

    private static FerruleInstance ReadInstance(JsonElement instance) =>
        Text(instance.GetProperty("kind")) switch
        {
            "Scalar" => new FerruleScalar(ReadValue(instance)),
            "Group" => new FerruleGroup(instance.GetProperty("fields").EnumerateArray()
                .Select(field => new FerruleField(
                    Text(field.GetProperty("name")), ReadInstance(field.GetProperty("value"))))),
            "Repeated" => new FerruleRepeated(instance.GetProperty("items").EnumerateArray()
                .Select(ReadInstance)),
            _ => throw new InvalidOperationException("Unrecorded frozen input instance kind."),
        };

    private static FerruleTargetSelection? ReadSelection(JsonElement selection) =>
        Text(selection.GetProperty("kind")) switch
        {
            "All" => null,
            "Primary" => FerruleTargetSelection.Primary(),
            "Named" => FerruleTargetSelection.Named(Text(selection.GetProperty("name"))),
            _ => throw new InvalidOperationException("Unrecorded frozen target selection."),
        };

    private static FerruleOwnedSourceLimits ReadLimits(JsonElement limits) => new(
        checked((nuint)limits.GetProperty("max_roots").GetUInt64()),
        checked((nuint)limits.GetProperty("max_active_frames").GetUInt64()),
        checked((nuint)limits.GetProperty("max_path_segments").GetUInt64()),
        checked((nuint)limits.GetProperty("max_metadata_bytes").GetUInt64()),
        checked((nuint)limits.GetProperty("max_walk_contexts").GetUInt64()));

    private static object InputSnapshot(FerruleInstance source, NamedInput[] supplied) => new
    {
        primary = InstanceSnapshot(source),
        supplied = supplied.Select(named => new
        {
            name = named.Name,
            instance = InstanceSnapshot(named.Instance),
        }).ToArray(),
    };

    private static object ValueSnapshot(FerruleValue value) => new
    {
        kind = "Scalar",
        type = value.Kind switch {FerruleValueKind.Int64=>"Int",FerruleValueKind.Double=>"Float",_=>value.Kind.ToString()},
        value = value.Kind switch
        {
            FerruleValueKind.String => (object?)value.StringValue,
            FerruleValueKind.Int64 => value.Int64Value,
            FerruleValueKind.Bool => value.BooleanValue,
            FerruleValueKind.Double => unchecked((ulong)BitConverter.DoubleToInt64Bits(value.DoubleValue)).ToString("x16"),
            FerruleValueKind.Null or FerruleValueKind.JsonNull or FerruleValueKind.XmlNil => null,
            _ => throw new InvalidOperationException("unrecorded scalar tag"),
        },
    };

    private static object? InstanceSnapshot(FerruleInstance? value) => value switch
    {
        null => null,
        FerruleScalar scalar => ValueSnapshot(scalar.Value),
        FerruleGroup group => new
        {
            kind = "Group",
            fields = group.Fields.Select(field => new { name = field.Name, value = InstanceSnapshot(field.Value) }).ToArray(),
        },
        FerruleRepeated repeated => new { kind = "Repeated", items = repeated.Items.Select(InstanceSnapshot).ToArray() },
        FerruleMappedSequence mapped => new { kind = "MappedSequence", items = mapped.Items.Select(InstanceSnapshot).ToArray() },
        FerruleDocumentSet documents => new
        {
            kind = "DocumentSet",
            documents = documents.Documents.Select(document => new
            {
                path = document.Path,
                resolved_source_path = document.ResolvedSourcePath,
                value = InstanceSnapshot(document.Value),
            }).ToArray(),
        },
        _ => throw new InvalidOperationException("unrecorded fixture instance type"),
    };

    private static object? ResultSnapshot(object? result) => result switch
    {
        ExecutionOutputs all => new
        {
            kind = "All",
            primary = InstanceSnapshot(all.Primary),
            extras = all.Extras.Select(extra => new { name = extra.Name, instance = InstanceSnapshot(extra.Instance) }).ToArray(),
        },
        SelectedTargetOutput.Primary primary => new { kind = "SelectedPrimary", instance = InstanceSnapshot(primary.Instance) },
        SelectedTargetOutput.Named named => new
        {
            kind = "SelectedNamed",
            name = named.Output.Name,
            instance = InstanceSnapshot(named.Output.Instance),
        },
        null => null,
        _ => throw new InvalidOperationException("unrecorded public return type"),
    };

    private static object ErrorSnapshot(Exception error)
    {
        var runtime = error as FerruleRuntimeException;
        var owned = error as FerruleOwnedSourceException;
        var bridge = error as FerruleBasicSourceBridgeException;
        var traversal = error as FerruleOwnedSourceTraversalException;
        return new
        {
            type = error.GetType().FullName,
            assembly_qualified_type = error.GetType().AssemblyQualifiedName,
            message = error.Message,
            hresult = error.HResult,
            source = error.Source,
            stack_trace = error.StackTrace,
            target_site = error.TargetSite?.ToString(),
            help_link = error.HelpLink,
            base_exception_same_error = ReferenceEquals(error.GetBaseException(), error),
            full_native = error.ToString(),
            argument = (error as ArgumentException)?.ParamName,
            data = error.Data.Keys.Cast<object>().Select(key => new
            {
                key_type = key.GetType().FullName,
                key = key.ToString(),
                value_type = error.Data[key]?.GetType().FullName,
                value = error.Data[key]?.ToString(),
            }).ToArray(),
            runtime = runtime is null ? null : new
            {
                category = runtime.Error.ToString(),
                category_numeric = (int)runtime.Error,
                same_captured_exception = ReferenceEquals(runtime, error),
                runtime.Node,
                runtime.Function,
                runtime.ExpectedArity,
                runtime.ActualArity,
                found_kind = runtime.FoundKind?.ToString(),
                aggregate = runtime.AggregateOperation?.ToString(),
                runtime.Detail,
                requested_items = runtime.RequestedItems?.ToString(),
                maximum_items = runtime.MaximumItems?.ToString(),
                runtime.MaximumDepth,
                runtime_value = runtime.RuntimeValue?.ToString(),
                runtime.FailureRule,
                runtime.MappingFailureMessage,
                runtime.MappingExceptionMessage,
                runtime.Join,
                runtime.UserFunction,
                runtime.FunctionParameter,
                expected_scalar = runtime.ExpectedScalarType?.ToString(),
                runtime.RuntimeParameter,
                runtime.SourceField,
                runtime.FoundInstance,
                runtime.PrimaryRoot,
                owned_same_inner = runtime.OwnedSourceFailure is not null && ReferenceEquals(runtime.OwnedSourceFailure, runtime.InnerException),
                bridge_same_inner = runtime.BasicSourceBridgeFailure is not null && ReferenceEquals(runtime.BasicSourceBridgeFailure, runtime.InnerException),
                owned = runtime.OwnedSourceFailure is null ? null : ErrorSnapshot(runtime.OwnedSourceFailure),
                bridge = runtime.BasicSourceBridgeFailure is null ? null : ErrorSnapshot(runtime.BasicSourceBridgeFailure),
            },
            owned = owned is null ? null : new
            {
                kind = owned.Error.ToString(),
                owner = owned.Owner is null ? null : new { is_primary = owned.Owner.IsPrimary, name = owned.Owner.Name },
                requested_name = owned.RequestedName,
                declarations = owned.DeclarationIndices?.ToArray(),
                path = owned.Path?.ToArray(),
                frame = owned.Frame?.ToArray(),
                collection = owned.Collection?.ToArray(),
                segment = owned.Segment.HasValue ? (ulong?)owned.Segment.Value : null,
                found = owned.FoundInstance?.ToString(),
                index = owned.RequestedIndex.HasValue ? (ulong?)owned.RequestedIndex.Value : null,
                limit = owned.LimitKind?.ToString(),
                used = owned.Used.HasValue ? (ulong?)owned.Used.Value : null,
                requested = owned.Requested.HasValue ? (ulong?)owned.Requested.Value : null,
                maximum = owned.Maximum.HasValue ? (ulong?)owned.Maximum.Value : null,
            },
            bridge = bridge is null ? null : new
            {
                kind = bridge.Error.ToString(),
                component = bridge.Component?.ToString(),
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
            {
                kind = traversal.Error.ToString(),
                path = traversal.Path.ToArray(),
                segment = traversal.Segment.HasValue ? (ulong?)traversal.Segment.Value : null,
                found = traversal.Found.ToString(),
            },
            inner = error.InnerException is null ? null : ErrorSnapshot(error.InnerException),
        };
    }

    private static JsonElement Element(object? value) => JsonSerializer.SerializeToElement(value);
    private static object SemanticErrorSnapshot(Exception error)
    {
        if (error is not FerruleRuntimeException runtime) return new {kind="UnexpectedError",type=error.GetType().FullName,display=error.Message};
        if (runtime.Error is FerruleRuntimeError.NotABool or FerruleRuntimeError.NotAnItemCount) return new {kind="Error",category=runtime.Error.ToString(),node=runtime.Node,found=runtime.FoundKind switch {FerruleValueKind.Int64=>"int",FerruleValueKind.Bool=>"bool",FerruleValueKind.Double=>"float",FerruleValueKind.String=>"string",_=>runtime.FoundKind?.ToString()}};
        if (runtime.Error==FerruleRuntimeError.MappingException) return new {kind="Error",category="MappingException",node=runtime.Node,mapping_exception_message=runtime.MappingExceptionMessage};
        if (runtime.Error==FerruleRuntimeError.OwnedSource && runtime.OwnedSourceFailure is {Error:FerruleOwnedSourceErrorKind.SourceUnavailable} owned)
            return new {kind="Error",category="OwnedSourceUnavailable",node=runtime.Node,owner=new {kind=owned.Owner!.IsPrimary?"Primary":"Named",name=owned.Owner.Name},scope=(object?)null,scope_metadata_available=false};
        return new {kind="UnexpectedError",complete_typed_error=ErrorSnapshot(error)};
    }

    private static int SourceChainLength(Exception error)
    {
        var count = 0;
        for (var inner = error.InnerException; inner is not null; inner = inner.InnerException)
            count = checked(count + 1);
        return count;
    }

    private static object? TypedErrorSnapshot(Exception? error)
    {
        if (error is null) return null;
        if (error is not FerruleRuntimeException runtime) return new {variant="UnexpectedError",complete_error=ErrorSnapshot(error)};
        if(runtime.Error is FerruleRuntimeError.NotABool or FerruleRuntimeError.NotAnItemCount) return new {variant=runtime.Error.ToString(),node=runtime.Node,found_kind=runtime.FoundKind?.ToString(),display=runtime.Message,source_chain_length=SourceChainLength(runtime),typed_inner_null=runtime.InnerException is null};
        if(runtime.Error==FerruleRuntimeError.MappingException) return new {variant="MappingException",node=runtime.Node,mapping_exception_message=runtime.MappingExceptionMessage,display=runtime.Message,source_chain_length=SourceChainLength(runtime),typed_inner_null=runtime.InnerException is null};
        if(runtime.Error==FerruleRuntimeError.OwnedSource && runtime.OwnedSourceFailure is {Error:FerruleOwnedSourceErrorKind.SourceUnavailable} owned)
            return new {variant="OwnedSource",node=runtime.Node,scope=(object?)null,scope_metadata_available=false,source=new {variant="SourceUnavailable",owner=new {kind=owned.Owner!.IsPrimary?"Primary":"Named",name=owned.Owner.Name}},display=runtime.Message,source_chain_length=SourceChainLength(runtime),typed_inner_null=runtime.InnerException is null,original_source_is_typed_cause=ReferenceEquals(owned,runtime.InnerException)};
        return new {variant="UnexpectedError",complete_error=ErrorSnapshot(error)};
    }

    private static object Invoke(
        FerruleInstance source,
        NamedInput[] supplied,
        FerruleTargetSelection? selection,
        FerruleOwnedSourceLimits limits,
        FerruleExecutionContext execution) => selection is null
        ? GeneratedMapping.ExecuteWithBasicOwnedSources(source, supplied, limits, execution)
        : GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(source, selection, supplied, limits, execution);

    private static Actual Reach(
        JsonElement call,
        JsonElement model,
        JsonElement oracle,
        FerruleOwnedSourceLimits limits,
        FerruleExecutionContext execution,
        JsonElement executionSpec)
    {
        var id = "WINDOW-csharp-" + Text(call.GetProperty("case"));
        var source = ReadInstance(call.GetProperty("primary"));
        var supplied = call.GetProperty("supplied").EnumerateArray().Select(input =>
            new NamedInput(Text(input.GetProperty("name")), ReadInstance(input.GetProperty("instance")))).ToArray();
        var selection = ReadSelection(call.GetProperty("selection"));
        var input = Element(InputSnapshot(source, supplied));
        Console.WriteLine(Json(new
        {
            id,
            classification = "before_subject",
            backend = "csharp",
            model = ProgramName,
            @case = Text(call.GetProperty("case")),
            complete_model_spec = model,
            complete_call_spec = call,
            actual_decoded_input = input,
            selection_spec = call.GetProperty("selection"),
            actual_selected_target = selection is null ? null : new
            {
                is_primary = selection.IsPrimary,
                name = selection.Name,
            },
            actual_generated_limits = new
            {
                max_roots = (ulong)limits.MaxRoots,
                max_active_frames = (ulong)limits.MaxActiveFrames,
                max_path_segments = (ulong)limits.MaxPathSegments,
                max_metadata_bytes = (ulong)limits.MaxMetadataBytes,
                max_walk_contexts = (ulong)limits.MaxWalkContexts,
            },
            execution_spec = executionSpec,
            actual_execution = new
            {
                mapping_path = execution.MappingFilePath,
                main_mapping_path = execution.MainMappingFilePath,
                current_date_time = execution.CurrentDateTime,
                runtime_parameter_count = execution.RuntimeParameters.Count,
                filter_map_cancellation = execution.FilterMapCancellation is not null,
            },
            frozen_oracle = oracle,
        }));
        object? result = null;
        Exception? error = null;
        try { result = Invoke(source, supplied, selection, limits, execution); }
        catch (Exception reached) { error = reached; }
        var publicResult = Element(ResultSnapshot(result));
        var completeError = Element(error is null ? null : ErrorSnapshot(error));
        var semantic = error is null ? publicResult : Element(SemanticErrorSnapshot(error));
        var typedError = Element(TypedErrorSnapshot(error));
        Console.WriteLine(Json(new
        {
            id,
            classification = "before_comparison",
            backend = "csharp",
            model = ProgramName,
            @case = Text(call.GetProperty("case")),
            complete_call_spec = call,
            selection_spec = call.GetProperty("selection"),
            result_type = result?.GetType().AssemblyQualifiedName,
            result = publicResult,
            error = completeError,
            semantic_snapshot = semantic,
            typed_error = typedError,
        }));
        var record=new {id,backend="csharp",classification="raw_return_before_any_classification",snapshot=semantic,typed_error=typedError,complete_error=completeError,complete_public_return=publicResult,decoded_input=input};
        var captureDirectory=Environment.GetEnvironmentVariable("FERRULE_CONFORMANCE_CAPTURE_DIR");
        if(captureDirectory is not null) {var body=JsonSerializer.SerializeToUtf8Bytes(record);using var stream=new FileStream(Path.Combine(captureDirectory,id+".capture.json"),FileMode.CreateNew,FileAccess.Write,FileShare.None);stream.Write(body);stream.Flush(true);}
        return new Actual(id, call.Clone(), source, supplied, input, result, error,
            publicResult, completeError, semantic, typedError);
    }

    public static void Main(string[] args)
    {
        if (args.Length != 2)
            throw new ArgumentException("Provide readonly programs.json and expected.json paths.", nameof(args));
        using var programsDocument = FrozenDocument(args[0], ProgramsSha256);
        using var expectedDocument = FrozenDocument(args[1], ExpectedSha256);
        var programs = programsDocument.RootElement;
        var expected = expectedDocument.RootElement;
        if (!ModelCases.TryGetValue(ProgramName, out var expectedCases))
            throw new InvalidOperationException("Unknown generated model label: " + ProgramName);
        var models = programs.GetProperty("models").EnumerateArray().ToArray();
        var model = models.Single(row => StringComparer.Ordinal.Equals(Text(row.GetProperty("id")), ProgramName));
        var calls = programs.GetProperty("calls").EnumerateArray()
            .Where(row => StringComparer.Ordinal.Equals(Text(row.GetProperty("model")), ProgramName)).ToArray();
        var oracles = expected.GetProperty("calls").EnumerateArray()
            .Where(row => StringComparer.Ordinal.Equals(Text(row.GetProperty("backend")), "csharp") &&
                StringComparer.Ordinal.Equals(Text(row.GetProperty("model")), ProgramName))
            .ToDictionary(row => Text(row.GetProperty("id")), row => row.Clone(), StringComparer.Ordinal);
        var limits = ReadLimits(programs.GetProperty("generated_limits"));
        var executionSpec = programs.GetProperty("execution");
        var execution = new FerruleExecutionContext(Text(executionSpec.GetProperty("mapping_path")));
        var actuals = new List<Actual>(calls.Length);
        foreach (var call in calls)
        {
            var id = "WINDOW-csharp-" + Text(call.GetProperty("case"));
            actuals.Add(Reach(call, model, oracles[id], limits, execution, executionSpec));
        }
        // All calls assigned to this model have complete raw inputs and returns before assertions.
        Check(models.Length == 10 && programs.GetProperty("calls").GetArrayLength() == 12 &&
            expected.GetProperty("calls").GetArrayLength() == 36, "Exact frozen global inventory.");
        Check(actuals.Count == expectedCases.Length && oracles.Count == expectedCases.Length,
            "Exact reached model inventory: " + ProgramName);
        Check(actuals.Select(actual => Text(actual.Call.GetProperty("case"))).SequenceEqual(expectedCases,
            StringComparer.Ordinal), "Every frozen model call reached in original order: " + ProgramName);
        Check(actuals.Select(actual => actual.Id).Distinct(StringComparer.Ordinal).Count() == actuals.Count,
            "No repeated public subject calls: " + ProgramName);
        Console.WriteLine(Json(new {classification="raw_CSharp_model_complete",model=ProgramName,calls=actuals.Count,expected_results_compared=false,full_window_parity=false,full_error_parity=false,total_parity=false}));
    }
}
