using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text.Json;
using Ferrule.Generated;
using Ferrule.Runtime;

namespace Ferrule.Conformance.BasicOwnedNestedAlias;

internal static class Program
{
    private const string ProgramName = "__PROGRAM__";
    private const string FixtureName = "basic-owned-nested-alias-v1";
    private const string ProgramsSha256 = "__PROGRAMS_SHA256__";
    private const string ExpectedSha256 = "__EXPECTED_SHA256__";
    private static readonly Dictionary<string, string[]> ModelCases = new(StringComparer.Ordinal)
    {
        ["outer_suffix"] = new[] { "C01" },
        ["nested_rows"] = new[] { "C02" },
        ["empty_root"] = new[] { "C03" },
        ["same_owner"] = new[] { "C04" },
        ["singleton"] = new[] { "C05" },
        ["raw_compact"] = new[] { "C06" },
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
        type = value.Kind == FerruleValueKind.Int64 ? "Int" : value.Kind.ToString(),
        value = value.Kind switch
        {
            FerruleValueKind.String => (object?)value.StringValue,
            FerruleValueKind.Int64 => value.Int64Value,
            FerruleValueKind.Bool => value.BooleanValue,
            FerruleValueKind.Double => BitConverter.DoubleToInt64Bits(value.DoubleValue),
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

    private static bool SameJson(JsonElement actual, JsonElement expected)
    {
        if (actual.ValueKind != expected.ValueKind) return false;
        switch (actual.ValueKind)
        {
            case JsonValueKind.Object:
                var actualProperties = actual.EnumerateObject().ToArray();
                var expectedProperties = expected.EnumerateObject().ToArray();
                if (actualProperties.Length != expectedProperties.Length) return false;
                return actualProperties.All(property => expected.TryGetProperty(property.Name, out var value) && SameJson(property.Value, value));
            case JsonValueKind.Array:
                var actualItems = actual.EnumerateArray().ToArray();
                var expectedItems = expected.EnumerateArray().ToArray();
                return actualItems.Length == expectedItems.Length && actualItems.Zip(expectedItems).All(pair => SameJson(pair.First, pair.Second));
            case JsonValueKind.String:
                return StringComparer.Ordinal.Equals(actual.GetString(), expected.GetString());
            case JsonValueKind.Number:
                return StringComparer.Ordinal.Equals(actual.GetRawText(), expected.GetRawText());
            default:
                return actual.GetRawText() == expected.GetRawText();
        }
    }
    private static JsonElement Element(object? value) => JsonSerializer.SerializeToElement(value);
    private static object SemanticErrorSnapshot(Exception error)
    {
        if (error is not FerruleRuntimeException runtime)
            return new { kind = "Error", category = error.GetType().FullName, display = error.Message };
        return runtime.Error switch
        {
            FerruleRuntimeError.NotABool => (object)new
            {
                kind = "Error",
                category = runtime.Error.ToString(),
                node = runtime.Node,
                found = runtime.FoundKind?.ToString(),
            },
            FerruleRuntimeError.MappingException => new
            {
                kind = "Error",
                category = runtime.Error.ToString(),
                node = runtime.Node,
                mapping_exception_message = runtime.MappingExceptionMessage,
            },
            FerruleRuntimeError.MappingFailure => new
            {
                kind = "Error",
                category = runtime.Error.ToString(),
                rule = runtime.FailureRule,
                mapping_failure_message = runtime.MappingFailureMessage,
            },
            _ => new
            {
                kind = "Error",
                category = runtime.Error.ToString(),
                node = runtime.Node,
                display = runtime.Message,
                full_typed_error = ErrorSnapshot(runtime),
            },
        };
    }

    private static int SourceChainLength(Exception error)
    {
        var count = 0;
        for (var inner = error.InnerException; inner is not null; inner = inner.InnerException)
            count = checked(count + 1);
        return count;
    }

    private static object? TypedErrorSnapshot(Exception? error) => error is FerruleRuntimeException runtime
        ? new
        {
            variant = runtime.Error.ToString(),
            node = runtime.Node,
            rule = runtime.FailureRule,
            found = runtime.FoundKind?.ToString(),
            mapping_exception_message = runtime.MappingExceptionMessage,
            mapping_failure_message = runtime.MappingFailureMessage,
            display = runtime.Message,
            source_chain_length = SourceChainLength(runtime),
            typed_inner_null = runtime.InnerException is null,
        }
        : null;

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
        var id = "NEST-csharp-" + Text(call.GetProperty("case"));
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
        return new Actual(id, call.Clone(), source, supplied, input, result, error,
            publicResult, completeError, semantic, typedError);
    }

    private static void ExactTypedError(Actual actual, JsonElement expected)
    {
        Check(actual.Result is null && actual.Error?.GetType() == typeof(FerruleRuntimeException),
            "Exact unsuccessful public return/type: " + actual.Id);
        var runtime = (FerruleRuntimeException)actual.Error!;
        Check(ReferenceEquals(runtime, actual.Error), "Original captured error identity: " + actual.Id);
        Check(SameJson(actual.TypedError, expected), "Complete static typed error oracle: " + actual.Id);
        Check(SameJson(Element(TypedErrorSnapshot(runtime)), actual.TypedError),
            "Typed error fields unchanged after raw capture: " + actual.Id);
        Check(SameJson(Element(ErrorSnapshot(runtime)), actual.CompleteError),
            "Complete CLR error unchanged after raw capture: " + actual.Id);
        Check(runtime.HResult == new Exception().HResult && runtime.Data.Count == 0 &&
            runtime.InnerException is null && runtime.HelpLink is null,
            "Complete ordinary base exception payload: " + actual.Id);
        Check(runtime.Error is FerruleRuntimeError.NotABool or FerruleRuntimeError.MappingException or
            FerruleRuntimeError.MappingFailure, "Only the frozen typed failure categories: " + actual.Id);
        Check(runtime.Function is null && runtime.ExpectedArity is null && runtime.ActualArity is null &&
            runtime.AggregateOperation is null && runtime.Detail is null && runtime.RequestedItems is null &&
            runtime.MaximumItems is null && runtime.MaximumDepth is null && runtime.RuntimeValue is null &&
            runtime.Join is null && runtime.UserFunction is null && runtime.FunctionParameter is null &&
            runtime.ExpectedScalarType is null && runtime.RuntimeParameter is null && runtime.SourceField is null &&
            runtime.FoundInstance is null && runtime.PrimaryRoot is null && runtime.OwnedSourceFailure is null &&
            runtime.BasicSourceBridgeFailure is null,
            "Every unrelated typed error payload remains null: " + actual.Id);
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
            var id = "NEST-csharp-" + Text(call.GetProperty("case"));
            actuals.Add(Reach(call, model, oracles[id], limits, execution, executionSpec));
        }
        // All calls assigned to this model have complete raw inputs and returns before assertions.
        Check(Text(programs.GetProperty("fixture")) == FixtureName &&
            Text(expected.GetProperty("fixture")) == FixtureName, "Exact frozen protocol fixture.");
        Check(models.Length == 6 && programs.GetProperty("calls").GetArrayLength() == 6 &&
            expected.GetProperty("calls").GetArrayLength() == 18, "Exact frozen global inventory.");
        var counts = programs.GetProperty("exact_counts");
        Check(counts.GetProperty("models").GetInt32() == 6 &&
            counts.GetProperty("calls_per_backend").GetInt32() == 6 &&
            counts.GetProperty("total_calls").GetInt32() == 18, "Exact frozen declared counts.");
        Check(models.Select(row => Text(row.GetProperty("id"))).OrderBy(id => id, StringComparer.Ordinal)
            .SequenceEqual(ModelCases.Keys.OrderBy(id => id, StringComparer.Ordinal), StringComparer.Ordinal),
            "Every frozen model has its exact nested alias label.");
        var frozenCalls = programs.GetProperty("calls").EnumerateArray().ToArray();
        Check(frozenCalls.All(call => ModelCases.TryGetValue(Text(call.GetProperty("model")), out var cases) &&
            cases.Contains(Text(call.GetProperty("case")), StringComparer.Ordinal)) &&
            frozenCalls.Select(call => Text(call.GetProperty("case"))).Distinct(StringComparer.Ordinal).Count() == 6,
            "Every frozen portable call has its exact nested alias model and case.");
        var frozenOracles = expected.GetProperty("calls").EnumerateArray().ToArray();
        Check(frozenOracles.All(oracle => oracle.GetProperty("typed_error").ValueKind == JsonValueKind.Null),
            "Every nested alias oracle requires a genuine complete success; no error credit.");
        foreach (var backend in new[] { "native", "rust", "csharp" })
        {
            var backendOracles = frozenOracles.Where(oracle =>
                StringComparer.Ordinal.Equals(Text(oracle.GetProperty("backend")), backend)).ToArray();
            Check(backendOracles.Length == 6 &&
                backendOracles.Select(oracle => Text(oracle.GetProperty("id"))).Distinct(StringComparer.Ordinal).Count() == 6 &&
                backendOracles.All(oracle =>
                    Text(oracle.GetProperty("id")) == "NEST-" + backend + "-" + Text(oracle.GetProperty("case")) &&
                    ModelCases.TryGetValue(Text(oracle.GetProperty("model")), out var cases) &&
                    cases.Contains(Text(oracle.GetProperty("case")), StringComparer.Ordinal)),
                "Exact six frozen oracle identities for backend: " + backend);
        }
        Check(actuals.Count == expectedCases.Length && oracles.Count == expectedCases.Length,
            "Exact reached model inventory: " + ProgramName);
        Check(actuals.Select(actual => Text(actual.Call.GetProperty("case"))).SequenceEqual(expectedCases,
            StringComparer.Ordinal), "Every frozen model call reached in original order: " + ProgramName);
        Check(actuals.Select(actual => actual.Id).Distinct(StringComparer.Ordinal).Count() == actuals.Count,
            "No repeated public subject calls: " + ProgramName);
        if (StringComparer.Ordinal.Equals(
            Environment.GetEnvironmentVariable("FERRULE_CONFORMANCE_CAPTURE_ONLY"), "1"))
        {
            // Diagnostic mode retains the frozen protocol and complete inputs;
            // it performs no semantic or typed error outcome comparison.
            foreach (var actual in actuals)
            {
                var oracle = oracles[actual.Id];
                Check(Text(oracle.GetProperty("id")) == actual.Id &&
                    Text(oracle.GetProperty("model")) == ProgramName &&
                    Text(oracle.GetProperty("case")) == Text(actual.Call.GetProperty("case")) &&
                    Text(oracle.GetProperty("backend")) == "csharp" &&
                    SameJson(oracle.GetProperty("selection"), actual.Call.GetProperty("selection")),
                    "Exact frozen call identity and selection: " + actual.Id);
                Check(SameJson(actual.Input, Element(new
                {
                    primary = actual.Call.GetProperty("primary"),
                    supplied = actual.Call.GetProperty("supplied"),
                })), "Decoded public input preserves complete portable tree and order: " + actual.Id);
                Check(SameJson(actual.Input, Element(InputSnapshot(actual.Source, actual.Supplied))),
                    "Caller input remains unchanged by public execution: " + actual.Id);
                Console.WriteLine(actual.Id + " capturedreturns");
            }
            Console.WriteLine(Json(new
            {
                classification = "capture_only_complete",
                backend = "csharp",
                model = ProgramName,
                complete_calls = actuals.Count,
                successful_calls = actuals.Count(actual => actual.Error is null),
                typed_error_calls = actuals.Count(actual => actual.Error is not null),
                all_returns_retained_before_comparison = true,
                oracle_comparisons = 0,
                full_filter_parity = false,
                full_error_parity = false,
                total_parity = false,
            }));
            return;
        }
        foreach (var actual in actuals)
        {
            var oracle = oracles[actual.Id];
            Check(Text(oracle.GetProperty("id")) == actual.Id &&
                Text(oracle.GetProperty("model")) == ProgramName &&
                Text(oracle.GetProperty("case")) == Text(actual.Call.GetProperty("case")) &&
                Text(oracle.GetProperty("backend")) == "csharp" &&
                SameJson(oracle.GetProperty("selection"), actual.Call.GetProperty("selection")),
                "Exact frozen call identity and selection: " + actual.Id);
            Check(SameJson(actual.Input, Element(new
            {
                primary = actual.Call.GetProperty("primary"),
                supplied = actual.Call.GetProperty("supplied"),
            })), "Decoded public input preserves complete portable tree and order: " + actual.Id);
            Check(SameJson(actual.Input, Element(InputSnapshot(actual.Source, actual.Supplied))),
                "Caller input remains unchanged by public execution: " + actual.Id);
            Check(SameJson(actual.Semantic, oracle.GetProperty("expected")),
                "Exact complete public tree or semantic error: " + actual.Id);
            if (oracle.GetProperty("typed_error").ValueKind == JsonValueKind.Null)
            {
                Check(actual.Error is null && actual.Result is not null &&
                    actual.CompleteError.ValueKind == JsonValueKind.Null &&
                    actual.TypedError.ValueKind == JsonValueKind.Null,
                    "Genuine complete success required: " + actual.Id);
                Check(SameJson(Element(ResultSnapshot(actual.Result)), actual.PublicResult),
                    "Complete public result unchanged after capture: " + actual.Id);
            }
            else
            {
                ExactTypedError(actual, oracle.GetProperty("typed_error"));
            }
            Console.WriteLine(Json(new
            {
                id = actual.Id,
                classification = "matched_oracles",
                backend = "csharp",
                model = ProgramName,
                complete_value_or_typed_error_match = true,
                original_error_identity_retained = actual.Error is null ||
                    ReferenceEquals(actual.Error, (FerruleRuntimeException)actual.Error),
                full_filter_parity = false,
                full_error_parity = false,
                total_parity = false,
            }));
            Console.WriteLine(actual.Id + " matchedoracles");
        }
        Console.WriteLine(Json(new
        {
            classification = "probe_complete",
            backend = "csharp",
            model = ProgramName,
            complete_calls = actuals.Count,
            matched_oracle_calls = actuals.Count,
            successful_calls = actuals.Count(actual => actual.Error is null),
            typed_error_calls = actuals.Count(actual => actual.Error is not null),
            all_returns_retained_before_comparison = true,
            full_filter_parity = false,
            full_error_parity = false,
            total_parity = false,
        }));
    }
}
