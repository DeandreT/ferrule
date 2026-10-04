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
        string descriptor,
        FerruleInstance instance,
        bool declaration,
        bool indent,
        string? defaultNamespace,
        string? schemaHintsJson)
    {
        string payload;
        XmlSchemaNode parsedSchema;
        try
        {
            var schema = FerruleEmbeddedSchema.Unwrap(descriptor, MaximumEmbeddedSchemaBytes);
            payload = schema.Payload;
            using var document = JsonDocument.Parse(payload,
                new JsonDocumentOptions { MaxDepth = MaximumSchemaDepth });
            parsedSchema = XmlSchemaNode.Parse(document.RootElement, 0, documentBoundary: true);
            RequireDocumentNamespaceMetadata(parsedSchema, root: true);
            if (schemaHintsJson is not null && HasSupplementaryDocumentName(parsedSchema))
                throw new FormatException("supplementary XML names are unsupported by the generated input and hinted-output parsers");
        }
        catch (Exception error) when (error is JsonException or FormatException or
            InvalidOperationException or ArgumentException or OverflowException or XmlException)
        {
            throw new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Schema, error.Message, error);
        }
        try
        {
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
                schemaHintsJson, documentBoundary: true).StringValue;
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

    private static void ValidateHintToken(string value)
    {
        if (value.Length == 0 || OutputUtf8.GetByteCount(value) > 4096 ||
            value.Any(character => character is ' ' or '\t' or '\r' or '\n'))
            throw new InvalidOperationException("XML schema-hint token must be nonempty, at most 4096 UTF-8 bytes, and contain no XML whitespace");
        _ = XmlConvert.VerifyXmlChars(value);
    }
}
