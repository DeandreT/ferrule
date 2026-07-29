using System.Buffers;
using System.Globalization;
using System.Text;
using System.Text.Encodings.Web;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    private static bool ReadJsonUniqueItems(
        string name,
        JsonElement element,
        bool repeating)
    {
        bool? uniqueItems = null;
        foreach (var property in element.EnumerateObject())
        {
            if (!string.Equals(
                    property.Name,
                    "json_unique_items",
                    StringComparison.Ordinal))
            {
                continue;
            }
            if (uniqueItems is not null)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has duplicate JSON unique-items metadata.");
            }
            uniqueItems = property.Value.ValueKind switch
            {
                JsonValueKind.True => true,
                JsonValueKind.False => false,
                _ => throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON unique-items metadata must be a boolean."),
            };
        }
        if (uniqueItems == true && !repeating)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has JSON unique-items metadata without an array.");
        }
        return uniqueItems ?? false;
    }

    private static void ValidateUniqueInputItems(
        JsonSchemaNode schema,
        JsonElement array)
    {
        if (!schema.JsonUniqueItems)
        {
            return;
        }
        var keys = new HashSet<byte[]>(UniqueItemKeyComparer.Instance);
        var budget = new UniqueItemBudget();
        var index = 0;
        foreach (var item in array.EnumerateArray())
        {
            if (index == MaximumNodes)
            {
                throw Boundary(
                    $"JSON uniqueItems array '{schema.Name}' exceeds the {MaximumNodes}-item limit.");
            }
            var key = CreateUniqueItemKey(item, budget);
            if (!keys.Add(key))
            {
                throw Boundary(
                    $"JSON array '{schema.Name}' violates uniqueItems at item {index + 1}.");
            }
            index = checked(index + 1);
        }
    }

    private static void WriteUniqueOutputItems(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        IReadOnlyList<FerruleInstance> items,
        NodeBudget nodeBudget,
        int depth)
    {
        if (items.Count > MaximumNodes)
        {
            throw Boundary(
                $"Normalized JSON uniqueItems array '{schema.Name}' exceeds the {MaximumNodes}-item limit.");
        }
        var keys = new HashSet<byte[]>(UniqueItemKeyComparer.Instance);
        var uniqueBudget = new UniqueItemBudget();
        for (var index = 0; index < items.Count; index++)
        {
            var buffer = new ArrayBufferWriter<byte>();
            using (var itemWriter = new Utf8JsonWriter(
                       buffer,
                       new JsonWriterOptions
                       {
                           Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
                           MaxDepth = MaximumDepth,
                           SkipValidation = false,
                       }))
            {
                WriteSingleNode(itemWriter, schema, items[index], nodeBudget, depth);
            }
            if (buffer.WrittenCount > MaximumDocumentBytes)
            {
                throw Boundary(
                    $"Normalized JSON item in array '{schema.Name}' exceeds the {MaximumDocumentBytes}-byte limit.");
            }
            using var document = JsonDocument.Parse(
                buffer.WrittenMemory,
                new JsonDocumentOptions
                {
                    MaxDepth = MaximumDepth,
                    CommentHandling = JsonCommentHandling.Disallow,
                    AllowTrailingCommas = false,
                });
            var key = CreateUniqueItemKey(document.RootElement, uniqueBudget);
            if (!keys.Add(key))
            {
                throw Boundary(
                    $"Normalized JSON array '{schema.Name}' violates uniqueItems at item {index + 1}.");
            }
            document.RootElement.WriteTo(writer);
        }
    }

    private static byte[] CreateUniqueItemKey(
        JsonElement value,
        UniqueItemBudget budget)
    {
        var buffer = new ArrayBufferWriter<byte>();
        using (var writer = new Utf8JsonWriter(
                   buffer,
                   new JsonWriterOptions
                   {
                       Encoder = JavaScriptEncoder.UnsafeRelaxedJsonEscaping,
                       MaxDepth = MaximumDepth,
                       SkipValidation = false,
                   }))
        {
            WriteCanonicalUniqueValue(writer, value, budget);
        }
        budget.AddBytes(buffer.WrittenCount);
        return buffer.WrittenSpan.ToArray();
    }

    private static void WriteCanonicalUniqueValue(
        Utf8JsonWriter writer,
        JsonElement value,
        UniqueItemBudget budget)
    {
        budget.Visit();
        switch (value.ValueKind)
        {
            case JsonValueKind.Null:
                writer.WriteNullValue();
                return;
            case JsonValueKind.True:
                writer.WriteBooleanValue(true);
                return;
            case JsonValueKind.False:
                writer.WriteBooleanValue(false);
                return;
            case JsonValueKind.String:
                var text = value.GetString() ?? string.Empty;
                writer.WriteStringValue(text);
                return;
            case JsonValueKind.Number:
                writer.WriteRawValue(
                    CanonicalUniqueNumber(value.GetRawText(), budget),
                    skipInputValidation: true);
                return;
            case JsonValueKind.Array:
                writer.WriteStartArray();
                foreach (var item in value.EnumerateArray())
                {
                    WriteCanonicalUniqueValue(writer, item, budget);
                }
                writer.WriteEndArray();
                return;
            case JsonValueKind.Object:
                writer.WriteStartObject();
                foreach (var property in LastObjectProperties(value)
                             .OrderBy(property => property.Key, StringComparer.Ordinal))
                {
                    writer.WritePropertyName(property.Key);
                    WriteCanonicalUniqueValue(writer, property.Value, budget);
                }
                writer.WriteEndObject();
                return;
            default:
                throw Boundary(
                    $"JSON uniqueItems encountered unsupported value kind '{value.ValueKind}'.");
        }
    }

    private static Dictionary<string, JsonElement> LastObjectProperties(JsonElement value)
    {
        var properties = new Dictionary<string, JsonElement>(StringComparer.Ordinal);
        foreach (var property in value.EnumerateObject())
        {
            properties[property.Name] = property.Value;
        }
        return properties;
    }

    private static string CanonicalUniqueNumber(
        string source,
        UniqueItemBudget budget)
    {
        var offset = source[0] == '-' ? 1 : 0;
        var exponentOffset = source.IndexOfAny('e', 'E');
        var mantissaEnd = exponentOffset < 0 ? source.Length : exponentOffset;
        var decimalOffset = source.IndexOf('.', offset, mantissaEnd - offset);
        var fractionalDigits = decimalOffset < 0 ? 0 : mantissaEnd - decimalOffset - 1;
        var digits = new StringBuilder(mantissaEnd - offset);
        for (var index = offset; index < mantissaEnd; index++)
        {
            if (source[index] != '.')
            {
                digits.Append(source[index]);
            }
        }

        var firstSignificant = 0;
        while (firstSignificant < digits.Length && digits[firstSignificant] == '0')
        {
            firstSignificant++;
        }
        if (firstSignificant == digits.Length)
        {
            return "0";
        }

        var end = digits.Length;
        while (end > firstSignificant && digits[end - 1] == '0')
        {
            end--;
        }
        var trailingZeros = digits.Length - end;
        var explicitExponent = 0L;
        if (exponentOffset >= 0 &&
            !long.TryParse(
                source.AsSpan(exponentOffset + 1),
                NumberStyles.AllowLeadingSign,
                CultureInfo.InvariantCulture,
                out explicitExponent))
        {
            throw Boundary("JSON uniqueItems number exponent exceeds the supported range.");
        }
        long exponent;
        try
        {
            exponent = checked(
                explicitExponent - (long)fractionalDigits + trailingZeros);
        }
        catch (OverflowException error)
        {
            throw Boundary("JSON uniqueItems number exponent exceeds the supported range.", error);
        }

        var coefficient = digits.ToString(firstSignificant, end - firstSignificant);
        if (source[0] == '-')
        {
            coefficient = string.Concat("-", coefficient);
        }
        return exponent == 0
            ? coefficient
            : string.Concat(
                coefficient,
                "e",
                exponent.ToString(CultureInfo.InvariantCulture));
    }

    private sealed class UniqueItemBudget
    {
        private int _nodes;
        private long _bytes;

        public void Visit()
        {
            _nodes = checked(_nodes + 1);
            if (_nodes > MaximumNodes)
            {
                throw Boundary(
                    $"JSON uniqueItems comparison exceeds the {MaximumNodes}-node limit.");
            }
        }

        public void AddBytes(int bytes)
        {
            _bytes = checked(_bytes + bytes);
            if (_bytes > MaximumDocumentBytes)
            {
                throw Boundary(
                    $"JSON uniqueItems comparison exceeds the {MaximumDocumentBytes}-byte key limit.");
            }
        }
    }

    private sealed class UniqueItemKeyComparer : IEqualityComparer<byte[]>
    {
        public static readonly UniqueItemKeyComparer Instance = new();

        public bool Equals(byte[]? left, byte[]? right) =>
            ReferenceEquals(left, right) ||
            left is not null &&
            right is not null &&
            left.AsSpan().SequenceEqual(right);

        public int GetHashCode(byte[] value)
        {
            unchecked
            {
                var hash = 2166136261U;
                foreach (var item in value)
                {
                    hash = (hash ^ item) * 16777619U;
                }
                return (int)hash;
            }
        }
    }
}
