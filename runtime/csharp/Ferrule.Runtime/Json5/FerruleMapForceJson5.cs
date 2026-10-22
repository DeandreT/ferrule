using System.Globalization;
using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public enum FerruleMapForceJson5Error
{
    Input,
    NonAsciiProperty,
    Number,
    InvalidLimit,
    OutputBytes,
}

public sealed class FerruleMapForceJson5Exception : Exception
{
    public FerruleMapForceJson5Error Error { get; }

    internal FerruleMapForceJson5Exception(FerruleMapForceJson5Error error, string message, Exception? inner = null)
        : base(message, inner) => Error = error;
}

/// <summary>Explicit text formatting only; no schema admission, mapping or file I/O.</summary>
public static class FerruleMapForceJson5
{
    private const int MaximumBytes = 64 * 1024 * 1024;
    private static readonly UTF8Encoding Utf8 = new(false, true);

    /// <summary>Retain member order and duplicates; emit tab/CRLF JSON5 with ASCII property names.</summary>
    public static string FormatAsciiKeys(string document, int maximumBytes = MaximumBytes)
    {
        ArgumentNullException.ThrowIfNull(document);
        if (maximumBytes < 0 || maximumBytes > MaximumBytes)
            throw new FerruleMapForceJson5Exception(FerruleMapForceJson5Error.InvalidLimit, "JSON5 output limit exceeds the hard maximum.");
        try
        {
            if (Utf8.GetByteCount(document) > MaximumBytes)
                throw new FerruleMapForceJson5Exception(FerruleMapForceJson5Error.Input, "JSON formatting input exceeds the byte limit.");
            if (document.StartsWith('\uFEFF')) document = document[1..];
            using var parsed = JsonDocument.Parse(document, new JsonDocumentOptions { MaxDepth = 64 });
            int remaining = 1_000_000;
            ValidateCount(parsed.RootElement, ref remaining);
            var output = new Output(maximumBytes);
            output.Value(parsed.RootElement, 0);
            return output.ToString();
        }
        catch (Exception error) when (error is JsonException or EncoderFallbackException)
        {
            throw new FerruleMapForceJson5Exception(FerruleMapForceJson5Error.Input, "Invalid strict JSON formatting input.", error);
        }
    }

    private static void ValidateCount(JsonElement value, ref int remaining)
    {
        if (remaining-- == 0)
            throw new FerruleMapForceJson5Exception(FerruleMapForceJson5Error.Input, "JSON input exceeds the value limit.");
        if (value.ValueKind == JsonValueKind.Object)
            foreach (var property in value.EnumerateObject()) ValidateCount(property.Value, ref remaining);
        else if (value.ValueKind == JsonValueKind.Array)
            foreach (var item in value.EnumerateArray()) ValidateCount(item, ref remaining);
    }

    private sealed class Output(int maximum)
    {
        private readonly StringBuilder text = new();
        private int bytes;

        public override string ToString() => text.ToString();

        private void Add(string value)
        {
            int count = Utf8.GetByteCount(value);
            if (count > maximum - bytes)
                throw new FerruleMapForceJson5Exception(FerruleMapForceJson5Error.OutputBytes, "JSON5 output exceeds the UTF-8 byte limit.");
            text.Append(value);
            bytes += count;
        }

        private void Line(int depth)
        {
            Add("\r\n");
            for (int i = 0; i < depth; i++) Add("\t");
        }

        private void Quote(string value)
        {
            Add("\"");
            foreach (char unit in value)
            {
                if (unit is '"' or '\\') Add("\\" + unit);
                else if (unit <= 31 || unit is >= '\u007F' and <= '\u009F' || char.IsSurrogate(unit))
                    Add("\\u" + ((int)unit).ToString("X4", CultureInfo.InvariantCulture));
                else Add(unit.ToString());
            }
            Add("\"");
        }

        internal void Value(JsonElement value, int depth)
        {
            switch (value.ValueKind)
            {
                case JsonValueKind.Object:
                    Add("{");
                    bool first = true;
                    foreach (var member in value.EnumerateObject())
                    {
                        if (member.Name.Any(c => c > 127))
                            throw new FerruleMapForceJson5Exception(FerruleMapForceJson5Error.NonAsciiProperty, "MapForce JSON5 formatting requires ASCII property names.");
                        if (!first) Add(",");
                        first = false;
                        Line(depth + 1);
                        if (member.Name.Length > 0 && char.IsAsciiLetter(member.Name[0]) && member.Name.All(char.IsAsciiLetterOrDigit)) Add(member.Name);
                        else Quote(member.Name);
                        Add(": ");
                        Value(member.Value, depth + 1);
                    }
                    if (!first) Line(depth);
                    Add("}");
                    break;
                case JsonValueKind.Array:
                    Add("[");
                    int index = 0;
                    foreach (var item in value.EnumerateArray())
                    {
                        if (index++ > 0) Add(",");
                        Line(depth + 1);
                        Value(item, depth + 1);
                    }
                    Line(depth);
                    Add("]");
                    break;
                case JsonValueKind.String: Quote(value.GetString()!); break;
                case JsonValueKind.Number:
                    try
                    {
                        string token = value.GetRawText();
                        if (token.Length > 4096) throw new FormatException("Number token exceeds the decimal lexical limit.");
                        int exponent = token.IndexOfAny(['e', 'E']);
                        if (exponent >= 0 && !int.TryParse(token[(exponent + 1)..], NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out _))
                            throw new OverflowException("Number exponent exceeds the supported range.");
                        decimal number = decimal.Parse(token, NumberStyles.Float, CultureInfo.InvariantCulture);
                        Add(number.ToString("0.#################", CultureInfo.InvariantCulture));
                    }
                    catch (Exception error) when (error is FormatException or OverflowException)
                    { throw new FerruleMapForceJson5Exception(FerruleMapForceJson5Error.Number, "JSON number is outside the decimal formatting profile.", error); }
                    break;
                case JsonValueKind.True: Add("true"); break;
                case JsonValueKind.False: Add("false"); break;
                case JsonValueKind.Null: Add("null"); break;
                default: throw new FerruleMapForceJson5Exception(FerruleMapForceJson5Error.Input, "Unknown JSON kind.");
            }
        }
    }
}
