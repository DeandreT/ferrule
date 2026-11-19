using System.Text;

namespace Ferrule.Runtime;

internal static class FerruleFlexTextStore
{
    private const int ValueLimit = 1 << 20;
    private static readonly UTF8Encoding Utf8 = new(false, true);

    internal static FerruleGroup Read(byte[] document, string field)
    {
        ArgumentNullException.ThrowIfNull(document);
        ReadOnlySpan<byte> bytes = document;
        if (bytes.StartsWith("\uFEFF"u8)) bytes = bytes[3..];
        if (bytes.Length > ValueLimit) throw new InvalidDataException("FlexText value exceeds byte limit");
        return new([new(field, new FerruleScalar(FerruleValue.FromString(Utf8.GetString(bytes))))]);
    }

    internal static byte[] Write(FerruleInstance value, string field, bool bom, string ending)
    {
        if (value is not FerruleGroup group || group.Fields.Count != 1 || group.Fields[0].Name != field || group.Fields[0].Value is not FerruleScalar scalar)
            throw new InvalidDataException("Expected one FlexText Store field");
        var text = scalar.Value.Kind switch {
            FerruleValueKind.Null or FerruleValueKind.JsonNull => "",
            FerruleValueKind.String => scalar.Value.StringValue,
            _ => throw new InvalidDataException("Expected FlexText string")
        };
        if (text.Length > ValueLimit || Utf8.GetByteCount(text) > ValueLimit) throw new InvalidDataException("FlexText value exceeds byte limit");
        if (ending == "\n" && text.Contains("\r\r\n", StringComparison.Ordinal)) throw new InvalidDataException("Ambiguous LF output");
        // Only CR immediately followed by LF is normalized. ReplaceLineEndings
        // would incorrectly rewrite lone CR and Unicode paragraph separators.
        text = text.Replace("\r\n", "\n", StringComparison.Ordinal);
        if (ending != "\n") text = text.Replace("\n", ending, StringComparison.Ordinal);
        // At worst two output bytes per input byte plus a three-byte BOM,
        // strictly inside the interpreter's 8 MiB document bound.
        return Utf8.GetBytes(bom ? "\uFEFF" + text : text);
    }
}
