using System.Globalization;
using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
    private const string JsonParseFieldName = "json_parse_field";
    private const string JsonParseObjectExistsName = "json_parse_object_exists";
    private const string JsonSerializeObjectName = "json_serialize_object";

    private static (FerruleRepeated Array, string Schema, IReadOnlyList<string> Path) ParseJsonArray(
        IReadOnlyList<FerruleValue> arguments, string function)
    {
        var input = RequireString(arguments[0], function);
        var schema = RequireString(arguments[1], function);
        var path = RequireString(arguments[2], function);
        try { FerruleJson.ValidateFunctionSchema(schema); }
        catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.JsonBoundary)
        { throw InvalidArgument(function, "schema descriptor is invalid"); }
        var segments = ParseJsonStringArray(path, function, "array path descriptor is invalid");
        FerruleInstance current;
        try { current = FerruleJson.ParseFunctionInput(schema, input); }
        catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.JsonBoundary)
        { throw InvalidArgument(function, "input does not match the JSON schema"); }
        if (!FerruleJson.IsFunctionArrayPath(schema, segments))
            throw InvalidArgument(function, "array path does not resolve to an array");
        foreach (var segment in segments)
        {
            if (current is FerruleScalar { Value.Kind: FerruleValueKind.Null })
                return (new FerruleRepeated(Array.Empty<FerruleInstance>()), schema, segments);
            if (current is not FerruleGroup group)
                throw InvalidArgument(function, "array path does not resolve to an array");
            if (!group.TryGetField(segment, out var child))
                return (new FerruleRepeated(Array.Empty<FerruleInstance>()), schema, segments);
            current = child;
        }
        if (current is FerruleScalar { Value.Kind: FerruleValueKind.Null })
            return (new FerruleRepeated(Array.Empty<FerruleInstance>()), schema, segments);
        return current is FerruleRepeated array ? (array, schema, segments)
            : throw InvalidArgument(function, "array path does not resolve to an array");
    }

    private static FerruleValue JsonParseArrayLength(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "json_parse_array_length";
        RequireArity(function, arguments, 3);
        return FerruleValue.FromInt64(ParseJsonArray(arguments, function).Array.Items.Count);
    }

    private static FerruleValue JsonParseArrayField(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "json_parse_array_field";
        RequireArity(function, arguments, 5);
        var parsed = ParseJsonArray(arguments, function);
        var array = parsed.Array;
        var segments = ParseJsonStringArray(RequireString(arguments[3], function), function,
            "field path descriptor is invalid");
        if (!FerruleJson.IsFunctionArrayScalarPath(parsed.Schema, parsed.Path, segments))
            throw InvalidArgument(function, "field path does not resolve to a scalar");
        if (arguments[4].Kind != FerruleValueKind.Int64) throw Type(function, arguments[4]);
        var index = arguments[4].Int64Value;
        if (index < 1 || index > array.Items.Count)
            throw InvalidArgument(function, "array index is out of range");
        var current = array.Items[(int)index - 1];
        foreach (var segment in segments)
        {
            if (current is FerruleScalar { Value.Kind: FerruleValueKind.Null })
                return FerruleValue.Null;
            if (current is not FerruleGroup group)
                throw InvalidArgument(function, "field path does not resolve to a scalar");
            if (!group.TryGetField(segment, out var child)) return FerruleValue.Null;
            current = child;
        }
        return current is FerruleScalar scalar ? scalar.Value
            : throw InvalidArgument(function, "field path does not resolve to a scalar");
    }

    private static FerruleValue JsonParseObjectExists(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = JsonParseObjectExistsName;
        RequireArity(function, arguments, 3);
        var schema = RequireString(arguments[1], function);
        var path = RequireString(arguments[2], function);
        var input = arguments[0];
        if (input.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
        {
            return FerruleValue.FromBoolean(false);
        }
        if (input.Kind != FerruleValueKind.String)
        {
            throw Type(function, input);
        }
        try
        {
            FerruleJson.ValidateFunctionSchema(schema);
        }
        catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.JsonBoundary)
        {
            throw InvalidArgument(function, "schema descriptor is invalid");
        }
        var segments = ParseJsonStringArray(path, function, "field path descriptor is invalid");
        if (!FerruleJson.IsOrdinaryFunctionObjectPath(schema, segments))
        {
            throw InvalidArgument(function, "field path does not resolve to an ordinary object");
        }
        FerruleInstance current;
        try
        {
            current = FerruleJson.ParseFunctionInput(schema, input.StringValue);
        }
        catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.JsonBoundary)
        {
            throw InvalidArgument(function, "input does not match the JSON schema");
        }
        if (current is FerruleRepeated or FerruleDocumentSet)
        {
            throw InvalidArgument(function, "input does not match the JSON schema");
        }
        foreach (var segment in segments)
        {
            if (current is not FerruleGroup group || !group.TryGetField(segment, out var child))
            {
                return FerruleValue.FromBoolean(false);
            }
            current = child;
        }
        return current switch
        {
            FerruleGroup => FerruleValue.FromBoolean(true),
            FerruleScalar scalar when scalar.Value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull
                => FerruleValue.FromBoolean(false),
            _ => throw InvalidArgument(function, "field path does not resolve to an ordinary object"),
        };
    }

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
            FerruleJson.ValidateFunctionSchema(schema);
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
            parsed = FerruleJson.ParseFunctionInput(schema, input.StringValue);
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
            return FerruleValue.FromString(RenderJsonObject(root));
        }
        catch (Exception error) when (
            error is InvalidOperationException or OverflowException)
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

    private static string RenderJsonObject(ConstructedJsonObject root)
    {
        var text = new StringBuilder();
        var frames = new Stack<JsonObjectRenderFrame>();
        text.Append('{');
        frames.Push(new JsonObjectRenderFrame(root));
        try
        {
            while (frames.Count > 0)
            {
                var frame = frames.Peek();
                if (!frame.Properties.MoveNext())
                {
                    text.Append('}');
                    frames.Pop().Dispose();
                    continue;
                }

                if (!frame.First)
                {
                    text.Append(',');
                }
                frame.First = false;
                var (name, child) = frame.Properties.Current;
                FerruleJson.AppendSerdeString(text, name);
                text.Append(':');
                if (child is ConstructedJsonObject nested)
                {
                    text.Append('{');
                    frames.Push(new JsonObjectRenderFrame(nested));
                }
                else if (child is ConstructedJsonScalar scalar)
                {
                    AppendJsonScalar(text, scalar.Value);
                }
                else
                {
                    throw new InvalidOperationException("Constructed JSON value is invalid.");
                }
            }

            return text.ToString();
        }
        finally
        {
            while (frames.Count > 0)
            {
                frames.Pop().Dispose();
            }
        }
    }

    private static void AppendJsonScalar(StringBuilder text, FerruleValue value)
    {
        switch (value.Kind)
        {
            case FerruleValueKind.JsonNull:
                text.Append("null");
                break;
            case FerruleValueKind.String:
                FerruleJson.AppendSerdeString(text, value.StringValue);
                break;
            case FerruleValueKind.Bool:
                text.Append(value.BooleanValue ? "true" : "false");
                break;
            case FerruleValueKind.Int64:
                text.Append(value.Int64Value.ToString(CultureInfo.InvariantCulture));
                break;
            case FerruleValueKind.Double:
                text.Append(FerruleJson.FormatSerdeFloat(value.DoubleValue));
                break;
            default:
                throw Type(JsonSerializeObjectName, value);
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
        catch (Exception error) when (error is JsonException or InvalidOperationException)
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

    private sealed class JsonObjectRenderFrame : IDisposable
    {
        internal JsonObjectRenderFrame(ConstructedJsonObject value)
        {
            Properties = value.Properties.GetEnumerator();
        }

        internal IEnumerator<KeyValuePair<string, ConstructedJsonValue>> Properties { get; }

        internal bool First { get; set; } = true;

        public void Dispose() => Properties.Dispose();
    }
}
