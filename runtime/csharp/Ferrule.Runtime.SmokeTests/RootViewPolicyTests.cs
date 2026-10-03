using Ferrule.Runtime;
using System.Text.Json;
namespace Ferrule.Runtime.SmokeTests;
internal static partial class Program
{
    private static void RootViewPaddedFactsAndClone()
    {
        foreach (var (literal, identity) in new[] { (" Derived ", "Derived"),
            ("  p:Derived  ", "{urn:owned}Derived"), (" Derived ", "{urn:default}Derived") })
        {
            var origin = FerruleXmlTypeOrigin.ExplicitPadded(literal, identity);
            var source = Group(Field("Code", Scalar(Text("ordinary")))).WithXmlTypeOrigin(origin);
            Equal(false, FerrulePrimaryRoot.XmlTypeEquals(source, identity));
            var copy = ScopeContext.FromSource(source).CopyCurrentGroup();
            Equal(FerruleXmlTypeOriginKind.ExplicitPadded, copy.XmlTypeOrigin.Kind);
            Equal(literal, copy.XmlTypeOrigin.Literal);
            Equal(identity, copy.XmlTypeOrigin.Identity);
            var projected = new FerruleGroup(copy.Fields);
            Equal(FerruleXmlTypeOriginKind.Unknown, projected.XmlTypeOrigin.Kind);
            Equal(false, FerrulePrimaryRoot.XmlTypeEquals(copy, "Other"));
        }
    }
    private static void RootViewPaddedConstructorRefusals()
    {
        foreach (var (literal, identity) in new[] { ("Derived", "Derived"), (" Derived ", "Other"),
            (" p:Derived ", "Derived"), (" p::Derived ", "{urn:x}Derived"),
            (" De rived ", "Derived"), ("\u2000Derived\u2000", "Derived"),
            (" p:Derived ", "{bad ns}Derived"), (" " + new string('A', 4095) + " ", new string('A', 4095)) })
        {
            try { _ = FerruleXmlTypeOrigin.ExplicitPadded(literal, identity); throw new Exception("invalid padded fact accepted"); }
            catch (ArgumentException) { }
        }
    }
    private static void RootViewPaddedJsonData()
    {
        const string schema = """{"name":"Root","kind":{"kind":"group","children":[{"name":"Code","kind":{"kind":"scalar","ty":"string"}},{"name":"\u001fferrule-xml-type-origin","kind":{"kind":"scalar","ty":"string"}}]}}""";
        var ordinary = Group(Field("Code", Scalar(Text("ordinary"))), Field("\u001fferrule-xml-type-origin", Scalar(Text("real data"))));
        var annotated = ordinary.WithXmlTypeOrigin(FerruleXmlTypeOrigin.ExplicitPadded(" Derived ", "Derived"));
        var text = FerruleJson.Serialize(schema, annotated);
        Equal(FerruleJson.Serialize(schema, ordinary), text);
        using var json = JsonDocument.Parse(text);
        Equal("real data", json.RootElement.GetProperty("\u001fferrule-xml-type-origin").GetString());
        Equal(2, json.RootElement.EnumerateObject().Count());
        Equal(FerruleXmlTypeOriginKind.Unknown, ((FerruleGroup)FerruleJson.Parse(schema, text)).XmlTypeOrigin.Kind);
        var bytes = FerruleJson.SerializeBytes(schema, annotated);
        Equal(text, System.Text.Encoding.UTF8.GetString(bytes));
        Equal(FerruleXmlTypeOriginKind.Unknown, ((FerruleGroup)FerruleJson.ParseBytes(schema, bytes)).XmlTypeOrigin.Kind);
    }
}
