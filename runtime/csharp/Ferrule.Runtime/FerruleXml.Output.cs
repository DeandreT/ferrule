using System.Globalization;
using System.Text;
using System.Text.Json;
using System.Xml;

namespace Ferrule.Runtime;

public static partial class FerruleXml
{
    private static readonly UTF8Encoding OutputUtf8 = new(false, true);
    public const int MaximumHintDescriptorBytes = 2 * 1024 * 1024;
    private sealed record XmlLiteralHints(
        string? NoNamespaceLocation,
        IReadOnlyList<(string Namespace, string Location)> Locations);

    /// <summary>Serializes with literal output policy; locations are never resolved or opened.</summary>
    public static string SerializeDocumentEmbedded(
        string descriptor, FerruleInstance instance, bool declaration, bool indent,
        string? defaultNamespace, string? schemaHintsJson) =>
        SerializeDocumentEmbedded(descriptor, instance, declaration, indent, defaultNamespace, schemaHintsJson, null);

    public static string SerializeDocumentEmbedded(
        string descriptor,
        FerruleInstance instance,
        bool declaration,
        bool indent,
        string? defaultNamespace,
        string? schemaHintsJson,
        string? scalarOutputJson)
    {
        string payload;
        XmlSchemaNode parsedSchema;
        try
        {
            var prepared = DocumentSchema(descriptor);
            payload = prepared.Payload;
            parsedSchema = prepared.Schema;
            if (schemaHintsJson is not null && prepared.HasSupplementaryName)
                throw new FormatException("supplementary XML names are unsupported by the generated input and hinted-output parsers");
        }
        catch (Exception error) when (error is JsonException or FormatException or
            InvalidOperationException or ArgumentException or OverflowException or XmlException)
        {
            throw new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Schema, error.Message, error);
        }
        try
        {
            if (scalarOutputJson is not null)
            {
                if (defaultNamespace is not null)
                    throw new InvalidOperationException("XML scalar output cannot be combined with a default namespace");
                (parsedSchema, instance) = ProjectScalarOutput(payload, parsedSchema, instance, scalarOutputJson);
            }
            if (defaultNamespace is not null &&
                (defaultNamespace.Length == 0 || OutputUtf8.GetByteCount(defaultNamespace) > 4096))
                throw new InvalidOperationException("default XML namespace must be nonempty and at most 4096 UTF-8 bytes");
            if (defaultNamespace is not null) _ = XmlConvert.VerifyXmlChars(defaultNamespace);
            if (defaultNamespace is "http://www.w3.org/XML/1998/namespace" or
                "http://www.w3.org/2000/xmlns/" ||
                parsedSchema.NamespaceUri is "http://www.w3.org/XML/1998/namespace" or
                "http://www.w3.org/2000/xmlns/")
                throw new InvalidOperationException("reserved XML namespace cannot be the default namespace");
            var output = SerializeCore(0, payload, instance, declaration, indent, defaultNamespace,
                schemaHintsJson, documentBoundary: true, parsedSchema).StringValue;
            try { _ = XmlConvert.VerifyXmlChars(output); }
            catch (XmlException error)
            {
                throw new InvalidOperationException("XML output must contain XML 1.0 characters", error);
            }
            return output;
        }
        catch (FerruleXmlBoundaryException) { throw; }
        catch (Exception error) when (error is FerruleRuntimeException or JsonException or
            FormatException or InvalidOperationException or ArgumentException or OverflowException or XmlException)
        {
            throw new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Output, error.Message, error);
        }
    }

    /// <summary>Prepares bounded UTF-8 output without allocating its returned byte array.</summary>
    public static FerruleXmlPreparedUtf8Document PrepareDocumentEmbeddedUtf8(
        string descriptor, FerruleInstance instance, bool declaration, bool indent,
        string? defaultNamespace, string? schemaHintsJson) =>
        PrepareDocumentEmbeddedUtf8(descriptor, instance, declaration, indent, defaultNamespace, schemaHintsJson, null);

    public static FerruleXmlPreparedUtf8Document PrepareDocumentEmbeddedUtf8(
        string descriptor,
        FerruleInstance instance,
        bool declaration,
        bool indent,
        string? defaultNamespace,
        string? schemaHintsJson,
        string? scalarOutputJson)
    {
        string payload;
        XmlSchemaNode parsedSchema;
        try
        {
            var prepared = DocumentSchema(descriptor);
            payload = prepared.Payload;
            parsedSchema = prepared.Schema;
            if (schemaHintsJson is not null && prepared.HasSupplementaryName)
                throw new FormatException("supplementary XML names are unsupported by the generated input and hinted-output parsers");
        }
        catch (Exception error) when (error is JsonException or FormatException or
            InvalidOperationException or ArgumentException or OverflowException or XmlException)
        {
            throw new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Schema, error.Message, error);
        }
        try
        {
            if (scalarOutputJson is not null)
            {
                if (defaultNamespace is not null)
                    throw new InvalidOperationException("XML scalar output cannot be combined with a default namespace");
                (parsedSchema, instance) = ProjectScalarOutput(payload, parsedSchema, instance, scalarOutputJson);
            }
            if (defaultNamespace is not null &&
                (defaultNamespace.Length == 0 || OutputUtf8.GetByteCount(defaultNamespace) > 4096))
                throw new InvalidOperationException("default XML namespace must be nonempty and at most 4096 UTF-8 bytes");
            if (defaultNamespace is not null) _ = XmlConvert.VerifyXmlChars(defaultNamespace);
            if (defaultNamespace is "http://www.w3.org/XML/1998/namespace" or
                "http://www.w3.org/2000/xmlns/" ||
                parsedSchema.NamespaceUri is "http://www.w3.org/XML/1998/namespace" or
                "http://www.w3.org/2000/xmlns/")
                throw new InvalidOperationException("reserved XML namespace cannot be the default namespace");
            var prepared = SerializeCore<FerruleXmlPreparedUtf8Document>(0, payload, instance,
                declaration, indent, defaultNamespace, schemaHintsJson, documentBoundary: true,
                PrepareSerializedUtf8, parsedSchema);
            if (prepared.LegacyText is { } output)
            {
                try { _ = XmlConvert.VerifyXmlChars(output); }
                catch (XmlException error)
                {
                    throw new InvalidOperationException("XML output must contain XML 1.0 characters", error);
                }
            }
            return prepared;
        }
        catch (FerruleXmlBoundaryException) { throw; }
        catch (Exception error) when (error is FerruleRuntimeException or JsonException or
            FormatException or InvalidOperationException or ArgumentException or OverflowException or XmlException)
        {
            throw new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Output, error.Message, error);
        }
    }

    private static void RequireDocumentNamespaceMetadata(XmlSchemaNode schema, bool root)
    {
        if (schema.Attribute && schema.Name == "xmlns" && schema.NamespaceUri is null ||
            !root && (schema.NamespaceUri == "http://www.w3.org/2000/xmlns/" ||
                !schema.Attribute && schema.NamespaceUri == "http://www.w3.org/XML/1998/namespace") ||
            schema.Alternatives.Any(alternative =>
                SplitExpandedName(alternative.Name).Namespace is
                    "http://www.w3.org/XML/1998/namespace" or "http://www.w3.org/2000/xmlns/"))
            throw new FormatException("unsupported XML namespace declaration metadata in document schema");
        foreach (var child in schema.Children) RequireDocumentNamespaceMetadata(child, root: false);
    }

    private static bool HasSupplementaryDocumentName(XmlSchemaNode schema) =>
        schema.Name.EnumerateRunes().Any(rune => rune.Value > 0xFFFF) ||
        schema.Children.Any(HasSupplementaryDocumentName);

    private static void RequireDocumentLocalName(string name)
    {
        if (!FerrulePrimaryRoot.NcNameIsValid(name))
            throw new FormatException("XML document schema requires local NCNames and canonical type identities");
    }

    private static XmlLiteralHints? ParseLiteralHints(string? descriptor)
    {
        if (descriptor is null) return null;
        if (OutputUtf8.GetByteCount(descriptor) > MaximumHintDescriptorBytes)
            throw new InvalidOperationException("XML schema-hint descriptor exceeds 2 MiB");
        using var document = JsonDocument.Parse(descriptor);
        var root = document.RootElement;
        if (root.ValueKind != JsonValueKind.Object)
            throw new InvalidOperationException("XML schema-hint descriptor must be an object");
        var rootProperties = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in root.EnumerateObject())
            if (!rootProperties.Add(property.Name) || property.Name is not ("no_namespace_location" or "locations"))
                throw new InvalidOperationException("duplicate or unknown XML schema-hint property");
        var noNamespace = root.TryGetProperty("no_namespace_location", out var noNamespaceValue)
            && noNamespaceValue.ValueKind != JsonValueKind.Null ? noNamespaceValue.GetString() : null;
        var locations = new List<(string Namespace, string Location)>();
        var namespaces = new HashSet<string>(StringComparer.Ordinal);
        if (root.TryGetProperty("locations", out var pairs))
        {
            if (pairs.ValueKind != JsonValueKind.Array || pairs.GetArrayLength() > 32)
                throw new InvalidOperationException("XML schema hints exceed 32 namespace/location pairs");
            foreach (var pair in pairs.EnumerateArray())
            {
                if (pair.ValueKind != JsonValueKind.Object)
                    throw new InvalidOperationException("XML schema-hint pair must be an object");
                var pairProperties = new HashSet<string>(StringComparer.Ordinal);
                foreach (var property in pair.EnumerateObject())
                    if (!pairProperties.Add(property.Name) || property.Name is not ("namespace" or "location"))
                        throw new InvalidOperationException("duplicate or unknown XML schema-hint pair property");
                if (!pair.TryGetProperty("namespace", out var namespaceValue) ||
                    !pair.TryGetProperty("location", out var locationValue))
                    throw new InvalidOperationException("missing XML schema-hint namespace or location");
                var uri = namespaceValue.GetString() ?? throw new InvalidOperationException("missing XML schema-hint namespace");
                var location = locationValue.GetString() ?? throw new InvalidOperationException("missing XML schema-hint location");
                ValidateHintToken(uri);
                ValidateHintToken(location);
                if (!namespaces.Add(uri)) throw new InvalidOperationException("duplicate XML schema-hint namespace");
                locations.Add((uri, location));
            }
        }
        if (noNamespace is not null) ValidateHintToken(noNamespace);
        if (noNamespace is null && locations.Count == 0)
            throw new InvalidOperationException("XML schema hints require at least one location");
        return new XmlLiteralHints(noNamespace, locations);
    }

    /// <summary>
    /// Refuses the first character outside the XML 1.0 Char production, after
    /// the size limit like the native writer. Unpaired UTF-16 surrogates have
    /// no Unicode scalar value and are refused at their own code unit.
    /// </summary>
    private static void RequireXmlCharacters(string output)
    {
        var byteOffset = 0;
        for (var index = 0; index < output.Length; index++)
        {
            var character = output[index];
            if (char.IsHighSurrogate(character) && index + 1 < output.Length &&
                char.IsLowSurrogate(output[index + 1]))
            {
                index++;
                byteOffset += 4;
                continue;
            }
            if (character is not ('\t' or '\n' or '\r') &&
                (character < 0x20 || character > 0xD7FF) &&
                (character < 0xE000 || character > 0xFFFD))
            {
                throw new InvalidOperationException(
                    $"XML output contains forbidden XML 1.0 character U+{(int)character:X4} at UTF-8 byte {byteOffset}");
            }
            byteOffset += character < 0x80 ? 1 : character < 0x800 ? 2 : 3;
        }
    }

    private static void ValidateHintToken(string value)
    {
        if (value.Length == 0 || OutputUtf8.GetByteCount(value) > 4096 ||
            value.Any(character => character is ' ' or '\t' or '\r' or '\n'))
            throw new InvalidOperationException("XML schema-hint token must be nonempty, at most 4096 UTF-8 bytes, and contain no XML whitespace");
        _ = XmlConvert.VerifyXmlChars(value);
    }
}

