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

    private const string PropertyCountJsonSchema =
        "{\"name\":\"Root\",\"property_count_range\":{\"minimum\":4,\"maximum\":5},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Id\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Nested\",\"property_count_range\":{\"minimum\":1,\"maximum\":1},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"A\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"B\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}},{\"name\":\"Rows\",\"repeating\":true,\"property_count_range\":{\"minimum\":1,\"maximum\":2},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Code\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Note\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}},{\"name\":\"Maybe\",\"container_nullable\":true,\"property_count_range\":{\"minimum\":1,\"maximum\":1},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}]}},{\"name\":\"Open\",\"property_count_range\":{\"minimum\":2,\"maximum\":3},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Fixed\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}],\"dynamic\":{\"name\":\"*\",\"json_any\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}}}],\"required\":[\"Id\",\"Nested\",\"Rows\",\"Maybe\"]}}";

    private const string PropertyDependenciesJsonSchema =
        "{\"name\":\"Root\",\"json_property_dependencies\":{\"Mode\":[\"Payload\"]},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Mode\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Payload\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Nested\",\"json_property_dependencies\":{\"Trigger\":[\"Value\"]},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Trigger\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Value\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}},{\"name\":\"Rows\",\"repeating\":true,\"json_property_dependencies\":{\"Trigger\":[\"Value\"]},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Trigger\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Value\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}},{\"name\":\"Maybe\",\"container_nullable\":true,\"json_property_dependencies\":{\"Trigger\":[\"Value\"]},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Trigger\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Value\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}}]}}";

    private const string ObjectOpennessJsonSchema =
        "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Known\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Nested\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Name\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}},{\"name\":\"Rows\",\"repeating\":true,\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Code\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}},{\"name\":\"Maybe\",\"container_nullable\":true,\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Id\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}]}},{\"name\":\"Open\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Fixed\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}],\"dynamic\":{\"name\":\"*\",\"json_any\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}}}]}}";

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

        JsonObjectOpennessBoundaries();
        JsonConstantBoundaries();
        JsonAllowedValuesBoundaries();
        JsonRangeBoundaries();
        JsonMultipleOfBoundaries();
        JsonItemCountBoundaries();
        JsonPropertyCountBoundaries();
        JsonPropertyDependencyBoundaries();
        JsonUniqueItemsBoundaries();
        JsonFormatAnnotationBoundaries();
        JsonStringLengthBoundaries();
        JsonPatternBoundaries();
        JsonScalarUnionBoundaries();
    }

    private static void JsonObjectOpennessBoundaries()
    {
        var firstUnexpected = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                BasicJsonSchema,
                "{\"Name\":\"sample\",\"unexpected\":1,\"later\":2}"));
        Equal(
            true,
            firstUnexpected.Message.Contains(
                "object 'Root' does not allow property 'unexpected'",
                StringComparison.Ordinal));
        Equal(false, firstUnexpected.Message.Contains("'later'", StringComparison.Ordinal));

        const string valid =
            "{\"Known\":\"root\",\"Nested\":{\"Name\":\"nested\"},\"Rows\":[{\"Code\":\"A\"},{\"Code\":\"B\"}],\"Maybe\":null,\"Open\":{\"Fixed\":\"declared\",\"extra\":{\"nested\":[1,true,null]}}}";
        var parsed = FerruleJson.Parse(ObjectOpennessJsonSchema, valid);
        Equal(
            "{\n  \"Known\": \"root\",\n  \"Nested\": {\n    \"Name\": \"nested\"\n  },\n  \"Rows\": [\n    {\n      \"Code\": \"A\"\n    },\n    {\n      \"Code\": \"B\"\n    }\n  ],\n  \"Maybe\": null,\n  \"Open\": {\n    \"Fixed\": \"declared\",\n    \"extra\": {\n      \"nested\": [\n        1,\n        true,\n        null\n      ]\n    }\n  }\n}\n",
            FerruleJson.Serialize(ObjectOpennessJsonSchema, parsed));

        foreach (var (input, objectName, propertyName) in new[]
                 {
                     (
                         "{\"Known\":\"root\",\"Nested\":{\"Name\":\"nested\",\"extra\":1},\"Rows\":[],\"Maybe\":null,\"Open\":{}}",
                         "Nested",
                         "extra"),
                     (
                         "{\"Known\":\"root\",\"Nested\":{},\"Rows\":[{\"Code\":\"A\",\"extra\":1}],\"Maybe\":null,\"Open\":{}}",
                         "Rows",
                         "extra"),
                     (
                         "{\"Known\":\"root\",\"Nested\":{},\"Rows\":[],\"Maybe\":{\"Id\":1,\"extra\":1},\"Open\":{}}",
                         "Maybe",
                         "extra"),
                 })
        {
            var error = Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(ObjectOpennessJsonSchema, input));
            Equal(
                true,
                error.Message.Contains(
                    $"object '{objectName}' does not allow property '{propertyName}'",
                    StringComparison.Ordinal));
        }
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

    private static void JsonPropertyCountBoundaries()
    {
        const string valid =
            "{\"Id\":\"first\",\"Id\":\"last\",\"Nested\":{\"A\":\"one\"},\"Rows\":[{\"Code\":\"A\"},{\"Code\":\"B\",\"Note\":null}],\"Maybe\":null,\"Open\":{\"Fixed\":\"declared\",\"extra\":{\"nested\":true}}}";
        var parsed = FerruleJson.Parse(PropertyCountJsonSchema, valid);
        Equal(
            "{\n  \"Id\": \"last\",\n  \"Nested\": {\n    \"A\": \"one\"\n  },\n  \"Rows\": [\n    {\n      \"Code\": \"A\"\n    },\n    {\n      \"Code\": \"B\",\n      \"Note\": null\n    }\n  ],\n  \"Maybe\": null,\n  \"Open\": {\n    \"Fixed\": \"declared\",\n    \"extra\": {\n      \"nested\": true\n    }\n  }\n}\n",
            FerruleJson.Serialize(PropertyCountJsonSchema, parsed));

        var countBeforeOpenness = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                PropertyCountJsonSchema,
                "{\"Id\":\"A\",\"Nested\":{\"A\":\"one\"},\"Rows\":[],\"Maybe\":null,\"Open\":{\"Fixed\":\"known\",\"extra\":1},\"unexpected\":true}"));
        Equal(
            true,
            countBeforeOpenness.Message.Contains(
                "object 'Root' has 6 properties",
                StringComparison.Ordinal));
        Equal(
            false,
            countBeforeOpenness.Message.Contains(
                "does not allow property",
                StringComparison.Ordinal));

        foreach (var input in new[]
                 {
                     "{\"Id\":\"A\",\"Nested\":{},\"Rows\":[],\"Maybe\":null}",
                     "{\"Id\":\"A\",\"Nested\":{\"A\":\"one\"},\"Rows\":[{}],\"Maybe\":null}",
                     "{\"Id\":\"A\",\"Nested\":{\"A\":\"one\"},\"Rows\":[{\"Code\":\"A\",\"Note\":\"B\",\"third\":true}],\"Maybe\":null}",
                     "{\"Id\":\"A\",\"Nested\":{\"A\":\"one\"},\"Rows\":[],\"Maybe\":{},\"Open\":{\"Fixed\":\"known\",\"extra\":1}}",
                     "{\"Id\":\"A\",\"Nested\":{\"A\":\"one\"},\"Rows\":[],\"Maybe\":null,\"Open\":{\"Fixed\":\"known\"}}",
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(PropertyCountJsonSchema, input));
        }

        Equal(
            "{\n  \"Id\": \"A\",\n  \"Nested\": {\n    \"A\": \"one\"\n  },\n  \"Rows\": [],\n  \"Maybe\": null\n}\n",
            FerruleJson.Serialize(
                PropertyCountJsonSchema,
                Group(
                    Field("Id", Scalar(Text("A"))),
                    Field(
                        "Nested",
                        Group(
                            Field("A", Scalar(Text("one"))),
                            Field("B", Scalar(FerruleValue.Null)))),
                    Field("Rows", Repeated()),
                    Field("Maybe", Scalar(FerruleValue.JsonNull)))));
        var outputRange = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                PropertyCountJsonSchema,
                Group(
                    Field("Id", Scalar(Text("A"))),
                    Field("Nested", Group(Field("A", Scalar(Text("one"))))),
                    Field("Rows", Repeated()),
                    Field("Maybe", Scalar(FerruleValue.JsonNull)),
                    Field("Open", Group(Field("Fixed", Scalar(Text("only one"))))))));
        Equal(
            true,
            outputRange.Message.Contains(
                "object 'Open' has 1 properties",
                StringComparison.Ordinal));

        foreach (var invalidSchema in new[]
                 {
                     "{\"name\":\"Value\",\"property_count_range\":{\"minimum\":1},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"property_count_range\":{},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"property_count_range\":{\"minimum\":2,\"maximum\":1},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"property_count_range\":{\"minimum\":-1},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"property_count_range\":{\"minimum\":1.0},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"property_count_range\":{\"minimum\":18446744073709551616},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"property_count_range\":{\"minimum\":1,\"maximim\":2},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"property_count_range\":{\"minimum\":1,\"minimum\":2},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"property_count_range\":{\"minimum\":2},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Only\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}}",
                     "{\"name\":\"Value\",\"property_count_range\":{\"maximum\":1},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"A\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"B\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}],\"required\":[\"A\",\"B\"]}}",
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(invalidSchema, "{}"));
        }
    }

    private static void JsonPropertyDependencyBoundaries()
    {
        const string valid =
            "{\"Mode\":null,\"Payload\":null,\"Nested\":{\"Trigger\":null,\"Value\":null},\"Rows\":[{\"Trigger\":\"A\",\"Value\":\"B\"},{}],\"Maybe\":null}";
        var parsed = FerruleJson.Parse(PropertyDependenciesJsonSchema, valid);
        Equal(
            "{\n  \"Mode\": null,\n  \"Payload\": null,\n  \"Nested\": {\n    \"Trigger\": null,\n    \"Value\": null\n  },\n  \"Rows\": [\n    {\n      \"Trigger\": \"A\",\n      \"Value\": \"B\"\n    },\n    {}\n  ],\n  \"Maybe\": null\n}\n",
            FerruleJson.Serialize(PropertyDependenciesJsonSchema, parsed));

        foreach (var input in new[]
                 {
                     "{\"Mode\":null}",
                     "{\"Nested\":{\"Trigger\":null},\"Rows\":[]}",
                     "{\"Nested\":{},\"Rows\":[{\"Trigger\":\"A\"}]}",
                     "{\"Nested\":{},\"Rows\":[],\"Maybe\":{\"Trigger\":\"A\"}}",
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(PropertyDependenciesJsonSchema, input));
        }

        Equal(
            "{\n  \"Mode\": null,\n  \"Payload\": null\n}\n",
            FerruleJson.Serialize(
                PropertyDependenciesJsonSchema,
                Group(
                    Field("Mode", Scalar(FerruleValue.JsonNull)),
                    Field("Payload", Scalar(FerruleValue.JsonNull)))));
        var omittedOutput = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                PropertyDependenciesJsonSchema,
                Group(
                    Field("Mode", Scalar(Text("active"))),
                    Field("Payload", Scalar(FerruleValue.Null)))));
        Equal(
            true,
            omittedOutput.Message.Contains(
                "property 'Mode' requires property 'Payload'",
                StringComparison.Ordinal));

        const string emptyNames =
            "{\"name\":\"Value\",\"json_property_dependencies\":{\"\": [\"Required\"]},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Required\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}}";
        _ = FerruleJson.Parse(emptyNames, "{\"\":null,\"Required\":null}");
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(emptyNames, "{\"\":null}"));

        foreach (var invalidSchema in new[]
                 {
                     "{\"name\":\"Value\",\"json_property_dependencies\":{\"A\":[\"B\"]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_property_dependencies\":null,\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"json_property_dependencies\":[],\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"json_property_dependencies\":{},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"json_property_dependencies\":{\"A\":[]},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"json_property_dependencies\":{\"A\":[1]},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"json_property_dependencies\":{\"\": [\"\"]},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"json_property_dependencies\":{\"A\":[\"A\"]},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"json_property_dependencies\":{\"A\":[\"B\",\"B\"]},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"json_property_dependencies\":{\"A\":[\"C\",\"B\"]},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"json_property_dependencies\":{\"B\":[\"C\"],\"A\":[\"C\"]},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"json_property_dependencies\":{\"A\":[\"B\"],\"A\":[\"C\"]},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"json_property_dependencies\":{\"A\":[\"B\"]},\"json_property_dependencies\":{\"A\":[\"B\"]},\"kind\":{\"kind\":\"group\",\"children\":[]}}",
                     "{\"name\":\"Value\",\"property_count_range\":{\"maximum\":1},\"json_property_dependencies\":{\"A\":[\"B\"]},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"A\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"B\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}],\"required\":[\"A\"]}}",
                     "{\"name\":\"Value\",\"json_property_dependencies\":{\"A\":[\"Missing\"]},\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"A\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}],\"required\":[\"A\"]}}",
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(invalidSchema, "{}"));
        }

        var tooManyTriggers = string.Join(
            ",",
            Enumerable.Range(0, 257).Select(index =>
                $"\"t{index:D3}\":[\"value\"]"));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                $"{{\"name\":\"Value\",\"json_property_dependencies\":{{{tooManyTriggers}}},\"kind\":{{\"kind\":\"group\",\"children\":[],\"dynamic\":{{\"name\":\"*\",\"json_any\":true,\"kind\":{{\"kind\":\"scalar\",\"ty\":\"string\"}}}}}}}}",
                "{}"));

        var tooManyEdges = string.Join(
            ",",
            Enumerable.Range(0, 4097).Select(index => $"\"v{index:D4}\""));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                $"{{\"name\":\"Value\",\"json_property_dependencies\":{{\"trigger\":[{tooManyEdges}]}},\"kind\":{{\"kind\":\"group\",\"children\":[],\"dynamic\":{{\"name\":\"*\",\"json_any\":true,\"kind\":{{\"kind\":\"scalar\",\"ty\":\"string\"}}}}}}}}",
                "{}"));

        var oversizedName = new string('x', 256 * 1024);
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                $"{{\"name\":\"Value\",\"json_property_dependencies\":{{\"a\":[\"{oversizedName}\"]}},\"kind\":{{\"kind\":\"group\",\"children\":[],\"dynamic\":{{\"name\":\"*\",\"json_any\":true,\"kind\":{{\"kind\":\"scalar\",\"ty\":\"string\"}}}}}}}}",
                "{}"));
    }

    private static void JsonUniqueItemsBoundaries()
    {
        const string numbers =
            "{\"name\":\"Numbers\",\"repeating\":true,\"json_unique_items\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}";
        var parsed = FerruleJson.Parse(numbers, "[1,2.5,-0]");
        Equal(
            "[\n  1.0,\n  2.5,\n  0.0\n]\n",
            FerruleJson.Serialize(numbers, parsed));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(numbers, "[1,1.0]"));
        var signedZero = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(numbers, "[-0,0.0]"));
        Equal(
            true,
            signedZero.Message.Contains("uniqueItems", StringComparison.Ordinal));
        var exactDecimals = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                numbers,
                "[0.100000000000000000000000000001,100000000000000000000000000001e-30]"));
        Equal(
            true,
            exactDecimals.Message.Contains("uniqueItems", StringComparison.Ordinal));
        var maximumExponent = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                numbers,
                "[1e9223372036854775807,10e9223372036854775806]"));
        Equal(
            true,
            maximumExponent.Message.Contains("uniqueItems", StringComparison.Ordinal));

        const string records =
            "{\"name\":\"Records\",\"repeating\":true,\"json_unique_items\":true,\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Code\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}},{\"name\":\"Text\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}}";
        var reorderedObject = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                records,
                "[{\"Code\":1,\"Text\":\"same\"},{\"Text\":\"same\",\"Code\":1.0}]"));
        Equal(
            true,
            reorderedObject.Message.Contains("uniqueItems", StringComparison.Ordinal));

        const string nestedArrays =
            "{\"name\":\"Rows\",\"repeating\":true,\"json_unique_items\":true,\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Values\",\"repeating\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}]}}";
        var orderedArrays = FerruleJson.Parse(
            nestedArrays,
            "[{\"Values\":[1,2]},{\"Values\":[2,1]}]");
        Equal(
            "[\n  {\n    \"Values\": [\n      1,\n      2\n    ]\n  },\n  {\n    \"Values\": [\n      2,\n      1\n    ]\n  }\n]\n",
            FerruleJson.Serialize(nestedArrays, orderedArrays));

        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                numbers,
                Repeated(
                    Scalar(FerruleValue.FromInt64(1)),
                    Scalar(FerruleValue.FromDouble(1.0)))));

        const string strings =
            "{\"name\":\"Strings\",\"repeating\":true,\"json_unique_items\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";
        var caseSensitive = FerruleJson.Parse(strings, "[\"A\",\"a\"]");
        Equal("[\n  \"A\",\n  \"a\"\n]\n", FerruleJson.Serialize(strings, caseSensitive));

        foreach (var invalidSchema in new[]
                 {
                     "{\"name\":\"Value\",\"json_unique_items\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"repeating\":true,\"json_unique_items\":null,\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"repeating\":true,\"json_unique_items\":\"yes\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"repeating\":true,\"json_unique_items\":true,\"json_unique_items\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(invalidSchema, "[]"));
        }
        Equal(
            FerruleValue.FromInt64(1),
            ((FerruleScalar)FerruleJson.Parse(
                "{\"name\":\"Value\",\"json_unique_items\":false,\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                "1")).Value);
    }

    private static void JsonMultipleOfBoundaries()
    {
        const string tenth =
            "{\"name\":\"Value\",\"nullable\":true,\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1,\"decimal_exponent\":-1}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}";
        foreach (var input in new[] { "0.3", "-0.3", "0", "-0.0", "1" })
        {
            _ = FerruleJson.Parse(tenth, input);
        }
        foreach (var input in new[] { "0.30000000000000004", "-0.31" })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(tenth, input));
        }
        Equal(
            FerruleValue.JsonNull,
            ((FerruleScalar)FerruleJson.Parse(tenth, "null")).Value);

        Equal(
            "0.3\n",
            FerruleJson.Serialize(
                tenth,
                Scalar(FerruleValue.FromDouble(0.3))));
        Equal(
            "0.3\n",
            FerruleJson.Serialize(tenth, Scalar(Text("0.3"))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                tenth,
                Scalar(FerruleValue.FromDouble(0.30000000000000004))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                tenth,
                Scalar(Text("0.30000000000000004"))));

        const string thirds =
            "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":3,\"decimal_exponent\":0}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}";
        foreach (var input in new[] { "6", "-6", "0" })
        {
            _ = FerruleJson.Parse(thirds, input);
        }
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(thirds, "7"));
        Equal(
            "6\n",
            FerruleJson.Serialize(thirds, Scalar(Text("6"))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(thirds, Scalar(Text("7"))));

        const string large =
            "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1,\"decimal_exponent\":20}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}";
        _ = FerruleJson.Parse(large, "1e21");
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(large, "1.0000000000000001e21"));

        const string small =
            "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1,\"decimal_exponent\":-7}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}";
        _ = FerruleJson.Parse(small, "1e-7");

        const string smallestSubnormal =
            "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":5,\"decimal_exponent\":-324}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}";
        _ = FerruleJson.Parse(smallestSubnormal, "5e-324");

        const string sixOrFive =
            "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":2,\"decimal_exponent\":0},{\"coefficient\":3,\"decimal_exponent\":0}],[{\"coefficient\":5,\"decimal_exponent\":0}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}";
        _ = FerruleJson.Parse(sixOrFive, "6");
        _ = FerruleJson.Parse(sixOrFive, "10");
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(sixOrFive, "4"));

        foreach (var invalidSchema in new[]
                 {
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":0,\"decimal_exponent\":0}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":10,\"decimal_exponent\":0}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1,\"decimal_exponent\":32768}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1,\"decimal_exponent\":309}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1,\"decimal_exponent\":-325}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1,\"decimal_exponent\":0},{\"coefficient\":1,\"decimal_exponent\":0}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1,\"decimal_exponent\":0}],[{\"coefficient\":1,\"decimal_exponent\":0}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1,\"decimal_exponent\":0,\"extra\":true}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1,\"decimal_exponent\":0}]],\"extra\":true},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1,\"decimal_exponent\":0}]]},\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":2,\"decimal_exponent\":0}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1,\"decimal_exponent\":0}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_any\":true,\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":1,\"decimal_exponent\":0}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                     "{\"name\":\"Value\",\"fixed\":\"0.3\",\"json_multiple_of\":{\"any_of\":[[{\"coefficient\":2,\"decimal_exponent\":-1}]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"float\"}}",
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(invalidSchema, "0"));
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
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(oneOrTwo, "\"\\uD800\""));

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

    private static void JsonPatternBoundaries()
    {
        var dnf = JsonPatternSchema(
            "{\"kind\":\"scalar\",\"ty\":\"string\"}",
            [["^A", "Z$"], ["^B$"]]);
        Equal(Text("ABZ"), ((FerruleScalar)FerruleJson.Parse(dnf, "\"ABZ\"")).Value);
        Equal(Text("B"), ((FerruleScalar)FerruleJson.Parse(dnf, "\"B\"")).Value);
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(dnf, "\"AX\""));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(dnf, Scalar(Text("other"))));

        var astral = JsonPatternSchema(
            "{\"kind\":\"scalar\",\"ty\":\"string\"}",
            [["^\\u{1F600}$"]]);
        Equal(
            Text("\U0001F600"),
            ((FerruleScalar)FerruleJson.Parse(astral, "\"\\uD83D\\uDE00\"")).Value);

        var dot = JsonPatternSchema(
            "{\"kind\":\"scalar\",\"ty\":\"string\"}",
            [["^.$"]]);
        Equal(Text("x"), ((FerruleScalar)FerruleJson.Parse(dot, "\"x\"")).Value);
        Equal(
            Text("\U0001F600"),
            ((FerruleScalar)FerruleJson.Parse(dot, "\"\\uD83D\\uDE00\"")).Value);
        foreach (var lineTerminator in new[] { "\n", "\r", "\u2028", "\u2029" })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(
                    dot,
                    System.Text.Json.JsonSerializer.Serialize(lineTerminator)));
        }

        var final = JsonPatternSchema(
            "{\"kind\":\"scalar\",\"ty\":\"string\"}",
            [["x$"]]);
        foreach (var value in new[] { "x", "x\n", "x\r", "x\r\n", "x\u2028", "x\u2029" })
        {
            Equal(
                Text(value),
                ((FerruleScalar)FerruleJson.Parse(
                    final,
                    System.Text.Json.JsonSerializer.Serialize(value))).Value);
        }
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(final, "\"x\\n\\n\""));

        var emptyClass = JsonPatternSchema(
            "{\"kind\":\"scalar\",\"ty\":\"string\"}",
            [["[]"]]);
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(emptyClass, "\"x\""));
        var complementedEmptyClass = JsonPatternSchema(
            "{\"kind\":\"scalar\",\"ty\":\"string\"}",
            [["[^]"]]);
        Equal(
            Text("\U0001F600"),
            ((FerruleScalar)FerruleJson.Parse(
                complementedEmptyClass,
                "\"\\uD83D\\uDE00\"")).Value);
        var groupedAssertion = JsonPatternSchema(
            "{\"kind\":\"scalar\",\"ty\":\"string\"}",
            [["(^)*a"]]);
        Equal(
            Text("a"),
            ((FerruleScalar)FerruleJson.Parse(groupedAssertion, "\"a\"")).Value);

        var normalizedString = JsonPatternSchema(
            "{\"kind\":\"scalar\",\"ty\":\"string\"}",
            [["^true$"]]);
        Equal(
            "\"true\"\n",
            FerruleJson.Serialize(
                normalizedString,
                Scalar(FerruleValue.FromBoolean(true))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(
                normalizedString,
                Scalar(FerruleValue.FromBoolean(false))));

        foreach (var (value, expected, pattern) in new[]
                 {
                     (1.0, "1", "^1$"),
                     (-0.0, "-0", "^-0$"),
                     (1e20, "100000000000000000000", "^100000000000000000000$"),
                     (1e-7, "0.0000001", "^0\\.0000001$"),
                 })
        {
            var normalizedFloat = JsonPatternSchema(
                "{\"kind\":\"scalar\",\"ty\":\"string\"}",
                [[pattern]]);
            Equal(
                System.Text.Json.JsonSerializer.Serialize(expected) + "\n",
                FerruleJson.Serialize(
                    normalizedFloat,
                    Scalar(FerruleValue.FromDouble(value))));
        }

        var scalarUnion = JsonPatternSchema(
            "{\"kind\":\"scalar_union\",\"types\":[\"string\",\"int\"]}",
            [["^A$"]]);
        Equal(
            FerruleValue.FromInt64(7),
            ((FerruleScalar)FerruleJson.Parse(scalarUnion, "7")).Value);
        Equal(
            "7\n",
            FerruleJson.Serialize(
                scalarUnion,
                Scalar(FerruleValue.FromInt64(7))));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(scalarUnion, "\"B\""));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Serialize(scalarUnion, Scalar(Text("B"))));

        foreach (var invalidSchema in new[]
                 {
                     "{\"name\":\"Value\",\"json_patterns\":{},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_patterns\":{\"any_of\":[],\"extra\":true},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_patterns\":{\"any_of\":[[]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_patterns\":{\"any_of\":[[\"A\",\"A\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_patterns\":{\"any_of\":[[\"A\"],[\"A\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_patterns\":{\"any_of\":[[\"\",\"A\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_patterns\":{\"any_of\":[[\"\"],[\"A\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_patterns\":{\"any_of\":[[7]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_patterns\":{\"any_of\":[[\"\\\\d\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_patterns\":{\"any_of\":[[\"[a-b-c]\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_patterns\":{\"any_of\":[[\"[[]\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_patterns\":{\"any_of\":[[\"A\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}",
                     "{\"name\":\"Value\",\"json_any\":true,\"json_patterns\":{\"any_of\":[[\"A\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"fixed\":\"B\",\"json_patterns\":{\"any_of\":[[\"^A$\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_patterns\":{\"any_of\":[[\"A\"]],\"any_of\":[[\"B\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                     "{\"name\":\"Value\",\"json_patterns\":{\"any_of\":[[\"A\"]]},\"json_patterns\":{\"any_of\":[[\"B\"]]},\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}",
                 })
        {
            Error(
                FerruleRuntimeError.JsonBoundary,
                () => FerruleJson.Parse(invalidSchema, "\"A\""));
        }

        var tooManyAlternatives = Enumerable.Range(0, 33)
            .Select(index => new[] { $"^{index}$" })
            .ToArray();
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                JsonPatternSchema(
                    "{\"kind\":\"scalar\",\"ty\":\"string\"}",
                    tooManyAlternatives),
                "\"0\""));
        var tooManyTerms = new[]
        {
            Enumerable.Range(0, 65).Select(index => $"term-{index}").ToArray(),
        };
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                JsonPatternSchema(
                    "{\"kind\":\"scalar\",\"ty\":\"string\"}",
                    tooManyTerms),
                "\"term-0\""));

        var distinctOverflow = Enumerable.Range(0, 65)
            .Select(index => ($"field-{index}", $"^value-{index}$"));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                JsonPatternGroupSchema(distinctOverflow),
                "{}"));
        var sharedPattern = Enumerable.Range(0, 65)
            .Select(index => ($"field-{index}", "^shared$"));
        _ = FerruleJson.Parse(JsonPatternGroupSchema(sharedPattern), "{}");
        var sourceOverflow = new[] { 'b', 'c', 'd', 'e', 'f' }
            .Select((marker, index) =>
                ($"field-{index}", $"[{"a".PadRight(60_000, 'a')}{marker}]"));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                JsonPatternGroupSchema(sourceOverflow),
                "{}"));
        var instructionOverflow = Enumerable.Range(0, 14)
            .Select(index => ($"field-{index}", $"{new string('a', 5_000)}{index}"));
        Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                JsonPatternGroupSchema(instructionOverflow),
                "{}"));

        var expensivePattern = new string('a', 1_000);
        var expensiveText = new string('a', 60_000);
        var expensiveValue =
            System.Text.Json.JsonSerializer.Serialize(expensiveText);
        var sharedBudgetSchema = JsonPatternGroupSchema(
            new[]
            {
                ("First", expensivePattern),
                ("Second", expensivePattern),
            });
        var sharedBudgetError = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(
                sharedBudgetSchema,
                $"{{\"First\":{expensiveValue},\"Second\":{expensiveValue}}}"));
        Equal(
            true,
            sharedBudgetError.Message.Contains("work units", StringComparison.Ordinal));

        var fixedChild = (string name) =>
            $"{{\"name\":{System.Text.Json.JsonSerializer.Serialize(name)}," +
            $"\"fixed\":{expensiveValue}," +
            $"\"json_patterns\":{{\"any_of\":[[{System.Text.Json.JsonSerializer.Serialize(expensivePattern)}]]}}," +
            "\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}";
        var fixedBudgetSchema =
            $"{{\"name\":\"Root\",\"kind\":{{\"kind\":\"group\",\"children\":[" +
            $"{fixedChild("First")},{fixedChild("Second")}]}}}}";
        var fixedBudgetError = Error(
            FerruleRuntimeError.JsonBoundary,
            () => FerruleJson.Parse(fixedBudgetSchema, "{}"));
        Equal(
            true,
            fixedBudgetError.Message.Contains("work units", StringComparison.Ordinal));
    }

    private static string JsonPatternSchema(
        string kind,
        IEnumerable<IEnumerable<string>> alternatives,
        bool jsonAny = false) =>
        $"{{\"name\":\"Value\",\"json_any\":{jsonAny.ToString().ToLowerInvariant()}," +
        $"\"json_patterns\":{{\"any_of\":{System.Text.Json.JsonSerializer.Serialize(alternatives)}}}," +
        $"\"kind\":{kind}}}";

    private static string JsonPatternGroupSchema(
        IEnumerable<(string Name, string Pattern)> children)
    {
        var childJson = children.Select(child =>
            $"{{\"name\":{System.Text.Json.JsonSerializer.Serialize(child.Name)}," +
            $"\"json_patterns\":{{\"any_of\":[[{System.Text.Json.JsonSerializer.Serialize(child.Pattern)}]]}}," +
            "\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}");
        return
            $"{{\"name\":\"Root\",\"kind\":{{\"kind\":\"group\",\"children\":[{string.Join(",", childJson)}]}}}}";
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
