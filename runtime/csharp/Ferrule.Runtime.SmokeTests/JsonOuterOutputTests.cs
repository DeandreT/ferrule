using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void JsonOuterOutputBytes()
    {
        const string textSchema =
            """{"name":"Text","kind":{"kind":"scalar","ty":"string"}}""";
        const string textOutput = "\"😀\u2028\u007f\\u001f\"\n";
        var text = Scalar(Text("😀\u2028\u007f\u001f"));
        Equal(textOutput, FerruleJson.Serialize(textSchema, text));
        Equal(
            true,
            Encoding.UTF8.GetBytes(textOutput).SequenceEqual(
                FerruleJson.SerializeBytes(textSchema, text)));

        const string groupSchema =
            """{"name":"Root","kind":{"kind":"group","children":[{"name":"😀","kind":{"kind":"scalar","ty":"string"}}]}}""";
        Equal(
            "{\n  \"😀\": \"😀\"\n}\n",
            FerruleJson.Serialize(groupSchema, Group(Field("😀", Scalar(Text("😀"))))));

        const string anyInput =
            "{\"😀\":\"😀\",\"sep\":\"\\u2028\",\"ctl\":\"\\u001f\",\"arr\":[1e1,-0],\"😀\":\"second😀\"}";
        const string anyOutput =
            "{\n  \"😀\": \"second😀\",\n  \"sep\": \"\u2028\",\n  \"ctl\": \"\\u001f\",\n  \"arr\": [\n    10.0,\n    -0.0\n  ]\n}\n";
        Equal(
            anyOutput,
            FerruleJson.Serialize(JsonAnyScalarSchema, Scalar(Text(anyInput))));

        const string floatSchema =
            """{"name":"Number","kind":{"kind":"scalar","ty":"float"}}""";
        foreach (var (value, expected) in new (double Value, string Expected)[]
                 {
                     (1e-5, "0.00001\n"),
                     (1e16, "1e+16\n"),
                     (double.Epsilon, "5e-324\n"),
                     (-0.0, "-0.0\n"),
                     (double.MaxValue, "1.7976931348623157e+308\n"),
                 })
        {
            Equal(
                expected,
                FerruleJson.Serialize(floatSchema, Scalar(FerruleValue.FromDouble(value))));
        }

        const string floatArraySchema =
            """{"name":"Numbers","repeating":true,"kind":{"kind":"scalar","ty":"float"}}""";
        Equal(
            "[\n  1e-307,\n  1e+16,\n  -0.0\n]\n",
            FerruleJson.Serialize(
                floatArraySchema,
                new FerruleRepeated(new FerruleInstance[]
                {
                    Scalar(FerruleValue.FromDouble(1e-307)),
                    Scalar(FerruleValue.FromDouble(1e16)),
                    Scalar(FerruleValue.FromDouble(-0.0)),
                })));
        Equal(
            "[]\n",
            FerruleJson.Serialize(floatArraySchema, new FerruleRepeated([])));

        const string nestedSchema =
            """{"name":"Root","kind":{"kind":"group","children":[{"name":"Rows","repeating":true,"kind":{"kind":"group","children":[{"name":"Value","kind":{"kind":"scalar","ty":"float"}},{"name":"Label","kind":{"kind":"scalar","ty":"string"}}]}}]}}""";
        var rows = new FerruleRepeated(new FerruleInstance[]
        {
            Group(
                Field("Value", Scalar(FerruleValue.FromDouble(1e-307))),
                Field("Label", Scalar(Text("😀")))),
            Group(
                Field("Value", Scalar(FerruleValue.FromDouble(-0.0))),
                Field("Label", Scalar(Text("second")))),
        });
        Equal(
            "{\n  \"Rows\": [\n    {\n      \"Value\": 1e-307,\n      \"Label\": \"😀\"\n    },\n    {\n      \"Value\": -0.0,\n      \"Label\": \"second\"\n    }\n  ]\n}\n",
            FerruleJson.Serialize(nestedSchema, Group(Field("Rows", rows))));
        Equal("{}\n", FerruleJson.Serialize(
            """{"name":"Empty","kind":{"kind":"group","children":[]}}""",
            Group()));
    }
}
