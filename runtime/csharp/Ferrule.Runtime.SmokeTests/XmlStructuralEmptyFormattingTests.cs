using Ferrule.Runtime;
using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string StructuralEmptyRowsSchema = """{"name":"Output","kind":{"kind":"group","children":[{"name":"Row","repeating":true,"kind":{"kind":"group","children":[{"name":"Result","kind":{"kind":"scalar","ty":"int"}}]}}]}}""";

    private static void XmlStructuralEmptyDocumentFormatting()
    {
        foreach (var value in new FerruleInstance[]
        {
            Group(),
            Group(Field("Row", new FerruleRepeated([]))),
        })
        foreach (var declaration in new[] { false, true })
        foreach (var indent in new[] { false, true })
        {
            var expected = indent ? "<Output>\n</Output>" : "<Output></Output>";
            if (declaration)
                expected = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>" + (indent ? "\n" : "") + expected;
            StructuralEmptyDocumentBytes("absent-or-empty-rows", StructuralEmptyRowsSchema,
                value, expected, declaration, indent);
        }

        const string scalar = """{"name":"Output","kind":{"kind":"group","children":[{"name":"Value","xml_optional":true,"kind":{"kind":"scalar","ty":"string"}}]}}""";
        StructuralEmptyDocumentBytes("null-scalar", scalar,
            Group(Field("Value", Scalar(FerruleValue.Null))), "<Output>\n</Output>", false, true);
        const string mapped = """{"name":"Output","kind":{"kind":"group","children":[{"name":"Row","kind":{"kind":"group","children":[{"name":"Result","kind":{"kind":"scalar","ty":"int"}}]}}]}}""";
        StructuralEmptyDocumentBytes("empty-mapped", mapped,
            Group(Field("Row", new FerruleMappedSequence([]))), "<Output>\n</Output>", false, true);
        const string nested = """{"name":"Outer","kind":{"kind":"group","children":[{"name":"Output","kind":{"kind":"group","children":[{"name":"Row","repeating":true,"kind":{"kind":"group","children":[{"name":"Result","kind":{"kind":"scalar","ty":"int"}}]}}]}}]}}""";
        StructuralEmptyDocumentBytes("nested-empty-rows", nested,
            Group(Field("Output", Group(Field("Row", new FerruleRepeated([]))))),
            "<Outer>\n  <Output>\n  </Output>\n</Outer>", false, true);
        StructuralEmptyDocumentBytes("default-namespace", StructuralEmptyRowsSchema,
            Group(Field("Row", new FerruleRepeated([]))), "<Output xmlns=\"urn:empty\">\n</Output>",
            false, true, "urn:empty");

        // Null is not a legal repeating container and must retain its original typed refusal.
        try
        {
            var returned = FerruleXml.SerializeEmbedded(96, StructuralEmptyRowsSchema,
                Group(Field("Row", Scalar(FerruleValue.Null))), false, true, null);
            Console.WriteLine(JsonSerializer.Serialize(new { Case = "repeating-null-error", Unexpected = returned.StringValue }));
            throw new InvalidOperationException("Repeating Null must retain its shape refusal.");
        }
        catch (FerruleRuntimeException error)
        {
            Console.WriteLine(error.ToString());
            Console.WriteLine(JsonSerializer.Serialize(new { Case = "repeating-null-error", error.Node, error.Error, error.Detail }));
            Equal(FerruleRuntimeError.XmlSerialization, error.Error);
            Equal((uint?)96, error.Node);
            Equal(true, error.Detail!.Contains("expected repeating elements", StringComparison.Ordinal));
        }
    }

    private static void XmlStructuralEmptyFormattingControls()
    {
        const string empty = """{"name":"Root","kind":{"kind":"group","children":[]}}""";
        const string attribute = """{"name":"Root","kind":{"kind":"group","children":[{"name":"Code","attribute":true,"kind":{"kind":"scalar","ty":"string"}}]}}""";
        const string text = """{"name":"Root","kind":{"kind":"group","children":[{"name":"#text","text":true,"kind":{"kind":"scalar","ty":"string"}}]}}""";
        const string scalar = """{"name":"Root","kind":{"kind":"scalar","ty":"string"}}""";
        const string ordered = """{"name":"Root","xml_repeating_choices":[{"required":false,"repeating":true,"members":["Code","Amount"]}],"kind":{"kind":"group","children":[{"name":"#text","text":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Code","repeating":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Amount","repeating":true,"kind":{"kind":"scalar","ty":"string"}}]}}""";
        const string orderedWithoutText = """{"name":"Root","xml_repeating_choices":[{"required":false,"repeating":true,"members":["Code","Amount"]}],"kind":{"kind":"group","children":[{"name":"Code","repeating":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Amount","repeating":true,"kind":{"kind":"scalar","ty":"string"}}]}}""";
        foreach (var indent in new[] { false, true })
        {
            StructuralEmptyDocumentBytes("childless-self-close", empty, Group(), "<Root/>", false, indent);
            StructuralEmptyDocumentBytes("attribute-only", attribute,
                Group(Field("Code", Scalar(Text("kept")))), "<Root Code=\"kept\"/>", false, indent);
            StructuralEmptyDocumentBytes("simple-text", text,
                Group(Field("#text", Scalar(Text("kept")))), "<Root>kept</Root>", false, indent);
            StructuralEmptyDocumentBytes("empty-text", text,
                Group(Field("#text", Scalar(Text("")))), "<Root/>", false, indent);
            StructuralEmptyDocumentBytes("empty-scalar", scalar,
                Scalar(Text("")), "<Root></Root>", false, indent);
            var emptyOrdered = Group(Field("#text", Scalar(Text(""))),
                Field("\u001fferrule-xml-mixed-content", new FerruleRepeated([])));
            StructuralEmptyDocumentBytes("ordered-empty", ordered,
                emptyOrdered, "<Root></Root>", false, indent);
            StructuralEmptyDocumentBytes("ordered-empty-without-text", orderedWithoutText,
                Group(Field("\u001fferrule-xml-mixed-content", new FerruleRepeated([]))), "<Root></Root>", false, indent);
            var oneOrdered = Group(Field("#text", Scalar(Text(""))),
                Field("\u001fferrule-xml-mixed-content", new FerruleRepeated([
                    Group(Field("NodeName", Scalar(Text("Code"))),
                        Field("\u001fferrule-xml-mixed-value", Scalar(Text("a"))))
                ])));
            StructuralEmptyDocumentBytes("ordered-populated", ordered, oneOrdered,
                indent ? "<Root>\n  <Code>a</Code></Root>" : "<Root><Code>a</Code></Root>", false, indent);
            StructuralEmptyDocumentBytes("ordinary-populated", StructuralEmptyRowsSchema,
                Group(Field("Row", new FerruleRepeated([
                    Group(Field("Result", Scalar(FerruleValue.FromInt64(10))))
                ]))),
                indent ? "<Output>\n  <Row>\n    <Result>10</Result>\n  </Row>\n</Output>" : "<Output><Row><Result>10</Result></Row></Output>",
                false, indent);
        }
    }

    private static void StructuralEmptyDocumentBytes(
        string label, string schema, FerruleInstance value, string expected,
        bool declaration, bool indent, string? defaultNamespace = null)
    {
        var text = FerruleXml.SerializeDocumentEmbedded(schema, value, declaration, indent, defaultNamespace, null);
        var prepared = FerruleXml.PrepareDocumentEmbeddedUtf8(schema, value, declaration, indent, defaultNamespace, null);
        var bytes = prepared.ToByteArray();
        Console.WriteLine(JsonSerializer.Serialize(new
        {
            Case = label, declaration, indent, defaultNamespace, Text = text,
            Utf8Hex = Convert.ToHexString(bytes), prepared.Utf8ByteCount,
        }));
        var literalBytes = Encoding.UTF8.GetBytes(expected);
        Equal(expected, text);
        Equal(literalBytes.Length, prepared.Utf8ByteCount);
        Equal(true, literalBytes.AsSpan().SequenceEqual(bytes));
    }
}
