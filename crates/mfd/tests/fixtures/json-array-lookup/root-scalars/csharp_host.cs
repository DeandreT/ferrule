using System.Text.Json.Nodes;
using Ferrule.Generated;
using Ferrule.Runtime;

var cases = JsonNode.Parse(File.ReadAllText(args[0]))!.AsArray();
var source = File.ReadAllText(args[1]);
var target = File.ReadAllText(args[2]);
foreach (var item in cases)
{
    var test = item!;
    var missing = test["parameter_failure"]?.GetValue<string>() == "missing";
    var parameters = new Dictionary<string, FerruleValue>();
    if (!missing) parameters["LookupText"] = FerruleValue.FromString(test["lookup"]!.GetValue<string>());
    var context = FerruleExecutionContext.WithParameters("mapping.mfd", new FerruleRuntimeParameters(parameters));
    var primary = FerruleJson.ParseEmbedded(source, test["primary"]!.ToJsonString());
    try
    {
        var actual = GeneratedMapping.Execute(primary, context);
        if (test["error"] is not null || missing) throw new InvalidOperationException("Expected typed failure: " + test);
        LookupOracle.Equal(actual, FerruleJson.ParseEmbedded(target, test["expected"]!.ToJsonString()));
        var json = FerruleJson.SerializeEmbedded(target, actual);
        if (!JsonNode.DeepEquals(JsonNode.Parse(json), test["expected"]))
            throw new InvalidOperationException("Output mismatch: " + json + " expected " + test["expected"]);
    }
    catch (FerruleRuntimeException error) when (test["error"] is not null)
    {
        if (error.Error != FerruleRuntimeError.FunctionInvalidArgument
            || !test["error_functions"]!.AsArray().Any(allowed => allowed!.GetValue<string>() == error.Function)
            || error.Detail != test["error"]!.GetValue<string>()) throw;
    }
    catch (FerruleRuntimeException error) when (missing)
    {
        if (error.RuntimeParameter != "LookupText" || error.Error != FerruleRuntimeError.MissingRuntimeParameter) throw;
    }
}
Console.WriteLine("C# root-scalar lookup host passed");
