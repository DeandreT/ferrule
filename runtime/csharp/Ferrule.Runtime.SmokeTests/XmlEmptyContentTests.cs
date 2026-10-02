using Ferrule.Runtime;
using System.Xml;
using System.Xml.Schema;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void XmlEmptyContent()
    {
        const string empty = """{"name":"Root","kind":{"kind":"group","children":[]}}""";
        const string attribute = """{"name":"Root","kind":{"kind":"group","children":[{"name":"Code","attribute":true,"kind":{"kind":"scalar","ty":"string"}}]}}""";
        const string xsd = """<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:element name="Root"><xs:complexType><xs:attribute name="Code" type="xs:string"/></xs:complexType></xs:element></xs:schema>""";
        var schemas = new XmlSchemaSet();
        using (var source = XmlReader.Create(new StringReader(xsd)))
        {
            schemas.Add(null, source);
        }
        schemas.Compile();
        foreach (var (schema, value) in new[]
        {
            (empty, Group()),
            (attribute, Group()),
            (attribute, Group(Field("Code", Scalar(FerruleValue.FromString("a"))))),
        })
        {
            foreach (var indent in new[] { false, true })
            {
                var xml = FerruleXml.SerializeEmbedded(1, schema, value, false, indent, null).StringValue!;
                Equal(true, xml.EndsWith("/>", StringComparison.Ordinal));
                var settings = new XmlReaderSettings { ValidationType = ValidationType.Schema, Schemas = schemas };
                settings.ValidationEventHandler += (_, error) => throw error.Exception;
                using var reader = XmlReader.Create(new StringReader(xml), settings);
                while (reader.Read())
                {
                    Equal(true, reader.NodeType is not (XmlNodeType.Text or XmlNodeType.Whitespace or XmlNodeType.SignificantWhitespace));
                }
            }
        }
    }

    private static void XmlEmptyTypedTextErrors()
    {
        foreach (var (type, fixedValue) in new[]
        {
            ("int", ""), ("float", ""), ("bool", ""), ("string", ",\"fixed\":\"required\""),
        })
        {
            var schema = "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"#text\",\"text\":true,\"kind\":{\"kind\":\"scalar\",\"ty\":\"" + type + "\"}" + fixedValue + "}]}}";
            foreach (var indent in new[] { false, true })
            {
                Error(FerruleRuntimeError.XmlSerialization, () => FerruleXml.SerializeEmbedded(
                    1, schema, Group(Field("#text", Scalar(FerruleValue.FromString("")))), false, indent, null));
            }
        }
    }
}