/// <summary>Opaque completed XML; each encoding returns a fresh caller-owned byte array.</summary>
public sealed class FerruleXmlPreparedUtf8Document
{
    private readonly Func<byte[]> _encode;

    internal FerruleXmlPreparedUtf8Document(int utf8ByteCount, Func<byte[]> encode,
        string? legacyText = null)
    {
        Utf8ByteCount = utf8ByteCount;
        _encode = encode;
        LegacyText = legacyText;
    }

    public int Utf8ByteCount { get; }
    internal string? LegacyText { get; }

    /// <summary>Encodes the completed document; no mapping or writer is run again.</summary>
    public byte[] ToByteArray() => _encode();
}

public static partial class FerruleXml
{
    private static (XmlSchemaNode, FerruleInstance) ProjectScalarOutput(
        string payload, XmlSchemaNode schema, FerruleInstance instance, string descriptor)
    {
        if (Encoding.UTF8.GetByteCount(descriptor) > 32 * 1024)
            throw new InvalidOperationException("XML scalar output descriptor exceeds 32 KiB");
        using var policy = JsonDocument.Parse(descriptor);
        if (policy.RootElement.ValueKind == JsonValueKind.Object &&
            policy.RootElement.TryGetProperty("mode", out var documentMode) &&
            documentMode.ValueKind == JsonValueKind.String &&
            documentMode.GetString() == "suppress_cast_in_document_v1")
        {
            ExactProperties(policy.RootElement, "mode");
            return ProjectDocumentScalarOutput(payload, schema, instance);
        }
        ExactProperties(policy.RootElement, "mode", "declarations");
        if (ScalarProperty(policy.RootElement, "mode").GetString() != "suppress_cast_in_subtree_v1")
            throw new InvalidOperationException("unsupported XML scalar output mode");
        var list = ScalarProperty(policy.RootElement, "declarations");
        if (list.ValueKind != JsonValueKind.Array || list.GetArrayLength() is < 1 or > 256)
            throw new InvalidOperationException("invalid XML scalar output field count");
        var names = new HashSet<string>(StringComparer.Ordinal);
        var bytes = 0;
        string? previous = null;
        foreach (var item in list.EnumerateArray())
        {
            var name = item.GetString() ?? throw new InvalidOperationException("missing field name");
            RequireScalarOutputName(name);
            bytes += Encoding.UTF8.GetByteCount(name);
            if (bytes > 16 * 1024 || previous is not null && StringComparer.Ordinal.Compare(previous, name) >= 0 || !names.Add(name))
                throw new InvalidOperationException("XML scalar output fields must be a bounded sorted unique set");
            previous = name;
        }
        using var document = JsonDocument.Parse(payload);
        var root = document.RootElement;
        RequireScalarSchemaNode(root, false);
        var rootKind = ScalarProperty(root, "kind");
        ExactProperties(rootKind, "kind", "children");
        if (ScalarProperty(rootKind, "kind").GetString() != "group") throw new InvalidOperationException("XML scalar output requires a group");
        var children = ScalarProperty(rootKind, "children");
        if (children.ValueKind != JsonValueKind.Array || children.GetArrayLength() > 256) throw new InvalidOperationException("too many XML scalar output fields");
        var doubles = new HashSet<string>(StringComparer.Ordinal);
        var fields = new HashSet<string>(StringComparer.Ordinal);
        foreach (var field in children.EnumerateArray())
        {
            RequireScalarSchemaNode(field, true);
            var name = ScalarProperty(field, "name").GetString()!;
            if (!fields.Add(name)) throw new InvalidOperationException("duplicate XML scalar output field");
            var kind = ScalarProperty(field, "kind");
            ExactProperties(kind, "kind", "ty");
            if (ScalarProperty(kind, "kind").GetString() != "scalar") throw new InvalidOperationException("XML scalar output requires flat fields");
            switch (ScalarProperty(kind, "ty").GetString())
            {
                case "float": doubles.Add(name); break;
                case "string": break;
                default: throw new InvalidOperationException("XML scalar output requires String/Float fields");
            }
        }
        if (!doubles.SetEquals(names)) throw new InvalidOperationException("XML scalar output declarations do not match all Float fields");
        if (instance is not FerruleGroup group) throw new InvalidOperationException("XML scalar output requires a group instance");
        var projectedSchema = schema with { Children = schema.Children.Select(field => names.Contains(field.Name)
            ? field with { ScalarType = XmlScalarType.String } : field).ToArray() };
        var projectedInstance = group.ExactCloneFields(group.Fields.Select(field => new FerruleField(field.Name,
            names.Contains(field.Name) ? LexicalScalarInstance(field.Value) : field.Value)));
        return (projectedSchema, projectedInstance);
    }

