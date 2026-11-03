using System.Text.Json.Nodes;
using Ferrule.Generated;
using Ferrule.Runtime;

var cases = JsonNode.Parse(File.ReadAllText(args[0]))!.AsArray();
var schema = File.ReadAllText(args[1]);
var parser = File.ReadAllText(args[3]);
var scope = ScopeContext.FromSource(new FerruleGroup(Array.Empty<FerruleField>()));
var arguments = new[] { FerruleValue.FromString("{\"Title\":\"first\"}"), FerruleValue.FromString(parser), FerruleValue.FromString("[\"Title\"]") };
FerruleFunctions.ResetJsonProjectionFullParseCount();
foreach (var title in new[] { "first", "other", "first" })
{
    arguments[0] = FerruleValue.FromString("{\"Title\":\"" + title + "\"}");
    if (!scope.CallJsonArrayProjection(0, "json_parse_field", arguments).Equals(FerruleValue.FromString(title)))
        throw new InvalidOperationException("Changed input aliased cached document");
}
if (FerruleFunctions.JsonProjectionFullParseCount != 3) throw new InvalidOperationException("Input replacement did not parse");
scope.CallJsonArrayProjection(1, "json_parse_field", arguments);
if (FerruleFunctions.JsonProjectionFullParseCount != 4) throw new InvalidOperationException("Independent owner merged equal text");
var changed = JsonNode.Parse(parser)!;
changed["kind"]!["children"]![0]!["kind"]!["ty"] = "int";
arguments[1] = FerruleValue.FromString(changed.ToJsonString());
try
{
    scope.CallJsonArrayProjection(0, "json_parse_field", arguments);
    throw new InvalidOperationException("Changed schema did not revalidate");
}
catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.FunctionInvalidArgument &&
    error.Function == "json_parse_field" && error.Detail == "input does not match the JSON schema") { }
if (FerruleFunctions.JsonProjectionFullParseCount != 5) throw new InvalidOperationException("Schema replacement did not parse");
arguments[1] = FerruleValue.FromString(parser);
var kept = scope.CallJsonArrayProjection(0, "json_parse_field", arguments);
arguments[0] = FerruleValue.FromString("{\"Title\":\"replacement\"}");
if (!scope.CallJsonArrayProjection(0, "json_parse_field", arguments).Equals(FerruleValue.FromString("replacement")) ||
    !kept.Equals(FerruleValue.FromString("first"))) throw new InvalidOperationException("Replacement invalidated an owned scalar");
// Even callers sharing one public context cannot race its document dictionary.
var shared = ScopeContext.FromSource(new FerruleGroup(Array.Empty<FerruleField>()));
var counts = new int[4];
var readers = Enumerable.Range(0, 4).Select(index => new Thread(() =>
{
    FerruleFunctions.ResetJsonProjectionFullParseCount();
    for (var repeat = 0; repeat < 40; repeat++)
        if (!shared.CallJsonArrayProjection(0, "json_parse_field", arguments).Equals(FerruleValue.FromString("replacement")))
            throw new InvalidOperationException("Shared context changed its projection");
    counts[index] = FerruleFunctions.JsonProjectionFullParseCount;
})).ToArray();
foreach (var reader in readers) reader.Start();
foreach (var reader in readers) reader.Join();
if (counts.Sum() != 1) throw new InvalidOperationException("Shared context duplicated document parses");
void Run()
{
    foreach (var item in cases)
    {
        var test = item!;
        var context = FerruleExecutionContext.WithParameters("synthetic.mfd", new FerruleRuntimeParameters(
            new Dictionary<string, FerruleValue> { ["InputText"] = FerruleValue.FromString(test["input"]!.GetValue<string>()) }));
        for (var repeat = 0; repeat < 2; repeat++)
        {
            FerruleFunctions.ResetJsonProjectionFullParseCount();
            try
            {
                var actual = GeneratedMapping.Execute(new FerruleGroup(Array.Empty<FerruleField>()), context);
                if (test["error"] is not null) throw new InvalidOperationException("Expected typed failure: " + test);
                var json = FerruleJson.SerializeEmbedded(schema, actual);
                if (!JsonNode.DeepEquals(JsonNode.Parse(json), test["expected"]))
                    throw new InvalidOperationException("Complete output mismatch: " + json + " expected " + test["expected"]);
            }
            catch (FerruleRuntimeException error) when (test["error"] is not null)
            {
                if (error.Error != FerruleRuntimeError.FunctionInvalidArgument ||
                    error.Function is not ("json_parse_field" or "json_parse_array_length") ||
                    error.Detail != "input does not match the JSON schema") throw;
            }
            var expectedParses = test["parses"]?.GetValue<int>() ?? 1;
            if (FerruleFunctions.JsonProjectionFullParseCount != expectedParses)
                throw new InvalidOperationException("Expected " + expectedParses + " full parses, got " + FerruleFunctions.JsonProjectionFullParseCount);
        }
        Console.WriteLine("C# Release rows=" + test["rows"] + " full_parses=" + (test["parses"]?.GetValue<int>() ?? 1));
    }
}
Run();
var threads = Enumerable.Range(0, 4).Select(_ => new Thread(Run)).ToArray();
foreach (var thread in threads) thread.Start();
foreach (var thread in threads) thread.Join();
Console.WriteLine("C# Release fields=" + args[2] + " complete output/error and invocation parse-count checks passed");
