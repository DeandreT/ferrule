using System.Globalization;
using System.Text;
using System.Xml;
using System.Xml.Linq;

namespace Ferrule.Runtime;

// Closed, generated instant-period projection. References are data, not locations to load.
internal sealed record XbrlTable(
    string Rows,
    string Concept,
    string Uri,
    string Taxonomy,
    bool Integer
)
{
    private const int Bytes = 1024 * 1024,
        Contexts = 1024,
        Text = 16384;
    private static readonly XNamespace I = "http://www.xbrl.org/2003/instance",
        L = "http://www.xbrl.org/2003/linkbase",
        X = "http://www.w3.org/1999/xlink";
    private static readonly UTF8Encoding Utf8 = new(false, true);

    private static FormatException Invalid() =>
        new("Unsupported or over-limit XBRL instance table.");

    private static void Require(bool condition)
    {
        if (!condition)
            throw Invalid();
    }

    private static void Attributes(XElement e, params XName[] names) =>
        Require(e.Attributes().All(a => a.IsNamespaceDeclaration || names.Contains(a.Name)));

    private static string Attribute(XElement e, XName name) => Nonempty((string?)e.Attribute(name));

    private static string Nonempty(string? value) =>
        !string.IsNullOrWhiteSpace(value) && value.Length <= Text ? value : throw Invalid();

    private static string Leaf(XElement e)
    {
        Require(e.Nodes().All(n => n is XText) && e.Nodes().Count() <= 1);
        return Nonempty(e.Value);
    }

    private static XElement[] Children(XElement e, params XName[] names)
    {
        Require(
            e.Nodes().All(n => n is XElement || n is XText t && string.IsNullOrWhiteSpace(t.Value))
        );
        var kids = e.Elements().ToArray();
        Require(kids.Select(k => k.Name).SequenceEqual(names));
        return kids;
    }

    private static FerruleScalar StringField(string s) => new(FerruleValue.FromString(s));

    internal FerruleInstance Read(byte[] bytes)
    {
        Require(bytes.Length <= Bytes);
        string xml = Utf8.GetString(bytes);
        var settings = new XmlReaderSettings
        {
            DtdProcessing = DtdProcessing.Prohibit,
            XmlResolver = null,
            MaxCharactersInDocument = Bytes,
        };
        using (var scan = XmlReader.Create(new StringReader(xml), settings))
        {
            int nodes = 0,
                attrs = 0;
            while (scan.Read())
                Require(
                    ++nodes <= 32768
                        && scan.Depth < 16
                        && (attrs += scan.AttributeCount) <= 32768
                        && scan.NodeType
                            is not (XmlNodeType.Comment or XmlNodeType.ProcessingInstruction)
                );
        }
        using var reader = XmlReader.Create(new StringReader(xml), settings);
        var doc = XDocument.Load(reader, LoadOptions.PreserveWhitespace);
        Require(
            doc.Declaration?.Encoding is null
                || string.Equals(
                    doc.Declaration.Encoding,
                    "UTF-8",
                    StringComparison.OrdinalIgnoreCase
                )
        );
        var root = doc.Root ?? throw Invalid();
        Require(root.Name == I + "xbrl");
        Attributes(root);
        var contexts = new Dictionary<string, (string Scheme, string Entity, string Instant)>(
            StringComparer.Ordinal
        );
        var facts = new Dictionary<string, (string Value, string? Unit)>(StringComparer.Ordinal);
        var units = new HashSet<string>(StringComparer.Ordinal);
        bool reference = false;
        foreach (var node in root.Nodes())
        {
            if (node is XText ws && string.IsNullOrWhiteSpace(ws.Value))
                continue;
            var e = node as XElement ?? throw Invalid();
            if (e.Name == I + "context")
            {
                Attributes(e, "id");
                var kids = Children(e, I + "entity", I + "period");
                Attributes(kids[0]);
                Attributes(kids[1]);
                var id = Children(kids[0], I + "identifier")[0];
                Attributes(id, "scheme");
                var instant = Children(kids[1], I + "instant")[0];
                Attributes(instant);
                Require(
                    contexts.Count < Contexts
                        && contexts.TryAdd(
                            Attribute(e, "id"),
                            (Attribute(id, "scheme"), Leaf(id), Leaf(instant))
                        )
                );
            }
            else if (e.Name == L + "schemaRef")
            {
                Attributes(e, X + "type", X + "href");
                Children(e);
                Require(!reference && Attribute(e, X + "type") == "simple");
                _ = Attribute(e, X + "href");
                reference = true;
            }
            else if (e.Name == I + "unit")
            {
                Attributes(e, "id");
                var m = Children(e, I + "measure")[0];
                Attributes(m);
                var q = Leaf(m).Split(':');
                Require(
                    q.Length == 2
                        && q[1] == "pure"
                        && m.GetNamespaceOfPrefix(q[0]) == I
                        && units.Count < Contexts
                        && units.Add(Attribute(e, "id"))
                );
            }
            else if (e.Name == XName.Get(Concept, Uri))
            {
                Attributes(e, "contextRef", "unitRef");
                Require(
                    facts.Count < Contexts
                        && facts.TryAdd(
                            Attribute(e, "contextRef"),
                            (Leaf(e), (string?)e.Attribute("unitRef"))
                        )
                );
            }
            else
                throw Invalid();
        }
        Require(contexts.Count > 0 && facts.Count == contexts.Count);
        var rows = new List<FerruleInstance>();
        foreach (var (id, c) in contexts)
        {
            Require(facts.TryGetValue(id, out var f) && (f.Unit is null || units.Contains(f.Unit)));
            var value = Integer
                ? new FerruleScalar(
                    FerruleValue.FromInt64(
                        long.Parse(
                            f.Value.Trim(),
                            NumberStyles.AllowLeadingSign,
                            CultureInfo.InvariantCulture
                        )
                    )
                )
                : StringField(f.Value);
            rows.Add(
                new FerruleGroup([
                    new(
                        "identifier",
                        new FerruleGroup([
                            new("scheme", StringField(c.Scheme)),
                            new("#text", StringField(c.Entity)),
                        ])
                    ),
                    new("period", new FerruleGroup([new("instant", StringField(c.Instant))])),
                    new(Concept, value),
                ])
            );
        }
        return new FerruleGroup([new(Rows, new FerruleRepeated(rows))]);
    }

    private static FerruleInstance Field(FerruleInstance x, string name) =>
        x is FerruleGroup g && g.TryGetField(name, out var f) ? f : throw Invalid();

    private static string Str(FerruleInstance x, string name) =>
        Nonempty(((FerruleScalar)Field(x, name)).Value.StringValue);

    private static string Escape(string s) =>
        XmlConvert
            .VerifyXmlChars(Nonempty(s))
            .Replace("&", "&amp;")
            .Replace("<", "&lt;")
            .Replace(">", "&gt;")
            .Replace("\"", "&quot;")
            .Replace("'", "&apos;");

    internal byte[] Write(FerruleInstance model)
    {
        var rows = ((FerruleRepeated)Field(model, Rows)).Items;
        Require(rows.Count is > 0 and <= Contexts);
        using var result = new MemoryStream();
        void Append(string text)
        {
            Require(result.Length + Utf8.GetByteCount(text) <= Bytes);
            result.Write(Utf8.GetBytes(text));
        }
        Append(
            $"<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<xbrli:xbrl xmlns:xbrli=\"{I}\" xmlns:xbrldi=\"http://xbrl.org/2006/xbrldi\" xmlns:link=\"{L}\" xmlns:xlink=\"{X}\" xmlns:ns1=\"{Uri}\">\n  <link:schemaRef xlink:type=\"simple\" xlink:href=\"{Taxonomy}\"/>"
        );
        int index = 0;
        foreach (var row in rows)
        {
            var identity = Field(row, "identifier");
            var scheme = Str(identity, "scheme");
            Require(scheme.All(c => char.IsAsciiLetterOrDigit(c) || ":/._-".Contains(c)));
            var instant = Escape(Str(Field(row, "period"), "instant"));
            var entity = Escape(Str(identity, "#text"));
            var value = ((FerruleScalar)Field(row, Concept)).Value;
            string fact = Integer
                ? value.Int64Value.ToString(CultureInfo.InvariantCulture)
                : Escape(value.StringValue);
            var id = (++index).ToString(CultureInfo.InvariantCulture);
            Append(
                $"\n  <xbrli:context id=\"c{id}\">\n    <xbrli:entity>\n      <xbrli:identifier scheme=\"{scheme}\">{entity}</xbrli:identifier>\n    </xbrli:entity>\n    <xbrli:period>\n      <xbrli:instant>{instant}</xbrli:instant>\n    </xbrli:period>\n  </xbrli:context>\n  <ns1:{Concept} contextRef=\"c{id}\">{fact}</ns1:{Concept}>"
            );
        }
        Append("\n</xbrli:xbrl>");
        return result.ToArray();
    }
}
