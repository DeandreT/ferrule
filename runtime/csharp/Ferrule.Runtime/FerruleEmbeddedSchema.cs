using System.Globalization;
using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

// This versioned descriptor is used only by generated hosts. Ordinary saved
// projects and graph-level JSON functions continue to consume plain schemas.
internal static class FerruleEmbeddedSchema
{
    internal const string V2Prefix = "FERRULE-EMBEDDED-SCHEMA/2\n";
    private const string VersionStem = "FERRULE-EMBEDDED-SCHEMA/";
    private const string FloatBitsPrefix = "FERRULE-F64-BITS:";
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    internal readonly record struct Descriptor(string Payload, bool ExactFloatMarkers);

    internal static Descriptor Unwrap(string descriptor, int maximumBytes)
    {
        ArgumentNullException.ThrowIfNull(descriptor);
        var bytes = StrictUtf8.GetByteCount(descriptor);
        if (bytes > maximumBytes)
        {
            throw new FormatException(
                $"embedded schema is {bytes} bytes; maximum is {maximumBytes}");
        }
        if (!descriptor.StartsWith(V2Prefix, StringComparison.Ordinal))
        {
            if (descriptor.StartsWith(VersionStem, StringComparison.Ordinal))
            {
                throw new FormatException("unsupported embedded schema descriptor version");
            }
            return new Descriptor(descriptor, false);
        }

        var payload = descriptor[V2Prefix.Length..];
        using var parsed = JsonDocument.Parse(payload, new JsonDocumentOptions
        {
            MaxDepth = 127,
            CommentHandling = JsonCommentHandling.Disallow,
            AllowTrailingCommas = false,
        });
        ValidateNode(parsed.RootElement, 1);
        return new Descriptor(payload, true);
    }

    internal static bool TryReadFloat(JsonElement element, out double value)
    {
        value = 0;
        if (element.ValueKind != JsonValueKind.String)
        {
            return false;
        }
        var marker = element.GetString();
        if (marker is null || !marker.StartsWith(FloatBitsPrefix, StringComparison.Ordinal) ||
            marker.Length != FloatBitsPrefix.Length + 16)
        {
            return false;
        }
        var digits = marker.AsSpan(FloatBitsPrefix.Length);
        foreach (var digit in digits)
        {
            if (digit is not (>= '0' and <= '9' or >= 'a' and <= 'f'))
            {
                return false;
            }
        }
        if (!ulong.TryParse(digits, NumberStyles.AllowHexSpecifier,
                CultureInfo.InvariantCulture, out var bits))
        {
            return false;
        }
        value = BitConverter.Int64BitsToDouble(unchecked((long)bits));
        return double.IsFinite(value);
    }

    private static void ValidateFloat(JsonElement value)
    {
        if (!TryReadFloat(value, out _))
        {
            throw new FormatException("invalid finite-float marker in embedded schema");
        }
    }

    private static JsonElement RequireObjectProperty(JsonElement parent, string name)
    {
        if (parent.ValueKind != JsonValueKind.Object ||
            !parent.TryGetProperty(name, out var value) ||
            value.ValueKind != JsonValueKind.Object)
        {
            throw new FormatException($"invalid embedded schema shape at '{name}'");
        }
        return value;
    }

    private static JsonElement RequireArrayProperty(JsonElement parent, string name)
    {
        if (parent.ValueKind != JsonValueKind.Object ||
            !parent.TryGetProperty(name, out var value) ||
            value.ValueKind != JsonValueKind.Array)
        {
            throw new FormatException($"invalid embedded schema shape at '{name}'");
        }
        return value;
    }

    private static bool HasTag(JsonElement parent, string tag) =>
        parent.TryGetProperty("kind", out var kind) &&
        kind.ValueKind == JsonValueKind.String &&
        string.Equals(kind.GetString(), tag, StringComparison.Ordinal);

    private static void ValidateNode(JsonElement node, int depth)
    {
        if (depth > 128 || node.ValueKind != JsonValueKind.Object)
        {
            throw new FormatException("embedded schema has excessive depth or an invalid node");
        }
        if (node.TryGetProperty("numeric_range", out var range) &&
            range.ValueKind != JsonValueKind.Null)
        {
            if (range.ValueKind != JsonValueKind.Object)
            {
                throw new FormatException("invalid embedded numeric range");
            }
            if (HasTag(range, "number"))
            {
                var bounds = RequireObjectProperty(range, "bounds");
                foreach (var side in new[] { "minimum", "maximum" })
                {
                    if (bounds.TryGetProperty(side, out var bound) &&
                        bound.ValueKind != JsonValueKind.Null)
                    {
                        var value = RequireObjectProperty(bounds, side);
                        if (!value.TryGetProperty("value", out var number))
                        {
                            throw new FormatException("missing embedded numeric bound");
                        }
                        ValidateFloat(number);
                    }
                }
            }
        }
        if (node.TryGetProperty("json_allowed_values", out var allowed) &&
            allowed.ValueKind != JsonValueKind.Null)
        {
            foreach (var entry in RequireArrayProperty(node, "json_allowed_values").EnumerateArray())
            {
                if (entry.ValueKind != JsonValueKind.Object)
                {
                    throw new FormatException("invalid embedded allowed value");
                }
                if (entry.TryGetProperty("type", out var type) &&
                    type.ValueKind == JsonValueKind.String && type.GetString() == "float")
                {
                    if (!entry.TryGetProperty("value", out var value))
                    {
                        throw new FormatException("missing embedded allowed float");
                    }
                    ValidateFloat(value);
                }
            }
        }
        foreach (var family in new[] { "json_contains", "json_dependent_schemas" })
        {
            if (!node.TryGetProperty(family, out var predicates) ||
                predicates.ValueKind == JsonValueKind.Null)
            {
                continue;
            }
            foreach (var entry in RequireArrayProperty(node, family).EnumerateArray())
            {
                var predicate = RequireObjectProperty(entry, "predicate");
                if (HasTag(predicate, "schema"))
                {
                    if (!predicate.TryGetProperty("schema", out var schema))
                    {
                        throw new FormatException("missing embedded predicate schema");
                    }
                    ValidateNode(schema, depth + 1);
                }
            }
        }
        var kind = RequireObjectProperty(node, "kind");
        if (!HasTag(kind, "group"))
        {
            return;
        }
        foreach (var child in RequireArrayProperty(kind, "children").EnumerateArray())
        {
            ValidateNode(child, depth + 1);
        }
        if (kind.TryGetProperty("dynamic", out var dynamic) &&
            dynamic.ValueKind != JsonValueKind.Null)
        {
            ValidateNode(dynamic, depth + 1);
        }
        if (!kind.TryGetProperty("alternatives", out var alternatives) ||
            alternatives.ValueKind == JsonValueKind.Null)
        {
            return;
        }
        foreach (var alternative in RequireArrayProperty(kind, "alternatives").EnumerateArray())
        {
            if (!alternative.TryGetProperty("constraints", out var constraints) ||
                constraints.ValueKind == JsonValueKind.Null)
            {
                continue;
            }
            foreach (var constraint in RequireArrayProperty(alternative, "constraints").EnumerateArray())
            {
                var value = RequireObjectProperty(constraint, "value");
                if (value.TryGetProperty("type", out var type) &&
                    type.ValueKind == JsonValueKind.String && type.GetString() == "float")
                {
                    if (!value.TryGetProperty("value", out var number))
                    {
                        throw new FormatException("missing embedded alternative float");
                    }
                    ValidateFloat(number);
                }
            }
        }
    }
}
