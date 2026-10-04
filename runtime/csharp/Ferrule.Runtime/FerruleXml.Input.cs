using System.Text;
using System.Text.Json;
using System.Xml;

namespace Ferrule.Runtime;

public static partial class FerruleXml
{
    public const int MaximumInputBytes = 64 * 1024 * 1024;
    private const int MaximumInputNodes = 1_000_000;
    private static readonly UTF8Encoding InputUtf8 = new(false, true);

    /// <summary>Reads one bounded, explicitly configured flat XML root.</summary>
    public static FerruleInstance ParseEmbedded(
        string descriptor,
        string xml,
        bool allowInactiveRootTypeMembers,
        bool rootViewPolicy)
    {
        ArgumentNullException.ThrowIfNull(descriptor);
        ArgumentNullException.ThrowIfNull(xml);
        int bytes;
        try
        {
            bytes = InputUtf8.GetByteCount(xml);
        }
        catch (EncoderFallbackException error)
        {
            throw new FerruleXmlBoundaryException(
                FerruleXmlBoundaryErrorKind.Utf8, "XML input is not well-formed Unicode", error);
        }
        RequireInputSize(bytes);
        if (!allowInactiveRootTypeMembers || !rootViewPolicy)
        {
            throw new FerruleXmlBoundaryException(
                FerruleXmlBoundaryErrorKind.Schema,
                "XML input requires the explicit flat root annotation policy");
        }
        var schema = ParseInputSchema(descriptor);
        try
        {
            return ReadFlatInput(xml, schema);
        }
        catch (FerruleXmlBoundaryException)
        {
            throw;
        }
        catch (Exception error) when (error is XmlException or InvalidOperationException or
            ArgumentException or FormatException)
        {
            throw new FerruleXmlBoundaryException(
                FerruleXmlBoundaryErrorKind.Input, error.Message, error);
        }
    }

    /// <summary>Checks the byte limit before strictly decoding UTF-8 XML.</summary>
    public static FerruleInstance ParseEmbeddedBytes(
        string descriptor,
        byte[] xml,
        bool allowInactiveRootTypeMembers,
        bool rootViewPolicy)
    {
        ArgumentNullException.ThrowIfNull(xml);
        RequireInputSize(xml.LongLength);
        string text;
        try
        {
            text = InputUtf8.GetString(xml);
        }
        catch (DecoderFallbackException error)
        {
            throw new FerruleXmlBoundaryException(
                FerruleXmlBoundaryErrorKind.Utf8, "XML input is not UTF-8", error);
        }
        return ParseEmbedded(descriptor, text, allowInactiveRootTypeMembers, rootViewPolicy);
    }

    private static void RequireInputSize(long bytes)
    {
        if (bytes > MaximumInputBytes)
        {
            throw new FerruleXmlBoundaryException(
                FerruleXmlBoundaryErrorKind.DocumentLimit,
                $"XML input is {bytes} bytes; maximum is {MaximumInputBytes}",
                bytes: bytes, limit: MaximumInputBytes);
        }
    }

