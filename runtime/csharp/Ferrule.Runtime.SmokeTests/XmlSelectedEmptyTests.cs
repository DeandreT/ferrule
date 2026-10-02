using Ferrule.Runtime;
using System.Text.Json.Nodes;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private const string SelectedEmptySchema = """{"name":"Root","xml_type_alternatives":true,"xml_default_type":"A","kind":{"kind":"group","children":[{"name":"Value","xml_optional":true,"kind":{"kind":"scalar","ty":"string"}}],"alternatives":[{"name":"A","members":[]},{"name":"B","members":["Value"]}]}}""";

    private static string SelectedEmptySerialize(string schema, FerruleInstance value, bool indent, int mode) =>
        (mode == 0
            ? FerruleXml.Serialize(93, schema, value, false, indent, null)
            : FerruleXml.SerializeEmbedded(93, (mode == 2 ? "FERRULE-EMBEDDED-SCHEMA/2\n" : "") + schema, value, false, indent, null)).StringValue;

    private static void XmlSelectedEmptyIdentity()
    {
        foreach (var indent in new[] { false, true })
        foreach (var mode in new[] { 0, 1, 2 })
        {
            Equal("<Root/>", SelectedEmptySerialize(SelectedEmptySchema, Group(), indent, mode));
            var marked = Group(Field("\u001fferrule-xml-type", Scalar(Text("A"))));
            Equal("<Root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"A\"/>", SelectedEmptySerialize(SelectedEmptySchema, marked, indent, mode));
            var legacy = (JsonObject)JsonNode.Parse(SelectedEmptySchema)!;
            legacy.Remove("xml_default_type");
            Equal("<Root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"A\"/>", SelectedEmptySerialize(legacy.ToJsonString(), marked, indent, mode));
            Error(FerruleRuntimeError.XmlSerialization, () => SelectedEmptySerialize(SelectedEmptySchema, Group(Field("\u001fferrule-xml-type", Scalar(Text("Unknown")))), indent, mode));
            Error(FerruleRuntimeError.XmlSerialization, () => SelectedEmptySerialize(SelectedEmptySchema, Group(Field("\u001fferrule-xml-type", Scalar(Text("A"))), Field("Value", Scalar(Text("foreign")))), indent, mode));
        }
    }

    private static void XmlSelectedAttributeOnly()
    {
        var schema = (JsonObject)JsonNode.Parse(SelectedEmptySchema)!;
        ((JsonArray)schema["kind"]!["children"]!).Insert(0, JsonNode.Parse("""{"name":"Code","attribute":true,"xml_attribute_required":true,"kind":{"kind":"scalar","ty":"string"}}"""));
        foreach (var alternative in (JsonArray)schema["kind"]!["alternatives"]!)
            ((JsonArray)alternative!["members"]!).Insert(0, "Code");
        foreach (var indent in new[] { false, true })
        foreach (var mode in new[] { 0, 1, 2 })
        foreach (var text in new[] { "", "kept" })
        {
            Equal($"<Root Code=\"{text}\"/>", SelectedEmptySerialize(schema.ToJsonString(), Group(Field("Code", Scalar(Text(text)))), indent, mode));
            Equal($"<Root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"A\" Code=\"{text}\"/>", SelectedEmptySerialize(schema.ToJsonString(), Group(Field("Code", Scalar(Text(text))), Field("\u001fferrule-xml-type", Scalar(Text("A")))), indent, mode));
        }
    }

    private static void XmlSelectedOrdinaryFormatting()
    {
        const string ordinary = """{"name":"Root","kind":{"kind":"group","children":[{"name":"Value","xml_optional":true,"kind":{"kind":"scalar","ty":"string"}}]}}""";
        foreach (var indent in new[] { false, true })
        foreach (var mode in new[] { 0, 1, 2 })
        {
            Equal("<Root></Root>", SelectedEmptySerialize(ordinary, Group(), indent, mode));
            Equal("<Root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"B\"></Root>", SelectedEmptySerialize(SelectedEmptySchema, Group(Field("\u001fferrule-xml-type", Scalar(Text("B")))), indent, mode));
            foreach (var text in new[] { "", "kept" })
            {
                var xml = SelectedEmptySerialize(SelectedEmptySchema, Group(Field("Value", Scalar(Text(text))), Field("\u001fferrule-xml-type", Scalar(Text("B")))), indent, mode);
                var newline = indent ? "\n" : "";
                var spaces = indent ? "  " : "";
                Equal($"<Root xmlns:xsi=\"http://www.w3.org/2001/XMLSchema-instance\" xsi:type=\"B\">{newline}{spaces}<Value>{text}</Value>{newline}</Root>", xml);
            }
        }
    }
}
