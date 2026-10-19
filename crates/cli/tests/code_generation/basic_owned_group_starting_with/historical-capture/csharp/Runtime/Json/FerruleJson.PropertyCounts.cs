using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    private static JsonPropertyCountRange? ReadPropertyCountRange(
        string name,
        JsonElement element,
        bool isGroup)
    {
        if (!element.TryGetProperty("property_count_range", out var rangeElement) ||
            rangeElement.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        RequireKind(
            rangeElement,
            JsonValueKind.Object,
            $"schema node '{name}' property-count range",
            "object");
        var fields = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in rangeElement.EnumerateObject())
        {
            if (property.Name is not ("minimum" or "maximum") ||
                !fields.Add(property.Name))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' property-count range has an unknown or duplicate field '{property.Name}'.");
            }
        }
        var minimum = OptionalUInt64(rangeElement, "minimum", false) ?? 0;
        var maximum = OptionalUInt64(rangeElement, "maximum", true);
        if (!isGroup ||
            minimum == 0 && maximum is null ||
            maximum is { } upper && minimum > upper)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has invalid property-count metadata.");
        }
        return new JsonPropertyCountRange(minimum, maximum);
    }

    private static void ValidatePropertyCountSchema(
        string name,
        JsonPropertyCountRange? range,
        IReadOnlyList<JsonSchemaNode> children,
        JsonSchemaNode? dynamic,
        IReadOnlyList<string> required,
        IReadOnlyList<JsonAlternative> alternatives)
    {
        if (range is not { } propertyCountRange)
        {
            return;
        }
        if (!PropertyCountFits(
                propertyCountRange,
                required.Count,
                dynamic is null ? children.Count : null))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' cannot satisfy its property-count range.");
        }
        foreach (var alternative in alternatives)
        {
            if (!PropertyCountFits(
                    propertyCountRange,
                    alternative.Required.Count,
                    alternative.Members.Count))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has an alternative that cannot satisfy its property-count range.");
            }
        }
    }

    private static bool PropertyCountFits(
        JsonPropertyCountRange range,
        int requiredCount,
        int? capacity) =>
        (range.Maximum is null || (ulong)requiredCount <= range.Maximum) &&
        (capacity is null || range.Minimum <= (ulong)capacity);

    private static void ValidatePropertyCount(JsonSchemaNode schema, int count)
    {
        if (schema.PropertyCountRange is { } range && !range.Contains(count))
        {
            throw Boundary(
                $"JSON object '{schema.Name}' has {count} properties outside its declared property-count range.");
        }
    }

    private static void ValidateOutputPropertyCount(
        JsonSchemaNode schema,
        FerruleGroup group)
    {
        if (schema.PropertyCountRange is null)
        {
            return;
        }

        var count = 0;
        if (schema.Dynamic is { } dynamic)
        {
            foreach (var field in group.Fields)
            {
                var child = schema.Child(field.Name) ?? dynamic;
                if (!BoundaryAbsence(child, field.Value))
                {
                    count = checked(count + 1);
                }
            }
        }
        else
        {
            foreach (var child in schema.Children)
            {
                if (group.TryGetField(child.Name, out var value) &&
                    !BoundaryAbsence(child, value))
                {
                    count = checked(count + 1);
                }
            }
        }

        ValidatePropertyCount(schema, count);
    }

    private sealed record JsonPropertyCountRange(ulong Minimum, ulong? Maximum)
    {
        public bool Contains(int count)
        {
            var value = (ulong)count;
            return value >= Minimum && (Maximum is null || value <= Maximum);
        }
    }
}
