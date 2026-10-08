using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void JsonFloatingMetadataCoercion()
    {
        foreach (var (metadata, actual) in new[]
                 {
                     ("9007199254740993", "9007199254740992.0"),
                     ("9223372036854775807", "9.223372036854776e18"),
                     ("18446744073709551615", "1.8446744073709552e19"),
                     ("-9007199254740993", "-9007199254740992.0"),
                 })
        {
            var range = "{\"name\":\"Value\",\"numeric_range\":{\"kind\":\"number\",\"bounds\":{\"maximum\":{\"value\":" + metadata + "}}},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}";
            _ = FerruleJson.Parse(range, "-1e30");
            var alternative = Alternative(metadata, true);
            _ = FerruleJson.Parse(alternative, "{\"X\":" + actual + "}");
            // Data retains exact integer-to-float conversion; typed f64
            // metadata permits the rounding performed by Rust deserialization.
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(alternative, "{\"X\":" + metadata + "}"));
        }
        foreach (var invalid in new[] { "1.7976931348623159e308", "1e400" })
        {
            var schema = Alternative(invalid, false);
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.Parse(schema, "{}"));
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.Serialize(schema, Group()));
        }

        static string Alternative(string expected, bool required) =>
            "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"X\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}],\"alternatives\":[{\"name\":\"Float\",\"members\":[\"X\"],\"required\":" +
            (required ? "[\"X\"]" : "[]") +
            ",\"constraints\":[{\"member\":\"X\",\"value\":{\"type\":\"float\",\"value\":" + expected + "}}]}]}}";
    }
}
