using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
    private const string JsonParseFieldName = "json_parse_field";

    private static FerruleValue JsonParseField(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity(JsonParseFieldName, arguments, 3);
        var schema = RequireString(arguments[1], JsonParseFieldName);
        var path = RequireString(arguments[2], JsonParseFieldName);
        var input = arguments[0];
        if (input.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
        {
            return FerruleValue.Null;
        }
        if (input.Kind != FerruleValueKind.String)
        {
            throw Type(JsonParseFieldName, input);
        }

        try
        {
            FerruleJson.ValidateSchema(schema);
        }
        catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.JsonBoundary)
        {
            throw InvalidArgument(JsonParseFieldName, "schema descriptor is invalid");
        }

        var segments = ParseJsonFieldPath(path);
        FerruleInstance parsed;
        try
        {
            parsed = FerruleJson.Parse(schema, input.StringValue);
        }
        catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.JsonBoundary)
        {
            throw InvalidArgument(JsonParseFieldName, "input does not match the JSON schema");
        }

        var current = parsed;
        foreach (var segment in segments)
        {
            if (current is not FerruleGroup group ||
                !group.TryGetField(segment, out var child))
            {
                throw InvalidArgument(
                    JsonParseFieldName,
                    "field path does not resolve to a scalar");
            }
            current = child;
        }

        return current is FerruleScalar scalar
            ? scalar.Value
            : throw InvalidArgument(
                JsonParseFieldName,
                "field path does not resolve to a scalar");
    }

    private static IReadOnlyList<string> ParseJsonFieldPath(string path)
    {
        try
        {
            using var document = JsonDocument.Parse(
                path,
                new JsonDocumentOptions
                {
                    MaxDepth = FerruleJson.MaximumDepth,
                    CommentHandling = JsonCommentHandling.Disallow,
                    AllowTrailingCommas = false,
                });
            if (document.RootElement.ValueKind != JsonValueKind.Array)
            {
                throw InvalidArgument(
                    JsonParseFieldName,
                    "field path descriptor is invalid");
            }

            var segments = new List<string>();
            foreach (var segment in document.RootElement.EnumerateArray())
            {
                if (segment.ValueKind != JsonValueKind.String)
                {
                    throw InvalidArgument(
                        JsonParseFieldName,
                        "field path descriptor is invalid");
                }
                segments.Add(segment.GetString()!);
            }
            return segments;
        }
        catch (FerruleRuntimeException)
        {
            throw;
        }
        catch (JsonException)
        {
            throw InvalidArgument(
                JsonParseFieldName,
                "field path descriptor is invalid");
        }
    }
}
