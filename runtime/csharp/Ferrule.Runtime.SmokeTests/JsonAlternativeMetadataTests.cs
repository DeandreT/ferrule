using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void JsonAlternativeMetadataBoundaries()
    {
        static string Schema(string scalar, string tag, string payload) =>
            "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"X\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"" + scalar +
            "\"}}],\"alternatives\":[{\"name\":\"Alternative\",\"members\":[\"X\"],\"constraints\":[{\"member\":\"X\",\"value\":{\"type\":\"" + tag +
            "\",\"value\":" + payload + "}}]}]}}";

        foreach (var (scalar, tag, payload) in new[]
                 {
                     ("string", "string", "\"valid\""),
                     ("bool", "bool", "false"),
                     ("int", "int", "0"),
                     ("float", "float", "1.5"),
                     ("string", "json_null", "null"),
                 })
        {
            var schema = Schema(scalar, tag, payload);
            var parsed = (FerruleGroup)FerruleJson.Parse(schema, "{}");
            Equal(FerruleValue.Null, ((FerruleScalar)parsed.Fields[0].Value).Value);
            Equal("{}\n", FerruleJson.Serialize(schema, Group()));
        }

        var unitVariant = Schema("string", "json_null", "null")
            .Replace(",\"value\":null", string.Empty, StringComparison.Ordinal);
        _ = FerruleJson.Parse(unitVariant, "{}");
        Equal("{}\n", FerruleJson.Serialize(unitVariant, Group()));

        // An absent optional field may skip data matching, but it must not
        // skip typed deserialization of the embedded schema declaration.
        foreach (var (scalar, tag, payload) in new[]
                 {
                     ("int", "int", "-0"),
                     ("int", "int", "1.0"),
                     ("int", "int", "1e0"),
                     ("int", "int", "9223372036854775808"),
                     ("float", "float", "1.7976931348623159e308"),
                     ("float", "float", "1e400"),
                     ("string", "string", "1"),
                     ("string", "string", "null"),
                     ("string", "string", "\"\\uD800\""),
                     ("bool", "bool", "0"),
                     ("bool", "bool", "\"true\""),
                     ("bool", "bool", "null"),
                     ("string", "unknown", "\"valid\""),
                     ("string", "json_null", "1"),
                     ("string", "json_null", "false"),
                     ("string", "json_null", "\"x\""),
                 })
        {
            var schema = Schema(scalar, tag, payload);
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(schema, "{}"));
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Serialize(schema, Group()));
        }
    }
}