    private static XmlSchemaNode ParseInputSchema(string descriptor)
    {
        try
        {
            var payload = FerruleEmbeddedSchema.Unwrap(descriptor, MaximumEmbeddedSchemaBytes);
            using var document = JsonDocument.Parse(payload.Payload,
                new JsonDocumentOptions { MaxDepth = MaximumSchemaDepth });
            var root = document.RootElement;
            RequireUniqueInputProperties(root);
            var kind = InputProperty(root, "kind");
            if (InputString(kind, "kind") != "group")
            {
                throw new FormatException("XML input schema requires a flat group root");
            }
            var children = InputProperty(kind, "children");
            var alternatives = InputProperty(kind, "alternatives");
            if (children.ValueKind != JsonValueKind.Array ||
                children.GetArrayLength() is < 1 or > 32 ||
                alternatives.ValueKind != JsonValueKind.Array ||
                alternatives.GetArrayLength() is < 2 or > 32)
            {
                throw new FormatException("XML input schema has an unsupported root member/type count");
            }
            ValidateInputMetadata(root, root: true);
            RequireInputEmpty(kind, "xml_restricted_alternatives");
            RequireInputEmpty(kind, "required");
            RequireInputNull(kind, "dynamic");
            foreach (var child in children.EnumerateArray())
            {
                ValidateInputMetadata(child, root: false);
                var childKind = InputProperty(child, "kind");
                if (InputString(childKind, "kind") != "scalar" ||
                    InputString(childKind, "ty") != "string")
                {
                    throw new FormatException("XML input requires ordinary String attributes");
                }
            }
            foreach (var alternative in alternatives.EnumerateArray())
            {
                if (!FerrulePrimaryRoot.TypeIdentityIsValid(InputString(alternative, "name")))
                {
                    throw new FormatException("XML input schema has an invalid type identity");
                }
                var members = InputProperty(alternative, "members");
                if (members.ValueKind != JsonValueKind.Array ||
                    members.GetArrayLength() > children.GetArrayLength())
                {
                    throw new FormatException("XML input type has an unsupported member count");
                }
                RequireInputEmpty(alternative, "required");
                RequireInputEmpty(alternative, "constraints");
            }
            var parsed = XmlSchemaNode.Parse(root, 0);
            if (parsed.DefaultType is null || !parsed.NamespaceIsExplicit ||
                parsed.Repeating || parsed.Attribute || parsed.Text || parsed.Nillable ||
                parsed.Children.Any(child => !child.NamespaceIsExplicit ||
                    !child.Attribute || child.ScalarType != XmlScalarType.String ||
                    child.Repeating || child.Text || child.Nillable || child.Fixed is not null))
            {
                throw new FormatException("XML input schema is outside the supported flat root policy");
            }
            return parsed;
        }
        catch (Exception error) when (error is JsonException or InvalidOperationException or
            ArgumentException or FormatException or XmlException)
        {
            throw new FerruleXmlBoundaryException(
                FerruleXmlBoundaryErrorKind.Schema, error.Message, error);
        }
    }

    private static void ValidateInputMetadata(JsonElement node, bool root)
    {
        var name = InputString(node, "name");
        if (!FerrulePrimaryRoot.NcNameIsValid(name))
        {
            throw new FormatException("XML input schema requires a bounded physical local name");
        }
        var identity = InputProperty(node, "xml_namespace");
        var namespaceKind = InputString(identity, "kind");
        if (namespaceKind == "qualified")
        {
            var uri = InputString(identity, "uri");
            if (uri.Length == 0 ||
                InputUtf8.GetByteCount(uri) + InputUtf8.GetByteCount(name) + 2 >
                    FerrulePrimaryRoot.MaximumIdentityBytes ||
                !FerrulePrimaryRoot.TypeIdentityIsValid($"{{{uri}}}{name}"))
            {
                throw new FormatException("XML input schema namespace is invalid or too large");
            }
        }
        else if (namespaceKind != "unqualified" || identity.TryGetProperty("uri", out _))
        {
            throw new FormatException("XML input schema requires an exact namespace identity");
        }
        foreach (var flag in new[] { "repeating", "text", "nillable", "xml_optional",
            "nullable", "container_nullable", "json_any", "json_unique_items" })
        {
            RequireInputBoolean(node, flag, false);
        }
        RequireInputBoolean(node, "attribute", !root);
        RequireInputBoolean(node, "xml_type_alternatives", root);
        if (root)
        {
            RequireInputBoolean(node, "xml_attribute_required", false);
            _ = InputString(node, "xml_default_type");
        }
        else
        {
            // Required attribute metadata is evaluated by a connected root-field
            // expression, not by this lenient input boundary.
            if (node.TryGetProperty("xml_attribute_required", out var required) &&
                required.ValueKind is not (JsonValueKind.True or JsonValueKind.False))
            {
                throw new FormatException("invalid XML attribute required metadata");
            }
            RequireInputNull(node, "xml_default_type");
        }
        foreach (var property in new[] { "recursive_ref", "fixed", "default", "value_generation",
            "xml_wildcard_namespace", "json_allowed_values", "numeric_range", "json_multiple_of",
            "item_count_range", "json_contains", "json_dependent_schemas", "property_count_range",
            "json_property_dependencies", "json_pattern_property_names", "json_property_names",
            "string_length_range", "json_patterns", "database_relation" })
        {
            RequireInputNull(node, property);
        }
        foreach (var property in new[] { "xml_name_alternatives", "xml_repeating_sequences",
            "xml_repeating_choices", "json_formats" })
        {
            RequireInputEmpty(node, property);
        }
        RequireInputEnum(node, "alternative_mode", "exclusive");
        RequireInputEnum(node, "xml_alternative_kind", "xsi_type");
        RequireInputEnum(node, "xml_wildcard_process_contents", "skip");
    }

