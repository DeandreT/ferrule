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

    private static void MapForceXmlPreparedStream()
    {
        string descriptor = "{\"name\":\"Root\",\"kind\":{\"kind\":\"group\",\"children\":[{\"name\":\"Text\",\"kind\":{\"kind\":\"scalar\",\"ty\":\"string\"}}]}}";
        static FerruleGroup Body(string text) => new([new("Text", new FerruleScalar(FerruleValue.FromString(text)))]);
        var prepared = FerruleMapForceXml.PrepareDocument(descriptor, Body("alpha"), null);
        var output = new ObservedXmlOutput();
        prepared.WriteTo(output);
        Equal(4, output.Flushes);
        Equal(false, output.Closed);
        Equal("<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<Root>\r\n\t<Text>alpha</Text>\r\n</Root>", Encoding.UTF8.GetString(output.ToArray()));
        output.WriteByte(0x21);
        var failed = new ObservedXmlOutput { FailWrite = true };
        Throws<IOException>(() => prepared.WriteTo(failed));
        Equal(1, failed.Writes); Equal(2, failed.Flushes); Equal(false, failed.Closed);
        prepared.WriteTo(new ObservedXmlOutput());

        var invalid = FerruleMapForceXml.PrepareDocument(descriptor, Body("x\0after"), null);
        var prefix = new ObservedXmlOutput();
        Throws<ArgumentException>(() => invalid.WriteTo(prefix));
        Equal("<?xml version=\"1.0\" encoding=\"utf-8\"?>\r\n<Root>\r\n\t<Text>", Encoding.UTF8.GetString(prefix.ToArray()));
        Equal(2, prefix.Flushes); Equal(false, prefix.Closed);
    }

    private sealed class ObservedXmlOutput : MemoryStream
    {
        public int Flushes, Writes;
        public bool Closed, FailWrite;
        public override void Write(byte[] buffer, int offset, int count)
        { Writes++; if (FailWrite) throw new IOException("injected write"); base.Write(buffer, offset, count); }
        public override void Flush() { Flushes++; base.Flush(); }
        protected override void Dispose(bool disposing) { Closed = true; base.Dispose(disposing); }
    }
}
