using System.Xml.Linq;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string ScalarOutputSchema = """
        {"name":"Envelope","kind":{"kind":"group","children":[
          {"name":"Amount","repeating":true,"kind":{"kind":"scalar","ty":"float"}},
          {"name":"Label","kind":{"kind":"scalar","ty":"string"}}]}}
        """;
    private const string ScalarOutputPolicy = """
        {"mode":"suppress_cast_in_subtree_v1","declarations":["Amount"]}
        """;

    private static FerruleInstance ScalarOutputValue(FerruleValue value) => Group(
        Field("Amount", new FerruleRepeated(new[] { Scalar(value), Scalar(FerruleValue.FromInt64(17)) })),
        Field("Label", Scalar(Text("safe"))));

    private static void XmlScalarOutputPolicyCases()
    {
        foreach (var text in new[] { "9007199254740993", "\t 2.5 \n", "not-a-number", "00029", "&<\r\t\n" })
        {
            var xml = FerruleXml.SerializeDocumentEmbedded(ScalarOutputSchema, ScalarOutputValue(Text(text)), true, true, null, null, ScalarOutputPolicy);
            var document = XDocument.Parse(xml, LoadOptions.PreserveWhitespace);
            var children = document.Root!.Elements().ToArray();
            Equal(3, children.Length);
            Equal("Amount", children[0].Name.LocalName);
            Equal(text, children[0].Value);
            Equal("17", children[1].Value);
            Equal("safe", children[2].Value);
        }
        Equal("0.0000001", XDocument.Parse(FerruleXml.SerializeDocumentEmbedded(ScalarOutputSchema,
            ScalarOutputValue(FerruleValue.FromDouble(0.0000001)), true, true, null, null, ScalarOutputPolicy)).Root!.Elements().First().Value);
        Equal("9007199254740993", XDocument.Parse(FerruleXml.SerializeDocumentEmbedded(ScalarOutputSchema,
            ScalarOutputValue(FerruleValue.FromInt64(9007199254740993)), true, true, null, null, ScalarOutputPolicy)).Root!.Elements().First().Value);
        foreach (var policy in new[] { "{}", "{\"mode\":\"unknown\",\"declarations\":[\"Amount\"]}", "{\"mode\":\"suppress_cast_in_subtree_v1\",\"declarations\":[]}" })
        {
            try {
                FerruleXml.SerializeDocumentEmbedded(ScalarOutputSchema, ScalarOutputValue(Text("29")), true, true, null, null, policy);
                throw new InvalidOperationException("invalid scalar output policy accepted");
            } catch (FerruleXmlBoundaryException error) { Equal(FerruleXmlBoundaryErrorKind.Output, error.Kind); }
        }
        foreach (var value in new[] { Text("\0"), FerruleValue.FromDouble(double.PositiveInfinity), FerruleValue.FromBoolean(true) })
        {
            try {
                FerruleXml.SerializeDocumentEmbedded(ScalarOutputSchema, ScalarOutputValue(value), true, true, null, null, ScalarOutputPolicy);
                throw new InvalidOperationException("unsafe scalar output accepted");
            } catch (FerruleXmlBoundaryException error) { Equal(FerruleXmlBoundaryErrorKind.Output, error.Kind); }
        }
        var strict = FerruleXml.SerializeDocumentEmbedded(ScalarOutputSchema, ScalarOutputValue(Text("00029")), true, true, null, null);
        Equal("29", XDocument.Parse(strict).Root!.Elements().First().Value);
        XmlDocumentScalarOutputPolicyCases();
    }

    private const string DocumentOutputSchema = """
        {"name":"Orders","kind":{"kind":"group","children":[
          {"name":"Order","repeating":true,"kind":{"kind":"group","children":[
            {"name":"Id","kind":{"kind":"scalar","ty":"int"}},
            {"name":"Price","kind":{"kind":"scalar","ty":"float"}},
            {"name":"Paid","kind":{"kind":"scalar","ty":"bool"}},
            {"name":"Note","repeating":true,"kind":{"kind":"scalar","ty":"string"}}]}}]}}
        """;
    private const string DocumentOutputPolicy = """{"mode":"suppress_cast_in_document_v1"}""";

    private static FerruleInstance DocumentOrder(FerruleInstance price) => Group(
        Field("Id", Scalar(Text("007"))),
        Field("Price", price),
        Field("Paid", Scalar(FerruleValue.FromBoolean(true))),
        Field("Note", new FerruleRepeated(new[] { Scalar(FerruleValue.FromDouble(0.0000001)), Scalar(FerruleValue.FromInt64(3)) })));

    private static FerruleInstance DocumentOrders(params FerruleInstance[] orders) =>
        Group(Field("Order", new FerruleRepeated(orders)));

    private static void XmlDocumentScalarOutputPolicyCases()
    {
        var xml = FerruleXml.SerializeDocumentEmbedded(DocumentOutputSchema,
            DocumentOrders(DocumentOrder(Scalar(Text("00025.00"))), DocumentOrder(Scalar(FerruleValue.FromDouble(2.5)))),
            false, false, null, null, DocumentOutputPolicy);
        Equal("<Orders><Order><Id>007</Id><Price>00025.00</Price><Paid>true</Paid><Note>0.0000001</Note><Note>3</Note></Order>" +
            "<Order><Id>007</Id><Price>2.5</Price><Paid>true</Paid><Note>0.0000001</Note><Note>3</Note></Order></Orders>", xml);
        const string singleOrderSchema = """
            {"name":"Orders","kind":{"kind":"group","children":[
              {"name":"Order","kind":{"kind":"group","children":[
                {"name":"Id","kind":{"kind":"scalar","ty":"int"}},
                {"name":"Price","kind":{"kind":"scalar","ty":"float"}},
                {"name":"Paid","kind":{"kind":"scalar","ty":"bool"}},
                {"name":"Note","repeating":true,"kind":{"kind":"scalar","ty":"string"}}]}}]}}
            """;
        Equal("<Orders></Orders>", FerruleXml.SerializeDocumentEmbedded(singleOrderSchema,
            Group(Field("Order", new FerruleMappedSequence(Array.Empty<FerruleInstance>()))), false, false, null, null, DocumentOutputPolicy));
        Equal("<Orders><Order><Id>007</Id><Price>00025.00</Price><Paid>true</Paid><Note>0.0000001</Note><Note>3</Note></Order></Orders>",
            FerruleXml.SerializeDocumentEmbedded(singleOrderSchema, Group(Field("Order", new FerruleMappedSequence(new[] { DocumentOrder(Scalar(Text("00025.00"))) }))),
                false, false, null, null, DocumentOutputPolicy));
        foreach (var policy in new[] { """{"mode":"suppress_cast_in_document_v1","declarations":["Price"]}""", """{"mode":"suppress_cast_in_document_v1","extra":true}""" })
        {
            try {
                FerruleXml.SerializeDocumentEmbedded(DocumentOutputSchema, DocumentOrders(DocumentOrder(Scalar(Text("1")))), false, false, null, null, policy);
                throw new InvalidOperationException("invalid document scalar output policy accepted");
            } catch (FerruleXmlBoundaryException error) { Equal(FerruleXmlBoundaryErrorKind.Output, error.Kind); }
        }
        foreach (var value in new FerruleInstance[] { DocumentOrders(DocumentOrder(Group())), Group(Field("Order", DocumentOrder(Scalar(Text("1"))))),
            DocumentOrders(DocumentOrder(Scalar(FerruleValue.FromDouble(double.NaN)))), DocumentOrders(DocumentOrder(Scalar(Text("\0")))) })
        {
            try {
                FerruleXml.SerializeDocumentEmbedded(DocumentOutputSchema, value, false, false, null, null, DocumentOutputPolicy);
                throw new InvalidOperationException("malformed document scalar output accepted");
            } catch (FerruleXmlBoundaryException error) { Equal(FerruleXmlBoundaryErrorKind.Output, error.Kind); }
        }
    }
}
