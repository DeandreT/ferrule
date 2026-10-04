using System.Text;
using System.Text.Json;
using System.Xml;

namespace Ferrule.Runtime;

public static partial class FerruleXml
{
    private enum StructuredScalarType { String, Int, Float, Bool }
    private sealed record StructuredSchema(string Name, bool NamespaceIsExplicit,
        string? NamespaceUri, bool Repeating, bool Attribute, bool Text, bool Nillable,
        StructuredScalarType? ScalarType, IReadOnlyList<StructuredSchema> Children);

    private static readonly string[] StructuredNullMetadata =
    {
        "recursive_ref", "fixed", "default", "value_generation", "xml_wildcard_namespace",
        "xml_default_type", "json_allowed_values", "numeric_range", "json_multiple_of",
        "item_count_range", "json_contains", "json_dependent_schemas", "property_count_range",
        "json_property_dependencies", "json_pattern_property_names", "json_property_names",
        "string_length_range", "json_patterns", "database_relation",
    };
    private static readonly string[] StructuredEmptyMetadata =
    {
        "xml_name_alternatives", "xml_repeating_sequences", "xml_repeating_choices", "json_formats",
    };
    private static readonly string[] StructuredFalseMetadata =
    {
        "nullable", "container_nullable", "json_any", "json_unique_items", "xml_type_alternatives",
    };
    private static readonly HashSet<string> StructuredNodeProperties = new(
        StructuredNullMetadata.Concat(StructuredEmptyMetadata).Concat(StructuredFalseMetadata).Concat(new[]
        { "name", "kind", "xml_namespace", "repeating", "attribute", "text", "nillable", "xml_optional",
          "xml_attribute_required", "alternative_mode", "xml_alternative_kind", "xml_wildcard_process_contents" }),
        StringComparer.Ordinal);
    private static readonly HashSet<string> StructuredKindTagProperty = new(new[] { "kind" },
        StringComparer.Ordinal);
    private static readonly HashSet<string> StructuredScalarKindProperties = new(new[] { "kind", "ty" },
        StringComparer.Ordinal);
    private static readonly HashSet<string> StructuredGroupKindProperties = new(new[]
        { "kind", "children", "required", "dynamic", "alternatives", "xml_restricted_alternatives" },
        StringComparer.Ordinal);
    private static readonly HashSet<string> StructuredNamespaceProperties = new(new[] { "kind", "uri" },
        StringComparer.Ordinal);

