using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    private const int Json5MaximumSchemaNodes = 4096;
    private const int Json5MaximumLogicalLevels = 64;
    private const int Json5MaximumNameBytes = 4096;
    private const int Json5MaximumTotalNameBytes = 1024 * 1024;

    private static string ValidateJson5Descriptor(string descriptor)
    {
        var bytes = FerruleJson5Syntax.CountBoundaryBytes(descriptor);
        Json5SchemaLimit(FerruleJson5SchemaResource.DescriptorBytes, bytes, MaximumSchemaBytes);
        var payload = descriptor.StartsWith(FerruleEmbeddedSchema.V2Prefix, StringComparison.Ordinal)
            ? descriptor[FerruleEmbeddedSchema.V2Prefix.Length..] : descriptor;
        if (payload == descriptor && descriptor.StartsWith("FERRULE-EMBEDDED-SCHEMA/", StringComparison.Ordinal))
        { throw new FormatException("Unsupported embedded schema version."); }
        // A complete strict parse owns malformed/depth causes before key checks.
        // JsonElement retains every duplicate; GetProperty alone would hide them.
        using var parsed = JsonDocument.Parse(payload, new JsonDocumentOptions { MaxDepth = MaximumParsedDepth });
        Json5DescriptorScalars(payload, parsed.RootElement);
        Json5DescriptorKeys(parsed.RootElement);
        var pending = new Stack<(JsonElement Node, int Depth)>();
        pending.Push((parsed.RootElement, 1));
        var nodes = 0;
        long names = 0;
        var groups = new List<JsonElement>();
        while (pending.TryPop(out var current))
        {
            Json5SchemaLimit(FerruleJson5SchemaResource.SchemaNodes, ++nodes, Json5MaximumSchemaNodes);
            Json5SchemaLimit(FerruleJson5SchemaResource.LogicalLevels, current.Depth, Json5MaximumLogicalLevels);
            var node = current.Node;
            Json5Kind(node, JsonValueKind.Object, "node");
            foreach (var field in node.EnumerateObject()) { Json5NodeMetadata(field); }
            Json5Name(Json5String(node, "name"), ref names);
            var kind = Json5Property(node, "kind");
            Json5Kind(kind, JsonValueKind.Object, "kind");
            var tag = Json5String(kind, "kind");
            if (current.Depth == 1 && tag != "group")
            { throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.RootObjectRequired); }
            foreach (var field in kind.EnumerateObject())
            {
                var allowed = tag switch
                {
                    "group" => field.Name is "kind" or "children" or "alternatives" or "required" or "xml_restricted_alternatives" or "dynamic",
                    "scalar" => field.Name is "kind" or "ty",
                    _ => throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.UnsupportedMetadata, "kind"),
                };
                if (!allowed) { throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.UnknownDescriptorField, field.Name); }
            }
            if (tag == "scalar")
            {
                if (Json5String(kind, "ty") is not ("string" or "int" or "float" or "bool"))
                { throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.UnsupportedMetadata, "kind.ty"); }
                continue;
            }
            if (tag != "group") { throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.UnsupportedMetadata, "kind"); }
            if (node.TryGetProperty("nullable", out var nullable) && nullable.ValueKind != JsonValueKind.False)
            { throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.UnsupportedMetadata, "nullable_group"); }
            var children = Json5Property(kind, "children");
            Json5Kind(children, JsonValueKind.Array, "kind.children");
            foreach (var field in new[] { "alternatives", "xml_restricted_alternatives" })
            {
                if (kind.TryGetProperty(field, out var value))
                { Json5Default(value.ValueKind == JsonValueKind.Array && value.GetArrayLength() == 0, field); }
            }
            if (kind.TryGetProperty("dynamic", out var dynamic)) { Json5Default(dynamic.ValueKind == JsonValueKind.Null, "dynamic"); }
            if (kind.TryGetProperty("required", out var required))
            {
                Json5Kind(required, JsonValueKind.Array, "kind.required");
                if (required.GetArrayLength() > children.GetArrayLength())
                { throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.InvalidRequiredName, "too_many_names"); }
                foreach (var item in required.EnumerateArray())
                {
                    Json5Kind(item, JsonValueKind.String, "kind.required.name");
                    Json5Name(item.GetString()!, ref names);
                }
            }
            Json5SchemaLimit(FerruleJson5SchemaResource.SchemaNodes,
                (long)nodes + pending.Count + children.GetArrayLength(), Json5MaximumSchemaNodes);
            // Reverse push preserves declared first-error order without recursion.
            for (var index = children.GetArrayLength() - 1; index >= 0; index--)
            { pending.Push((children[index], current.Depth + 1)); }
            groups.Add(kind);
        }
        // Only after the complete bounded census allocate child/required sets.
        foreach (var kind in groups)
        {
            var children = new HashSet<string>(StringComparer.Ordinal);
            foreach (var child in Json5Property(kind, "children").EnumerateArray())
            {
                var name = Json5String(child, "name");
                if (!children.Add(name))
                { throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.DuplicateChildName, name: name); }
            }
            if (!kind.TryGetProperty("required", out var required)) { continue; }
            var seen = new HashSet<string>(StringComparer.Ordinal);
            foreach (var item in required.EnumerateArray())
            {
                var name = item.GetString()!;
                var failure = name.Length == 0 ? "empty_name"
                    : !seen.Add(name) ? "duplicate_name"
                    : !children.Contains(name) ? "undeclared_name" : null;
                if (failure is not null)
                { throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.InvalidRequiredName, failure, name); }
            }
        }
        // Admission has checked every original field, including explicit null
        // defaults, before adapting those defaults for the unchanged reader.
        return Json5ReaderDescriptor(parsed.RootElement,
            descriptor.StartsWith(FerruleEmbeddedSchema.V2Prefix, StringComparison.Ordinal));
    }

    private static void Json5DescriptorScalars(string payload, JsonElement root)
    {
        // JsonDocument defers scalar conversion; the shared strict Value parse
        // rejects all nonfinite numbers before duplicate/profile inspection.
        ValidateJsonScalars(root);
        // Reuse the strict codec's raw escaped-surrogate check for every string,
        // including property names and values of unknown metadata. The retained
        // descriptor cap bounds this complete lexical pass and temporary text.
        for (var index = 0; index < payload.Length; index++)
        {
            if (payload[index] != '"') { continue; }
            var start = index++;
            while (index < payload.Length && payload[index] != '"')
            { if (payload[index] == '\\') { index++; } index++; }
            try
            {
                using var scalar = JsonDocument.Parse(payload.Substring(start, index - start + 1));
                if (!HasValidJsonStringScalar(scalar.RootElement))
                { throw new JsonException("Embedded JSON5 descriptor contains an invalid Unicode string."); }
            }
            catch (Exception error) when (error is InvalidOperationException or FormatException or OverflowException)
            { throw new JsonException("Embedded JSON5 descriptor string is invalid.", error); }
        }
    }

    private static void Json5DescriptorKeys(JsonElement value)
    {
        // The complete strict parse already bounded this recursion to 127
        // containers. Visit original source order, including nested unknown
        // metadata, before any semantic descriptor admission.
        if (value.ValueKind == JsonValueKind.Object)
        {
            var keys = new HashSet<string>(StringComparer.Ordinal);
            foreach (var property in value.EnumerateObject())
            {
                Json5SchemaLimit(FerruleJson5SchemaResource.NameLength,
                    StrictUtf8.GetByteCount(property.Name), Json5MaximumNameBytes);
                if (!keys.Add(property.Name))
                { throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.DuplicateDescriptorField, property.Name); }
                Json5DescriptorKeys(property.Value);
            }
        }
        else if (value.ValueKind == JsonValueKind.Array)
        { foreach (var item in value.EnumerateArray()) { Json5DescriptorKeys(item); } }
    }

    private static void Json5NodeMetadata(System.Text.Json.JsonProperty field)
    {
        var value = field.Value;
        var accepted = field.Name switch
        {
            "name" or "kind" => true,
            "nullable" => value.ValueKind is JsonValueKind.True or JsonValueKind.False,
            "xml_namespace" or "xml_wildcard_namespace" or "recursive_ref" or "fixed"
            or "json_allowed_values" or "numeric_range" or "json_multiple_of" or "item_count_range"
            or "json_contains" or "json_dependent_schemas" or "property_count_range"
            or "json_property_dependencies" or "json_pattern_property_names" or "json_property_names"
            or "string_length_range" or "json_patterns" or "default" or "value_generation"
            or "xml_default_type" or "database_relation" => value.ValueKind == JsonValueKind.Null,
            "repeating" or "attribute" or "text" or "nillable" or "xml_optional"
            or "xml_attribute_required" or "container_nullable" or "json_any"
            or "json_unique_items" or "xml_type_alternatives" => value.ValueKind == JsonValueKind.False,
            "xml_name_alternatives" or "xml_repeating_sequences" or "xml_repeating_choices" or "json_formats"
                => value.ValueKind == JsonValueKind.Array && value.GetArrayLength() == 0,
            "xml_wildcard_process_contents" => value.ValueKind == JsonValueKind.String && value.GetString() == "skip",
            "alternative_mode" => value.ValueKind == JsonValueKind.String && value.GetString() == "exclusive",
            "xml_alternative_kind" => value.ValueKind == JsonValueKind.String && value.GetString() == "xsi_type",
            _ => throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.UnknownDescriptorField, field.Name),
        };
        Json5Default(accepted, field.Name);
    }

    private static void Json5Default(bool accepted, string field)
    { if (!accepted) { throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.UnsupportedMetadata, field); } }
    private static JsonElement Json5Property(JsonElement value, string field) =>
        value.ValueKind == JsonValueKind.Object && value.TryGetProperty(field, out var result)
            ? result : throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.InvalidDescriptorShape, field);
    private static string Json5String(JsonElement value, string field)
    {
        var item = Json5Property(value, field); Json5Kind(item, JsonValueKind.String, field);
        return item.GetString()!;
    }
    private static void Json5Kind(JsonElement value, JsonValueKind expected, string field)
    { if (value.ValueKind != expected) { throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.InvalidDescriptorShape, field); } }
    private static void Json5Name(string name, ref long total)
    {
        var bytes = StrictUtf8.GetByteCount(name);
        Json5SchemaLimit(FerruleJson5SchemaResource.NameLength, bytes, Json5MaximumNameBytes);
        total += bytes; Json5SchemaLimit(FerruleJson5SchemaResource.NameBytes, total, Json5MaximumTotalNameBytes);
    }
    private static void Json5SchemaLimit(FerruleJson5SchemaResource resource, long requested, long maximum)
    { if (requested > maximum) { throw new FerruleJson5SchemaException(FerruleJson5SchemaKind.Limit, resource: resource, requested: requested, maximum: maximum); } }
}
