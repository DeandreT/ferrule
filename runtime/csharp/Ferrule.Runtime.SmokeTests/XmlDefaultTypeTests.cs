using Ferrule.Runtime;
namespace Ferrule.Runtime.SmokeTests;
internal static partial class Program
{
    private static void XmlDefaultTypeMetadata()
    {
        const string value = "{\"ID\":\"7\",\"First\":\"A\",\"Last\":\"B\"}";
        var valid = XmlTypeAlternativeJsonSchema.Replace(
            "\"xml_type_alternatives\":true", "\"xml_type_alternatives\":true,\"xml_default_type\":\"ContactType\"", StringComparison.Ordinal);
        Equal(FerruleJson.Serialize(XmlTypeAlternativeJsonSchema, FerruleJson.Parse(XmlTypeAlternativeJsonSchema, value)),
            FerruleJson.Serialize(valid, FerruleJson.Parse(valid, value)));
        foreach (var defaultValue in new[] { "\"\"", "\"Missing\"", "\"ContactTypeWithAddress\"", "17", "true", "[]" })
        {
            var invalid = valid.Replace("\"xml_default_type\":\"ContactType\"", "\"xml_default_type\":" + defaultValue, StringComparison.Ordinal);
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.Parse(invalid, value));
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.Serialize(invalid, Group(Field("ID", Scalar(Text("7"))))));
            var nested = "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[" + invalid + "]}}";
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.Parse(nested, "{}"));
        }
        var unmarked = valid.Replace("\"xml_type_alternatives\":true", "\"xml_type_alternatives\":false", StringComparison.Ordinal);
        Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.Parse(unmarked, value));
        foreach (var role in new[] { "\"attribute\":true", "\"text\":true", "\"xml_alternative_kind\":\"substitution_group\"", "\"alternative_mode\":\"inclusive\"" })
        {
            var invalid = valid.Replace("\"name\":\"Contact\"", "\"name\":\"Contact\"," + role, StringComparison.Ordinal);
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.Parse(invalid, value));
        }
        var nullDefault = valid.Replace("\"xml_default_type\":\"ContactType\"", "\"xml_default_type\":null", StringComparison.Ordinal);
        Equal(FerruleJson.Serialize(XmlTypeAlternativeJsonSchema, FerruleJson.Parse(XmlTypeAlternativeJsonSchema, value)),
            FerruleJson.Serialize(nullDefault, FerruleJson.Parse(nullDefault, value)));
        foreach (var restrictions in new[] { "[\"ContactType\"]", "[\"ContactTypeWithAddress\"]", "[\"Missing\"]", "[\"ContactTypeWithAddress\",\"ContactTypeWithAddress\"]", "17" })
        {
            var invalid = valid.Replace("\"alternatives\":[", "\"xml_restricted_alternatives\":" + restrictions + ",\"alternatives\":[", StringComparison.Ordinal);
            Error(FerruleRuntimeError.JsonBoundary, () => FerruleJson.Parse(invalid, value));
        }
        var restricted = valid.Replace("\"xml_default_type\":\"ContactType\"", "\"xml_default_type\":\"ContactTypeWithAddress\"", StringComparison.Ordinal)
            .Replace("\"alternatives\":[", "\"xml_restricted_alternatives\":[\"ContactType\"],\"alternatives\":[", StringComparison.Ordinal);
        FerruleJson.Parse(restricted, value);
        var qualified = valid.Replace("ContactType", "{urn:ferrule:default}ContactType", StringComparison.Ordinal);
        FerruleJson.Parse(qualified, value);
    }
}
