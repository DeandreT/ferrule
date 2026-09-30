using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void JsonAllowedValuesBoundaries()
    {
        JsonAllowedValueMetadataNumberTags();
        const string mixed =
            "{\"name\":\"Value\",\"nullable\":true,\"json_allowed_values\":[{\"type\":\"json_null\"},{\"type\":\"bool\",\"value\":false},{\"type\":\"bool\",\"value\":true},{\"type\":\"int\",\"value\":-2},{\"type\":\"int\",\"value\":1},{\"type\":\"float\",\"value\":1.5},{\"type\":\"string\",\"value\":\"A\"}],\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"int\",\"float\",\"bool\"]}}";
        foreach (var input in new[] { "null", "false", "true", "-2", "1", "1.0", "1.5", "\"A\"" })
        {
            _ = FerruleJson.Parse(mixed, input);
        }
        foreach (var input in new[] { "2", "1.5000000000000002", "\"B\"" })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(mixed, input));
        }
        Equal(
            "null\n",
            FerruleJson.Serialize(mixed, Scalar(FerruleValue.JsonNull)));
        Equal(
            "1\n",
            FerruleJson.Serialize(mixed, Scalar(FerruleValue.FromInt64(1))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                mixed,
                Scalar(FerruleValue.FromDouble(2.0))));

        const string floating =
            "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":-9223372036854775808},{\"type\":\"int\",\"value\":1},{\"type\":\"float\",\"value\":1.5}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}";
        foreach (var input in new[] { "-9223372036854775808", "1", "1.0", "1.5" })
        {
            _ = FerruleJson.Parse(floating, input);
        }
        Equal(
            "1.0\n",
            FerruleJson.Serialize(floating, Scalar(Text("1"))));
        Equal(
            "1.5\n",
            FerruleJson.Serialize(floating, Scalar(Text("1.5"))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(floating, "1.5000000000000002"));

        const string exactNumericUnion =
            "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":9007199254740993},{\"type\":\"float\",\"value\":1.5}],\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"int\",\"float\"]}}";
        _ = FerruleJson.Parse(exactNumericUnion, "9007199254740993");
        Equal(
            "9007199254740993\n",
            FerruleJson.Serialize(
                exactNumericUnion,
                Scalar(FerruleValue.FromInt64(9_007_199_254_740_993))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(exactNumericUnion, "9007199254740992"));

        const string integer =
            "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":-1},{\"type\":\"int\",\"value\":2}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}";
        Equal(
            "2\n",
            FerruleJson.Serialize(integer, Scalar(Text("2"))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(integer, Scalar(Text("1"))));

        const string text =
            "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"string\",\"value\":\"1\"},{\"type\":\"string\",\"value\":\"x\"}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";
        Equal(
            "\"1\"\n",
            FerruleJson.Serialize(text, Scalar(FerruleValue.FromInt64(1))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(text, Scalar(Text("y"))));

        const string nullableText =
            "{\"name\":\"Value\",\"nullable\":true,\"json_allowed_values\":[{\"type\":\"json_null\"},{\"type\":\"string\",\"value\":\"x\"}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";
        Equal(
            "null\n",
            FerruleJson.Serialize(nullableText, Scalar(FerruleValue.JsonNull)));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(nullableText, Scalar(Text("y"))));

        const string unicodeOrder =
            "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"string\",\"value\":\"\uE000\"},{\"type\":\"string\",\"value\":\"\uD800\uDC00\"}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";
        _ = FerruleJson.Parse(unicodeOrder, "\"\uE000\"");
        _ = FerruleJson.Parse(unicodeOrder, "\"\uD800\uDC00\"");

        const string signedFloats =
            "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"float\",\"value\":-1.5},{\"type\":\"float\",\"value\":1.5}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}";
        _ = FerruleJson.Parse(signedFloats, "-1.5");
        _ = FerruleJson.Parse(signedFloats, "1.5");

        const string nullMetadata =
            "{\"name\":\"Value\",\"json_allowed_values\":null,\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}";
        _ = FerruleJson.Parse(nullMetadata, "7");

        foreach (var invalidSchema in new[]
                 {
                     "{\"name\":\"Value\",\"json_allowed_values\":{},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":1}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":1},{\"type\":\"int\",\"value\":1}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"string\",\"value\":\"x\"},{\"type\":\"int\",\"value\":1}],\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"int\"]}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"bool\",\"value\":true},{\"type\":\"bool\",\"value\":false}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"bool\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"float\",\"value\":1.5},{\"type\":\"float\",\"value\":-1.5}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"string\",\"value\":\"\uD800\uDC00\"},{\"type\":\"string\",\"value\":\"\uE000\"}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"float\",\"value\":1.0},{\"type\":\"string\",\"value\":\"x\"}],\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"float\"]}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"float\",\"value\":-0.0},{\"type\":\"string\",\"value\":\"x\"}],\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"float\"]}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"json_null\"},{\"type\":\"int\",\"value\":1}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"nullable\":true,\"json_allowed_values\":[{\"type\":\"int\",\"value\":1},{\"type\":\"int\",\"value\":2}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":1},{\"type\":\"string\",\"value\":\"x\"}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":1},{\"type\":\"float\",\"value\":1.5}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":1},{\"type\":\"int\",\"value\":2}],\"json_any\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"fixed\":\"1\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":1},{\"type\":\"int\",\"value\":2}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":1},{\"type\":\"int\",\"value\":2}],\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"json_null\",\"value\":null},{\"type\":\"int\",\"value\":1}],\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\"},{\"type\":\"int\",\"value\":2}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":1.0},{\"type\":\"int\",\"value\":2}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":1,\"extra\":true},{\"type\":\"int\",\"value\":2}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"type\":\"int\",\"value\":1},{\"type\":\"int\",\"value\":2}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"unknown\",\"value\":1},{\"type\":\"int\",\"value\":2}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"bool\",\"value\":1},{\"type\":\"int\",\"value\":2}],\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"int\",\"bool\"]}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"float\",\"value\":1e400},{\"type\":\"string\",\"value\":\"x\"}],\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"float\"]}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"string\",\"value\":\"\\uD800\"},{\"type\":\"string\",\"value\":\"z\"}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":1},{\"type\":\"int\",\"value\":2}],\"json_allowed_values\":[{\"type\":\"int\",\"value\":2},{\"type\":\"int\",\"value\":3}],\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(invalidSchema, "1"));
        }

        var maximumValues = string.Join(
            ',',
            Enumerable.Range(0, 4096)
                .Select(index => $"{{\"type\":\"int\",\"value\":{index}}}"));
        _ = FerruleJson.Parse(
            $"{{\"name\":\"Value\",\"json_allowed_values\":[{maximumValues}],\"kind\":{{\"kind\":\"scalar\",\"ty\":\"int\"}}}}",
            "4095");
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                $"{{\"name\":\"Value\",\"json_allowed_values\":[{maximumValues},{{\"type\":\"int\",\"value\":4096}}],\"kind\":{{\"kind\":\"scalar\",\"ty\":\"int\"}}}}",
                "1"));

        var maximumString = new string('x', 256 * 1024);
        _ = FerruleJson.Parse(
            $"{{\"name\":\"Value\",\"json_allowed_values\":[{{\"type\":\"string\",\"value\":{System.Text.Json.JsonSerializer.Serialize(maximumString)}}},{{\"type\":\"string\",\"value\":\"z\"}}],\"kind\":{{\"kind\":\"scalar\",\"ty\":\"string\"}}}}",
            "\"z\"");
        var oversizedString = maximumString + "x";
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                $"{{\"name\":\"Value\",\"json_allowed_values\":[{{\"type\":\"string\",\"value\":{System.Text.Json.JsonSerializer.Serialize(oversizedString)}}},{{\"type\":\"string\",\"value\":\"z\"}}],\"kind\":{{\"kind\":\"scalar\",\"ty\":\"string\"}}}}",
                "\"z\""));
    }

    private static void JsonAllowedValueMetadataNumberTags()
    {
        static string Schema(string integer) =>
            "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"int\",\"value\":" + integer +
            "},{\"type\":\"string\",\"value\":\"x\"}],\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"int\"]}}";

        foreach (var integer in new[] { "0", "9223372036854775807", "-9223372036854775808" })
        {
            var schema = Schema(integer);
            Equal(Text("x"), ((FerruleScalar)FerruleJson.Parse(schema, "\"x\"")).Value);
            Equal("\"x\"\n", FerruleJson.Serialize(schema, Scalar(Text("x"))));
        }
        // Typed metadata requires the integer tag even when a floating token
        // represents an integral value. In serde_json, -0 is a floating token.
        foreach (var floatingOrOutOfRange in new[] { "-0", "1.0", "1e0", "9223372036854775808" })
        {
            var schema = Schema(floatingOrOutOfRange);
            var error = Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(schema, "\"x\""));
            Equal(true, error.Message.Contains("JSON allowed int payload", StringComparison.Ordinal));
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Serialize(schema, Scalar(Text("x"))));
        }

        static string FloatSchema(string number) =>
            "{\"name\":\"Value\",\"json_allowed_values\":[{\"type\":\"float\",\"value\":" + number +
            "},{\"type\":\"string\",\"value\":\"x\"}],\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"float\"]}}";

        // Float metadata uses typed finite-f64 deserialization, which permits
        // integer rounding. Ordinary float input still requires exact coercion.
        foreach (var (metadata, actual) in new[]
                 {
                     ("9223372036854775807", "9.223372036854776e18"),
                     ("18446744073709551615", "1.8446744073709552e19"),
                 })
        {
            var schema = FloatSchema(metadata);
            Equal(Text("x"), ((FerruleScalar)FerruleJson.Parse(schema, "\"x\"")).Value);
            Equal("\"x\"\n", FerruleJson.Serialize(schema, Scalar(Text("x"))));
            Equal(
                FerruleValueKind.Double,
                ((FerruleScalar)FerruleJson.Parse(schema, actual)).Value.Kind);
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(schema, metadata));
        }
        foreach (var integralFloat in new[] { "9007199254740993", "-9007199254740993", "-0" })
        {
            var error = Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(FloatSchema(integralFloat), "\"x\""));
            Equal(true, error.Message.Contains("JSON allowed float payload", StringComparison.Ordinal));
        }
    }
}
