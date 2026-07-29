using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    private const int MaximumJsonPropertyDependencyTriggers = 256;
    private const int MaximumJsonPropertyDependencyEdges = 4096;
    private const int MaximumJsonPropertyDependencyNameBytes = 256 * 1024;

    private static IReadOnlyList<JsonPropertyDependency> ReadJsonPropertyDependencies(
        string name,
        JsonElement element,
        bool isGroup)
    {
        JsonElement? declaredDependencies = null;
        foreach (var property in element.EnumerateObject())
        {
            if (!string.Equals(
                    property.Name,
                    "json_property_dependencies",
                    StringComparison.Ordinal))
            {
                continue;
            }
            if (declaredDependencies is not null)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has duplicate JSON property-dependency metadata.");
            }
            declaredDependencies = property.Value;
        }
        if (declaredDependencies is not { } dependenciesElement)
        {
            return Array.Empty<JsonPropertyDependency>();
        }
        if (!isGroup)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has JSON property dependencies without an object domain.");
        }
        RequireKind(
            dependenciesElement,
            JsonValueKind.Object,
            $"schema node '{name}' JSON property dependencies",
            "object");

        var dependencies = new List<JsonPropertyDependency>();
        var triggers = new HashSet<string>(StringComparer.Ordinal);
        var edges = 0;
        var nameBytes = 0;
        string? previousTrigger = null;
        foreach (var property in dependenciesElement.EnumerateObject())
        {
            if (!triggers.Add(property.Name))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a duplicate JSON property-dependency trigger.");
            }
            if (previousTrigger is not null &&
                CompareUtf8Ordinal(previousTrigger, property.Name) >= 0)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has noncanonical JSON property-dependency trigger order.");
            }
            if (dependencies.Count == MaximumJsonPropertyDependencyTriggers)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has more than {MaximumJsonPropertyDependencyTriggers} JSON property-dependency triggers.");
            }
            nameBytes = checked(nameBytes + Encoding.UTF8.GetByteCount(property.Name));
            RequireKind(
                property.Value,
                JsonValueKind.Array,
                $"schema node '{name}' dependency for '{property.Name}'",
                "array");

            var required = new List<string>();
            string? previous = null;
            foreach (var requiredElement in property.Value.EnumerateArray())
            {
                if (requiredElement.ValueKind != JsonValueKind.String)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' dependency for '{property.Name}' must contain strings.");
                }
                var requiredName = requiredElement.GetString() ?? string.Empty;
                if (string.Equals(requiredName, property.Name, StringComparison.Ordinal))
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' dependency for '{property.Name}' cannot require itself.");
                }
                if (previous is not null &&
                    CompareUtf8Ordinal(previous, requiredName) >= 0)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' dependency for '{property.Name}' has duplicate or noncanonical required properties.");
                }
                edges = checked(edges + 1);
                if (edges > MaximumJsonPropertyDependencyEdges)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' has more than {MaximumJsonPropertyDependencyEdges} JSON property-dependency edges.");
                }
                nameBytes = checked(
                    nameBytes + Encoding.UTF8.GetByteCount(requiredName));
                if (nameBytes > MaximumJsonPropertyDependencyNameBytes)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' JSON property-dependency names exceed {MaximumJsonPropertyDependencyNameBytes} UTF-8 bytes.");
                }
                required.Add(requiredName);
                previous = requiredName;
            }
            if (required.Count == 0)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' dependency for '{property.Name}' must require at least one property.");
            }
            dependencies.Add(new JsonPropertyDependency(property.Name, required));
            previousTrigger = property.Name;
        }
        if (dependencies.Count == 0)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has empty JSON property-dependency metadata.");
        }
        return dependencies;
    }

    private static int CompareUtf8Ordinal(string left, string right)
    {
        var leftBytes = Encoding.UTF8.GetBytes(left);
        var rightBytes = Encoding.UTF8.GetBytes(right);
        return leftBytes.AsSpan().SequenceCompareTo(rightBytes);
    }

    private static void ValidatePropertyDependencySchema(
        string name,
        IReadOnlyList<JsonPropertyDependency> dependencies,
        IReadOnlyList<JsonSchemaNode> children,
        JsonSchemaNode? dynamic,
        IReadOnlyList<string> required,
        IReadOnlyList<JsonAlternative> alternatives,
        JsonPropertyCountRange? propertyCountRange)
    {
        if (dependencies.Count == 0)
        {
            return;
        }

        var unconditional = DependencyClosure(dependencies, required);
        if (dynamic is null &&
            unconditional.Any(requiredName =>
                !children.Any(child => string.Equals(
                    child.Name,
                    requiredName,
                    StringComparison.Ordinal))))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' cannot satisfy its JSON property dependencies.");
        }

        if (propertyCountRange?.Maximum is { } maximum &&
            (ulong)unconditional.Count > maximum)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' cannot satisfy its JSON property dependencies within its property-count range.");
        }

        foreach (var alternative in alternatives)
        {
            var initial = required.Concat(alternative.Required).Distinct(
                StringComparer.Ordinal).ToArray();
            var closure = DependencyClosure(dependencies, initial);
            if (closure.Any(requiredName =>
                    !alternative.Members.Contains(requiredName, StringComparer.Ordinal)))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has an alternative that cannot satisfy its JSON property dependencies.");
            }
        }
    }

    private static HashSet<string> DependencyClosure(
        IReadOnlyList<JsonPropertyDependency> dependencies,
        IReadOnlyList<string> initial)
    {
        var closure = new HashSet<string>(initial, StringComparer.Ordinal);
        var changed = true;
        while (changed)
        {
            changed = false;
            foreach (var dependency in dependencies)
            {
                if (!closure.Contains(dependency.Trigger))
                {
                    continue;
                }
                foreach (var requiredName in dependency.Required)
                {
                    changed |= closure.Add(requiredName);
                }
            }
        }
        return closure;
    }

    private static void ValidatePropertyDependencies(
        JsonSchemaNode schema,
        IReadOnlyList<JsonProperty> properties)
    {
        if (schema.PropertyDependencies.Count == 0)
        {
            return;
        }
        var present = new HashSet<string>(
            properties.Select(property => property.Name),
            StringComparer.Ordinal);
        ValidatePropertyDependencies(schema, present);
    }

    private static void ValidateOutputPropertyDependencies(
        JsonSchemaNode schema,
        FerruleGroup group)
    {
        if (schema.PropertyDependencies.Count == 0)
        {
            return;
        }

        var present = new HashSet<string>(StringComparer.Ordinal);
        if (schema.Dynamic is { } dynamic)
        {
            foreach (var field in group.Fields)
            {
                var child = schema.Child(field.Name) ?? dynamic;
                if (!BoundaryAbsence(child, field.Value))
                {
                    present.Add(field.Name);
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
                    present.Add(child.Name);
                }
            }
        }

        ValidatePropertyDependencies(schema, present);
    }

    private static void ValidatePropertyDependencies(
        JsonSchemaNode schema,
        IReadOnlySet<string> present)
    {
        foreach (var dependency in schema.PropertyDependencies)
        {
            if (!present.Contains(dependency.Trigger))
            {
                continue;
            }
            foreach (var requiredName in dependency.Required)
            {
                if (!present.Contains(requiredName))
                {
                    throw Boundary(
                        $"JSON object '{schema.Name}' property '{dependency.Trigger}' requires property '{requiredName}'.");
                }
            }
        }
    }

    private sealed record JsonPropertyDependency(
        string Trigger,
        IReadOnlyList<string> Required);
}
