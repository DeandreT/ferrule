using System.Text.Json.Nodes;
using Ferrule.Generated;
using Ferrule.Runtime;

var cases = JsonNode.Parse(File.ReadAllText(args[0]))!.AsArray();
var source = File.ReadAllText(args[1]);
var target = File.ReadAllText(args[2]);
foreach (var item in cases)
{
    var test = item!;
    var failure = test["parameter_failure"]?.GetValue<string>();
    var parameters = new Dictionary<string, FerruleValue>();
    if (failure != "missing") parameters["LookupText"] = failure switch {
        "null" => FerruleValue.Null,
        "int" => FerruleValue.FromInt64(7),
        _ => FerruleValue.FromString(test["lookup"]!.GetValue<string>())
    };
    var context = FerruleExecutionContext.WithParameters("mapping.mfd", new FerruleRuntimeParameters(parameters));
    var primary = FerruleJson.ParseEmbedded(source, test["primary"]!.ToJsonString());
    try
    {
        var actual = GeneratedMapping.Execute(primary, context);
        if (test["error"] is not null || failure is not null) throw new InvalidOperationException("Expected typed failure: " + test);
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
    catch (FerruleRuntimeException error) when (failure is not null)
    {
        if (error.RuntimeParameter != "LookupText") throw;
        if (failure == "missing") {
            if (error.Error != FerruleRuntimeError.MissingRuntimeParameter) throw;
        } else if (error.Error != FerruleRuntimeError.RuntimeParameterType || error.ExpectedScalarType != FerruleScalarType.String
            || error.FoundKind != (failure == "null" ? FerruleValueKind.Null : FerruleValueKind.Int64)) throw;
    }
}
Console.WriteLine("C# parsed-array lookup host passed");
