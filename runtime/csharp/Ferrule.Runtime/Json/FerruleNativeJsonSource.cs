using System.Globalization;
using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public static class FerruleNativeJsonSource
{
    /// <summary>BOM-aware, non-flushing wire-compatible input. Lone escaped
    /// UTF-16 surrogates become U+FFFD; this is not a lossless general JSON reader.
    /// Separately qualify reached string operations and output encoding. The
    /// caller must supply an ASCII-name projection; strict Parse is unchanged.</summary>
    public static FerruleJsonSourceDocument Parse(string identity, byte[] bytes, string projectionDescriptor, FerruleJsonSourceLimits? limits = null)
    {
        ArgumentNullException.ThrowIfNull(bytes); limits ??= new();
        if (limits.DocumentBytes is < 0 or > FerruleJsonSourceDocument.MaxDocumentBytes || limits.ContainerDepth is < 0 or > FerruleJsonSourceDocument.MaxContainerDepth || limits.Values is < 0 or > FerruleJsonSourceDocument.MaxValues)
            throw new FerruleJsonSourceException(FerruleJsonSourceErrorKind.InvalidLimits);
        _ = FerruleJsonSourceProjection.Parse(projectionDescriptor);
        using var projection = JsonDocument.Parse(projectionDescriptor);
        if (!Ascii(projection.RootElement)) throw new ArgumentException("Native JSON requires ASCII projected property names.");
        if (bytes.Length > limits.DocumentBytes || bytes.Length > FerruleJsonSourceDocument.MaxDocumentBytes)
            throw new ArgumentException("Native JSON exceeds its byte limit.");
        var text = FerruleTextDecoding.BomAwareNonFlushing(bytes);
        if (Encoding.UTF8.GetByteCount(text) > limits.DocumentBytes) throw new ArgumentException("Decoded JSON exceeds its byte limit.");
        return FerruleJsonSourceDocument.Parse(identity, Encoding.UTF8.GetBytes(Normalize(text)), limits);
    }

    private static bool Ascii(JsonElement shape)
    {
        return shape.GetProperty("kind").GetString() switch
        {
            "object" => shape.GetProperty("properties").EnumerateArray().All(p => p.GetProperty("name").GetString()!.All(c => c <= 127) && Ascii(p.GetProperty("shape"))),
            "array" => Ascii(shape.GetProperty("item")),
            _ => true,
        };
    }

    private static bool Hex(string text, int index, out ushort value)
    {
        value = 0;
        return index + 6 <= text.Length && text[index] == '\\' && text[index + 1] == 'u'
            && ushort.TryParse(text.AsSpan(index + 2, 4), NumberStyles.AllowHexSpecifier, CultureInfo.InvariantCulture, out value);
    }
    private static string Normalize(string text)
    {
        var output = new StringBuilder(text.Length); bool quoted = false;
        for (int i = 0; i < text.Length;)
        {
            char c = text[i];
            if (quoted && c == '\\')
            {
                if (Hex(text, i, out var unit))
                {
                    if (unit is >= 0xd800 and <= 0xdbff)
                    {
                        if (Hex(text, i + 6, out var low) && low is >= 0xdc00 and <= 0xdfff) { output.Append(text, i, 12); i += 12; continue; }
                        output.Append("\\uFFFD"); i += 6; continue;
                    }
                    if (unit is >= 0xdc00 and <= 0xdfff) { output.Append("\\uFFFD"); i += 6; continue; }
                    output.Append(text, i, 6); i += 6; continue;
                }
                output.Append(c); i++;
                if (i < text.Length) output.Append(text[i++]);
                continue;
            }
            if (c == '"') quoted = !quoted;
            if (quoted && c < 32) output.Append("\\u").Append(((int)c).ToString("X4", CultureInfo.InvariantCulture));
            else output.Append(c);
            i++;
        }
        return output.ToString();
    }
}
