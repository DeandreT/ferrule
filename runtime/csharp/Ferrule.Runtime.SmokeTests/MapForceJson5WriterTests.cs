using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void MapForceJson5Writer()
    {
        using var cases = JsonDocument.Parse(File.ReadAllText(Path.Combine(AppContext.BaseDirectory, "json5-writer-cases.json")));
        foreach (var item in cases.RootElement.EnumerateArray())
        {
            string output = FerruleMapForceJson5.FormatAsciiKeys(item.GetProperty("input").GetString()!);
            string actual = Convert.ToHexStringLower(Encoding.UTF8.GetBytes(output));
            if (actual != item.GetProperty("expected_hex").GetString())
                throw new InvalidOperationException($"JSON5 writer case {item.GetProperty("id")} byte mismatch.");
            const string any = "{\"name\":\"value\",\"json_any\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";
            string typed = FerruleJson.SerializeMapForceJson5AsciiKeys(any,
                new FerruleScalar(FerruleValue.FromString(item.GetProperty("input").GetString()!)));
            if (typed != output)
                throw new InvalidOperationException($"Typed JSON5 writer case {item.GetProperty("id")} byte mismatch.");
        }
        foreach (var (input, maximum, expected) in new[]
        {
            ("{\"é\":1}", 1024, FerruleMapForceJson5Error.NonAsciiProperty),
            ("1e30", 1024, FerruleMapForceJson5Error.Number),
            ("{", 1024, FerruleMapForceJson5Error.Input),
            ("\"é\"", 3, FerruleMapForceJson5Error.OutputBytes),
            ("0", -1, FerruleMapForceJson5Error.InvalidLimit),
        })
        {
            try { FerruleMapForceJson5.FormatAsciiKeys(input, maximum); }
            catch (FerruleMapForceJson5Exception error) when (error.Error == expected) { continue; }
            throw new InvalidOperationException($"JSON5 writer did not refuse {expected}.");
        }
        if (FerruleMapForceJson5.FormatAsciiKeys("\"é\"", 4) != "\"é\"")
            throw new InvalidOperationException("JSON5 writer changed the exact UTF-8 limit.");
    }
}