    private static StructuredSchema ParseStructuredSchema(string descriptor)
    {
        try
        {
            var payload = FerruleEmbeddedSchema.Unwrap(descriptor, MaximumEmbeddedSchemaBytes);
            using var json = JsonDocument.Parse(payload.Payload, new JsonDocumentOptions
            {
                // Match the existing descriptor transport bound, separately
                // from the logical 64-level schema acceptance limit. Exact
                // paired container-depth edges remain required runtime gates.
                MaxDepth = 127,
                CommentHandling = JsonCommentHandling.Disallow,
                AllowTrailingCommas = false,
            });
            var count = 0;
            return ReadStructuredSchemaNode(json.RootElement, 1, ref count);
        }
        catch (Exception error) when (error is JsonException or InvalidOperationException or
            ArgumentException or FormatException or XmlException or OverflowException)
        {
            throw new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Schema,
                error.Message, error);
        }
    }

    private static StructuredSchema ReadStructuredSchemaNode(JsonElement node, int depth, ref int count)
    {
        if (depth > MaximumStructuredDepth || ++count > MaximumStructuredSchemaNodes)
            throw new FormatException("structured XML schema exceeds its node/depth limit");
        RequireStructuredUniqueKnown(node, StructuredNodeProperties);
        foreach (var property in StructuredNullMetadata) RequireInputNull(node, property);
        foreach (var property in StructuredEmptyMetadata) RequireInputEmpty(node, property);
        foreach (var property in StructuredFalseMetadata) RequireInputBoolean(node, property, false);
        RequireInputEnum(node, "alternative_mode", "exclusive");
        RequireInputEnum(node, "xml_alternative_kind", "xsi_type");
        RequireInputEnum(node, "xml_wildcard_process_contents", "skip");
        var name = InputString(node, "name");
        var repeating = StructuredBoolean(node, "repeating");
        var attribute = StructuredBoolean(node, "attribute");
        var text = StructuredBoolean(node, "text");
        var nillable = StructuredBoolean(node, "nillable");
        var optional = StructuredBoolean(node, "xml_optional");
        var requiredAttribute = StructuredBoolean(node, "xml_attribute_required");
        var kind = InputProperty(node, "kind");
        RequireStructuredUniqueKnown(kind, StructuredKindTagProperty);
        var kindName = InputString(kind, "kind");
        if (kindName == "scalar") RequireStructuredUniqueKnown(kind, StructuredScalarKindProperties);
        else if (kindName == "group") RequireStructuredUniqueKnown(kind, StructuredGroupKindProperties);
        StructuredScalarType? scalar = kindName == "scalar" ? InputString(kind, "ty") switch
        {
            "string" => StructuredScalarType.String,
            "int" => StructuredScalarType.Int,
            "float" => StructuredScalarType.Float,
            "bool" => StructuredScalarType.Bool,
            _ => throw new FormatException("unsupported structured XML scalar type"),
        } : null;
        if (kindName is not ("group" or "scalar"))
            throw new FormatException("unsupported structured XML schema kind");
        if (depth == 1 && (repeating || attribute || text || optional))
            throw new FormatException("structured XML root must be one ordinary element");
        if (attribute && text || (attribute || text) && (repeating || nillable || scalar is null) ||
            optional && (depth == 1 || repeating || attribute || text) || requiredAttribute && !attribute)
            throw new FormatException("unsupported structured XML field role");
        if (text)
        {
            if (name != "#text" || depth == 1)
                throw new FormatException("structured XML text requires a #text scalar child");
        }
        else if (!FerrulePrimaryRoot.NcNameIsValid(name) || name.Any(char.IsSurrogate))
            throw new FormatException("structured XML requires a bounded BMP physical local name");
        if (name == "#text" && !text)
            throw new FormatException("structured XML #text requires text metadata");

        var explicitNamespace = false;
        string? uri = null;
        if (node.TryGetProperty("xml_namespace", out var identity) && identity.ValueKind != JsonValueKind.Null)
        {
            RequireStructuredUniqueKnown(identity, StructuredNamespaceProperties);
            explicitNamespace = true;
            switch (InputString(identity, "kind"))
            {
                case "unqualified" when !identity.TryGetProperty("uri", out var content) ||
                    content.ValueKind == JsonValueKind.Null: break;
                case "qualified":
                    uri = InputString(identity, "uri");
                    if (uri.Length == 0 ||
                        InputUtf8.GetByteCount(uri) + InputUtf8.GetByteCount(name) + 2 > FerrulePrimaryRoot.MaximumIdentityBytes ||
                        !FerrulePrimaryRoot.TypeIdentityIsValid($"{{{uri}}}{name}"))
                        throw new FormatException("structured XML source namespace is invalid or too large");
                    XmlConvert.VerifyXmlChars(uri);
                    break;
                default: throw new FormatException("invalid structured XML namespace identity");
            }
        }
        if (text && explicitNamespace || attribute && (name == "xmlns" || uri == "http://www.w3.org/2000/xmlns/") ||
            !attribute && !text && (uri is "http://www.w3.org/XML/1998/namespace" or "http://www.w3.org/2000/xmlns/") ||
            attribute && uri == XsiNamespace && (name is "nil" or "type"))
            throw new FormatException("unsupported structured XML namespace role");

        var children = new List<StructuredSchema>();
        if (scalar is null)
        {
            if (nillable) throw new FormatException("nillable groups are outside the structured XML profile");
            RequireInputEmpty(kind, "required");
            RequireInputEmpty(kind, "alternatives");
            RequireInputEmpty(kind, "xml_restricted_alternatives");
            RequireInputNull(kind, "dynamic");
            var fields = InputProperty(kind, "children");
            if (fields.ValueKind != JsonValueKind.Array)
                throw new FormatException("structured XML group children must be an array");
            var names = new HashSet<string>(StringComparer.Ordinal);
            foreach (var field in fields.EnumerateArray())
            {
                var child = ReadStructuredSchemaNode(field, depth + 1, ref count);
                if (!names.Add(child.Name)) throw new FormatException("duplicate structured XML field name");
                children.Add(child);
            }
            if (children.Any(child => child.Text) && children.Any(child => !child.Text && !child.Attribute))
                throw new FormatException("mixed text/element groups are outside the structured XML profile");
        }
        return new StructuredSchema(name, explicitNamespace, uri, repeating, attribute, text,
            nillable, scalar, children);
    }

    private static bool StructuredBoolean(JsonElement node, string name)
    {
        if (!node.TryGetProperty(name, out var value)) return false;
        return value.ValueKind switch
        {
            JsonValueKind.True => true,
            JsonValueKind.False => false,
            _ => throw new FormatException($"invalid structured XML flag '{name}'"),
        };
    }

    private static void RequireStructuredUniqueKnown(JsonElement node, HashSet<string> known)
    {
        if (node.ValueKind != JsonValueKind.Object) throw new FormatException("invalid structured XML schema object");
        var seen = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in node.EnumerateObject())
            if (known.Contains(property.Name) && !seen.Add(property.Name))
                throw new FormatException($"duplicate structured XML schema property '{property.Name}'");
        // Unknown properties retain the existing closed Rust descriptor decode behavior.
    }
}
