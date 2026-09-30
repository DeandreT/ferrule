using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void JsonSyntaxDepthBoundaries()
    {
        foreach (var objectShape in new[] { false, true })
        {
            static string Nest(string value, int depth, bool objects)
            {
                for (var level = 0; level < depth; level++)
                {
                    value = objects ? "{\"value\":" + value + "}" : "[" + value + "]";
                }
                return value;
            }
            var accepted = Nest("0", 127, objectShape);
            _ = FerruleJson.Parse(JsonAnyScalarSchema, accepted);
            using var output = JsonDocument.Parse(
                FerruleJson.Serialize(JsonAnyScalarSchema, Scalar(Text(accepted))),
                new JsonDocumentOptions { MaxDepth = 256 });
            Equal(
                objectShape ? JsonValueKind.Object : JsonValueKind.Array,
                output.RootElement.ValueKind);

            foreach (var depth in new[] { 128, 129 })
            {
                var rejected = Nest("0", depth, objectShape);
                Error(
                    FerruleRuntimeError.JsonBoundary,
                    () => FerruleJson.Parse(JsonAnyScalarSchema, rejected));
                using var fallback = JsonDocument.Parse(
                    FerruleJson.Serialize(JsonAnyScalarSchema, Scalar(Text(rejected))));
                Equal(JsonValueKind.String, fallback.RootElement.ValueKind);
                Equal(rejected, fallback.RootElement.GetString());
            }
        }

        static string Schema(int wrappers)
        {
            var value = "{\"name\":\"Leaf\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"int\"}}";
            for (var depth = 0; depth < wrappers; depth++)
            {
                value = "{\"name\":\"Group\",\"kind\":{\"kind\":\"group\",\"children\":[" + value + "]}}";
            }
            return value;
        }
        _ = FerruleJson.Parse(Schema(41), "{}");
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.Parse(Schema(42), "{}"));
    }
}
