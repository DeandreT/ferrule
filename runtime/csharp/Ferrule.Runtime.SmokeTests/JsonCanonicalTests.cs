using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string JsonAnyScalarSchema =
        """{"name":"Any","json_any":true,"kind":{"kind":"scalar","ty":"string"}}""";

    private const string CompactValueSchema =
        """{"name":"Root","kind":{"kind":"group","children":[{"name":"Value","kind":{"kind":"scalar","ty":"string"}}]}}""";
    private const string CompactValueDescriptor = "FERRULE-EMBEDDED-SCHEMA/2\n" + CompactValueSchema;

    private static void AssertEmbeddedCompact(string descriptor, FerruleInstance instance,
        string expected, int? expectedBytes = null)
    {
        var text = FerruleJson.SerializeEmbeddedCompact(descriptor, instance);
        var bytes = FerruleJson.SerializeEmbeddedCompactBytes(descriptor, instance);
        Equal(expected, text);
        Equal(true, bytes.AsSpan().SequenceEqual(Encoding.UTF8.GetBytes(expected)));
        Equal(expectedBytes ?? Encoding.UTF8.GetByteCount(expected), bytes.Length);
    }

    private static void JsonEmbeddedCompactDocuments()
    {
        foreach (var (value, expected) in new[]
        {
            ("P.root", "{\"Value\":\"P.root\"}"),
            ("N.root", "{\"Value\":\"N.root\"}"),
        })
        {
            var instance = Group(Field("Value", Scalar(Text(value))));
            AssertEmbeddedCompact(CompactValueDescriptor, instance, expected, 18);
            var pretty = "{\n  \"Value\": \"" + value + "\"\n}\n";
            Equal(pretty, FerruleJson.Serialize(CompactValueSchema, instance));
            Equal(pretty, FerruleJson.SerializeEmbedded(CompactValueDescriptor, instance));
            Equal(true, FerruleJson.SerializeBytes(CompactValueSchema, instance).AsSpan()
                .SequenceEqual(Encoding.UTF8.GetBytes(pretty)));
            Equal(true, FerruleJson.SerializeEmbeddedBytes(CompactValueDescriptor, instance).AsSpan()
                .SequenceEqual(Encoding.UTF8.GetBytes(pretty)));
        }
        var rows = Repeated(Group(Field("Value", Scalar(Text("N1")))),
            Group(Field("Value", Scalar(Text("N2")))));
        const string twoRows = "[{\"Value\":\"N1\"},{\"Value\":\"N2\"}]";
        AssertEmbeddedCompact(CompactValueDescriptor, rows, twoRows, 31);
        const string repeatedDescriptor =
            "FERRULE-EMBEDDED-SCHEMA/2\n" +
            """{"name":"Rows","repeating":true,"item_count_range":{"minimum":2,"maximum":2},"kind":{"kind":"group","children":[{"name":"Value","kind":{"kind":"scalar","ty":"string"}}]}}""";
        AssertEmbeddedCompact(repeatedDescriptor, rows, twoRows, 31);
        using (var parsed = JsonDocument.Parse(FerruleJson.SerializeEmbeddedCompact(repeatedDescriptor, rows)))
        {
            Equal(2, parsed.RootElement.GetArrayLength());
            Equal("N1", parsed.RootElement[0].GetProperty("Value").GetString());
            Equal("N2", parsed.RootElement[1].GetProperty("Value").GetString());
        }
        AssertEmbeddedCompact(CompactValueDescriptor, Group(), "{}", 2);
        AssertEmbeddedCompact(CompactValueDescriptor, Repeated(), "[]", 2);
        AssertEmbeddedCompact("""{"name":"Null","nullable":true,"kind":{"kind":"scalar","ty":"string"}}""",
            Scalar(FerruleValue.JsonNull), "null", 4);

        const string ordered =
            "FERRULE-EMBEDDED-SCHEMA/2\n" +
            """{"name":"Ordered","kind":{"kind":"group","children":[{"name":"Zed","kind":{"kind":"scalar","ty":"string"}},{"name":"Alpha","kind":{"kind":"scalar","ty":"int"}}]}}""";
        AssertEmbeddedCompact(ordered,
            Group(Field("Alpha", Scalar(Text("2.000"))), Field("Zed", Scalar(Text("first")))),
            "{\"Zed\":\"first\",\"Alpha\":2}");
    }

    private static void JsonEmbeddedCompactStringsAndNumbers()
    {
        const string value = " \tline\n\r\b\f\"\\ /雪😀\u2028\u2029\u001f\u007f end ";
        const string expected = "{\"Value\":\" \\tline\\n\\r\\b\\f\\\"\\\\ /雪😀\u2028\u2029\\u001f\u007f end \"}";
        AssertEmbeddedCompact(CompactValueDescriptor, Group(Field("Value", Scalar(Text(value)))), expected);
        const string unicodeNameSchema =
            """{"name":"Unicode","kind":{"kind":"group","children":[{"name":"  列😀  ","kind":{"kind":"scalar","ty":"string"}}]}}""";
        AssertEmbeddedCompact(unicodeNameSchema, Group(Field("  列😀  ", Scalar(Text(" a b ")))),
            "{\"  列😀  \":\" a b \"}");
        AssertEmbeddedCompact(JsonAnyScalarSchema,
            Scalar(Text("{ \"first\": [ 1e-307, -0, 18446744073709551615 ], \"text\": \" a b \\t c \" }")),
            "{\"first\":[1e-307,-0.0,18446744073709551615],\"text\":\" a b \\t c \"}");

        // Fixed bit and lexical oracles; expected numbers are never recovered
        // by parsing or compacting another subject serialization.
        var cases = new (ulong Bits, string Expected)[]
        {
            (0x0000000000000000UL, "0.0"),
            (0x8000000000000000UL, "-0.0"),
            (0x0000000000000001UL, "5e-324"),
            (0x0031fa182c40c60dUL, "1e-307"),
            (0x0031fa182c40c60eUL, "1.0000000000000001e-307"),
            (0x3fb9999999999999UL, "0.09999999999999999"),
            (0x3ff0000000000001UL, "1.0000000000000002"),
            (0x4340000000000001UL, "9007199254740994.0"),
            (0x7fefffffffffffffUL, "1.7976931348623157e+308"),
        };
        const string floatSchema = """{"name":"Number","kind":{"kind":"scalar","ty":"float"}}""";
        foreach (var (bits, lexical) in cases)
        {
            var instance = Scalar(FerruleValue.FromDouble(BitConverter.Int64BitsToDouble(unchecked((long)bits))));
            AssertEmbeddedCompact(floatSchema, instance, lexical);
            Equal(lexical + "\n", FerruleJson.SerializeEmbedded(floatSchema, instance));
        }
        const string numbersSchema = """{"name":"Numbers","repeating":true,"kind":{"kind":"scalar","ty":"float"}}""";
        var numbers = new FerruleRepeated(cases.Select(item => (FerruleInstance)Scalar(
            FerruleValue.FromDouble(BitConverter.Int64BitsToDouble(unchecked((long)item.Bits))))));
        AssertEmbeddedCompact(numbersSchema, numbers,
            "[0.0,-0.0,5e-324,1e-307,1.0000000000000001e-307,0.09999999999999999,1.0000000000000002,9007199254740994.0,1.7976931348623157e+308]");
        AssertEmbeddedCompact("""{"name":"Integer","kind":{"kind":"scalar","ty":"int"}}""",
            Scalar(Text("9007199254740993.0")), "9007199254740993");
    }

    private static void JsonEmbeddedCompactValidation()
    {
        var cases = new (string Schema, FerruleInstance Instance, string Detail)[]
        {
            (RequiredJsonSchema, Group(Field("Note", Scalar(FerruleValue.JsonNull))), "requires property 'Id'"),
            (ConstantJsonSchema, Group(Field("Status", Scalar(Text("wrong")))), "requires constant"),
            (RangeJsonSchema, Group(Field("Count", Scalar(Text("9")))), "outside its numeric range"),
            ("""{"name":"Text","string_length_range":{"minimum":2,"maximum":2},"kind":{"kind":"scalar","ty":"string"}}""",
                Scalar(Text("abc")), "string-length range"),
            ("""{"name":"Rows","repeating":true,"item_count_range":{"minimum":2},"kind":{"kind":"scalar","ty":"int"}}""",
                Repeated(Scalar(FerruleValue.FromBoolean(true))), "item-count range"),
            ("""{"name":"Value","numeric_range":{"kind":"integer","bounds":{"minimum":6,"maximum":8}},"json_multiple_of":{"any_of":[[{"coefficient":3,"decimal_exponent":0}]]},"kind":{"kind":"scalar","ty":"int"}}""",
                Scalar(Text("7.000")), "multiple-of constraints"),
        };
        foreach (var (schema, instance, detail) in cases)
        {
            var text = Error(FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.SerializeEmbeddedCompact(schema, instance));
            var bytes = Error(FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.SerializeEmbeddedCompactBytes(schema, instance));
            Equal(true, text.Message.Contains(detail, StringComparison.Ordinal));
            Equal(text.Message, text.Detail);
            Equal(text.Message, bytes.Message);
            Equal<Exception?>(null, text.InnerException);
            Equal<Exception?>(null, bytes.InnerException);
        }
        const string exactFloatDescriptor =
            "FERRULE-EMBEDDED-SCHEMA/2\n" +
            """{"name":"Number","numeric_range":{"kind":"number","bounds":{"minimum":{"value":"FERRULE-F64-BITS:0031fa182c40c60d"},"maximum":{"value":"FERRULE-F64-BITS:0031fa182c40c60d"}}},"kind":{"kind":"scalar","ty":"float"}}""";
        AssertEmbeddedCompact(exactFloatDescriptor,
            Scalar(FerruleValue.FromDouble(BitConverter.Int64BitsToDouble(0x0031fa182c40c60d))), "1e-307");
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbeddedCompact(exactFloatDescriptor,
            Scalar(FerruleValue.FromDouble(BitConverter.Int64BitsToDouble(0x0031fa182c40c60e)))));
        foreach (var number in new[] { double.NaN, double.PositiveInfinity, double.NegativeInfinity })
        {
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.SerializeEmbeddedCompact(
                """{"name":"Number","kind":{"kind":"scalar","ty":"float"}}""", Scalar(FerruleValue.FromDouble(number))));
        }
        Error(FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.SerializeEmbeddedCompact("FERRULE-EMBEDDED-SCHEMA/999\n{}", Group()));
    }

    private static void JsonEmbeddedCompactDocumentLimit()
    {
        const string descriptor =
            "FERRULE-EMBEDDED-SCHEMA/2\n" + """{"name":"Text","kind":{"kind":"scalar","ty":"string"}}""";
        var payload = new string('x', FerruleJson.MaximumDocumentBytes - 2);
        var instance = Scalar(Text(payload));
        var exact = FerruleJson.SerializeEmbeddedCompactBytes(descriptor, instance);
        Equal(FerruleJson.MaximumDocumentBytes, exact.Length);
        Equal((byte)'"', exact[0]); Equal((byte)'"', exact[^1]);
        Equal(true, exact.AsSpan(1, exact.Length - 2).IndexOfAnyExcept((byte)'x') < 0);
        var pretty = Error(FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.SerializeEmbedded(descriptor, instance));
        Equal($"JSON output is {FerruleJson.MaximumDocumentBytes + 1} bytes; maximum is {FerruleJson.MaximumDocumentBytes}.", pretty.Message);
        var oversized = Error(FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.SerializeEmbeddedCompact(descriptor,
                Scalar(Text(new string('x', FerruleJson.MaximumDocumentBytes - 1)))));
        Equal(pretty.Message, oversized.Message);
        Equal<Exception?>(null, pretty.InnerException); Equal<Exception?>(null, oversized.InnerException);
    }

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
