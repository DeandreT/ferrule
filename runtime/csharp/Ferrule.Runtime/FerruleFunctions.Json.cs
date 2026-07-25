using System.Buffers;
using System.Globalization;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
    private const string JsonParseFieldName = "json_parse_field";
    private const string JsonSerializeObjectName = "json_serialize_object";

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

        var segments = ParseJsonStringArray(
            path,
            JsonParseFieldName,
            "field path descriptor is invalid");
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

    private static FerruleValue JsonSerializeObject(IReadOnlyList<FerruleValue> arguments)
    {
        if (arguments.Count == 0 || arguments.Count % 3 != 0)
        {
            throw InvalidArgument(
                JsonSerializeObjectName,
                "expected path, scalar type, and value triples");
        }

        var root = new ConstructedJsonObject();
        for (var index = 0; index < arguments.Count; index += 3)
        {
            if (arguments[index].Kind != FerruleValueKind.String ||
                arguments[index + 1].Kind != FerruleValueKind.String)
            {
                throw InvalidArgument(
                    JsonSerializeObjectName,
                    "paths and scalar types must be strings");
            }

            var value = arguments[index + 2];
            if (value.Kind == FerruleValueKind.Null)
            {
                continue;
            }

            var path = ParseJsonStringArray(
                arguments[index].StringValue,
                JsonSerializeObjectName,
                "path descriptors must be JSON string arrays");
            if (path.Count == 0)
            {
                throw InvalidArgument(
                    JsonSerializeObjectName,
                    "property paths cannot be empty");
            }

            InsertJsonProperty(
                root,
                path,
                JsonScalarValue(arguments[index + 1].StringValue, value));
        }

        try
        {
            var buffer = new ArrayBufferWriter<byte>();
            using (var writer = new Utf8JsonWriter(
                       buffer,
                       new JsonWriterOptions
                       {
                           Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
                           MaxDepth = FerruleJson.MaximumDepth,
                           SkipValidation = false,
                       }))
            {
                WriteJsonObject(writer, root);
            }
            return FerruleValue.FromString(Encoding.UTF8.GetString(buffer.WrittenSpan));
        }
        catch (Exception error) when (
            error is JsonException or InvalidOperationException or OverflowException)
        {
            throw InvalidArgument(
                JsonSerializeObjectName,
                "constructed object could not be serialized");
        }
    }

    private static ConstructedJsonScalar JsonScalarValue(
        string scalarType,
        FerruleValue value)
    {
        if (value.Kind == FerruleValueKind.JsonNull &&
            scalarType is "string" or "integer" or "number" or "boolean")
        {
            return new ConstructedJsonScalar(FerruleValue.JsonNull);
        }

        return scalarType switch
        {
            "string" => value.Kind switch
            {
                FerruleValueKind.String => new ConstructedJsonScalar(value),
                FerruleValueKind.Bool or FerruleValueKind.Int64 =>
                    new ConstructedJsonScalar(FerruleValue.FromString(ScalarText(value))),
                FerruleValueKind.Double when double.IsFinite(value.DoubleValue) =>
                    new ConstructedJsonScalar(FerruleValue.FromString(ScalarText(value))),
                _ => throw Type(JsonSerializeObjectName, value),
            },
            "integer" => value.Kind switch
            {
                FerruleValueKind.Int64 => new ConstructedJsonScalar(value),
                FerruleValueKind.Double
                    when double.IsFinite(value.DoubleValue) &&
                         Math.Truncate(value.DoubleValue) == value.DoubleValue &&
                         value.DoubleValue >= long.MinValue &&
                         value.DoubleValue < 9223372036854775808.0 =>
                    new ConstructedJsonScalar(
                        FerruleValue.FromInt64((long)value.DoubleValue)),
                FerruleValueKind.String
                    when long.TryParse(
                        TrimRustWhitespace(value.StringValue),
                        NumberStyles.AllowLeadingSign,
                        CultureInfo.InvariantCulture,
                        out var integer) =>
                    new ConstructedJsonScalar(FerruleValue.FromInt64(integer)),
                _ => throw Type(JsonSerializeObjectName, value),
            },
            "number" => value.Kind switch
            {
                FerruleValueKind.Int64 => new ConstructedJsonScalar(value),
                FerruleValueKind.Double when double.IsFinite(value.DoubleValue) =>
                    new ConstructedJsonScalar(value),
                FerruleValueKind.String
                    when TryFiniteDouble(value.StringValue, out var number) =>
                    new ConstructedJsonScalar(FerruleValue.FromDouble(number)),
                _ => throw Type(JsonSerializeObjectName, value),
            },
            "boolean" => value.Kind switch
            {
                FerruleValueKind.Bool => new ConstructedJsonScalar(value),
                FerruleValueKind.String
                    when TrimRustWhitespace(value.StringValue) is "true" or "1" =>
                    new ConstructedJsonScalar(FerruleValue.FromBoolean(true)),
                FerruleValueKind.String
                    when TrimRustWhitespace(value.StringValue) is "false" or "0" =>
                    new ConstructedJsonScalar(FerruleValue.FromBoolean(false)),
                _ => throw Type(JsonSerializeObjectName, value),
            },
            _ => throw Type(JsonSerializeObjectName, value),
        };
    }

    private static void InsertJsonProperty(
        ConstructedJsonObject root,
        IReadOnlyList<string> path,
        ConstructedJsonScalar value)
    {
        var current = root;
        for (var index = 0; index < path.Count - 1; index++)
        {
            var name = path[index];
            if (!current.Properties.TryGetValue(name, out var existing))
            {
                var child = new ConstructedJsonObject();
                current.Properties.Add(name, child);
                current = child;
                continue;
            }
            if (existing is not ConstructedJsonObject existingObject)
            {
                throw InvalidArgument(
                    JsonSerializeObjectName,
                    "property path conflicts with a scalar property");
            }
            current = existingObject;
        }

        if (!current.Properties.TryAdd(path[^1], value))
        {
            throw InvalidArgument(
                JsonSerializeObjectName,
                "property paths must be unique");
        }
    }

    private static void WriteJsonObject(Utf8JsonWriter writer, ConstructedJsonObject value)
    {
        writer.WriteStartObject();
        foreach (var (name, child) in value.Properties)
        {
            writer.WritePropertyName(name);
            switch (child)
            {
                case ConstructedJsonObject nested:
                    WriteJsonObject(writer, nested);
                    break;
                case ConstructedJsonScalar scalar:
                    WriteJsonScalar(writer, scalar.Value);
                    break;
            }
        }
        writer.WriteEndObject();
    }

    private static void WriteJsonScalar(Utf8JsonWriter writer, FerruleValue value)
    {
        switch (value.Kind)
        {
            case FerruleValueKind.JsonNull:
                writer.WriteNullValue();
                break;
            case FerruleValueKind.String:
                writer.WriteStringValue(value.StringValue);
                break;
            case FerruleValueKind.Bool:
                writer.WriteBooleanValue(value.BooleanValue);
                break;
            case FerruleValueKind.Int64:
                writer.WriteNumberValue(value.Int64Value);
                break;
            case FerruleValueKind.Double:
                writer.WriteNumberValue(value.DoubleValue);
                break;
        }
    }

    private static IReadOnlyList<string> ParseJsonStringArray(
        string serialized,
        string function,
        string invalidDetail)
    {
        try
        {
            using var document = JsonDocument.Parse(
                serialized,
                new JsonDocumentOptions
                {
                    MaxDepth = FerruleJson.MaximumDepth,
                    CommentHandling = JsonCommentHandling.Disallow,
                    AllowTrailingCommas = false,
                });
            if (document.RootElement.ValueKind != JsonValueKind.Array)
            {
                throw InvalidArgument(
                    function,
                    invalidDetail);
            }

            var segments = new List<string>();
            foreach (var segment in document.RootElement.EnumerateArray())
            {
                if (segment.ValueKind != JsonValueKind.String)
                {
                    throw InvalidArgument(
                        function,
                        invalidDetail);
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
                function,
                invalidDetail);
        }
    }

    private abstract class ConstructedJsonValue
    {
    }

    private sealed class ConstructedJsonObject : ConstructedJsonValue
    {
        internal Dictionary<string, ConstructedJsonValue> Properties { get; } =
            new(StringComparer.Ordinal);
    }

    private sealed class ConstructedJsonScalar(FerruleValue value) : ConstructedJsonValue
    {
        internal FerruleValue Value { get; } = value;
    }
}