    private static JsonElement InputProperty(JsonElement node, string name)
    {
        if (node.ValueKind != JsonValueKind.Object || !node.TryGetProperty(name, out var value))
        {
            throw new FormatException($"XML input schema is missing '{name}'");
        }
        return value;
    }
    private static string InputString(JsonElement node, string name)
    {
        var value = InputProperty(node, name);
        return value.ValueKind == JsonValueKind.String
            ? value.GetString()! : throw new FormatException($"XML input schema '{name}' must be text");
    }
    private static void RequireInputBoolean(JsonElement node, string name, bool expected)
    {
        if (!node.TryGetProperty(name, out var value))
        {
            if (expected) throw new FormatException($"XML input schema requires '{name}'");
            return;
        }
        if (value.ValueKind != (expected ? JsonValueKind.True : JsonValueKind.False))
        {
            throw new FormatException($"unsupported XML input schema flag '{name}'");
        }
    }
    private static void RequireInputNull(JsonElement node, string name)
    {
        if (node.TryGetProperty(name, out var value) && value.ValueKind != JsonValueKind.Null)
        {
            throw new FormatException($"unsupported XML input schema property '{name}'");
        }
    }
    private static void RequireInputEmpty(JsonElement node, string name)
    {
        if (node.TryGetProperty(name, out var value) &&
            (value.ValueKind != JsonValueKind.Array || value.GetArrayLength() != 0))
        {
            throw new FormatException($"unsupported XML input schema list '{name}'");
        }
    }
    private static void RequireInputEnum(JsonElement node, string name, string expected)
    {
        if (node.TryGetProperty(name, out var value) &&
            (value.ValueKind != JsonValueKind.String || value.GetString() != expected))
        {
            throw new FormatException($"unsupported XML input schema mode '{name}'");
        }
    }

    private static FerruleInstance ReadFlatInput(string xml, XmlSchemaNode schema)
    {
        xml = PrepareInputDocument(xml);
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
        using var text = new StringReader(xml);
        if (xml.StartsWith('\uFEFF')) _ = text.Read();
        using var reader = XmlReader.Create(text, settings);
        List<FerruleField>? fields = null;
        FerruleXmlTypeOrigin? origin = null;
        var nodes = 1;
        var previousTextDepth = -1;
        var contentBuffer = new char[4096];
        while (reader.Read())
        {
            var isText = reader.NodeType is XmlNodeType.Text or XmlNodeType.CDATA or
                XmlNodeType.Whitespace or XmlNodeType.SignificantWhitespace;
            var countNode = reader.NodeType is not (XmlNodeType.EndElement or XmlNodeType.XmlDeclaration or
                XmlNodeType.DocumentType or XmlNodeType.EndEntity) &&
                !(isText && (reader.Depth == 0 || previousTextDepth == reader.Depth));
            previousTextDepth = isText && reader.Depth > 0 ? reader.Depth : -1;
            if (countNode && ++nodes > MaximumInputNodes)
            {
                throw new XmlException("XML input exceeds its parser node limit");
            }
            switch (reader.NodeType)
            {
                case XmlNodeType.XmlDeclaration:
                    var encoding = reader.GetAttribute("encoding");
                    if (encoding is not null && !encoding.Equals("UTF-8", StringComparison.OrdinalIgnoreCase))
                    {
                        throw new XmlException("XML root input requires UTF-8 declaration encoding");
                    }
                    break;
                case XmlNodeType.Element:
                    if (fields is not null || reader.Depth != 0)
                    {
                        throw new XmlException("Flat XML root contains unproved child content");
                    }
                    if (reader.LocalName != schema.Name ||
                        reader.NamespaceURI != (schema.NamespaceUri ?? ""))
                    {
                        throw new XmlException("XML input has an unexpected document root");
                    }
                    (fields, origin) = ReadInputAttributes(reader, schema);
                    break;
                case XmlNodeType.Text:
                case XmlNodeType.CDATA:
                case XmlNodeType.Whitespace:
                case XmlNodeType.SignificantWhitespace:
                    int read;
                    while ((read = reader.ReadValueChunk(contentBuffer, 0, contentBuffer.Length)) != 0)
                        for (var index = 0; index < read; ++index)
                            if (!IsInputSpace(contentBuffer[index]))
                                throw new XmlException("Flat XML root contains unproved text content");
                    break;
            }
        }
        if (fields is null || origin is null)
        {
            throw new XmlException("XML input has no document root");
        }
        return new FerruleGroup(fields).WithXmlTypeOrigin(origin);
    }

