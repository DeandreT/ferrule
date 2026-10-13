using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Json;
using Ferrule.Generated;
using Ferrule.Runtime;

namespace Ferrule.Conformance.BasicOwnedOccurrence;

internal static class Program
{
    private const string ProgramName = "__PROGRAM__";
    private static readonly string[] InputNames = { "mixed", "empty", "singleton" };
    private static readonly string[] SelectionNames = { "all", "primary" };
    private sealed record Actual(string Id, string Input, string Selection, object? Result, Exception? Error);

    private static string Json(object? value) => JsonSerializer.Serialize(value);
    private static void Check(bool value, string message)
    {
        if (!value) throw new InvalidOperationException(message);
    }

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

    private static object SemanticErrorSnapshot(Exception error)
    {
        var cause = (error as FerruleRuntimeException)?.OwnedSourceFailure;
        return new
        {
            kind = "Error",
            semantic_kind = cause?.Error.ToString() ?? error.GetType().FullName,
            owner = cause?.Owner is null ? null : new { is_primary = cause.Owner.IsPrimary, name = cause.Owner.Name },
            collection = cause?.Collection?.ToArray(),
        };
    }

    private static object ErrorSnapshot(Exception error)
    {
        var runtime = error as FerruleRuntimeException;
        var owned = error as FerruleOwnedSourceException;
        var bridge = error as FerruleBasicSourceBridgeException;
        var traversal = error as FerruleOwnedSourceTraversalException;
        return new
        {
            type = error.GetType().FullName,
            message = error.Message,
            hresult = error.HResult,
            source = error.Source,
            stack_trace = error.StackTrace,
            target_site = error.TargetSite?.ToString(),
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

    private static FerruleInstance Text(string value) => new FerruleScalar(FerruleValue.FromString(value));
    private static FerruleInstance Integer(long value) => new FerruleScalar(FerruleValue.FromInt64(value));
    private static FerruleInstance Group(params (string Name, FerruleInstance Value)[] fields) =>
        new FerruleGroup(fields.Select(field => new FerruleField(field.Name, field.Value)));
    private static FerruleInstance Endpoint(string input) => input switch
    {
        "mixed" => new FerruleRepeated(new FerruleInstance[]
        {
            new FerruleScalar(FerruleValue.Null),
            Group(("Value", Text("kept1"))),
            new FerruleScalar(FerruleValue.JsonNull),
            Group(("Value", Text("kept2"))),
        }),
        "empty" => new FerruleRepeated(Array.Empty<FerruleInstance>()),
        "singleton" => Group(("Value", Text("singleton"))),
        _ => throw new InvalidOperationException("unknown fixture input"),
    };
    private static FerruleInstance NamedPrimary() =>
        Group(("Value", Text("primary.shadow")), ("Ref", Group(("Value", Text("embedded.shadow")))));
    private static FerruleInstance ExpectedTree(string input) => input switch
    {
        "mixed" => new FerruleRepeated(new FerruleInstance[]
        {
            Group(("Value", Text("kept1")), ("Position", Integer(2))),
            Group(("Value", Text("kept2")), ("Position", Integer(4))),
        }),
        "empty" => new FerruleRepeated(Array.Empty<FerruleInstance>()),
        _ => throw new InvalidOperationException("no expected successful tree for this input"),
    };

    private static object FrozenExpected(string input, string selection, bool primaryOwner)
    {
        if (StringComparer.Ordinal.Equals(input, "singleton"))
            return new
            {
                kind = "Error",
                semantic_kind = "InactiveSourceFrame",
                owner = new { is_primary = primaryOwner, name = primaryOwner ? null : "Ref" },
                collection = Array.Empty<string>(),
            };
        var tree = InstanceSnapshot(ExpectedTree(input));
        return StringComparer.Ordinal.Equals(selection, "all")
            ? new { kind = "All", primary = tree, extras = Array.Empty<object>() }
            : (object)new { kind = "SelectedPrimary", instance = tree };
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
    private static string Id(string input, string selection) => $"OCCURRENCE-CSharp-{ProgramName}-{input}-{selection}";

    private static Dictionary<string, JsonElement> ReadExpectations(string path, bool primaryOwner)
    {
        using var document = JsonDocument.Parse(File.ReadAllBytes(path));
        var calls = document.RootElement.GetProperty("calls");
        Check(calls.ValueKind == JsonValueKind.Array && calls.GetArrayLength() == 36, "exact common 36-call oracle inventory");
        var rows = calls.EnumerateArray()
            .Where(row => row.GetProperty("lane").GetString() == "CSharp" && row.GetProperty("program").GetString() == ProgramName)
            .ToDictionary(row => row.GetProperty("id").GetString() ?? throw new InvalidOperationException("oracle ID is null"), row => row.Clone(), StringComparer.Ordinal);
        Check(rows.Count == 6, "exact six-call C# program inventory");
        foreach (var input in InputNames)
        foreach (var selection in SelectionNames)
        {
            var id = Id(input, selection);
            Check(rows.TryGetValue(id, out var row), "required frozen C# oracle ID: " + id);
            Check(row.GetProperty("input").GetString() == input && row.GetProperty("selection").GetString() == selection, "exact input/selection metadata: " + id);
            Check(SameJson(row.GetProperty("expected"), Element(FrozenExpected(input, selection, primaryOwner))), "supplied frozen full expectation differs: " + id);
            if (input == "singleton")
            {
                var provenance = row.GetProperty("expected_error_provenance");
                Check(provenance.GetProperty("node").ValueKind == JsonValueKind.Null && provenance.GetProperty("node_field_present").GetBoolean() && provenance.GetProperty("native_semantic_node").GetInt32() == 2, "frozen C# error provenance gap: " + id);
            }
        }
        return rows;
    }

    private static Actual Reach(string input, string selection, FerruleInstance source, NamedInput[] supplied, FerruleOwnedSourceLimits limits, JsonElement expected, Func<object> subject)
    {
        var id = Id(input, selection);
        var selected = selection == "all" ? null : FerruleTargetSelection.Primary();
        Console.WriteLine(Json(new
        {
            id,
            classification = "before_subject",
            lane = "CSharp",
            program = ProgramName,
            input,
            selection,
            primary = InstanceSnapshot(source),
            supplied = supplied.Select(named => new { name = named.Name, instance = InstanceSnapshot(named.Instance) }).ToArray(),
            selected_target = selected is null ? null : new { is_primary = selected.IsPrimary, name = selected.Name },
            execution = (object?)null,
            limits = new
            {
                roots = (ulong)limits.MaxRoots,
                frames = (ulong)limits.MaxActiveFrames,
                segments = (ulong)limits.MaxPathSegments,
                bytes = (ulong)limits.MaxMetadataBytes,
                contexts = (ulong)limits.MaxWalkContexts,
            },
            frozen_oracle = expected,
        }));
        object? result = null;
        Exception? error = null;
        try { result = subject(); }
        catch (Exception reached) { error = reached; }
        Console.WriteLine(Json(new
        {
            id,
            classification = "before_comparison",
            lane = "CSharp",
            program = ProgramName,
            input,
            selection,
            result = ResultSnapshot(result),
            error = error is null ? null : ErrorSnapshot(error),
        }));
        return new Actual(id, input, selection, result, error);
    }

    private static void ExactInactive(Actual actual, bool primaryOwner)
    {
        Check(actual.Result is null && actual.Error?.GetType() == typeof(FerruleRuntimeException), "exact unsuccessful outer return: " + actual.Id);
        var outer = (FerruleRuntimeException)actual.Error!;
        Check(outer.Error == FerruleRuntimeError.OwnedSource && outer.Message == "owned source access failed", "exact outer error kind/message: " + actual.Id);
        Check(outer.HResult == new Exception().HResult && outer.Data.Count == 0, "complete outer base exception metadata: " + actual.Id);
        Check(outer.Node is null && outer.Function is null && outer.ExpectedArity is null && outer.ActualArity is null && outer.FoundKind is null && outer.AggregateOperation is null && outer.Detail is null && outer.RequestedItems is null && outer.MaximumItems is null && outer.MaximumDepth is null && outer.RuntimeValue is null && outer.FailureRule is null && outer.MappingFailureMessage is null && outer.MappingExceptionMessage is null && outer.Join is null && outer.UserFunction is null && outer.FunctionParameter is null && outer.ExpectedScalarType is null && outer.RuntimeParameter is null && outer.SourceField is null && outer.FoundInstance is null && outer.PrimaryRoot is null && outer.BasicSourceBridgeFailure is null, "complete unrelated outer metadata stays null: " + actual.Id);
        var cause = outer.OwnedSourceFailure;
        Check(cause is not null && cause.GetType() == typeof(FerruleOwnedSourceException) && ReferenceEquals(cause, outer.InnerException), "exact owned inner identity/type: " + actual.Id);
        var owned = cause!;
        Check(owned.Error == FerruleOwnedSourceErrorKind.InactiveSourceFrame && owned.Message == "Owned source operation failed.", "exact inactive cause kind/message: " + actual.Id);
        Check(owned.HResult == new Exception().HResult && owned.Data.Count == 0 && owned.InnerException is null, "complete inactive base cause metadata: " + actual.Id);
        Check(owned.Owner is not null && owned.Owner.IsPrimary == primaryOwner && (primaryOwner ? owned.Owner.Name is null : owned.Owner.Name == "Ref"), "exact inactive owner: " + actual.Id);
        Check(owned.Collection is not null && owned.Collection.Count == 0, "present-empty inactive collection: " + actual.Id);
        Check((primaryOwner ? owned.RequestedName is null : owned.RequestedName == "Ref") && owned.DeclarationIndices is null && owned.Path is null && owned.Frame is null && owned.Segment is null && owned.FoundInstance is null && owned.RequestedIndex is null && owned.LimitKind is null && owned.Used is null && owned.Requested is null && owned.Maximum is null, "complete unrelated owned metadata stays null: " + actual.Id);
    }

    public static void Main(string[] args)
    {
        Check(args.Length == 1, "provide the frozen expected.json path");
        var primaryOwner = StringComparer.Ordinal.Equals(ProgramName, "primary");
        Check(primaryOwner || StringComparer.Ordinal.Equals(ProgramName, "named"), "unrecognized program label");
        var expected = ReadExpectations(args[0], primaryOwner);
        var limits = new FerruleOwnedSourceLimits(16, 8, 16, 1048576, 64);
        var actuals = new List<Actual>(6);
        foreach (var input in InputNames)
        {
            var endpoint = Endpoint(input);
            var source = primaryOwner ? endpoint : NamedPrimary();
            var supplied = primaryOwner ? Array.Empty<NamedInput>() : new[] { new NamedInput("Ref", endpoint) };
            actuals.Add(Reach(input, "all", source, supplied, limits, expected[Id(input, "all")], () => GeneratedMapping.ExecuteWithBasicOwnedSources(source, supplied, limits, null)));
            actuals.Add(Reach(input, "primary", source, supplied, limits, expected[Id(input, "primary")], () => GeneratedMapping.ExecuteSelectedWithBasicOwnedSources(source, FerruleTargetSelection.Primary(), supplied, limits, null)));
        }
        // Every complete return and recursive error is retained before any comparison.
        Check(actuals.Count == 6 && actuals.Select(actual => actual.Id).Distinct(StringComparer.Ordinal).Count() == 6, "exact reached six-call inventory");
        foreach (var actual in actuals)
        {
            var shared = actual.Error is null ? ResultSnapshot(actual.Result) : SemanticErrorSnapshot(actual.Error);
            Check(SameJson(Element(shared), expected[actual.Id].GetProperty("expected")), "complete common return oracle differs: " + actual.Id);
            if (actual.Input == "singleton")
            {
                ExactInactive(actual, primaryOwner);
                Console.WriteLine(Json(new
                {
                    id = actual.Id,
                    classification = "error_parity_gap",
                    lane = "CSharp",
                    program = ProgramName,
                    per_backend_contract_success = true,
                    full_error_parity = false,
                    provenance_gap = true,
                    node_field_present = true,
                    actual_node = ((FerruleRuntimeException)actual.Error!).Node,
                    native_semantic_node = 2,
                    shared_error = shared,
                }));
            }
            else
            {
                Check(actual.Error is null && actual.Result is not null, "complete success required: " + actual.Id);
                Console.WriteLine(Json(new { id = actual.Id, classification = "value_contract_match", lane = "CSharp", program = ProgramName, per_backend_contract_success = true, shared_result = shared }));
            }
        }
        Console.WriteLine(Json(new
        {
            classification = "probe_complete",
            lane = "CSharp",
            program = ProgramName,
            complete_calls = 6,
            per_backend_contract_success_calls = 6,
            complete_value_contract_calls = 4,
            complete_inactive_error_contract_calls = 2,
            full_error_parity = false,
            provenance_gap = true,
            generated_parity_claim = false,
        }));
    }
}
