using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static object ProjectionView(FerruleInstance value) => value switch
    {
        FerruleScalar { Value.Kind: FerruleValueKind.String } scalar => scalar.Value.StringValue,
        FerruleScalar { Value.Kind: FerruleValueKind.Bool } scalar => scalar.Value.BooleanValue,
        FerruleRepeated repeated => repeated.Items.Select(ProjectionView).ToArray(),
        FerruleGroup group => group.Fields.ToDictionary(f => f.Name, f => ProjectionView(f.Value)),
        _ => throw new Exception("Unexpected projection value"),
    };

    private static void JsonSourceProjection()
    {
        var firstNonempty = FerruleAggregateOperation.FirstNonemptyText;
        Equal(FerruleValue.Null, FerruleAggregates.Apply(firstNonempty, Array.Empty<FerruleValue>()));
        Equal(FerruleValue.FromString(" "), FerruleAggregates.Apply(firstNonempty,
            new[] { FerruleValue.Null, FerruleValue.FromString(""), FerruleValue.FromString(" "), FerruleValue.FromString("later") }));
        Throws<FerruleRuntimeException>(() => FerruleAggregates.Apply(firstNonempty,
            new[] { FerruleValue.FromString("first"), FerruleValue.FromInt64(2) }));
        using var corpus = JsonDocument.Parse(File.ReadAllBytes(Path.Combine(AppContext.BaseDirectory, "json-source-projection.json")));
        var shape = FerruleJsonSourceProjection.Parse(corpus.RootElement.GetProperty("shape").GetRawText());
        foreach (var item in corpus.RootElement.GetProperty("cases").EnumerateArray())
        {
            var document = FerruleJsonSourceDocument.Parse("doc.json", Encoding.UTF8.GetBytes(item.GetProperty("input").GetString()!));
            var projected = shape.ProjectDocument(document);
            Equal(true, JsonNode.DeepEquals(JsonNode.Parse(item.GetProperty("expected").GetRawText()),
                JsonSerializer.SerializeToNode(ProjectionView(projected))));
        }
        var number = FerruleJsonSourceProjection.Parse("""{"kind":"number"}""");
        var huge = FerruleJsonSourceDocument.Parse("unchanged", "1e999"u8);
        _ = number.ProjectDocument(huge, new() { Values = 5, NameBytes = 13 });
        ProjectionRefuses(FerruleJsonProjectionError.Values, () => number.ProjectDocument(huge, new() { Values = 4, NameBytes = 13 }));
        ProjectionRefuses(FerruleJsonProjectionError.Names, () => number.ProjectDocument(huge, new() { Values = 5, NameBytes = 12 }));
        ProjectionRefuses(FerruleJsonProjectionError.InvalidLimits, () => number.ProjectDocument(huge, new() { Values = 1_000_001 }));
        Equal("1e999", huge.Root.NumberText);
        var parameterProjection = number.ProjectValue(huge.Root, new() { Values = 2, NameBytes = 5 });
        Equal(true, parameterProjection is FerruleGroup { Fields.Count: 1 });
        Equal("Value", ((FerruleGroup)parameterProjection!).Fields[0].Name);
        Equal("1e999", ((FerruleScalar)((FerruleGroup)parameterProjection).Fields[0].Value).Value.StringValue);
        ProjectionRefuses(FerruleJsonProjectionError.Values, () => number.ProjectValue(huge.Root, new() { Values = 1 }));
        Equal(true, number.ProjectValue(FerruleJsonSourceDocument.Parse("parameter-only", "{}"u8).Root) is null);
        foreach (var descriptor in new[] {
            """{"kind":"number","coerce":true}""",
            """{"kind":"number","kind":"string"}""",
            """{"kind":"object","properties":[{"name":"a","shape":{"kind":"string"}},{"name":"a","shape":{"kind":"number"}}]}""",
            """{"kind":"object","properties":[{"name":"","shape":{"kind":"string"}}]}""",
            string.Concat(Enumerable.Repeat("{\"kind\":\"array\",\"item\":", 25)) + "{\"kind\":\"string\"}" + new string('}', 25),
        }) ProjectionRefuses(FerruleJsonProjectionError.Plan, () => FerruleJsonSourceProjection.Parse(descriptor));
        var calls = new List<string>();
        IEnumerable<string> Requests(string path)
        {
            Equal("input/*.json", path);
            calls.Add("select");
            try { yield return "a"; throw new InvalidOperationException("request"); }
            finally { calls.Add("dispose"); }
        }
        var sequence = number.Acquire(
            () => { calls.Add("filename"); return "input/*.json"; },
            Requests,
            path => { Equal("a", path); calls.Add("load"); return FerruleJsonSourceDocument.Parse("actual/a.json", "1e999"u8); });
        Equal(0, calls.Count);
        using var source = sequence.GetEnumerator();
        Equal(0, calls.Count);
        Equal(true, source.MoveNext());
        var retained = source.Current;
        Equal("filename,select,load", string.Join(',', calls));
        Throws<InvalidOperationException>(() => source.MoveNext());
        Equal(false, source.MoveNext());
        Equal("filename,select,load,dispose", string.Join(',', calls));
        Equal("1e999", ((FerruleScalar)((FerruleGroup)((FerruleRepeated)((FerruleGroup)retained).Fields[1].Value).Items[0]).Fields[0].Value).Value.StringValue);
        using var zero = number.Acquire(() => "zero", _ => new[] { "zero" },
            _ => throw new Exception("limit must precede load"), maximum: 0).GetEnumerator();
        Throws<FerruleJsonSourceLoadException>(() => zero.MoveNext());
        Equal(false, zero.MoveNext());
        var requestCalls = new List<string>();
        IEnumerable<string> OrderedRequests()
        {
            try
            {
                requestCalls.Add("request"); yield return "same";
                requestCalls.Add("request"); yield return "same";
                requestCalls.Add("request"); throw new InvalidOperationException("request");
            }
            finally { requestCalls.Add("dispose"); }
        }
        var ordered = number.AcquireRequests(OrderedRequests(), path =>
        {
            Equal("same", path); requestCalls.Add("load");
            return FerruleJsonSourceDocument.Parse(path, "1e999"u8);
        });
        Equal(0, requestCalls.Count);
        using var orderedIterator = ordered.GetEnumerator();
        Equal(true, orderedIterator.MoveNext());
        Equal(true, orderedIterator.MoveNext());
        Throws<InvalidOperationException>(() => orderedIterator.MoveNext());
        Equal(false, orderedIterator.MoveNext());
        Equal("request,load,request,load,request,dispose", string.Join(',', requestCalls));
        int orderedLoads = 0;
        using var limited = number.AcquireRequests(new[] { "same", "same", "later" }, path =>
        {
            orderedLoads++; return FerruleJsonSourceDocument.Parse(path, "1"u8);
        }, maximum: 2).GetEnumerator();
        Equal(true, limited.MoveNext()); Equal(true, limited.MoveNext());
        Throws<FerruleJsonSourceLoadException>(() => limited.MoveNext());
        Equal(false, limited.MoveNext()); Equal(2, orderedLoads);
    }

    private static void ProjectionRefuses(FerruleJsonProjectionError expected, Action action)
    {
        try { action(); }
        catch (FerruleJsonProjectionException error) { Equal(expected, error.Error); return; }
        throw new Exception($"Expected {expected} projection failure");
    }
}