    private static (List<FerruleField>, FerruleXmlTypeOrigin) ReadInputAttributes(
        XmlReader reader, XmlSchemaNode schema)
    {
        var values = new Dictionary<(string Namespace, string Name), string>();
        string? literal = null;
        while (reader.MoveToNextAttribute())
        {
            if (reader.IsDefault || reader.NamespaceURI == "http://www.w3.org/2000/xmlns/") continue;
            if (reader.NamespaceURI == XsiNamespace)
            {
                if (reader.LocalName == "type") literal = reader.Value;
                if (reader.LocalName == "nil" && reader.Value is not ("true" or "false"))
                {
                    throw new XmlException("Invalid root xsi:nil value");
                }
            }
            if (schema.Children.Any(child => child.Name == reader.LocalName &&
                (child.NamespaceUri ?? "") == reader.NamespaceURI))
            {
                values.Add((reader.NamespaceURI, reader.LocalName), reader.Value);
            }
        }
        reader.MoveToElement();
        var fields = schema.Children.Select(child => new FerruleField(child.Name,
            new FerruleScalar(values.TryGetValue((child.NamespaceUri ?? "", child.Name), out var value)
                ? FerruleValue.FromString(value) : FerruleValue.Null))).ToList();
        if (literal is not null)
        {
            var identity = ResolveInputType(reader, literal, out var padded);
            if (padded)
            {
                return (fields, FerruleXmlTypeOrigin.ExplicitPadded(literal, identity));
            }
            if (schema.Alternatives.Any(alternative => alternative.Name == identity))
            {
                fields.Add(new FerruleField(XmlTypeField,
                    new FerruleScalar(FerruleValue.FromString(identity))));
            }
            return (fields, FerruleXmlTypeOrigin.Explicit(identity));
        }
        var populated = fields.Where(field => field.Value is FerruleScalar scalar &&
            scalar.Value.Kind != FerruleValueKind.Null).Select(field => field.Name).ToArray();
        var selected = schema.Alternatives.First(alternative => alternative.Name == schema.DefaultType);
        if (!populated.All(selected.Members.Contains))
        {
            var candidates = schema.Alternatives.Where(alternative =>
                populated.All(alternative.Members.Contains)).ToArray();
            if (candidates.Length == 0)
            {
                throw new XmlException("No matching XML root type alternative");
            }
            var minimum = candidates.Min(alternative => alternative.Members.Count);
            var narrowest = candidates.Where(alternative => alternative.Members.Count == minimum).ToArray();
            if (narrowest.Length != 1)
            {
                throw new XmlException("Ambiguous XML root type alternative");
            }
            selected = narrowest[0];
        }
        if (selected.Name != schema.DefaultType)
        {
            fields.Add(new FerruleField(XmlTypeField,
                new FerruleScalar(FerruleValue.FromString(selected.Name))));
        }
        return (fields, FerruleXmlTypeOrigin.Absent);
    }

    private static string ResolveInputType(XmlReader reader, string literal, out bool padded)
    {
        if (InputUtf8.GetByteCount(literal) > FerrulePrimaryRoot.MaximumIdentityBytes)
        {
            throw new XmlException("XML type annotation exceeds its byte limit");
        }
        var core = literal.Trim(' ', '\t', '\r', '\n');
        var colon = core.IndexOf(':');
        var prefix = colon >= 0 ? core[..colon] : "";
        var local = colon >= 0 ? core[(colon + 1)..] : core;
        if (!FerrulePrimaryRoot.NcNameIsValid(local) ||
            (colon >= 0 && !FerrulePrimaryRoot.NcNameIsValid(prefix)))
        {
            throw new XmlException("Malformed XML type annotation QName");
        }
        var uri = reader.LookupNamespace(prefix);
        if (colon >= 0 && string.IsNullOrEmpty(uri))
        {
            throw new XmlException("Unbound XML type annotation prefix");
        }
        if (!string.IsNullOrEmpty(uri) &&
            (long)InputUtf8.GetByteCount(uri) + InputUtf8.GetByteCount(local) + 2 >
                FerrulePrimaryRoot.MaximumIdentityBytes)
        {
            throw new XmlException("Expanded XML type identity exceeds its byte limit");
        }
        var resolved = string.IsNullOrEmpty(uri) ? local : $"{{{uri}}}{local}";
        if (!FerrulePrimaryRoot.TypeIdentityIsValid(resolved))
        {
            throw new XmlException("Invalid expanded XML type identity");
        }
        padded = core.Length != literal.Length;
        return resolved;
    }

