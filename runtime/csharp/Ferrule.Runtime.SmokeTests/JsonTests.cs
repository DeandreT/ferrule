using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string BasicJsonSchema =
        "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Name\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Count\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}},{\"name\":\"Note\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}}";

    private const string AlternativeJsonSchema =
        "{\"name\":\"Choice\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Type\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Text\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Count\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}],\"alternatives\":[{\"members\":[\"Type\",\"Text\"],\"required\":[\"Type\",\"Text\"],\"constraints\":[{\"member\":\"Type\",\"value\":{\"type\":\"string\",\"value\":\"text\"}}]},{\"members\":[\"Type\",\"Count\"],\"required\":[\"Type\",\"Count\"],\"constraints\":[{\"member\":\"Type\",\"value\":{\"type\":\"string\",\"value\":\"count\"}}]}]}}";

    private const string StringOrIntJsonSchema =
        "{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"int\"]}}";

    private const string IntOrFloatJsonSchema =
        "{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"int\",\"float\"]}}";

    private const string FloatOrBoolJsonSchema =
        "{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"float\",\"bool\"]}}";

    private const string ScalarUnionGroupJsonSchema =
        "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"int\"]}},{\"name\":\"Items\",\"repeating\":true,\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"int\"]}}]}}";

    private const string RequiredJsonSchema =
        "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Id\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}},{\"name\":\"Note\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}],\"required\":[\"Id\",\"Note\"]}}";

    private const string ConstantJsonSchema =
        "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Status\",\"fixed\":\"ready\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Count\",\"fixed\":\"7\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}},{\"name\":\"Ratio\",\"fixed\":\"1.25\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}},{\"name\":\"Enabled\",\"fixed\":\"true\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"bool\"}}]}}";

    private const string RangeJsonSchema =
        "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Count\",\"numeric_range\":{\"kind\":\"integer\",\"bounds\":{\"minimum\":5,\"maximum\":8}},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}},{\"name\":\"Ratio\",\"nullable\":true,\"numeric_range\":{\"kind\":\"number\",\"bounds\":{\"minimum\":{\"value\":0.25,\"exclusive\":true},\"maximum\":{\"value\":2.5}}},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}]}}";

    private const string ItemCountJsonSchema =
        "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Rows\",\"repeating\":true,\"item_count_range\":{\"minimum\":1,\"maximum\":2},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}},{\"name\":\"OptionalRows\",\"repeating\":true,\"item_count_range\":{\"minimum\":1,\"maximum\":2},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}],\"required\":[\"Rows\"]}}";

    private static void JsonDocumentBoundaries()
    {
        var parsed = (FerruleGroup)FerruleJson.Parse(
            BasicJsonSchema,
            "\uFEFF{\"Name\":\"sample\",\"Count\":3,\"Note\":null}");
        Equal(Text("sample"), ((FerruleScalar)parsed.Fields[0].Value).Value);
        Equal(FerruleValue.FromInt64(3), ((FerruleScalar)parsed.Fields[1].Value).Value);
        Equal(FerruleValue.JsonNull, ((FerruleScalar)parsed.Fields[2].Value).Value);

        var byteInput = new byte[]
        {
            0xEF, 0xBB, 0xBF,
            (byte)'{', (byte)'"', (byte)'N', (byte)'a', (byte)'m', (byte)'e', (byte)'"',
            (byte)':', (byte)'"', (byte)'c', (byte)'a', (byte)'f', 0xC3, 0xA9,
            (byte)'"', (byte)',', (byte)'"', (byte)'C', (byte)'o', (byte)'u', (byte)'n',
            (byte)'t', (byte)'"', (byte)':', (byte)'3', (byte)'}',
        };
        var parsedBytes = (FerruleGroup)FerruleJson.ParseBytes(BasicJsonSchema, byteInput);
        Equal(Text("caf\u00E9"), ((FerruleScalar)parsedBytes.Fields[0].Value).Value);
        Equal(
            "{\n  \"Name\": \"caf\u00E9\",\n  \"Count\": 3\n}\n",
            System.Text.Encoding.UTF8.GetString(
                FerruleJson.SerializeBytes(BasicJsonSchema, parsedBytes)));
        var utf8Error = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.ParseBytes(BasicJsonSchema, new byte[] { 0xFF }));
        Equal(true, utf8Error.Message.Contains("UTF-8", StringComparison.Ordinal));
        var oversized = new byte[FerruleJson.MaximumDocumentBytes + 1];
        oversized[0] = 0xFF;
        var sizeError = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.ParseBytes(BasicJsonSchema, oversized));
        Equal(true, sizeError.Message.Contains("maximum", StringComparison.Ordinal));
        Equal(false, sizeError.Message.Contains("UTF-8", StringComparison.Ordinal));

        var rendered = FerruleJson.Serialize(
            BasicJsonSchema,
            Group(
                Field("Name", Scalar(Text("caf\u00E9"))),
                Field("Count", Scalar(FerruleValue.FromInt64(3))),
                Field("Note", Scalar(FerruleValue.Null))));
        Equal("{\n  \"Name\": \"caf\u00E9\",\n  \"Count\": 3\n}\n", rendered);

        var choice = FerruleJson.Serialize(
            AlternativeJsonSchema,
            Group(
                Field("Type", Scalar(Text("text"))),
                Field("Text", Scalar(Text("value")))));
        Equal("{\n  \"Type\": \"text\",\n  \"Text\": \"value\"\n}\n", choice);

        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(BasicJsonSchema, "{\"Count\":\"wrong\"}"));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                "{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                "9007199254740993"));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                AlternativeJsonSchema,
                Group(
                    Field("Type", Scalar(Text("text"))),
                    Field("Count", Scalar(FerruleValue.FromInt64(1))))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(RequiredJsonSchema, "{\"Note\":null}"));
        var required = (FerruleGroup)FerruleJson.Parse(
            RequiredJsonSchema,
            "{\"Id\":7,\"Note\":null}");
        Equal(
            FerruleValue.JsonNull,
            ((FerruleScalar)required.Fields[1].Value).Value);
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                RequiredJsonSchema,
                Group(
                    Field("Id", Scalar(FerruleValue.Null)),
                    Field("Note", Scalar(Text("present"))))));
        Equal(
            "{\n  \"Id\": 7,\n  \"Note\": null\n}\n",
            FerruleJson.Serialize(RequiredJsonSchema, required));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                "{\"name\":\"Broken\",\"kind\":{\"kind\":\"group\",\"children\":[],\"required\":[\"missing\"]}}",
                "{}"));

        JsonConstantBoundaries();
        JsonRangeBoundaries();
        JsonItemCountBoundaries();
        JsonFormatAnnotationBoundaries();
        JsonStringLengthBoundaries();
        JsonScalarUnionBoundaries();
    }

    private static void JsonConstantBoundaries()
    {
        const string valid =
            "{\"Status\":\"ready\",\"Count\":7,\"Ratio\":1.25,\"Enabled\":true}";
        var parsed = FerruleJson.Parse(ConstantJsonSchema, valid);
        Equal(
            "{\n  \"Status\": \"ready\",\n  \"Count\": 7,\n  \"Ratio\": 1.25,\n  \"Enabled\": true\n}\n",
            FerruleJson.Serialize(ConstantJsonSchema, parsed));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                ConstantJsonSchema,
                "{\"Status\":\"wrong\",\"Count\":7,\"Ratio\":1.25,\"Enabled\":true}"));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                ConstantJsonSchema,
                Group(
                    Field("Status", Scalar(Text("ready"))),
                    Field("Count", Scalar(FerruleValue.FromInt64(8))),
                    Field("Ratio", Scalar(FerruleValue.FromDouble(1.25))),
                    Field("Enabled", Scalar(FerruleValue.FromBoolean(true))))));

        foreach (var invalidSchema in new[]
                 {
                     "{\"name\":\"Value\",\"fixed\":\"x\",\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"fixed\":\"x\",\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"int\"]}}",
                     "{\"name\":\"Value\",\"fixed\":\"not-an-int\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"fixed\":\"x\",\"json_any\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(invalidSchema, "\"x\""));
        }
    }

    private static void JsonRangeBoundaries()
    {
        var parsed = FerruleJson.Parse(RangeJsonSchema, "{\"Count\":5,\"Ratio\":2.5}");
        Equal(
            "{\n  \"Count\": 5,\n  \"Ratio\": 2.5\n}\n",
            FerruleJson.Serialize(RangeJsonSchema, parsed));
        Equal(
            "{\n  \"Count\": 8,\n  \"Ratio\": null\n}\n",
            FerruleJson.Serialize(
                RangeJsonSchema,
                Group(
                    Field("Count", Scalar(Text("8"))),
                    Field("Ratio", Scalar(FerruleValue.JsonNull)))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(RangeJsonSchema, "{\"Count\":4,\"Ratio\":1}"));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(RangeJsonSchema, "{\"Count\":5,\"Ratio\":0.25}"));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                RangeJsonSchema,
                Group(
                    Field("Count", Scalar(Text("9"))),
                    Field("Ratio", Scalar(Text("1.5"))))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                RangeJsonSchema,
                Group(
                    Field("Count", Scalar(FerruleValue.FromBoolean(true))),
                    Field("Ratio", Scalar(Text("1.5"))))));

        const string largeIntegerRange =
            "{\"name\":\"Value\",\"numeric_range\":{\"kind\":\"integer\",\"bounds\":{\"minimum\":9007199254740993,\"maximum\":9223372036854775807}},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}";
        Equal(
            FerruleValue.FromInt64(9_007_199_254_740_993),
            ((FerruleScalar)FerruleJson.Parse(largeIntegerRange, "9007199254740993")).Value);
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(largeIntegerRange, "9007199254740992"));

        foreach (var invalidSchema in new[]
                 {
                     "{\"name\":\"Value\",\"numeric_range\":{\"kind\":\"integer\",\"bounds\":{}},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"numeric_range\":{\"kind\":\"integer\",\"bounds\":{\"minimum\":2,\"maximum\":1}},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"numeric_range\":{\"kind\":\"integer\",\"bounds\":{\"minimum\":1}},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"fixed\":\"0\",\"numeric_range\":{\"kind\":\"integer\",\"bounds\":{\"minimum\":1}},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"numeric_range\":{\"kind\":\"number\",\"bounds\":{\"minimum\":{\"value\":1.7976931348623157E+308,\"exclusive\":true}}},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(invalidSchema, "1"));
        }
    }

    private static void JsonItemCountBoundaries()
    {
        var parsed = FerruleJson.Parse(
            ItemCountJsonSchema,
            "{\"Rows\":[1],\"OptionalRows\":[2,3]}");
        Equal(
            "{\n  \"Rows\": [\n    1\n  ],\n  \"OptionalRows\": [\n    2,\n    3\n  ]\n}\n",
            FerruleJson.Serialize(ItemCountJsonSchema, parsed));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(ItemCountJsonSchema, "{\"Rows\":[]}"));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(ItemCountJsonSchema, "{\"Rows\":[1,2,3]}"));
        const string atLeastTwo =
            "{\"name\":\"Rows\",\"repeating\":true,\"item_count_range\":{\"minimum\":2},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}";
        var countFirst = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(atLeastTwo, "[true]"));
        Equal(
            true,
            countFirst.Message.Contains("item-count", StringComparison.Ordinal));

        Equal(
            "{\n  \"Rows\": [\n    1\n  ]\n}\n",
            FerruleJson.Serialize(
                ItemCountJsonSchema,
                Group(
                    Field("Rows", Repeated(Scalar(FerruleValue.FromInt64(1)))),
                    Field("OptionalRows", Repeated()))));
        var requiredEmpty = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                ItemCountJsonSchema,
                Group(
                    Field("Rows", Repeated()),
                    Field("OptionalRows", Repeated()))));
        Equal(
            true,
            requiredEmpty.Message.Contains("requires property 'Rows'", StringComparison.Ordinal));
        var outputCountFirst = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                atLeastTwo,
                Repeated(Scalar(FerruleValue.FromBoolean(true)))));
        Equal(
            true,
            outputCountFirst.Message.Contains("item-count", StringComparison.Ordinal));

        const string nullable =
            "{\"name\":\"Rows\",\"repeating\":true,\"container_nullable\":true,\"item_count_range\":{\"minimum\":1,\"maximum\":2},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}";
        Equal(
            FerruleValue.JsonNull,
            ((FerruleScalar)FerruleJson.Parse(nullable, "null")).Value);
        Equal("null\n", FerruleJson.Serialize(nullable, Scalar(FerruleValue.JsonNull)));

        const string maximum =
            "{\"name\":\"Rows\",\"repeating\":true,\"item_count_range\":{\"maximum\":18446744073709551615},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}";
        Equal("[]\n", FerruleJson.Serialize(maximum, Repeated()));
        const string nullableMaximum =
            "{\"name\":\"Rows\",\"repeating\":true,\"item_count_range\":{\"minimum\":1,\"maximum\":null},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}";
        Equal(
            "[\n  1\n]\n",
            FerruleJson.Serialize(
                nullableMaximum,
                Repeated(Scalar(FerruleValue.FromInt64(1)))));

        foreach (var invalidSchema in new[]
                 {
                     "{\"name\":\"Rows\",\"item_count_range\":{\"minimum\":1},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Rows\",\"repeating\":true,\"item_count_range\":{},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Rows\",\"repeating\":true,\"item_count_range\":{\"minimum\":2,\"maximum\":1},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Rows\",\"repeating\":true,\"item_count_range\":{\"minimum\":-1},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Rows\",\"repeating\":true,\"item_count_range\":{\"minimum\":1.0},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Rows\",\"repeating\":true,\"item_count_range\":{\"minimum\":18446744073709551616},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Rows\",\"repeating\":true,\"item_count_range\":{\"minimum\":1,\"maximim\":2},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(invalidSchema, "[]"));
        }
    }

    private static void JsonFormatAnnotationBoundaries()
    {
        var annotated = JsonFormatSchema(
            "{\"kind\":\"scalar\",\"ty\":\"string\"}",
            new[] { string.Empty, "email" });
        var parsed = FerruleJson.Parse(annotated, "\"not-an-email\"");
        Equal(
            Text("not-an-email"),
            ((FerruleScalar)parsed).Value);
        Equal(
            "\"not-an-email\"\n",
            FerruleJson.Serialize(annotated, parsed));

        var repeated = JsonFormatSchema(
            "{\"kind\":\"scalar\",\"ty\":\"string\"}",
            new[] { "date" },
            repeating: true);
        Equal(
            "[\n  \"not-a-date\"\n]\n",
            FerruleJson.Serialize(
                repeated,
                FerruleJson.Parse(repeated, "[\"not-a-date\"]")));

        var maximumCount = Enumerable.Range(0, 64).Select(index => $"format-{index}");
        var maximumSingle = new[] { new string('\u00E9', 512) };
        var maximumTotal = Enumerable.Range(0, 16)
            .Select(index => new string((char)('a' + index), 1024));
        foreach (var validSchema in new[] { maximumCount, maximumSingle, maximumTotal }
                     .Select(formats => JsonFormatSchema(
                         "{\"kind\":\"scalar\",\"ty\":\"string\"}",
                         formats)))
        {
            Equal(
                Text("value"),
                ((FerruleScalar)FerruleJson.Parse(validSchema, "\"value\"")).Value);
        }

        var tooMany = Enumerable.Range(0, 65).Select(index => $"format-{index}");
        var tooLarge = new[] { new string('\u00E9', 513) };
        var tooLargeInTotal = Enumerable.Range(0, 17)
            .Select(index => new string((char)('a' + index), 1024));
        foreach (var invalidSchema in new[]
                 {
                     "{\"name\":\"Value\",\"json_formats\":\"email\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_formats\":[7],\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     JsonFormatSchema(
                         "{\"kind\":\"scalar\",\"ty\":\"string\"}",
                         new[] { "email", "email" }),
                     JsonFormatSchema(
                         "{\"kind\":\"scalar\",\"ty\":\"int\"}",
                         new[] { "email" }),
                     JsonFormatSchema(
                         "{\"kind\":\"group\",\"children\":[]}",
                         new[] { "email" }),
                     JsonFormatSchema(
                         "{\"kind\":\"scalar\",\"ty\":\"string\"}",
                         new[] { "email" },
                         jsonAny: true),
                     JsonFormatSchema(
                         "{\"kind\":\"scalar\",\"ty\":\"string\"}",
                         tooMany),
                     JsonFormatSchema(
                         "{\"kind\":\"scalar\",\"ty\":\"string\"}",
                         tooLarge),
                     JsonFormatSchema(
                         "{\"kind\":\"scalar\",\"ty\":\"string\"}",
                         tooLargeInTotal),
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(invalidSchema, "\"value\""));
        }
    }

    private static string JsonFormatSchema(
        string kind,
        IEnumerable<string> formats,
        bool jsonAny = false,
        bool repeating = false) =>
        $"{{\"name\":\"Value\",\"repeating\":{repeating.ToString().ToLowerInvariant()}," +
        $"\"json_any\":{jsonAny.ToString().ToLowerInvariant()}," +
        $"\"json_formats\":{System.Text.Json.JsonSerializer.Serialize(formats)}," +
        $"\"kind\":{kind}}}";

    private static void JsonStringLengthBoundaries()
    {
        const string oneOrTwo =
            "{\"name\":\"Value\",\"string_length_range\":{\"minimum\":1,\"maximum\":2},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";
        Equal(
            Text("\U0001F600"),
            ((FerruleScalar)FerruleJson.Parse(oneOrTwo, "\"\\uD83D\\uDE00\"")).Value);
        Equal(
            "\"\\uD83D\\uDE00\"\n",
            FerruleJson.Serialize(oneOrTwo, Scalar(Text("\U0001F600"))));
        Equal(
            Text("e\u0301"),
            ((FerruleScalar)FerruleJson.Parse(oneOrTwo, "\"e\\u0301\"")).Value);
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(oneOrTwo, "\"\""));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(oneOrTwo, "\"abc\""));

        const string exactlyTwo =
            "{\"name\":\"Value\",\"string_length_range\":{\"minimum\":2,\"maximum\":2},\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"int\"]}}";
        Equal(
            Text("ab"),
            ((FerruleScalar)FerruleJson.Parse(exactlyTwo, "\"ab\"")).Value);
        Equal(
            FerruleValue.FromInt64(7),
            ((FerruleScalar)FerruleJson.Parse(exactlyTwo, "7")).Value);
        Equal(
            "7\n",
            FerruleJson.Serialize(exactlyTwo, Scalar(FerruleValue.FromInt64(7))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(exactlyTwo, "\"a\""));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(exactlyTwo, Scalar(Text("abc"))));

        const string exactlyFour =
            "{\"name\":\"Value\",\"string_length_range\":{\"minimum\":4,\"maximum\":4},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";
        Equal(
            "\"true\"\n",
            FerruleJson.Serialize(
                exactlyFour,
                Scalar(FerruleValue.FromBoolean(true))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                exactlyFour,
                Scalar(FerruleValue.FromBoolean(false))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                oneOrTwo,
                Scalar(Text(new string('\uD800', 1)))));

        const string nullable =
            "{\"name\":\"Value\",\"nullable\":true,\"string_length_range\":{\"minimum\":1},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";
        Equal(
            FerruleValue.JsonNull,
            ((FerruleScalar)FerruleJson.Parse(nullable, "null")).Value);
        Equal(
            "null\n",
            FerruleJson.Serialize(nullable, Scalar(FerruleValue.JsonNull)));

        const string fixedWithinRange =
            "{\"name\":\"Value\",\"fixed\":\"ab\",\"string_length_range\":{\"minimum\":2,\"maximum\":2},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";
        Equal(
            Text("ab"),
            ((FerruleScalar)FerruleJson.Parse(fixedWithinRange, "\"ab\"")).Value);

        foreach (var invalidSchema in new[]
                 {
                     "{\"name\":\"Value\",\"string_length_range\":{},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"string_length_range\":{\"minimum\":2,\"maximum\":1},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"string_length_range\":{\"minimum\":-1},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"string_length_range\":{\"minimum\":1.0},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"string_length_range\":{\"minimum\":18446744073709551616},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"string_length_range\":{\"minimum\":1,\"maximim\":2},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"string_length_range\":{\"minimum\":1,\"minimum\":2},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"string_length_range\":{\"minimum\":1},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"string_length_range\":{\"minimum\":1},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"json_any\":true,\"string_length_range\":{\"minimum\":1},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"fixed\":\"x\",\"string_length_range\":{\"minimum\":2},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(invalidSchema, "\"value\""));
        }
    }

    private static void JsonScalarUnionBoundaries()
    {
        Equal(
            Text("42"),
            ((FerruleScalar)FerruleJson.Parse(StringOrIntJsonSchema, "\"42\"")).Value);
        Equal(
            FerruleValue.FromInt64(42),
            ((FerruleScalar)FerruleJson.Parse(StringOrIntJsonSchema, "42")).Value);
        Equal(
            FerruleValue.FromInt64(42),
            ((FerruleScalar)FerruleJson.Parse(IntOrFloatJsonSchema, "42")).Value);
        Equal(
            FerruleValue.FromDouble(1.5),
            ((FerruleScalar)FerruleJson.Parse(IntOrFloatJsonSchema, "1.5")).Value);
        Equal(
            FerruleValue.FromDouble(1.0),
            ((FerruleScalar)FerruleJson.Parse(IntOrFloatJsonSchema, "1.0")).Value);
        Equal(
            FerruleValue.FromBoolean(true),
            ((FerruleScalar)FerruleJson.Parse(FloatOrBoolJsonSchema, "true")).Value);
        var group = (FerruleGroup)FerruleJson.Parse(
            ScalarUnionGroupJsonSchema,
            "{\"Items\":[\"A\",8]}");
        Equal(
            FerruleValue.Null,
            ((FerruleScalar)group.Fields[0].Value).Value);
        var items = (FerruleRepeated)group.Fields[1].Value;
        Equal(Text("A"), ((FerruleScalar)items.Items[0]).Value);
        Equal(FerruleValue.FromInt64(8), ((FerruleScalar)items.Items[1]).Value);
        Equal(
            "{\n  \"Items\": [\n    \"A\",\n    8\n  ]\n}\n",
            FerruleJson.Serialize(ScalarUnionGroupJsonSchema, group));

        const string nullableUnion =
            "{\"name\":\"Value\",\"nullable\":true,\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"bool\"]}}";
        Equal(
            FerruleValue.JsonNull,
            ((FerruleScalar)FerruleJson.Parse(nullableUnion, "null")).Value);

        const long exactAboveTwoToThe53 = 9_007_199_254_740_994;
        const long inexactAboveTwoToThe53 = 9_007_199_254_740_993;
        const string floatSchema =
            "{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}";
        Equal(
            FerruleValue.FromDouble(exactAboveTwoToThe53),
            ((FerruleScalar)FerruleJson.Parse(
                floatSchema,
                exactAboveTwoToThe53.ToString(System.Globalization.CultureInfo.InvariantCulture)))
            .Value);
        Equal(
            FerruleValue.FromDouble(long.MinValue),
            ((FerruleScalar)FerruleJson.Parse(
                floatSchema,
                long.MinValue.ToString(System.Globalization.CultureInfo.InvariantCulture)))
            .Value);
        Equal(
            FerruleValue.FromDouble(9_223_372_036_854_775_808D),
            ((FerruleScalar)FerruleJson.Parse(
                floatSchema,
                "9223372036854775808"))
            .Value);
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                floatSchema,
                inexactAboveTwoToThe53.ToString(
                    System.Globalization.CultureInfo.InvariantCulture)));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                floatSchema,
                long.MaxValue.ToString(System.Globalization.CultureInfo.InvariantCulture)));

        Equal(
            "\"42\"\n",
            FerruleJson.Serialize(StringOrIntJsonSchema, Scalar(Text("42"))));
        Equal(
            "42\n",
            FerruleJson.Serialize(
                IntOrFloatJsonSchema,
                Scalar(FerruleValue.FromInt64(42))));
        Equal(
            "1.0\n",
            FerruleJson.Serialize(
                IntOrFloatJsonSchema,
                Scalar(FerruleValue.FromDouble(1.0))));
        Equal(
            $"{exactAboveTwoToThe53}\n",
            FerruleJson.Serialize(
                FloatOrBoolJsonSchema,
                Scalar(FerruleValue.FromInt64(exactAboveTwoToThe53))));
        Equal(
            "true\n",
            FerruleJson.Serialize(FloatOrBoolJsonSchema, Scalar(Text("true"))));
        Equal(
            "null\n",
            FerruleJson.Serialize(nullableUnion, Scalar(FerruleValue.JsonNull)));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                IntOrFloatJsonSchema,
                Scalar(Text("1"))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                FloatOrBoolJsonSchema,
                Scalar(FerruleValue.FromInt64(inexactAboveTwoToThe53))));

        foreach (var invalidSchema in new[]
                 {
                     "{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar_union\",\"types\":[]}}",
                     "{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\"]}}",
                     "{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"string\",\"string\"]}}",
                     "{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar_union\",\"types\":[\"int\",\"string\"]}}",
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(invalidSchema, "\"value\""));
        }
    }
}
