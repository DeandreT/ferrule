using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    private const int MaximumJsonPropertyNames = 4096;
    private const int MaximumJsonPropertyNameBytes = 256 * 1024;
    private const int MaximumJsonPropertyNameTotalBytes = 1024 * 1024;

    private static JsonPropertyNameConstraints? ReadJsonPropertyNames(
        string name,
        JsonElement element,
        bool isGroup,
        JsonPatternSchemaContext patternContext)
    {
        JsonElement? declared = null;
        foreach (var property in element.EnumerateObject())
        {
            if (!string.Equals(
                    property.Name,
                    "json_property_names",
                    StringComparison.Ordinal))
            {
                continue;
            }
            if (declared is not null)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has duplicate JSON property-name metadata.");
            }
            declared = property.Value;
        }
        if (declared is not { } constraintsElement ||
            constraintsElement.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        if (!isGroup)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has JSON property-name constraints without an object domain.");
        }

        RequireKind(
            constraintsElement,
            JsonValueKind.Object,
            $"schema node '{name}' JSON property-name constraints",
            "object");
        var kind = RequiredString(constraintsElement, "kind");
        var fields = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in constraintsElement.EnumerateObject())
        {
            var known = string.Equals(kind, "never", StringComparison.Ordinal)
                ? property.Name is "kind"
                : property.Name is "kind" or "allowed" or "length" or "patterns" or "formats";
            if (!known || !fields.Add(property.Name))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON property-name constraints have an unknown or duplicate field '{property.Name}'.");
            }
        }

        if (string.Equals(kind, "never", StringComparison.Ordinal))
        {
            return JsonPropertyNameConstraints.Never;
        }
        if (!string.Equals(kind, "schema", StringComparison.Ordinal))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has unknown JSON property-name constraint kind '{kind}'.");
        }

        var allowed = constraintsElement.TryGetProperty("allowed", out var allowedElement) &&
                      allowedElement.ValueKind != JsonValueKind.Null
            ? ReadPropertyNameSet(name, allowedElement)
            : null;
        var length = constraintsElement.TryGetProperty("length", out var lengthElement) &&
                     lengthElement.ValueKind != JsonValueKind.Null
            ? ReadPropertyNameLength(name, lengthElement)
            : null;
        var patterns = constraintsElement.TryGetProperty("patterns", out var patternsElement) &&
                       patternsElement.ValueKind != JsonValueKind.Null
            ? ReadPropertyNamePatterns(name, patternsElement, patternContext)
            : null;
        var formats = constraintsElement.TryGetProperty("formats", out var formatsElement)
            ? ReadPropertyNameFormats(name, formatsElement)
            : Array.Empty<string>();
        if (allowed is null &&
            length is null &&
            patterns is null &&
            formats.Count == 0)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has tautological JSON property-name constraints that must be omitted.");
        }

        if (allowed is not null &&
            allowed.Any(candidate =>
                length is not null && !length.Contains(name, candidate) ||
                patterns is not null && !patternContext.FixedMatches(patterns, candidate)))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has noncanonical JSON property-name allowed values.");
        }
        return new JsonPropertyNameConstraints(
            false,
            allowed,
            length,
            patterns,
            formats);
    }

    private static IReadOnlyList<string> ReadPropertyNameSet(
        string name,
        JsonElement element)
    {
        RequireKind(
            element,
            JsonValueKind.Array,
            $"schema node '{name}' JSON property-name allowed values",
            "array");
        var names = new List<string>();
        var totalBytes = 0;
        string? previous = null;
        foreach (var nameElement in element.EnumerateArray())
        {
            if (nameElement.ValueKind != JsonValueKind.String)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON property-name allowed values must contain strings.");
            }
            if (names.Count == MaximumJsonPropertyNames)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has more than {MaximumJsonPropertyNames} allowed property names.");
            }

            var candidate = nameElement.GetString() ?? string.Empty;
            if (previous is not null &&
                CompareUtf8Ordinal(previous, candidate) >= 0)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON property-name allowed values are not strictly sorted and unique.");
            }
            var bytes = StrictUtf8.GetByteCount(candidate);
            if (bytes > MaximumJsonPropertyNameBytes)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a property name larger than {MaximumJsonPropertyNameBytes} UTF-8 bytes.");
            }
            totalBytes = checked(totalBytes + bytes);
            if (totalBytes > MaximumJsonPropertyNameTotalBytes)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' property names exceed {MaximumJsonPropertyNameTotalBytes} UTF-8 bytes.");
            }
            names.Add(candidate);
            previous = candidate;
        }
        if (names.Count == 0)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has an empty JSON property-name allowed set.");
        }
        return names;
    }

    private static JsonStringLengthRange ReadPropertyNameLength(
        string name,
        JsonElement element)
    {
        RequireKind(
            element,
            JsonValueKind.Object,
            $"schema node '{name}' JSON property-name length",
            "object");
        var fields = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in element.EnumerateObject())
        {
            if (property.Name is not ("minimum" or "maximum") ||
                !fields.Add(property.Name))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON property-name length has an unknown or duplicate field '{property.Name}'.");
            }
        }
        var minimum = OptionalUInt64(element, "minimum", false) ?? 0;
        var maximum = OptionalUInt64(element, "maximum", true);
        if (minimum == 0 && maximum is null ||
            maximum is { } upper && minimum > upper)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has invalid JSON property-name length metadata.");
        }
        return new JsonStringLengthRange(minimum, maximum);
    }

    private static JsonPatternConstraints ReadPropertyNamePatterns(
        string name,
        JsonElement element,
        JsonPatternSchemaContext context)
    {
        RequireKind(
            element,
            JsonValueKind.Object,
            $"schema node '{name}' JSON property-name patterns",
            "object");
        var fields = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in element.EnumerateObject())
        {
            if (property.Name is not "any_of" || !fields.Add(property.Name))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON property-name patterns have an unknown or duplicate field '{property.Name}'.");
            }
        }

        var alternativesElement = RequiredProperty(element, "any_of");
        RequireKind(
            alternativesElement,
            JsonValueKind.Array,
            $"schema node '{name}' JSON property-name pattern alternatives",
            "array");
        var alternatives = new List<JsonPatternAlternative>();
        var canonicalSources = new List<string[]>();
        var distinctSources = new HashSet<string>(StringComparer.Ordinal);
        var sourceBytes = 0;
        var instructions = 0;
        var terms = 0;
        foreach (var alternativeElement in alternativesElement.EnumerateArray())
        {
            if (alternatives.Count == MaximumJsonPatternAlternatives)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has more than {MaximumJsonPatternAlternatives} JSON property-name pattern alternatives.");
            }
            RequireKind(
                alternativeElement,
                JsonValueKind.Array,
                $"schema node '{name}' JSON property-name pattern alternative",
                "array");

            var sources = new List<string>();
            var compiledTerms = new List<FerruleJsonPattern>();
            foreach (var termElement in alternativeElement.EnumerateArray())
            {
                terms = checked(terms + 1);
                if (terms > MaximumJsonPatternTerms)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' has more than {MaximumJsonPatternTerms} JSON property-name pattern terms.");
                }
                if (termElement.ValueKind != JsonValueKind.String)
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' JSON property-name pattern terms must be strings.");
                }

                var source = termElement.GetString() ?? string.Empty;
                if (sources.Contains(source, StringComparer.Ordinal))
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' has a duplicate JSON property-name pattern term.");
                }
                var compiled = context.GetOrCompile(name, source);
                if (distinctSources.Add(source))
                {
                    sourceBytes = checked(sourceBytes + StrictUtf8.GetByteCount(source));
                    instructions = checked(instructions + compiled.InstructionCount);
                    if (distinctSources.Count > MaximumDistinctJsonPatterns ||
                        sourceBytes > MaximumJsonPatternSourceBytes ||
                        instructions > MaximumJsonPatternInstructions)
                    {
                        throw Boundary(
                            $"Embedded JSON schema node '{name}' exceeds a local JSON property-name pattern metadata limit.");
                    }
                }
                sources.Add(source);
                compiledTerms.Add(compiled);
            }
            if (sources.Count == 0)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has an empty JSON property-name pattern alternative.");
            }
            if (sources.Count != 1 &&
                sources.Contains(string.Empty, StringComparer.Ordinal))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a noncanonical tautological JSON property-name pattern alternative.");
            }
            if (canonicalSources.Any(previous =>
                    previous.SequenceEqual(sources, StringComparer.Ordinal)))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a duplicate JSON property-name pattern alternative.");
            }
            canonicalSources.Add(sources.ToArray());
            alternatives.Add(new JsonPatternAlternative(sources.ToArray(), compiledTerms));
        }
        if (alternatives.Count == 0)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has empty JSON property-name pattern constraints.");
        }
        if (canonicalSources.Count != 1 &&
            canonicalSources.Any(sources =>
                sources.Length == 1 && sources[0].Length == 0))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has noncanonical tautological JSON property-name pattern constraints.");
        }
        if (canonicalSources.Count == 1 &&
            canonicalSources[0].Length == 1 &&
            canonicalSources[0][0].Length == 0)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has tautological JSON property-name pattern constraints that must be omitted.");
        }
        return new JsonPatternConstraints(alternatives);
    }

    private static IReadOnlyList<string> ReadPropertyNameFormats(
        string name,
        JsonElement element)
    {
        RequireKind(
            element,
            JsonValueKind.Array,
            $"schema node '{name}' JSON property-name formats",
            "array");
        var formats = new List<string>();
        var seen = new HashSet<string>(StringComparer.Ordinal);
        var totalBytes = 0;
        foreach (var formatElement in element.EnumerateArray())
        {
            if (formatElement.ValueKind != JsonValueKind.String)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON property-name formats must contain strings.");
            }
            if (formats.Count == MaximumJsonFormats)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has more than {MaximumJsonFormats} JSON property-name format annotations.");
            }
            var format = formatElement.GetString() ?? string.Empty;
            var bytes = Encoding.UTF8.GetByteCount(format);
            if (bytes > MaximumJsonFormatBytes)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON property-name format annotation is larger than {MaximumJsonFormatBytes} UTF-8 bytes.");
            }
            totalBytes = checked(totalBytes + bytes);
            if (totalBytes > MaximumJsonFormatTotalBytes)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON property-name format annotations exceed {MaximumJsonFormatTotalBytes} UTF-8 bytes.");
            }
            if (!seen.Add(format))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a duplicate JSON property-name format annotation '{format}'.");
            }
            formats.Add(format);
        }
        return formats;
    }

    private static void ValidatePropertyNameSchema(
        string name,
        JsonPropertyNameConstraints? constraints,
        IReadOnlyList<JsonSchemaNode> children,
        JsonSchemaNode? dynamic,
        IReadOnlyList<JsonPropertyDependency> propertyDependencies,
        IReadOnlyList<string> required,
        IReadOnlyList<JsonAlternative> alternatives,
        JsonPropertyCountRange? propertyCountRange)
    {
        if (constraints is null)
        {
            return;
        }
        var effectiveRequired = DependencyClosure(propertyDependencies, required);
        if (effectiveRequired.Any(
                requiredName => !constraints.AcceptsForSchema(requiredName)))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' requires a property forbidden by its JSON property-name constraints.");
        }

        var capacity = dynamic is null
            ? children.Count(
                child => constraints.AcceptsForSchema(child.Name))
            : constraints.Allowed?.Count;
        if (propertyCountRange is { } range &&
            capacity is { } maximum &&
            range.Minimum > (ulong)maximum)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' cannot satisfy its property-count range within its JSON property-name constraints.");
        }
        foreach (var alternative in alternatives)
        {
            var alternativeRequired = DependencyClosure(
                propertyDependencies,
                effectiveRequired.Concat(alternative.Required).ToArray());
            if (alternativeRequired.Any(
                    requiredName =>
                        !constraints.AcceptsForSchema(requiredName)))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has an alternative requiring a property forbidden by its JSON property-name constraints.");
            }
            var alternativeCapacity = alternative.Members.Count(
                member => constraints.AcceptsForSchema(member));
            if (propertyCountRange is { } alternativeRange &&
                alternativeRange.Minimum > (ulong)alternativeCapacity)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has an alternative that cannot satisfy its property-count range within its JSON property-name constraints.");
            }
        }
    }

    private static void ValidatePropertyNames(
        JsonSchemaNode schema,
        IReadOnlyList<JsonProperty> properties,
        NodeBudget budget)
    {
        if (schema.PropertyNames is not { } constraints)
        {
            return;
        }
        foreach (var property in properties)
        {
            ValidatePropertyName(schema.Name, property.Name, constraints, budget);
        }
    }

    private static void ValidateOutputPropertyNames(
        JsonSchemaNode schema,
        FerruleGroup group,
        NodeBudget budget)
    {
        if (schema.PropertyNames is not { } constraints)
        {
            return;
        }
        if (schema.Dynamic is { } dynamic)
        {
            foreach (var field in group.Fields)
            {
                var child = schema.Child(field.Name) ?? dynamic;
                if (!BoundaryAbsence(child, field.Value))
                {
                    ValidatePropertyName(
                        schema.Name,
                        field.Name,
                        constraints,
                        budget);
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
                    ValidatePropertyName(
                        schema.Name,
                        child.Name,
                        constraints,
                        budget);
                }
            }
        }
    }

    private static void ValidatePropertyName(
        string objectName,
        string propertyName,
        JsonPropertyNameConstraints constraints,
        NodeBudget budget)
    {
        if (!constraints.Accepts(propertyName, budget))
        {
            throw Boundary(
                $"JSON object '{objectName}' has invalid property name '{propertyName}'.");
        }
    }

    private sealed record JsonPropertyNameConstraints(
        bool RejectAll,
        IReadOnlyList<string>? Allowed,
        JsonStringLengthRange? Length,
        JsonPatternConstraints? Patterns,
        IReadOnlyList<string> Formats)
    {
        public static JsonPropertyNameConstraints Never { get; } =
            new(true, null, null, null, Array.Empty<string>());

        public bool AcceptsWithoutBudget(string name) =>
            !RejectAll &&
            (Allowed is null || Allowed.Contains(name, StringComparer.Ordinal)) &&
            (Length is null || Length.Contains(string.Empty, name));

        public bool AcceptsForSchema(string name)
        {
            var remainingWork = FerruleJsonPattern.MaximumBoundaryWork;
            return AcceptsWithoutBudget(name) &&
                (Patterns is null || Patterns.IsMatch(name, ref remainingWork));
        }

        public bool Accepts(string name, NodeBudget budget) =>
            AcceptsWithoutBudget(name) &&
            (Patterns is null || budget.Matches(Patterns, name));
    }
}