    private static void RequireUniqueInputProperties(JsonElement value)
    {
        if (value.ValueKind == JsonValueKind.Object)
        {
            var names = new HashSet<string>(StringComparer.Ordinal);
            foreach (var property in value.EnumerateObject())
            {
                if (!names.Add(property.Name))
                    throw new FormatException("XML input schema has duplicate properties");
                RequireUniqueInputProperties(property.Value);
            }
        }
        else if (value.ValueKind == JsonValueKind.Array)
            foreach (var item in value.EnumerateArray()) RequireUniqueInputProperties(item);
    }

    // Project only internal entity text. Declaration metadata does not supply
    // attributes, namespace bindings, or tokenized attribute normalization.
    private static string PrepareInputDocument(string xml)
    {
        var entities = new Dictionary<string, string>(StringComparer.Ordinal);
        var prolog = new StringBuilder();
        var start = 0;
        var at = xml.StartsWith('\uFEFF') ? 1 : 0;
        while (at < xml.Length)
        {
            if (IsInputSpace(xml[at])) { ++at; continue; }
            if (At(xml, at, "<!--")) { at = InputTerminator(xml, at + 4, "-->"); continue; }
            if (At(xml, at, "<?")) { at = InputTerminator(xml, at + 2, "?>"); continue; }
            if (!At(xml, at, "<!DOCTYPE")) break;
            start = at;
            at += 9;
            RequireInputSpace(xml, ref at);
            _ = InputName(xml, ref at);
            SkipInputSpace(xml, ref at);
            ReadInputExternalId(xml, ref at);
            SkipInputSpace(xml, ref at);
            if (at < xml.Length && xml[at] == '[')
            {
                ++at;
                while (true)
                {
                    SkipInputSpace(xml, ref at);
                    if (at >= xml.Length) throw new XmlException("Unclosed XML document type");
                    if (xml[at] == ']') { ++at; break; }
                    if (At(xml, at, "<!--") || At(xml, at, "<?"))
                    {
                        var end = At(xml, at, "<!--") ? InputTerminator(xml, at + 4, "-->")
                            : InputTerminator(xml, at + 2, "?>");
                        prolog.Append(xml.AsSpan(at, end - at));
                        at = end;
                    }
                    else if (At(xml, at, "<!ENTITY")) ReadInputEntity(xml, ref at, entities);
                    else if (At(xml, at, "<!ELEMENT") || At(xml, at, "<!ATTLIST") ||
                        At(xml, at, "<!NOTATION"))
                    {
                        // These declarations are retained as no runtime metadata.
                        at = InputTerminator(xml, at, ">");
                    }
                    else throw new XmlException("Unsupported XML document type token");
                }
            }
            SkipInputSpace(xml, ref at);
            RequireInputCharacter(xml, ref at, '>');
            var output = new StringBuilder();
            long bytes = 0;
            AppendInputProjection(output, xml.AsSpan(0, start), ref bytes);
            AppendInputProjection(output, prolog.ToString().AsSpan(), ref bytes);
            ExpandInputDocument(xml.AsSpan(at), entities, output, ref bytes);
            return output.ToString();
        }
        if (!xml.Contains('&')) return xml;
        var plain = new StringBuilder();
        long plainBytes = 0;
        ExpandInputDocument(xml.AsSpan(), entities, plain, ref plainBytes);
        return plain.ToString();
    }

