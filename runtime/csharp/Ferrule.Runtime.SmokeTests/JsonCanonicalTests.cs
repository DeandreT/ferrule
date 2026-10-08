using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string JsonAnyScalarSchema =
        """{"name":"Any","json_any":true,"kind":{"kind":"scalar","ty":"string"}}""";

    private static void JsonCanonicalNumbers()
    {
        var examples = new (string Input, string Canonical)[]
        {
            ("0", "0"),
            ("-0", "-0.0"),
            ("-0.0", "-0.0"),
            ("1e1", "10.0"),
            ("1e-5", "0.00001"),
            ("1e-6", "1e-6"),
            ("1e15", "1000000000000000.0"),
            ("1e16", "1e+16"),
            ("1e-307", "1e-307"),
            ("1e-400", "0.0"),
            ("-1e-400", "-0.0"),
            ("0e-309", "0.0"),
            ("-0e-309", "-0.0"),
            ("5e-324", "5e-324"),
            ("9007199254740993", "9007199254740993"),
            ("9223372036854775807", "9223372036854775807"),
            ("9223372036854775808", "9223372036854775808"),
            ("18446744073709551615", "18446744073709551615"),
            ("18446744073709551616", "1.8446744073709552e+19"),
            ("-9223372036854775809", "-9.223372036854776e+18"),
            ("1.7976931348623157e308", "1.7976931348623157e+308"),
        };

        foreach (var (input, canonical) in examples)
        {
            var parsed = (FerruleScalar)FerruleJson.Parse(JsonAnyScalarSchema, input);
            Equal(Text(canonical), parsed.Value);
            // Correctly rounded input preserves the shortest canonical text
            // through both the stored arbitrary-JSON string and the writer.
            Equal(canonical + "\n", FerruleJson.Serialize(JsonAnyScalarSchema, parsed));
            Equal(
                canonical + "\n",
                FerruleJson.Serialize(JsonAnyScalarSchema, Scalar(Text(input))));
        }

        const string objectInput =
            """{"first":1e1,"nested":{"x":1,"x":-0},"first":1e-400,"last":18446744073709551615}""";
        const string objectCanonical =
            """{"first":0.0,"nested":{"x":-0.0},"last":18446744073709551615}""";
        var objectInstance =
            (FerruleScalar)FerruleJson.Parse(JsonAnyScalarSchema, objectInput);
        Equal(Text(objectCanonical), objectInstance.Value);
        using (var rendered = JsonDocument.Parse(
                   FerruleJson.Serialize(JsonAnyScalarSchema, objectInstance)))
        {
            var members = rendered.RootElement.EnumerateObject().ToArray();
            Equal(3, members.Length);
            Equal("first", members[0].Name);
            Equal("0.0", members[0].Value.GetRawText());
            Equal("nested", members[1].Name);
            var nested = members[1].Value.EnumerateObject().ToArray();
            Equal(1, nested.Length);
            Equal("-0.0", nested[0].Value.GetRawText());
            Equal("last", members[2].Name);
            Equal("18446744073709551615", members[2].Value.GetRawText());
        }

        const string unicodeInput =
            "{\"😀\":\"😀\",\"control\":\"\\u001f\",\"slash\":\"/\",\"cjk\":\"中\",\"separator\":\"\\u2028\",\"del\":\"\\u007f\"}";
        const string unicodeCanonical =
            "{\"😀\":\"😀\",\"control\":\"\\u001f\",\"slash\":\"/\",\"cjk\":\"中\",\"separator\":\"\u2028\",\"del\":\"\u007f\"}";
        var unicodeInstance =
            (FerruleScalar)FerruleJson.Parse(JsonAnyScalarSchema, unicodeInput);
        Equal(Text(unicodeCanonical), unicodeInstance.Value);

        foreach (var sign in new[] { "", "-" })
        {
            foreach (var finalDigit in new[] { "", "1" })
            {
                var longFraction = sign + "0." + new string('0', 1_000) + finalDigit;
                var zero = sign == "-" ? "-0.0" : "0.0";
                var parsed = (FerruleScalar)FerruleJson.Parse(
                    JsonAnyScalarSchema,
                    longFraction);
                Equal(Text(zero), parsed.Value);
                Equal(
                    zero + "\n",
                    FerruleJson.Serialize(JsonAnyScalarSchema, Scalar(Text(longFraction))));
            }
        }

        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(JsonAnyScalarSchema, "1e309"));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(JsonAnyScalarSchema, "1.7976931348623159e308"));
        Equal(
            "\"1e309\"\n",
            FerruleJson.Serialize(JsonAnyScalarSchema, Scalar(Text("1e309"))));
        Equal(
            "\"{\\\"nested\\\":1e309}\"\n",
            FerruleJson.Serialize(
                JsonAnyScalarSchema,
                Scalar(Text("""{"nested":1e309}"""))));
    }
}
