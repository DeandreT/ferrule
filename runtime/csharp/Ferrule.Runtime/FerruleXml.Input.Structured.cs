using System.Text;
using System.Xml;

namespace Ferrule.Runtime;

/// <summary>A structured-input resource count, separate from document bytes.</summary>
public sealed class FerruleXmlStructuredResourceException : Exception
{
    public string Resource { get; }
    public ulong ObservedCount { get; }
    public ulong Limit { get; }

    public FerruleXmlStructuredResourceException(string resource, ulong observedCount, ulong limit)
        : base($"structured XML input exceeds {resource} limit: {observedCount} > {limit}")
    { Resource = resource; ObservedCount = observedCount; Limit = limit; }
}

public static partial class FerruleXml
{
    // Acceptance/work limits for the structured profile; these are not RSS limits.
    public const int MaximumStructuredDepth = 64;
    public const int MaximumStructuredPhysicalNodes = 1_000_000;
    public const int MaximumStructuredSchemaNodes = 1_000_000;
    public const int MaximumStructuredInstanceNodes = 1_000_000;
    public const long MaximumStructuredFieldAndStringBytes = 64L * 1024 * 1024;
    public const long MaximumStructuredProjectionWork = 100_000_000;
    public const ulong MaximumStructuredParserReservationSlots = 1_000_000;
    public const ulong MaximumStructuredNamespaceReferences = 1_000_000;
    public const ulong MaximumStructuredParserStructuralWork = 100_000_000;
    public const ulong MaximumStructuredNamespaceRegistryUtf8Bytes = 64UL * 1024 * 1024;
    private const string StructuredXmlNamespaceUri = "http://www.w3.org/XML/1998/namespace";