    private const int MaximumScalarOutputDocumentNodes = 1024;
    private const int MaximumScalarOutputDocumentDepth = 32;

    // Mirrors the Rust document profile: a bounded plain unqualified element tree.
    private static (XmlSchemaNode, FerruleInstance) ProjectDocumentScalarOutput(
        string payload, XmlSchemaNode schema, FerruleInstance instance)
    {
        using var document = JsonDocument.Parse(payload);
        var nodes = 0;
        RequireDocumentSchemaNode(document.RootElement, 0, ref nodes);
        if (ScalarProperty(ScalarProperty(document.RootElement, "kind"), "kind").GetString() != "group")
            throw new InvalidOperationException("XML scalar output requires a group");
        return (LexicalDocumentSchema(schema), ProjectDocumentInstance(schema, instance));
    }

    private static void RequireDocumentSchemaNode(JsonElement node, int depth, ref int nodes)
    {
        if (++nodes > MaximumScalarOutputDocumentNodes || depth > MaximumScalarOutputDocumentDepth)
            throw new InvalidOperationException("XML scalar output document schema exceeds its limits");
        RequireScalarSchemaNode(node, depth != 0);
        var kind = ScalarProperty(node, "kind");
        switch (ScalarProperty(kind, "kind").GetString())
        {
            case "scalar":
                ExactProperties(kind, "kind", "ty");
                if (ScalarProperty(kind, "ty").GetString() is not ("string" or "int" or "float" or "bool"))
                    throw new InvalidOperationException("unsupported XML scalar output field type");
                break;
            case "group":
                ExactProperties(kind, "kind", "children");
                var children = ScalarProperty(kind, "children");
                if (children.ValueKind != JsonValueKind.Array) throw new InvalidOperationException("invalid XML scalar output children");
                var names = new HashSet<string>(StringComparer.Ordinal);
                foreach (var child in children.EnumerateArray())
                {
                    RequireDocumentSchemaNode(child, depth + 1, ref nodes);
                    if (!names.Add(ScalarProperty(child, "name").GetString()!))
                        throw new InvalidOperationException("duplicate XML scalar output field");
                }
                break;
            default:
                throw new InvalidOperationException("unsupported XML scalar output schema kind");
        }
    }

