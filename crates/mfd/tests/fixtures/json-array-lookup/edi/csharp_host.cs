using System.Text;
using System.Text.Json.Nodes;
using Ferrule.Generated;
using Ferrule.Runtime;

var cases = JsonNode.Parse(File.ReadAllText(args[0]))!.AsArray();
var target = File.ReadAllText(args[2]);
foreach (var item in cases)
{
    var test = item!;
    var context = FerruleExecutionContext.WithParameters("mapping.mfd", new FerruleRuntimeParameters(
        new Dictionary<string, FerruleValue> { ["LookupText"] = FerruleValue.FromString(test["lookup"]!.GetValue<string>()) }));
    var wire = test["input"]!.GetValue<string>();
    foreach (var primary in new[] { GeneratedMapping.ParseX12(wire), GeneratedMapping.ParseX12Bytes(Encoding.UTF8.GetBytes(wire)) })
    {
        try
        {
            var actual = GeneratedMapping.Execute(primary, context);
            if (test["error"] is not null) throw new InvalidOperationException("Expected typed parse failure: " + test);
            LookupOracle.Equal(actual, FerruleJson.ParseEmbedded(target, test["expected"]!.ToJsonString()));
            var json = FerruleJson.SerializeEmbedded(target, actual);
            if (!JsonNode.DeepEquals(JsonNode.Parse(json), test["expected"]))
                throw new InvalidOperationException("Output mismatch: " + json + " expected " + test["expected"]);
        }
        catch (FerruleRuntimeException error) when (test["error"] is not null)
        {
            if (error.Error != FerruleRuntimeError.FunctionInvalidArgument || error.Function != "json_parse_array_length"
                || error.Detail != test["error"]!.GetValue<string>()) throw;
        }
    }
}
Console.WriteLine("C# nested-EDI parsed-array lookup host passed");
