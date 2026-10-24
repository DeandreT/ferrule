using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void MapForceXmlDocumentBytes()
    {
        static object Scalar(string name) => new { name, kind = new { kind = "scalar", ty = "string" } };
        string descriptor = JsonSerializer.Serialize(new { name = "Root", kind = new { kind = "group", children = new[] { Scalar("Text"), Scalar("Item") } } });
        static FerruleScalar Text(string text) => new(FerruleValue.FromString(text));
        var value = new FerruleGroup([new("Item", new FerruleMappedSequence([Text("one"), Text("")])), new("Text", Text("A\rB\nC\r\n<&>\"'é😀"))]);
        Equal("<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<Root>\r\n\t<Text>A\r\nB\r\nC\r\n&lt;&amp;&gt;\"'é😀</Text>\r\n\t<Item>one</Item>\r\n\t<Item />\r\n</Root>", Encoding.UTF8.GetString(FerruleMapForceXml.SerializeDocument(descriptor, value, null)));
        Throws<ArgumentException>(() => FerruleMapForceXml.SerializeDocument(descriptor, new FerruleGroup([new("Text", Text("\0"))]), null));
        Throws<InvalidOperationException>(() => FerruleMapForceXml.SerializeDocument(descriptor, new FerruleGroup([new("Other", Text("value"))]), null));
        Throws<InvalidOperationException>(() => FerruleMapForceXml.SerializeDocument(descriptor, new FerruleGroup([new("Item", new FerruleRepeated([new FerruleRepeated([Text("value")])]))]), null));
    }
}