    private static bool At(string text, int at, string token) =>
        text.AsSpan(at).StartsWith(token, StringComparison.Ordinal);
    private static bool IsInputSpace(char value) => value is ' ' or '\t' or '\r' or '\n';
    private static void SkipInputSpace(string text, ref int at)
    {
        while (at < text.Length && IsInputSpace(text[at])) ++at;
    }
    private static void RequireInputSpace(string text, ref int at)
    {
        var begin = at;
        SkipInputSpace(text, ref at);
        if (at == begin) throw new XmlException("XML document type requires whitespace");
    }
    private static void RequireInputCharacter(string text, ref int at, char value)
    {
        if (at >= text.Length || text[at++] != value)
            throw new XmlException("Invalid XML document type delimiter");
    }
    private static string InputName(string text, ref int at)
    {
        var start = at;
        while (at < text.Length && !IsInputSpace(text[at]) && text[at] is not ('>' or '[')) ++at;
        var name = text[start..at];
        XmlConvert.VerifyName(name);
        return name;
    }
    private static string InputQuoted(string text, ref int at)
    {
        if (at >= text.Length || text[at] is not ('\'' or '"'))
            throw new XmlException("XML document type requires a quoted value");
        var quote = text[at++];
        var start = at;
        while (at < text.Length && text[at] != quote) ++at;
        if (at >= text.Length) throw new XmlException("Unclosed XML document type value");
        return text[start..at++];
    }
    private static bool ReadInputExternalId(string text, ref int at)
    {
        var system = At(text, at, "SYSTEM");
        if (!system && !At(text, at, "PUBLIC")) return false;
        at += 6;
        RequireInputSpace(text, ref at);
        _ = InputQuoted(text, ref at);
        if (!system) { RequireInputSpace(text, ref at); _ = InputQuoted(text, ref at); }
        return true;
    }
    private static void ReadInputEntity(string text, ref int at, Dictionary<string, string> entities)
    {
        at += 8;
        RequireInputSpace(text, ref at);
        if (at < text.Length && text[at] == '%') { ++at; RequireInputSpace(text, ref at); }
        var name = InputName(text, ref at);
        RequireInputSpace(text, ref at);
        if (at < text.Length && text[at] is '\'' or '"')
            entities.TryAdd(name, InputQuoted(text, ref at));
        else if (ReadInputExternalId(text, ref at))
        {
            SkipInputSpace(text, ref at);
            if (At(text, at, "NDATA"))
            {
                at += 5;
                RequireInputSpace(text, ref at);
                _ = InputName(text, ref at);
            }
        }
        else throw new XmlException("Unsupported XML entity declaration");
        SkipInputSpace(text, ref at);
        RequireInputCharacter(text, ref at, '>');
    }
    private static int InputTerminator(string text, int at, string delimiter)
    {
        var end = text.IndexOf(delimiter, at, StringComparison.Ordinal);
        if (end < 0) throw new XmlException("Unclosed XML declaration");
        return end + delimiter.Length;
    }
    private static void AppendInputProjection(StringBuilder output, ReadOnlySpan<char> value, ref long bytes)
    {
        var count = InputUtf8.GetByteCount(value);
        if (bytes + count > MaximumInputBytes)
            throw new XmlException("Expanded XML input exceeds its document limit");
        bytes += count;
        output.Append(value);
    }
    private static void ExpandInputDocument(ReadOnlySpan<char> text,
        Dictionary<string, string> entities, StringBuilder output, ref long bytes)
    {
        var inTag = false;
        var quote = '\0';
        for (var at = 0; at < text.Length;)
        {
            if (quote == '\0' && (text[at..].StartsWith("<!--") ||
                text[at..].StartsWith("<![CDATA[") || text[at..].StartsWith("<?")))
            {
                var marker = text[at..].StartsWith("<!--") ? "-->" :
                    text[at..].StartsWith("<?") ? "?>" : "]]>";
                var end = text[(at + 2)..].IndexOf(marker);
                if (end < 0) throw new XmlException("Unclosed XML content");
                end += at + 2 + marker.Length;
                AppendInputProjection(output, text[at..end], ref bytes);
                at = end;
                continue;
            }
            var c = text[at];
            if (c == '&')
            {
                var end = text[at..].IndexOf(';');
                if (end < 0) throw new XmlException("Malformed XML entity reference");
                var name = text.Slice(at + 1, end - 1).ToString();
                if (entities.TryGetValue(name, out var entity) &&
                    name is not ("amp" or "lt" or "gt" or "apos" or "quot"))
                {
                    var references = 0;
                    ExpandInputEntity(entity, entities, output, ref bytes, quote != '\0', 1, ref references);
                }
                else if (name.StartsWith('#'))
                {
                    var character = InputNumericReference(name);
                    AppendInputProjection(output, character == "\uFFFD" ? character.AsSpan() :
                        text.Slice(at, end + 1), ref bytes);
                }
                else AppendInputProjection(output, text.Slice(at, end + 1), ref bytes);
                at += end + 1;
                continue;
            }
            if (inTag)
            {
                if (quote != '\0') { if (c == quote) quote = '\0'; }
                else if (c is '\'' or '"') quote = c;
                else if (c == '>') inTag = false;
            }
            else if (c == '<') inTag = true;
            var endPlain = at + 1;
            while (endPlain < text.Length && text[endPlain] is not ('&' or '<' or '>' or '\'' or '"'))
                ++endPlain;
            // Projection slices end at ASCII delimiters, retaining surrogate pairs.
            AppendInputProjection(output, text[at..endPlain], ref bytes);
            at = endPlain;
        }
    }
    private static void ExpandInputEntity(string text, Dictionary<string, string> entities,
        StringBuilder output, ref long bytes, bool attribute, int depth, ref int references)
    {
        if (depth > 10) throw new XmlException("XML entity nesting exceeds its limit");
        for (var at = 0; at < text.Length; ++at)
        {
            var c = text[at];
            if (c == '&')
            {
                var end = text.IndexOf(';', at);
                if (end < 0) throw new XmlException("Malformed XML entity reference");
                var name = text[(at + 1)..end];
                var character = name switch {
                    "amp" => "&", "lt" => "<", "gt" => ">", "apos" => "'", "quot" => "\"",
                    _ => null,
                };
                if (name.StartsWith('#'))
                {
                    character = InputNumericReference(name);
                }
                if (character is not null)
                {
                    if (attribute && character == "<")
                        throw new XmlException("Escaped XML markup in an attribute entity");
                    if (character.Length == 1 && IsInputSpace(character[0]))
                        character = attribute ? " " : character == "\r" ? "\n" : character;
                    AppendInputProjection(output, EscapeInputEntityCharacter(character, attribute).AsSpan(), ref bytes);
                }
                else if (entities.TryGetValue(name, out var entity))
                {
                    if (++references > 255) throw new XmlException("XML entity references exceed their limit");
                    ExpandInputEntity(entity, entities, output, ref bytes, attribute, depth + 1, ref references);
                }
                else throw new XmlException("External or undeclared XML entity is unavailable");
                at = end;
            }
            else
            {
                if (c == '\r' && at + 1 < text.Length && text[at + 1] == '\n') ++at;
                if (attribute && IsInputSpace(c)) c = ' ';
                else if (!attribute && c == '\r') c = '\n';
                var value = char.IsHighSurrogate(c) && at + 1 < text.Length ? text.Substring(at++, 2) : c.ToString();
                AppendInputProjection(output, attribute ? EscapeInputEntityCharacter(value, true).AsSpan() : value.AsSpan(), ref bytes);
            }
        }
    }
    private static string InputNumericReference(string name)
    {
        var hexadecimal = name.StartsWith("#x", StringComparison.Ordinal);
        var digits = name.AsSpan(hexadecimal ? 2 : 1);
        if (digits.IsEmpty || digits.ContainsAny('+', '-') ||
            !uint.TryParse(digits, hexadecimal ? System.Globalization.NumberStyles.AllowHexSpecifier :
                System.Globalization.NumberStyles.None, System.Globalization.CultureInfo.InvariantCulture,
                out var code))
            throw new XmlException("Malformed XML numeric character reference");
        var character = code > 0x10ffff || code is >= 0xd800 and <= 0xdfff
            ? "\uFFFD" : char.ConvertFromUtf32((int)code);
        XmlConvert.VerifyXmlChars(character);
        return character;
    }
    private static string EscapeInputEntityCharacter(string value, bool attribute) => value switch {
        "&" => "&amp;", "<" => "&lt;", ">" => "&gt;", "\"" when attribute => "&quot;",
        "'" when attribute => "&apos;", _ => value,
    };
}
