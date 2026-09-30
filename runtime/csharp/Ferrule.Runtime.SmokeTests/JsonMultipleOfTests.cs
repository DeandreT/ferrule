using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void JsonMultipleOfMetadataNumberTags()
    {
        static string Schema(string coefficient, string exponent) =>
            "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":" + coefficient +
            ",\"decimal_exponent\":" + exponent + "}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}";

        foreach (var (coefficient, exponent) in new[]
                 {
                     ("1", "0"), ("1", "-1"), ("18446744073709551615", "0"),
                 })
        {
            var schema = Schema(coefficient, exponent);
            Equal(
                FerruleValue.FromDouble(0.0),
                ((FerruleScalar)FerruleJson.Parse(schema, "0.0")).Value);
            Equal("0.0\n", FerruleJson.Serialize(schema, Scalar(Text("0.0"))));
        }

        foreach (var coefficient in new[] { "-0", "1.0", "1e0", "18446744073709551616" })
        {
            var schema = Schema(coefficient, "0");
            var error = Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(schema, "0.0"));
            Equal(true, error.Message.Contains("positive canonical unsigned integer", StringComparison.Ordinal));
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Serialize(schema, Scalar(Text("0.0"))));
        }
        foreach (var exponent in new[] { "-0", "1.0", "1e0", "32768", "-32769" })
        {
            var schema = Schema("1", exponent);
            var error = Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(schema, "0.0"));
            Equal(true, error.Message.Contains("signed 16-bit integer", StringComparison.Ordinal));
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Serialize(schema, Scalar(Text("0.0"))));
        }
        foreach (var finiteRangeViolation in new[] { "32767", "-32768" })
        {
            var error = Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(Schema("1", finiteRangeViolation), "0.0"));
            Equal(true, error.Message.Contains("positive finite decimal", StringComparison.Ordinal));
        }
    }
}
