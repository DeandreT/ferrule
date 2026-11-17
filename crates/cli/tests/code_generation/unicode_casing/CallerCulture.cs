using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Ferrule.Generated;
using Ferrule.Runtime;

try
{
    if (args.Length != 2)
        throw new ArgumentException("Expected a culture name and the literal cases file.");

    if (!AppContext.TryGetSwitch("System.Globalization.Invariant", out var invariant) || invariant)
        throw new InvalidOperationException("The caller must run with InvariantGlobalization=false.");
    if (!AppContext.TryGetSwitch("System.Globalization.PredefinedCulturesOnly", out var predefinedOnly) || !predefinedOnly)
        throw new InvalidOperationException("Predefined-only culture construction must be enabled.");

    var culture = args[0] switch
    {
        "invariant" => CultureInfo.InvariantCulture,
        "en-US" or "tr-TR" or "az-Latn-AZ" or "lt-LT" => new CultureInfo(args[0], useUserOverride: false),
        _ => throw new ArgumentException($"Unexpected culture: {args[0]}"),
    };
    var expectedName = args[0] == "invariant" ? "" : args[0];
    CultureInfo.CurrentCulture = culture;
    CultureInfo.CurrentUICulture = culture;
    VerifyCulture(expectedName);
    Console.Error.WriteLine($"Requested={args[0]}; CurrentCulture={CultureInfo.CurrentCulture.Name}; CurrentUICulture={CultureInfo.CurrentUICulture.Name}");

    using var fixture = JsonDocument.Parse(File.ReadAllText(args[1]));
    var cases = fixture.RootElement.EnumerateArray().ToArray();
    if (cases.Length == 0)
        throw new InvalidOperationException("The literal casing cases must not be empty.");

    // The expected strings are authored literals, never .NET casing results.
    var expected = new
    {
        Cases = cases.Select(test => new
        {
            Upper = test.GetProperty("upper").GetString()!,
            Lower = test.GetProperty("lower").GetString()!,
        }).ToArray(),
    };
    var input = JsonSerializer.Serialize(new
    {
        Cases = cases.Select(test => new { Text = test.GetProperty("text").GetString()! }).ToArray(),
    });
    var direct = new
    {
        Cases = cases.Select(test =>
        {
            VerifyCulture(expectedName);
            var values = new[] { FerruleValue.FromString(test.GetProperty("text").GetString()!) };
            return new
            {
                Upper = FerruleFunctions.Call("upper", values).StringValue,
                Lower = FerruleFunctions.Call("lower", values).StringValue,
            };
        }).ToArray(),
    };
    File.WriteAllText($"{args[0]}-direct.json", JsonSerializer.Serialize(direct));
    VerifyCulture(expectedName);
    var generated = GeneratedMapping.ExecuteJson(input);
    File.WriteAllText($"{args[0]}-generated.json", generated);
    VerifyCulture(expectedName);
    var generatedBytes = GeneratedMapping.ExecuteJsonBytes(Encoding.UTF8.GetBytes(input));
    File.WriteAllBytes($"{args[0]}-generated-bytes.json", generatedBytes);
    VerifyCulture(expectedName);

    var expectedJson = JsonSerializer.SerializeToNode(expected);
    var directJson = JsonSerializer.SerializeToNode(direct);
    var generatedJson = JsonNode.Parse(generated);
    var generatedBytesJson = JsonNode.Parse(generatedBytes);
    // Emit every observation before comparisons, including both full outputs and
    // the literal oracle, so a failure does not hide later rows or a boundary.
    Console.WriteLine(JsonSerializer.Serialize(new
    {
        culture = CultureInfo.CurrentCulture.Name,
        uiCulture = CultureInfo.CurrentUICulture.Name,
        input,
        expected = expectedJson,
        direct = directJson,
        generated = generatedJson,
        generatedBytes = generatedBytesJson,
    }));
    if (!JsonNode.DeepEquals(expectedJson, directJson) ||
        !JsonNode.DeepEquals(expectedJson, generatedJson) ||
        !JsonNode.DeepEquals(expectedJson, generatedBytesJson))
        throw new InvalidOperationException("Portable casing differs from literal expectations; see the complete stdout report.");
    return 0;
}
catch (Exception error)
{
    Console.Error.WriteLine($"Caller culture {(args.Length > 0 ? args[0] : "<missing>")} failed:\n{error}");
    return 1;
}

static void VerifyCulture(string expectedName)
{
    if (!string.Equals(CultureInfo.CurrentCulture.Name, expectedName, StringComparison.Ordinal) ||
        !string.Equals(CultureInfo.CurrentUICulture.Name, expectedName, StringComparison.Ordinal) ||
        (expectedName.Length != 0 &&
            (CultureInfo.CurrentCulture.Equals(CultureInfo.InvariantCulture) ||
             CultureInfo.CurrentUICulture.Equals(CultureInfo.InvariantCulture))))
        throw new InvalidOperationException($"Expected both caller cultures {JsonSerializer.Serialize(expectedName)}, got CurrentCulture={JsonSerializer.Serialize(CultureInfo.CurrentCulture.Name)}, CurrentUICulture={JsonSerializer.Serialize(CultureInfo.CurrentUICulture.Name)}.");
}
