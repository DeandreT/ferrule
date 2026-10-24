using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void DeferredXmlPreparation()
    {
        static FerruleGroup Group(string name, string value) => new([new(name, new FerruleScalar(FerruleValue.FromString(value)))]);
        var descriptor = JsonSerializer.Serialize(new { name = "Root", kind = new { kind = "group", children = new[] {
            new { name = "Early", kind = new { kind = "scalar", ty = "string" } },
            new { name = "Late", kind = new { kind = "scalar", ty = "string" } } } } });
        int calls = 0;
        var prepared = new FerruleDeferredXmlDocument(descriptor, new FerruleDocument("one.xml", Group("Early", "saved")), () => Group("Late", (++calls).ToString()));
        for (int i = 1; i <= 2; i++)
        {
            var document = prepared.Materialize();
            Equal("one.xml", document.Path);
            var fields = ((FerruleGroup)document.Value).Fields;
            Equal("saved", ((FerruleScalar)fields.Single(f => f.Name == "Early").Value).Value.StringValue);
            Equal(i.ToString(), ((FerruleScalar)fields.Single(f => f.Name == "Late").Value).Value.StringValue);
        }
        var overlap = new FerruleDeferredXmlDocument(descriptor, new FerruleDocument("one.xml", Group("Early", "saved")), () => Group("Early", "saved"));
        Throws<InvalidOperationException>(() => overlap.Materialize());
        var unknown = new FerruleDeferredXmlDocument(descriptor, new FerruleDocument("one.xml", Group("Early", "saved")), () => Group("Other", "value"));
        Throws<InvalidOperationException>(() => unknown.Materialize());
    }
}