    private static XmlSchemaNode LexicalDocumentSchema(XmlSchemaNode schema) => schema.ScalarType is not null
        ? schema with { ScalarType = XmlScalarType.String }
        : schema with { Children = schema.Children.Select(LexicalDocumentSchema).ToArray() };

    // Walks the validated schema, never the value: an unexpected nested value
    // is refused at its slot before anything beneath it is visited.
    private static FerruleInstance ProjectDocumentInstance(XmlSchemaNode schema, FerruleInstance instance)
    {
        if (schema.Repeating)
        {
            if (instance is not FerruleRepeated repeated) throw new InvalidOperationException("expected repeating XML elements");
            return new FerruleRepeated(repeated.Items.Select(item => ProjectDocumentItem(schema, item)).ToArray());
        }
        // As in the ordinary writer, a non-repeating group may receive a
        // mapped sequence of zero or more items.
        if (schema.ScalarType is null && instance is FerruleMappedSequence mapped)
            return new FerruleMappedSequence(mapped.Items.Select(item => ProjectDocumentItem(schema, item)).ToArray());
        return ProjectDocumentItem(schema, instance);
    }

    private static FerruleInstance ProjectDocumentItem(XmlSchemaNode schema, FerruleInstance instance)
    {
        if (schema.ScalarType is not null) return LexicalScalar(instance, boolText: true);
        if (instance is not FerruleGroup group) throw new InvalidOperationException("expected an XML element group");
        return group.ExactCloneFields(group.Fields.Select(field => new FerruleField(field.Name, ProjectDocumentInstance(
            schema.Children.FirstOrDefault(child => child.Name == field.Name) ??
                throw new InvalidOperationException("unexpected XML scalar output field"), field.Value))).ToArray());
    }

