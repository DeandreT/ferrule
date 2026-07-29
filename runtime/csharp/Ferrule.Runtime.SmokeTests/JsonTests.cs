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

    private static void JsonDocumentBoundaries()
    {
        var parsed = (FerruleGroup)FerruleJson.Parse(
            BasicJsonSchema,
            "\uFEFF{\"Name\":\"sample\",\"Count\":3,\"Note\":null}");
        Equal(Text("sample"), ((FerruleScalar)parsed.Fields[0].Value).Value);
        Equal(FerruleValue.FromInt64(3), ((FerruleScalar)parsed.Fields[1].Value).Value);
        Equal(FerruleValue.JsonNull, ((FerruleScalar)parsed.Fields[2].Value).Value);

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

        JsonScalarUnionBoundaries();
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
