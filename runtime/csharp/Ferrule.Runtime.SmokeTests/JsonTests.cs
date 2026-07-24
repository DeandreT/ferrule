using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string BasicJsonSchema =
        "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Name\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Count\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}},{\"name\":\"Note\",\"nullable\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}}";

    private const string AlternativeJsonSchema =
        "{\"name\":\"Choice\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Type\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Text\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}},{\"name\":\"Count\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}],\"alternatives\":[{\"members\":[\"Type\",\"Text\"],\"required\":[\"Type\",\"Text\"],\"constraints\":[{\"member\":\"Type\",\"value\":{\"type\":\"string\",\"value\":\"text\"}}]},{\"members\":[\"Type\",\"Count\"],\"required\":[\"Type\",\"Count\"],\"constraints\":[{\"member\":\"Type\",\"value\":{\"type\":\"string\",\"value\":\"count\"}}]}]}}";

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
    }
}