    /// <summary>Reads a closed ordinary XML document after exact result preflight.</summary>
    public static FerruleInstance ParseStructuredEmbedded(string descriptor, string xml)
    {
        ArgumentNullException.ThrowIfNull(descriptor);
        ArgumentNullException.ThrowIfNull(xml);
        long bytes;
        try { bytes = InputUtf8.GetByteCount(xml); }
        catch (EncoderFallbackException error)
        {
            throw new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Utf8,
                "XML input is not well-formed Unicode", error);
        }
        catch (Exception error) when (error is ArgumentException or OverflowException)
        {
            // GetByteCount has an Int32 result. Preserve the typed long byte
            // limit for an otherwise valid exceptionally large CLR string.
            bytes = StructuredLargeUtf8ByteCount(xml);
        }
        RequireInputSize(bytes);
        var schema = ParseStructuredSchema(descriptor);
        try
        {
            StructuredParserBudget.CheckRawReservation(xml);
            var root = ReadStructuredDom(xml);
            if (!StructuredElementMatches(schema, root))
                throw StructuredInput("XML input root does not match the source schema");
            var budget = new StructuredBudget();
            PreflightStructuredNode(schema, root, budget);
            // No FerruleInstance, field list or decoded String result exists
            // before the complete successful prospective walk above.
            return MaterializeStructuredNode(schema, root);
        }
        catch (FerruleXmlBoundaryException) { throw; }
        catch (Exception error) when (error is FerruleXmlStructuredResourceException or XmlException or ArgumentException or
            InvalidOperationException or FormatException or OverflowException)
        {
            throw new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Input,
                error.Message, error);
        }
    }

    /// <summary>Checks original bytes before strict UTF-8 decoding.</summary>
    public static FerruleInstance ParseStructuredEmbeddedBytes(string descriptor, byte[] xml)
    {
        ArgumentNullException.ThrowIfNull(xml);
        RequireInputSize(xml.LongLength);
        string text;
        try { text = InputUtf8.GetString(xml); }
        catch (DecoderFallbackException error)
        {
            throw new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Utf8,
                "XML input is not UTF-8", error);
        }
        return ParseStructuredEmbedded(descriptor, text);
    }

    private static FerruleXmlBoundaryException StructuredInput(string detail) =>
        new(FerruleXmlBoundaryErrorKind.Input, detail);

    private static long StructuredLargeUtf8ByteCount(string value)
    {
        var remaining = value.AsSpan();
        long bytes = 0;
        while (!remaining.IsEmpty)
        {
            if (Rune.DecodeFromUtf16(remaining, out var rune, out var consumed) !=
                System.Buffers.OperationStatus.Done)
                throw new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Utf8,
                    "XML input is not well-formed Unicode");
            bytes += rune.Utf8SequenceLength;
            remaining = remaining[consumed..];
        }
        return bytes;
    }

    private enum StructuredDomKind { Element, Text, Comment, ProcessingInstruction }
    private sealed record StructuredAttribute(string Name, string Namespace, string Value);
    private sealed class StructuredDom
    {
        internal readonly StructuredDomKind Kind;
        internal readonly string Name;
        internal readonly string Namespace;
        internal readonly List<StructuredAttribute> Attributes = new();
        internal readonly List<StructuredDom> Children = new();
        internal readonly List<string> TextParts = new();
        internal ulong NamespaceDeclarationEvents;
        internal StructuredDom(StructuredDomKind kind, string name = "", string ns = "")
        { Kind = kind; Name = name; Namespace = ns; }
    }

    private static StructuredDom ReadStructuredDom(string xml)
    {
        var settings = new XmlReaderSettings
        {
            DtdProcessing = DtdProcessing.Prohibit,
            XmlResolver = null,
            MaxCharactersInDocument = MaximumInputBytes,
            MaxCharactersFromEntities = MaximumInputBytes,
            IgnoreComments = false,
            IgnoreProcessingInstructions = false,
            IgnoreWhitespace = false,
        };
        using var input = new StringReader(xml);
        if (xml.StartsWith('\uFEFF')) _ = input.Read();
        using var reader = XmlReader.Create(input, settings);
        var parserBudget = new StructuredParserBudget();
        var parents = new Stack<StructuredDom>();
        StructuredDom? root = null;
        var nodes = 1; // Document node; declaration and attributes are not nodes.
        void ChargeNode()
        {
            if (++nodes > MaximumStructuredPhysicalNodes)
                throw StructuredInput("XML input exceeds its physical node limit");
        }
        while (reader.Read())
        {
            switch (reader.NodeType)
            {
                case XmlNodeType.Element:
                    if (parents.Count + 1 > MaximumStructuredDepth)
                        throw StructuredInput("XML input exceeds its element depth limit");
                    var ancestorDeclarations = parents.TryPeek(out var headerParent)
                        ? headerParent.NamespaceDeclarationEvents : 0;
                    var declarations = parserBudget.PreflightElement(reader, ancestorDeclarations);
                    ChargeNode();
                    var element = new StructuredDom(StructuredDomKind.Element,
                        reader.LocalName, reader.NamespaceURI);
                    element.NamespaceDeclarationEvents = declarations;
                    if (reader.MoveToFirstAttribute())
                    {
                        do
                        {
                            // Native attributes() excludes all namespace declarations.
                            if (reader.NamespaceURI != "http://www.w3.org/2000/xmlns/")
                                element.Attributes.Add(new StructuredAttribute(
                                    reader.LocalName, reader.NamespaceURI, reader.Value));
                        } while (reader.MoveToNextAttribute());
                        reader.MoveToElement();
                    }
                    if (parents.TryPeek(out var parent)) parent.Children.Add(element);
                    else if (root is null) root = element;
                    else throw StructuredInput("XML input contains more than one root");
                    if (!reader.IsEmptyElement) parents.Push(element);
                    break;
                case XmlNodeType.EndElement:
                    if (parents.Count == 0) throw StructuredInput("invalid XML element nesting");
                    parents.Pop();
                    break;
                case XmlNodeType.Text:
                case XmlNodeType.CDATA:
                case XmlNodeType.Whitespace:
                case XmlNodeType.SignificantWhitespace:
                    // Whitespace outside the root is not a native DOM text node.
                    if (!parents.TryPeek(out var textParent)) break;
                    StructuredDom text;
                    if (textParent.Children.Count != 0 &&
                        textParent.Children[^1].Kind == StructuredDomKind.Text)
                        text = textParent.Children[^1];
                    else
                    {
                        ChargeNode();
                        text = new StructuredDom(StructuredDomKind.Text);
                        textParent.Children.Add(text);
                    }
                    // Keep separate decoded pieces until the result budget has
                    // passed. No repeated concatenation of adjacent CDATA.
                    var part = reader.Value;
                    if (part.Length != 0) text.TextParts.Add(part);
                    break;
                case XmlNodeType.Comment:
                case XmlNodeType.ProcessingInstruction:
                    if (reader.NodeType == XmlNodeType.ProcessingInstruction && reader.Name.Any(char.IsSurrogate))
                        throw StructuredInput("XML input requires names within the Basic Multilingual Plane");
                    ChargeNode();
                    if (parents.TryPeek(out var barrierParent))
                    {
                        var barrier = new StructuredDom(reader.NodeType == XmlNodeType.Comment
                            ? StructuredDomKind.Comment : StructuredDomKind.ProcessingInstruction);
                        if (barrier.Kind == StructuredDomKind.Comment) barrier.TextParts.Add(reader.Value);
                        barrierParent.Children.Add(barrier);
                    }
                    break;
                case XmlNodeType.DocumentType:
                    throw StructuredInput("DOCTYPE is outside the structured XML input profile");
                case XmlNodeType.XmlDeclaration:
                    var encoding = reader.GetAttribute("encoding");
                    if (encoding is not null && !encoding.Equals("UTF-8", StringComparison.OrdinalIgnoreCase))
                        throw StructuredInput("XML input declaration must use UTF-8");
                    break;
                case XmlNodeType.EndEntity:
                    break;
                default:
                    throw StructuredInput("unsupported XML parser node");
            }
        }
        if (parents.Count != 0 || root is null)
            throw StructuredInput("XML input has no complete root element");
        return root;
    }

    // One document-wide normalized registry; no inherited namespace maps are copied.
    // XmlReader has already parsed each event before these private-DOM checks.
    private sealed class StructuredParserBudget
    {
        private ulong _references;
        private ulong _work;
        private ulong _registryBytes;
        private readonly HashSet<(string? Prefix, string Uri)> _namespaces = new()
        { ("xml", StructuredXmlNamespaceUri) };

        internal static void CheckRawReservation(string xml)
        {
            ulong slots = 0;
            foreach (var character in xml)
                if (character is '<' or '=') ++slots;
            Require("parser_reservation_slots", slots, MaximumStructuredParserReservationSlots);
        }

        internal ulong PreflightElement(XmlReader reader, ulong ancestorDeclarations)
        {
            if (reader.LocalName.Any(char.IsSurrogate) || reader.Prefix.Any(char.IsSurrogate))
                throw StructuredInput("structured XML physical names must be BMP local names");
            ulong attributes = 0;
            ulong localDeclarations = 0;
            if (reader.MoveToFirstAttribute())
            {
                do
                {
                    ++attributes;
                    if (reader.LocalName.Any(char.IsSurrogate) || reader.Prefix.Any(char.IsSurrogate))
                        throw StructuredInput("structured XML physical names must be BMP local names");
                    if (reader.LocalName == "xmlns" && reader.Prefix.Length != 0 && reader.Prefix != "xmlns")
                        throw StructuredInput("prefixed local xmlns is outside the structured XML input profile");
                    if (reader.Prefix == "xmlns" && reader.LocalName == "xmlns")
                        throw StructuredInput("the reserved xmlns prefix cannot be declared");
                    if (IsDeclaration(reader)) ++localDeclarations;
                } while (reader.MoveToNextAttribute());
                reader.MoveToElement();
            }
            var inheritedAndLocal = Add(ancestorDeclarations, localDeclarations);
            // These lower bounds are paid before normalized registry retention or
            // any private DOM element/ordinary attribute copies.
            if (localDeclarations != 0)
                Charge("namespace_references", ref _references, inheritedAndLocal,
                    MaximumStructuredNamespaceReferences);
            var work = Multiply(attributes, Add(attributes, 1)) / 2;
            if (localDeclarations != 0)
                work = Add(work, Multiply(ancestorDeclarations, inheritedAndLocal));
            work = Add(work, Multiply(Add(attributes, 1), inheritedAndLocal));
            Charge("parser_structural_work", ref _work, work, MaximumStructuredParserStructuralWork);

            if (reader.MoveToFirstAttribute())
            {
                do
                {
                    if (!IsDeclaration(reader)) continue;
                    string? prefix = reader.Prefix.Length == 0 ? null : reader.LocalName;
                    var key = (prefix, reader.Value); // Already XML1.0-normalized by XmlReader.
                    var distinct = (ulong)_namespaces.Count; // Includes implicit xml binding.
                    if (_namespaces.Contains(key))
                    {
                        Charge("parser_structural_work", ref _work, Add(BitWidth(distinct), 1),
                            MaximumStructuredParserStructuralWork);
                    }
                    else
                    {
                        Charge("parser_structural_work", ref _work, Multiply(2, distinct),
                            MaximumStructuredParserStructuralWork);
                        var bytes = Add((ulong)InputUtf8.GetByteCount(prefix ?? ""),
                            (ulong)InputUtf8.GetByteCount(key.Item2));
                        Charge("namespace_registry_utf8_bytes", ref _registryBytes, bytes,
                            MaximumStructuredNamespaceRegistryUtf8Bytes);
                        _namespaces.Add(key);
                    }
                } while (reader.MoveToNextAttribute());
                reader.MoveToElement();
            }
            return inheritedAndLocal;
        }

        private static bool IsDeclaration(XmlReader reader) =>
            reader.Prefix == "xmlns" || reader.Prefix.Length == 0 && reader.LocalName == "xmlns";
        private static ulong BitWidth(ulong value)
        {
            ulong bits = 0;
            while (value != 0) { ++bits; value >>= 1; }
            return bits;
        }
        private static ulong Add(ulong left, ulong right) =>
            right > ulong.MaxValue - left ? ulong.MaxValue : left + right;
        private static ulong Multiply(ulong left, ulong right) =>
            left != 0 && right > ulong.MaxValue / left ? ulong.MaxValue : left * right;
        private static void Require(string resource, ulong observed, ulong limit)
        {
            if (observed > limit) throw new FerruleXmlStructuredResourceException(resource, observed, limit);
        }
        private static void Charge(string resource, ref ulong total, ulong count, ulong limit)
        {
            total = Add(total, count);
            Require(resource, total, limit);
        }
    }

    private sealed class StructuredBudget
    {
        private long _nodes;
        private long _bytes;
        private long _work;
        internal void Node()
        {
            if (++_nodes > MaximumStructuredInstanceNodes)
                throw StructuredInput("XML input exceeds its materialized instance node limit");
        }
        internal void Bytes(long bytes)
        {
            if (bytes > MaximumStructuredFieldAndStringBytes - _bytes)
                throw StructuredInput("XML input exceeds its materialized field/string byte limit");
            _bytes += bytes;
        }
        internal void Work(long count = 1)
        {
            if (count > MaximumStructuredProjectionWork - _work)
                throw StructuredInput("XML input exceeds its projection work limit");
            _work += count;
        }
    }
}
