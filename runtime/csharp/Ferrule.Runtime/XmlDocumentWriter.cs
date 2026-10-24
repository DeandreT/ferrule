using System.Text;
using System.Text.Json;
using System.Xml;

namespace Ferrule.Runtime;

/// <summary>Explicit element-only, single-namespace native XML output.
/// This serializes a materialized body; it does not open or accept a file.</summary>
public static class FerruleMapForceXml
{
    private const int MaximumBytes = 64 * 1024 * 1024;
    private const string Xsi = "http://www.w3.org/2001/XMLSchema-instance";

    public static byte[] SerializeDocument(string descriptor, FerruleInstance value, string? schemaLocation)
    {
        var document = PrepareDocument(descriptor, value, schemaLocation);
        using var output = new MemoryStream();
        document.WriteTo(output);
        return output.ToArray();
    }

    /// <summary>Materialize the DOM without opening an output or validating XML character encoding.</summary>
    public static FerrulePreparedMapForceXmlDocument PrepareDocument(string descriptor, FerruleInstance value, string? schemaLocation)
    {
        ArgumentNullException.ThrowIfNull(value);
        var unwrapped = FerruleEmbeddedSchema.Unwrap(descriptor, 8 * 1024 * 1024);
        using var schema = JsonDocument.Parse(unwrapped.Payload, new JsonDocumentOptions { MaxDepth = 128 });
        var root = schema.RootElement;
        if (root.GetProperty("kind").GetProperty("kind").GetString() != "group" || Flag(root, "repeating"))
            throw new InvalidOperationException("Expected one group document root.");
        string ns = Namespace(root, "");
        if (ns is "http://www.w3.org/XML/1998/namespace" or "http://www.w3.org/2000/xmlns/" or Xsi)
            throw new InvalidOperationException("Reserved root namespace.");
        var document = new XmlDocument(); int nodes = 0;
        long textBytes = Encoding.UTF8.GetByteCount(ns) + (long)(schemaLocation is null ? 0 : Encoding.UTF8.GetByteCount(schemaLocation));
        if (textBytes > MaximumBytes) throw new InvalidOperationException("XML output limit exceeded.");
        Append(root, value, document, document, ns, 0, true, ref nodes, ref textBytes);
        var element = document.DocumentElement ?? throw new InvalidOperationException("Missing document root.");
        if (schemaLocation is not null)
        {
            if (Encoding.UTF8.GetByteCount(schemaLocation) > MaximumBytes) throw new InvalidOperationException("XML output limit exceeded.");
            element.SetAttribute("xmlns:xsi", Xsi);
            element.SetAttribute("schemaLocation", Xsi, schemaLocation);
        }
        return new FerrulePreparedMapForceXmlDocument(document);
    }

    internal static void WriteDocument(XmlDocument document, Stream output)
    {
        ArgumentNullException.ThrowIfNull(output);
        using var bounded = new BoundedOutput(output);
        using (var writer = XmlWriter.Create(bounded, new XmlWriterSettings {
            Encoding = new UTF8Encoding(false, true), Indent = true, IndentChars = "\t", NewLineChars = "\r\n", CloseOutput = false
        })) { document.Save(writer); writer.Flush(); }
    }