    private static void RequireScalarOutputName(string name)
    {
        if (name.Length is < 1 or > 256 || !(char.IsAsciiLetter(name[0]) || name[0] == '_') ||
            name.Any(c => !(char.IsAsciiLetterOrDigit(c) || c is '_' or '-' or '.')))
            throw new InvalidOperationException("invalid XML scalar output field name");
    }

    private static JsonElement ScalarProperty(JsonElement value, string name) =>
        value.TryGetProperty(name, out var property) ? property : throw new InvalidOperationException("missing XML scalar output property " + name);

    private static void ExactProperties(JsonElement value, params string[] allowed)
    {
        if (value.ValueKind != JsonValueKind.Object) throw new InvalidOperationException("expected XML scalar output object");
        var names = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in value.EnumerateObject())
            if (!allowed.Contains(property.Name, StringComparer.Ordinal) || !names.Add(property.Name))
                throw new InvalidOperationException("unknown or repeated XML scalar output property");
    }

    private static void RequireScalarSchemaNode(JsonElement node, bool child)
    {
        ExactProperties(node, "name", "repeating", "xml_optional", "xml_namespace", "kind");
        RequireScalarOutputName(ScalarProperty(node, "name").GetString() ?? throw new InvalidOperationException("missing XML field name"));
        if (!child && node.TryGetProperty("repeating", out var repeating) && repeating.GetBoolean())
            throw new InvalidOperationException("XML scalar output root cannot repeat");
        if (!child && node.TryGetProperty("xml_optional", out var optional) && optional.GetBoolean())
            throw new InvalidOperationException("XML scalar output root cannot be optional");
        if (node.TryGetProperty("xml_namespace", out var ns))
        {
            ExactProperties(ns, "kind");
            if (ScalarProperty(ns, "kind").GetString() != "unqualified") throw new InvalidOperationException("XML scalar output requires unqualified names");
        }
    }

    private static FerruleInstance LexicalScalarInstance(FerruleInstance instance) => instance switch
    {
        FerruleRepeated repeated => new FerruleRepeated(repeated.Items.Select(item => LexicalScalar(item))),
        _ => LexicalScalar(instance),
    };

    // The flat profile's double fields keep their original String/Int/Float
    // admission; the document profile also writes Bool values lexically.
    private static FerruleInstance LexicalScalar(FerruleInstance instance, bool boolText = false)
    {
        if (instance is not FerruleScalar scalar) throw new InvalidOperationException("expected suppressed scalar");
        var value = scalar.Value;
        return value.Kind switch
        {
            FerruleValueKind.String or FerruleValueKind.Null => instance,
            FerruleValueKind.Int64 => new FerruleScalar(FerruleValue.FromString(value.Int64Value.ToString(CultureInfo.InvariantCulture))),
            FerruleValueKind.Bool when boolText => new FerruleScalar(FerruleValue.FromString(value.BooleanValue ? "true" : "false")),
            FerruleValueKind.Double when double.IsFinite(value.DoubleValue) => new FerruleScalar(FerruleValue.FromString(FerruleValueMaps.RustFloatText(value.DoubleValue))),
            _ => throw new InvalidOperationException("unsupported suppressed-cast value"),
        };
    }
}
