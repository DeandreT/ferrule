using System.Text.Json.Nodes;
using Ferrule.Generated;
using Ferrule.Runtime;

var cases = JsonNode.Parse(File.ReadAllText(args[0]))!.AsArray();
var schema = File.ReadAllText(args[1]);
foreach (var item in cases)
{
    var test = item!;
    var context = FerruleExecutionContext.WithParameters("mapping.mfd", new FerruleRuntimeParameters(
        new Dictionary<string, FerruleValue> { ["InputText"] = FerruleValue.FromString(test["input"]!.GetValue<string>()) }));
    try
    {
        var actual = GeneratedMapping.Execute(new FerruleGroup(Array.Empty<FerruleField>()), context);
        if (test["error"] is not null) throw new InvalidOperationException("Expected typed parse failure: " + test);
        var json = FerruleJson.SerializeEmbedded(schema, actual);
        if (!JsonNode.DeepEquals(JsonNode.Parse(json), test["expected"]))
            throw new InvalidOperationException("Output mismatch: " + json + " expected " + test["expected"]);
    }
    catch (FerruleRuntimeException error) when (test["error"] is not null)
    {
        if (error.Error != FerruleRuntimeError.FunctionInvalidArgument || error.Function != "json_parse_array_length"
            || error.Detail != test["error"]!.GetValue<string>()) throw;
    }
}
var parserSchema = File.ReadAllText(args[2]);
foreach (var index in new long[] { long.MinValue, -1, 0, 2, long.MaxValue })
{
    try
    {
        FerruleFunctions.Call("json_parse_array_field", new[] {
            FerruleValue.FromString("{\"Rows\":[{\"Qty\":2}]}"), FerruleValue.FromString(parserSchema),
            FerruleValue.FromString("[\"Rows\"]"), FerruleValue.FromString("[\"Qty\"]"), FerruleValue.FromInt64(index)
        });
        throw new InvalidOperationException("Expected out-of-range index failure");
    }
    catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.FunctionInvalidArgument
        && error.Function == "json_parse_array_field" && error.Detail == "array index is out of range") { }
}
foreach (var input in new[] { FerruleValue.Null, FerruleValue.JsonNull, FerruleValue.FromInt64(7) })
{
    try
    {
        FerruleFunctions.Call("json_parse_array_length", new[] {
            input, FerruleValue.FromString(parserSchema), FerruleValue.FromString("[\"Rows\"]")
        });
        throw new InvalidOperationException("Expected typed invalid-input failure");
    }
    catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.FunctionType
        && error.Function == "json_parse_array_length" && error.FoundKind == input.Kind) { }
}
Console.WriteLine("C# array-record host passed");