    private static bool Flag(JsonElement node, string name) => node.TryGetProperty(name, out var flag) && flag.ValueKind == JsonValueKind.True;
    private static string Namespace(JsonElement node, string inherited) => node.TryGetProperty("xml_namespace", out var ns)
        ? ns.GetProperty("kind").GetString() switch { "qualified" => ns.GetProperty("uri").GetString()!, "unqualified" => "", _ => throw new InvalidOperationException("Unknown XML namespace kind.") } : inherited;
    private static bool HasValues(JsonElement node, string name) => node.TryGetProperty(name, out var value) && value.ValueKind == JsonValueKind.Array && value.GetArrayLength() != 0;
    private static bool ValidName(string name) => name.Length != 0 && (char.IsAsciiLetter(name[0]) || name[0] == '_')
        && name.Skip(1).All(c => char.IsAsciiLetterOrDigit(c) || c is '_' or '.' or '-');
    private static void Append(JsonElement schema, FerruleInstance value, XmlDocument document, XmlNode parent,
        string ns, int depth, bool root, ref int nodes, ref long textBytes)
    {
        if (depth > 64 || ++nodes > 1_000_000) throw new InvalidOperationException("XML output limit exceeded.");
        if (value is FerruleScalar { Value.Kind: FerruleValueKind.Null }) return;
        IReadOnlyList<FerruleInstance>? rows = value switch { FerruleRepeated r => r.Items, FerruleMappedSequence r => r.Items, _ => null };
        if (rows is not null)
        {
            if (root) throw new InvalidOperationException("Multiple document roots.");
            if (rows.Count > 1_000_000 - nodes) throw new InvalidOperationException("XML output limit exceeded.");
            foreach (var row in rows)
            {
                if (row is FerruleRepeated or FerruleMappedSequence) throw new InvalidOperationException("Nested occurrence sequences.");
                Append(schema, row, document, parent, ns, depth, false, ref nodes, ref textBytes);
            }
            return;
        }
        string name = schema.GetProperty("name").GetString()!;
        if (!ValidName(name) || Flag(schema, "attribute") || Flag(schema, "text") || HasValues(schema, "xml_name_alternatives")
            || (schema.TryGetProperty("recursive_ref", out var recursive) && recursive.ValueKind != JsonValueKind.Null)
            || (schema.TryGetProperty("xml_wildcard_namespace", out var wildcard) && wildcard.ValueKind != JsonValueKind.Null)
            || Namespace(schema, ns) != ns)
            throw new InvalidOperationException("Active node is outside the element-only namespace profile.");
        textBytes += name.Length;
        if (textBytes > MaximumBytes) throw new InvalidOperationException("XML output limit exceeded.");
        var element = document.CreateElement(name, ns); parent.AppendChild(element);
        var kind = schema.GetProperty("kind");
        if (kind.GetProperty("kind").GetString() == "scalar" && value is FerruleScalar scalar)
        {
            string text = (kind.GetProperty("ty").GetString(), scalar.Value.Kind) switch {
                ("string", FerruleValueKind.String) => scalar.Value.StringValue,
                ("int", FerruleValueKind.Int64) => scalar.Value.Int64Value.ToString(System.Globalization.CultureInfo.InvariantCulture),
                ("bool", FerruleValueKind.Bool) => scalar.Value.BooleanValue ? "true" : "false",
                _ => throw new InvalidOperationException("Active scalar requires a separate XML profile.")
            };
            textBytes += Encoding.UTF8.GetByteCount(text);
            if (textBytes > MaximumBytes) throw new InvalidOperationException("XML output limit exceeded.");
            if (text.Length != 0) element.AppendChild(document.CreateTextNode(text));
        }
        else if (kind.GetProperty("kind").GetString() == "group" && value is FerruleGroup group
            && !HasValues(kind, "alternatives") && (!kind.TryGetProperty("dynamic", out var dynamic) || dynamic.ValueKind == JsonValueKind.Null))
        {
            var children = kind.GetProperty("children").EnumerateArray().ToArray();
            var fields = new Dictionary<string,FerruleInstance>(StringComparer.Ordinal);
            foreach (var field in group.Fields)
                if (!fields.TryAdd(field.Name, field.Value) || children.Count(child => child.GetProperty("name").GetString() == field.Name) != 1)
                    throw new InvalidOperationException("Unknown or duplicate element field.");
            foreach (var child in children)
                if (fields.TryGetValue(child.GetProperty("name").GetString()!, out var field))
                    Append(child, field, document, element, ns, depth + 1, false, ref nodes, ref textBytes);
        }
        else throw new InvalidOperationException("Active value requires a separate XML profile.");
    }
    private sealed class BoundedOutput(Stream output) : Stream
    {
        private long _written;
        public override bool CanRead => false;
        public override bool CanSeek => output.CanSeek;
        public override bool CanWrite => output.CanWrite;
        public override long Length => output.Length;
        public override long Position { get => output.Position; set => throw new NotSupportedException(); }
        public override void Flush() => output.Flush();
        public override int Read(byte[] buffer, int offset, int count) => throw new NotSupportedException();
        public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
        public override void SetLength(long value) => throw new NotSupportedException();
        public override void Write(byte[] buffer, int offset, int count)
        { if (count > MaximumBytes - _written) throw new InvalidOperationException("XML output limit exceeded."); output.Write(buffer, offset, count); _written += count; }
        public override void Write(ReadOnlySpan<byte> buffer)
        { if (buffer.Length > MaximumBytes - _written) throw new InvalidOperationException("XML output limit exceeded."); output.Write(buffer); _written += buffer.Length; }
    }
}

/// <summary>Reusable prepared XML. Writing never closes or accepts the host's output.</summary>
public sealed class FerrulePreparedMapForceXmlDocument
{
    private readonly XmlDocument _document;
    internal FerrulePreparedMapForceXmlDocument(XmlDocument document) => _document = document;
    public void WriteTo(Stream output) => FerruleMapForceXml.WriteDocument(_document, output);
}
