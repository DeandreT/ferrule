using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string XmlCharacterPlainSchema = """{"name":"Root","kind":{"kind":"group","children":[{"name":"id","attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Value","kind":{"kind":"scalar","ty":"string"}}]}}""";
    private const string XmlCharacterSimpleSchema = """{"name":"Root","kind":{"kind":"group","children":[{"name":"id","attribute":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"#text","text":true,"kind":{"kind":"scalar","ty":"string"}}]}}""";

    private static FerruleValue XmlCharacterNode(
        string schema, string field, string id, string value, bool indent = false) =>
        FerruleXml.Serialize(21, schema,
            Group(Field("id", Scalar(Text(id))), Field(field, Scalar(Text(value)))),
            indent, indent, null);

    /// <summary>Builds text from scalar values; BMP surrogates stay unpaired code units.</summary>
    private static string Chars(params int[] codepoints) => string.Concat(codepoints.Select(
        codepoint => codepoint is >= 0xD800 and <= 0xDFFF
            ? ((char)codepoint).ToString()
            : char.ConvertFromUtf32(codepoint)));

    private static void XmlCharacterRefusal(int codepoint, int byteOffset, Action serialize)
    {
        var error = Error(FerruleRuntimeError.XmlSerialization, serialize);
        Equal<uint?>(21, error.Node);
        Equal($"XML output contains forbidden XML 1.0 character U+{codepoint:X4} at UTF-8 byte {byteOffset}",
            error.Detail);
    }

    private static void XmlInvalidCharacterClasses()
    {
        // Each class of the XML 1.0 Char production's complement that a well-formed
        // scalar value can carry, in element text, simple text and attributes.
        foreach (var codepoint in new[] { 0x0, 0x1, 0x8, 0xB, 0xC, 0xE, 0x1F, 0xFFFE, 0xFFFF })
        {
            var invalid = Chars(0xE9, codepoint);
            // Offsets count the framing before the two-byte e-acute.
            XmlCharacterRefusal(codepoint, 23,
                () => XmlCharacterNode(XmlCharacterPlainSchema, "Value", "ok", invalid));
            XmlCharacterRefusal(codepoint, 16,
                () => XmlCharacterNode(XmlCharacterSimpleSchema, "#text", "ok", invalid));
            XmlCharacterRefusal(codepoint, 12,
                () => XmlCharacterNode(XmlCharacterPlainSchema, "Value", invalid, "ok"));
            // The declaration and indentation precede the scanned offset.
            XmlCharacterRefusal(codepoint, 65,
                () => XmlCharacterNode(XmlCharacterPlainSchema, "Value", "ok", invalid, indent: true));
        }
    }

    private static void XmlInvalidCharacterSurrogates()
    {
        // Ferrule text values are well-formed, so an unpaired surrogate never
        // reaches the writer as data; the node writer still refuses one that
        // arrives through its unvalidated default-namespace argument.
        foreach (var unpaired in new[] { Chars(0xD800), Chars(0xDC00), Chars(0xDC00, 0xD800), Chars(0x61, 0xD800, 0x62) })
        {
            Throws<ArgumentException>(() => Text(unpaired));
            var lone = unpaired.First(char.IsSurrogate);
            XmlCharacterRefusal(lone, 17 + unpaired.IndexOf(lone), () => FerruleXml.Serialize(21,
                XmlCharacterPlainSchema, Group(), false, false, "urn:" + unpaired));
        }
        // A paired supplementary character counts four UTF-8 bytes before the refusal.
        XmlCharacterRefusal(0x1, 21, () => FerruleXml.Serialize(21, XmlCharacterPlainSchema, Group(),
            false, false, "urn:" + Chars(0x1F600, 0x1)));
    }

    private static void XmlInvalidCharacterValidBoundaries()
    {
        var boundary = Chars(0x20, 0xD7FF, 0xE000, 0xFFFD, 0x10000, 0x1F600, 0x10FFFF);
        var value = "\t\n\r" + boundary;
        var text = "\t\n&#xD;" + boundary;
        var attribute = "&#x9;&#xA;&#xD;" + boundary;
        Equal(Text($"<Root id=\"{attribute}\"><Value>{text}</Value></Root>"),
            XmlCharacterNode(XmlCharacterPlainSchema, "Value", value, value));
        Equal(Text($"<Root id=\"{attribute}\">{text}</Root>"),
            XmlCharacterNode(XmlCharacterSimpleSchema, "#text", value, value));
        Equal(Text($"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<Root id=\"{attribute}\">\n  <Value>{text}</Value>\n</Root>"),
            XmlCharacterNode(XmlCharacterPlainSchema, "Value", value, value, indent: true));
    }

    private static void XmlInvalidCharacterSizeFirst()
    {
        // The native writer checks the output size before scanning characters.
        const string framing = "<Root id=\"\"><Value></Value></Root>";
        var oversized = new string('x', FerruleXml.MaximumOutputBytes + 1 - framing.Length - 1) + Chars(0x1);
        var error = Error(FerruleRuntimeError.XmlSerialization,
            () => XmlCharacterNode(XmlCharacterPlainSchema, "Value", "", oversized));
        Equal($"serialized output exceeds {FerruleXml.MaximumOutputBytes} bytes", error.Detail);
    }
}
