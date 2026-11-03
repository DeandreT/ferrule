using System.Text.Json.Nodes;
using Ferrule.Generated;
using Ferrule.Runtime;

var root = args[0];
var cases = JsonNode.Parse(File.ReadAllText(Path.Combine(root, "cases.json")))!.AsArray();
var sourceSchema = File.ReadAllText(Path.Combine(root, "source.json"));
var targetSchema = File.ReadAllText(Path.Combine(root, "target.json"));
var parser = File.ReadAllText(Path.Combine(root, "parser.json"));
var counterFailures = new List<string>();
void Count(string profile, int rows, int expected)
{
    var actual = FerruleFunctions.JsonProjectionFullParseCount;
    Console.WriteLine($"{profile} rows={rows} DOM-attempts={actual} expected={expected}");
    if (actual != expected) counterFailures.Add($"{profile}/{rows}: {actual} != {expected}");
}

// Actual generated owned sorted GroupBy: root demand, retained sorted
// candidate selection contexts, and finished grouped target all use owner 0.
foreach (var item in cases)
{
    var test = item!;
    var source = FerruleJson.Parse(sourceSchema, test["source"]!.ToJsonString());
    var execution = FerruleExecutionContext.WithParameters("synthetic.mfd", new FerruleRuntimeParameters(
        new Dictionary<string, FerruleValue> { ["InputText"] = FerruleValue.FromString(test["input"]!.GetValue<string>()) }));
    for (var repeat = 0; repeat < 2; repeat++)
    {
        FerruleFunctions.ResetJsonProjectionFullParseCount();
        try
        {
            var actual = GeneratedMapping.ExecuteWithBasicOwnedSources(source, Array.Empty<NamedInput>(),
                new FerruleOwnedSourceLimits(1, 16, 32, 65536, 128), execution).Primary;
            if (test["error"] is not null) throw new InvalidOperationException("Expected generated grouping error");
            var json = FerruleJson.SerializeEmbedded(targetSchema, actual);
            if (!JsonNode.DeepEquals(JsonNode.Parse(json), test["expected"]))
                throw new InvalidOperationException("Complete generated grouping output mismatch: " + json);
        }
        catch (FerruleRuntimeException error) when (test["error"] is not null)
        {
            if (error.Error != FerruleRuntimeError.FunctionInvalidArgument || error.Function != "json_parse_field" ||
                error.Detail != "input does not match the JSON schema") throw;
        }
        Count("generated-sorted-grouping", test["rows"]!.GetValue<int>(), 1);
    }
}

// Direct-runtime filter/map control, NOT generated-graph filter/map proof.
// Each outer source context visits the real upper-bound/capture/stage boundary
// code; their main-graph projection callbacks have the same proven owner 0.
var input = "{\"Title\":\"team\",\"Limit\":3,\"Rows\":[{}, {\"F0\":null}]}";
FerruleValue Read(ScopeContext context, string text, string path) => context.CallJsonArrayProjection(0,
    "json_parse_field", new[] { FerruleValue.FromString(text), FerruleValue.FromString(parser), FerruleValue.FromString("[\"" + path + "\"]") });
IReadOnlyList<FerruleValue> Map(ScopeContext context, string text, string secondPath = "Title") => FerruleFilterMap.Evaluate(
    context, 50, null, new FerruleFilterMapInput(8, c => Read(c, text, "Limit")),
    new[] { new FerruleFilterMapCapture(9, FerruleScalarType.String, c => Read(c, text, "Title")),
        new FerruleFilterMapCapture(10, FerruleScalarType.String, c => Read(c, text, secondPath)) },
    new FerruleFilterMapStage(70, 1, (_, _) => FerruleValue.FromBoolean(true)),
    new FerruleFilterMapStage(71, 2, (_, values) => FerruleValue.FromString(values[2].StringValue + ":" + values[3].StringValue + ":" + values[0].Int64Value)),
    FerruleScalarType.String);
foreach (var rows in new[] { 0, 1, 4, 13 })
{
    var source = new FerruleGroup(new[] { new FerruleField("Rows", new FerruleRepeated(
        Enumerable.Range(0, rows).Select(_ => (FerruleInstance)new FerruleGroup(Array.Empty<FerruleField>())))) });
    var context = ScopeContext.FromSource(source);
    FerruleFunctions.ResetJsonProjectionFullParseCount();
    var kept = Read(context, input, "Title");
    var complete = new List<string[]>();
    foreach (var candidate in context.IterateSource("Rows")) complete.Add(Map(candidate, input).Select(value => value.StringValue).ToArray());
    if (complete.Count != rows || complete.Any(values => !values.SequenceEqual(new[] { "team:team:1", "team:team:2", "team:team:3" })))
        throw new InvalidOperationException("Complete direct filter/map output mismatch");
    Count("direct-runtime-filter-map", rows, 1);
    Read(context, "{\"Title\":\"replacement\",\"Limit\":1}", "Title");
    if (!kept.Equals(FerruleValue.FromString("team")) || complete.Any(values => values[0] != "team:team:1"))
        throw new InvalidOperationException("Replacement invalidated owned captures/results");
}
foreach (var capture in new[] { false, true })
{
    var context = ScopeContext.FromSource(new FerruleGroup(Array.Empty<FerruleField>()));
    FerruleFunctions.ResetJsonProjectionFullParseCount();
    var kept = Read(context, input, "Title");
    try
    {
        Map(context, capture ? input : "{\"Title\":\"bad\",\"Limit\":3,\"Rows\":[{\"F0\":42}]}", capture ? "Missing" : "Title");
        throw new InvalidOperationException("Expected direct filter/map typed failure");
    }
    catch (FerruleFilterMapException error)
    {
        var expectedPhase = capture ? FerruleFilterMapPhase.Capture : FerruleFilterMapPhase.Source;
        var expectedNode = capture ? 10U : 8U;
        var expectedDetail = capture ? "field path does not resolve to a scalar" : "input does not match the JSON schema";
        if (error.Boundary.Phase != expectedPhase || error.Boundary.Node != expectedNode ||
            error.Boundary.CaptureIndex != (capture ? 1 : (int?)null) ||
            error.Cause is not FerruleRuntimeException cause || cause.Error != FerruleRuntimeError.FunctionInvalidArgument ||
            cause.Function != "json_parse_field" || cause.Detail != expectedDetail) throw;
    }
    if (!kept.Equals(FerruleValue.FromString("team"))) throw new InvalidOperationException("Failure invalidated prior owned projection");
}
if (counterFailures.Count != 0) throw new InvalidOperationException(string.Join("; ", counterFailures));
Console.WriteLine("C# Release generated grouping + direct-runtime filter/map context controls passed");
