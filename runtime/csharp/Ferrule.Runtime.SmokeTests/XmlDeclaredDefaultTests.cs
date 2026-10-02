using Ferrule.Runtime;
using System.Text.Json.Nodes;
using System.Xml;
using System.Xml.Linq;
using System.Xml.Schema;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string DeclaredDefaultSchema = """{"name":"Root","repeating":false,"xml_type_alternatives":true,"xml_default_type":"A","kind":{"kind":"group","children":[{"name":"Value","repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"A","members":["Value"]},{"name":"B","members":["Value","Extra"]}]}}""";

    private static string DeclaredDefaultSerialize(string schema, FerruleInstance instance, bool indent, int mode) =>
        (mode == 0
            ? FerruleXml.Serialize(91, schema, instance, false, indent, null)
            : FerruleXml.SerializeEmbedded(91, (mode == 2 ? "FERRULE-EMBEDDED-SCHEMA/2\n" : "") + schema, instance, false, indent, null)).StringValue;

    private static void XmlDeclaredDefaultIdentity()
    {
        foreach (var schema in new[] { DeclaredDefaultSchema, """{"name":"Root","repeating":false,"xml_type_alternatives":true,"xml_default_type":"A","kind":{"kind":"group","children":[{"name":"Value","repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"A","members":["Value"]},{"name":"B","members":["Value"]}]}}""", """{"name":"Root","repeating":false,"xml_type_alternatives":true,"xml_default_type":"A","kind":{"kind":"group","children":[{"name":"Value","repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"A","members":["Value","Extra"]},{"name":"B","members":["Value"]}],"xml_restricted_alternatives":["B"]}}""", """{"name":"Root","xml_namespace":{"kind":"qualified","uri":"urn:declared"},"repeating":false,"xml_type_alternatives":true,"xml_default_type":"{urn:declared}A","kind":{"kind":"group","children":[{"name":"Value","xml_namespace":{"kind":"unqualified"},"repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","xml_namespace":{"kind":"unqualified"},"repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"{urn:declared}A","members":["Value"]},{"name":"{urn:declared}B","members":["Value","Extra"]}]}}""", """{"name":"Root","repeating":false,"xml_type_alternatives":true,"xml_default_type":"A","kind":{"kind":"group","children":[{"name":"Value","repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"A","members":["Value"]}]}}""" })
        {
            var identity = schema == """{"name":"Root","xml_namespace":{"kind":"qualified","uri":"urn:declared"},"repeating":false,"xml_type_alternatives":true,"xml_default_type":"{urn:declared}A","kind":{"kind":"group","children":[{"name":"Value","xml_namespace":{"kind":"unqualified"},"repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","xml_namespace":{"kind":"unqualified"},"repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"{urn:declared}A","members":["Value"]},{"name":"{urn:declared}B","members":["Value","Extra"]}]}}""" ? "{urn:declared}A" : "A";
            foreach (var indent in new[] { false, true })
            foreach (var mode in new[] { 0, 1, 2 })
            {
                var plain = XElement.Parse(DeclaredDefaultSerialize(schema, Group(Field("Value", Scalar(Text("x")))), indent, mode));
                Equal((string?)null, (string?)plain.Attribute(XName.Get("type", "http://www.w3.org/2001/XMLSchema-instance")));
                Equal("x", plain.Element("Value")!.Value);
                var marked = XElement.Parse(DeclaredDefaultSerialize(schema, Group(Field("Value", Scalar(Text("x"))), Field("\u001fferrule-xml-type", Scalar(Text(identity)))), indent, mode));
                Equal(true, marked.Attribute(XName.Get("type", "http://www.w3.org/2001/XMLSchema-instance")) is not null);
                Equal("x", marked.Element("Value")!.Value);
            }
        }
        foreach (var indent in new[] { false, true })
        foreach (var mode in new[] { 0, 1, 2 })
        {
            var derived = XElement.Parse(DeclaredDefaultSerialize(DeclaredDefaultSchema, Group(Field("Value", Scalar(Text("x"))), Field("Extra", Scalar(Text("e")))), indent, mode));
            Equal("B", (string?)derived.Attribute(XName.Get("type", "http://www.w3.org/2001/XMLSchema-instance")));
            Equal("e", derived.Element("Extra")!.Value);
            var schema = new XmlSchemaSet { XmlResolver = null };
            using var source = XmlReader.Create(new StringReader("""<xs:schema xmlns:xs="http://www.w3.org/2001/XMLSchema"><xs:complexType name="A"><xs:sequence><xs:element name="Value" type="xs:string" minOccurs="0"/></xs:sequence></xs:complexType><xs:complexType name="B"><xs:complexContent><xs:extension base="A"><xs:sequence><xs:element name="Extra" type="xs:string" minOccurs="0"/></xs:sequence></xs:extension></xs:complexContent></xs:complexType><xs:element name="Root" type="A"/></xs:schema>"""), new XmlReaderSettings { XmlResolver = null, DtdProcessing = DtdProcessing.Prohibit });
            schema.Add(null, source);
            schema.Compile();
            var errors = new List<string>();
            new XDocument(new XElement(derived)).Validate(schema, (_, error) => errors.Add(error.Message));
            Equal(0, errors.Count);
        }
    }

    private static void XmlDeclaredDefaultMetadata()
    {
        var mutations = new Action<JsonObject>[]
        {
            node => node["xml_default_type"] = "Missing",
            node => node["xml_default_type"] = "",
            node => node["xml_default_type"] = "B",
            node => node["xml_default_type"] = 17,
            node => node["xml_default_type"] = true,
            node => node["xml_default_type"] = new JsonArray(),
            node => node["attribute"] = true,
            node => node["text"] = true,
            node => node["recursive_ref"] = "Root",
            node => node["xml_type_alternatives"] = false,
            node => node["alternative_mode"] = "inclusive",
            node => node["xml_alternative_kind"] = "substitution_group",
            node => node["kind"]!["xml_restricted_alternatives"] = new JsonArray("A"),
            node => node["kind"]!["xml_restricted_alternatives"] = new JsonArray("Missing"),
            node => node["kind"]!["xml_restricted_alternatives"] = new JsonArray("B", "B"),
            node => node["kind"]!["xml_restricted_alternatives"] = 17,
            node => node["kind"]!["alternatives"]![0]!["required"] = new JsonArray("Value"),
            node => ((JsonArray)node["kind"]!["alternatives"]![0]!["members"]!).Add("Missing"),
            node => ((JsonArray)node["kind"]!["alternatives"]![0]!["members"]!).Add("Value"),
            node => node["kind"]!["alternatives"]![1]!["name"] = "A",
            node => ((JsonArray)node["kind"]!["children"]!).Add(node["kind"]!["children"]![0]!.DeepClone()),
            node => node["kind"]!["dynamic"] = JsonNode.Parse("{\"name\":\"Dynamic\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}"),
            node => node["kind"] = JsonNode.Parse("{\"kind\":\"scalar\",\"ty\":\"string\"}"),
            node => node["kind"]!["alternatives"] = new JsonArray(),
        };
        foreach (var mutation in mutations)
        {
            var node = (JsonObject)JsonNode.Parse(DeclaredDefaultSchema)!;
            mutation(node);
            foreach (var indent in new[] { false, true })
            foreach (var mode in new[] { 0, 1, 2 })
            {
                var error = Error(FerruleRuntimeError.XmlSerialization, () => DeclaredDefaultSerialize(node.ToJsonString(), Group(), indent, mode));
                Equal((uint?)91, error.Node);
                Equal(true, !string.IsNullOrEmpty(error.Detail));
            }
        }
        var nested = JsonNode.Parse(DeclaredDefaultSchema)!;
        nested["name"] = "Nested";
        nested["xml_default_type"] = "Missing";
        var absent = new JsonObject { ["name"] = "Root", ["kind"] = new JsonObject { ["kind"] = "group", ["children"] = new JsonArray(nested) } };
        var absentError = Error(FerruleRuntimeError.XmlSerialization, () => DeclaredDefaultSerialize(absent.ToJsonString(), Group(), false, 2));
        Equal((uint?)91, absentError.Node);
    }

    private static void XmlDeclaredDefaultLegacyAndMarkers()
    {
        foreach (var useNull in new[] { false, true })
        {
            var legacy = (JsonObject)JsonNode.Parse(DeclaredDefaultSchema)!;
            if (useNull) legacy["xml_default_type"] = null;
            else legacy.Remove("xml_default_type");
            foreach (var indent in new[] { false, true })
            foreach (var mode in new[] { 0, 1, 2 })
            {
                var xml = XElement.Parse(DeclaredDefaultSerialize(legacy.ToJsonString(), Group(Field("Value", Scalar(Text("x")))), indent, mode));
                Equal("A", (string?)xml.Attribute(XName.Get("type", "http://www.w3.org/2001/XMLSchema-instance")));
                Error(FerruleRuntimeError.XmlSerialization, () => DeclaredDefaultSerialize("""{"name":"Root","repeating":false,"xml_type_alternatives":true,"kind":{"kind":"group","children":[{"name":"Value","repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"A","members":["Value"]},{"name":"B","members":["Value"]}]}}""", Group(Field("Value", Scalar(Text("x")))), indent, mode));
                foreach (var marker in new[] { Scalar(Text("Missing")), Scalar(FerruleValue.FromInt64(17)), Scalar(FerruleValue.Null) })
                    Error(FerruleRuntimeError.XmlSerialization, () => DeclaredDefaultSerialize(DeclaredDefaultSchema, Group(Field("Value", Scalar(Text("x"))), Field("\u001fferrule-xml-type", marker)), indent, mode));
                Error(FerruleRuntimeError.XmlSerialization, () => DeclaredDefaultSerialize(DeclaredDefaultSchema, Group(Field("Value", Scalar(Text("x"))), Field("Extra", Scalar(Text("e"))), Field("\u001fferrule-xml-type", Scalar(Text("A")))), indent, mode));
            }
        }
    }

    private static void XmlDeclaredDefaultOrderedAndRepeated()
    {
        const string ordered = """{"name":"Root","repeating":false,"xml_type_alternatives":true,"xml_default_type":"A","xml_repeating_choices":[{"required":false,"members":["X","Y"]}],"kind":{"kind":"group","children":[{"name":"X","repeating":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Y","repeating":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"A","members":["X","Y"]},{"name":"B","members":["X","Y","Extra"]}]}}""";
        var stream = new FerruleRepeated([
            Group(Field("NodeName", Scalar(Text("Y"))), Field("#text", Scalar(Text(""))), Field("\u001fferrule-xml-mixed-value", Scalar(Text("y")))),
            Group(Field("NodeName", Scalar(Text("X"))), Field("#text", Scalar(Text(""))), Field("\u001fferrule-xml-mixed-value", Scalar(Text("x"))))]);
        var input = Group(Field("X", new FerruleRepeated([Scalar(Text("x"))])), Field("Y", new FerruleRepeated([Scalar(Text("y"))])), Field("\u001fferrule-xml-mixed-content", stream));
        foreach (var indent in new[] { false, true })
        foreach (var mode in new[] { 0, 1, 2 })
        {
            Error(FerruleRuntimeError.XmlSerialization, () => DeclaredDefaultSerialize(ordered, input, indent, mode));
            var derived = Group(Field("X", new FerruleRepeated([Scalar(Text("x"))])), Field("Y", new FerruleRepeated([Scalar(Text("y"))])), Field("Extra", Scalar(Text("e"))), Field("\u001fferrule-xml-type", Scalar(Text("B"))), Field("\u001fferrule-xml-mixed-content", stream));
            Error(FerruleRuntimeError.XmlSerialization, () => DeclaredDefaultSerialize(ordered, derived, indent, mode));
            var malformed = Group(Field("\u001fferrule-xml-mixed-content", Scalar(Text("invalid"))));
            Error(FerruleRuntimeError.XmlSerialization, () => DeclaredDefaultSerialize(ordered, malformed, indent, mode));
            var repeated = Group(Field("Item", new FerruleRepeated([
                Group(Field("Value", Scalar(Text("plain")))),
                Group(Field("Value", Scalar(Text("marked"))), Field("\u001fferrule-xml-type", Scalar(Text("A"))))])));
            var children = XElement.Parse(DeclaredDefaultSerialize("""{"name":"Root","repeating":false,"kind":{"kind":"group","children":[{"name":"Item","repeating":true,"xml_type_alternatives":true,"xml_default_type":"A","kind":{"kind":"group","children":[{"name":"Value","repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}},{"name":"Extra","repeating":false,"xml_optional":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"A","members":["Value"]},{"name":"B","members":["Value","Extra"]}]}}]}}""", repeated, indent, mode)).Elements("Item").ToArray();
            Equal(2, children.Length);
            Equal((string?)null, (string?)children[0].Attribute(XName.Get("type", "http://www.w3.org/2001/XMLSchema-instance")));
            Equal("A", (string?)children[1].Attribute(XName.Get("type", "http://www.w3.org/2001/XMLSchema-instance")));
        }
    }
}
