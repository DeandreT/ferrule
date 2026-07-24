using System.Text.Json;
using System.Text.Json.Nodes;
using Ferrule.Generated;

var input = File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "input.json"));
var expectedText = File.ReadAllText(
    Path.Combine(AppContext.BaseDirectory, "expected-output.json"));
var output = GeneratedMapping.ExecuteJson(input);

var actual = JsonNode.Parse(output);
var expected = JsonNode.Parse(expectedText);
if (!JsonNode.DeepEquals(actual, expected))
{
    throw new InvalidOperationException("Generated JSON differs from the expected output.");
}

using var document = JsonDocument.Parse(output);
var invoices = document.RootElement.GetProperty("Invoices");
foreach (var invoice in invoices.EnumerateArray())
{
    Console.WriteLine(
        $"{invoice.GetProperty("Sequence").GetInt64()}: " +
        $"{invoice.GetProperty("Display").GetString()}");
}

Console.WriteLine("C# generated mapping example passed.");
