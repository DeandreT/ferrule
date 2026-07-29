using System.Text;
using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    private const int MaximumJsonContainsConstraints = 32;

    private static IReadOnlyList<JsonContainsConstraint> ReadJsonContains(
        string name,
        JsonElement element,
        bool repeating,
        NodeBudget schemaBudget,
        JsonPatternSchemaContext patternContext,
        int depth)
    {
        JsonElement? declared = null;
        foreach (var property in element.EnumerateObject())
        {
            if (!string.Equals(property.Name, "json_contains", StringComparison.Ordinal))
            {
                continue;
            }
            if (declared is not null)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has duplicate JSON contains metadata.");
            }
            declared = property.Value;
        }
        if (declared is not { } constraintsElement ||
            constraintsElement.ValueKind == JsonValueKind.Null)
        {
            return Array.Empty<JsonContainsConstraint>();
        }
        if (!repeating)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has JSON contains constraints without an array.");
        }
        RequireKind(
            constraintsElement,
            JsonValueKind.Array,
            $"schema node '{name}' JSON contains constraints",
            "array");

        var constraints = new List<JsonContainsConstraint>();
        var canonicalTerms = new HashSet<byte[]>(UniqueItemKeyComparer.Instance);
        var canonicalBudget = new UniqueItemBudget();
        foreach (var termElement in constraintsElement.EnumerateArray())
        {
            if (constraints.Count == MaximumJsonContainsConstraints)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has more than {MaximumJsonContainsConstraints} JSON contains constraints.");
            }
            RequireKind(
                termElement,
                JsonValueKind.Object,
                $"schema node '{name}' JSON contains constraint",
                "object");
            var fields = new HashSet<string>(StringComparer.Ordinal);
            foreach (var property in termElement.EnumerateObject())
            {
                if (property.Name is not ("predicate" or "range") ||
                    !fields.Add(property.Name))
                {
                    throw Boundary(
                        $"Embedded JSON schema node '{name}' JSON contains constraint has an unknown or duplicate field '{property.Name}'.");
                }
            }

            var predicate = ReadContainsPredicate(
                name,
                RequiredProperty(termElement, "predicate"),
                schemaBudget,
                patternContext,
                depth + 1);
            var range = ReadContainsRange(
                name,
                RequiredProperty(termElement, "range"));
            if (predicate.Schema is null && range.Contains(0))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a tautological JSON contains constraint.");
            }
            var canonical = CreateUniqueItemKey(termElement, canonicalBudget);
            if (!canonicalTerms.Add(canonical))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has a duplicate JSON contains constraint.");
            }
            constraints.Add(new JsonContainsConstraint(predicate, range));
        }
        if (constraints.Count == 0)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has empty JSON contains constraints.");
        }
        return constraints;
    }

    private static JsonContainsPredicate ReadContainsPredicate(
        string name,
        JsonElement element,
        NodeBudget schemaBudget,
        JsonPatternSchemaContext patternContext,
        int depth)
    {
        RequireKind(
            element,
            JsonValueKind.Object,
            $"schema node '{name}' JSON contains predicate",
            "object");
        var kind = RequiredString(element, "kind");
        var fields = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in element.EnumerateObject())
        {
            var known = string.Equals(kind, "never", StringComparison.Ordinal)
                ? property.Name is "kind"
                : property.Name is "kind" or "schema";
            if (!known || !fields.Add(property.Name))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON contains predicate has an unknown or duplicate field '{property.Name}'.");
            }
        }
        if (string.Equals(kind, "never", StringComparison.Ordinal))
        {
            return new JsonContainsPredicate(null);
        }
        if (!string.Equals(kind, "schema", StringComparison.Ordinal))
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has unknown JSON contains predicate kind '{kind}'.");
        }
        return new JsonContainsPredicate(
            ReadSchemaNode(
                RequiredProperty(element, "schema"),
                schemaBudget,
                patternContext,
                depth));
    }

    private static JsonItemCountRange ReadContainsRange(
        string name,
        JsonElement element)
    {
        RequireKind(
            element,
            JsonValueKind.Object,
            $"schema node '{name}' JSON contains range",
            "object");
        var fields = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in element.EnumerateObject())
        {
            if (property.Name is not ("minimum" or "maximum") ||
                !fields.Add(property.Name))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' JSON contains range has an unknown or duplicate field '{property.Name}'.");
            }
        }
        var minimum = OptionalUInt64(element, "minimum", false) ?? 0;
        var maximum = OptionalUInt64(element, "maximum", true);
        if (minimum == 0 && maximum is null ||
            maximum is { } upper && minimum > upper)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has invalid JSON contains range metadata.");
        }
        return new JsonItemCountRange(minimum, maximum);
    }

    private static void ValidateContainsInputItems(
        JsonSchemaNode schema,
        JsonElement array,
        NodeBudget budget,
        int depth)
    {
        if (schema.JsonContains.Count == 0)
        {
            return;
        }
        var matcher = budget.Matcher();
        foreach (var constraint in schema.JsonContains)
        {
            var matches = 0;
            if (constraint.Predicate.Schema is { } predicate)
            {
                foreach (var item in array.EnumerateArray())
                {
                    if (MatchesContainsPredicate(
                            predicate,
                            item,
                            matcher,
                            depth))
                    {
                        matches = checked(matches + 1);
                        if (constraint.Range.Maximum is { } maximum &&
                            (ulong)matches > maximum)
                        {
                            break;
                        }
                    }
                }
            }
            ValidateContainsCount(schema.Name, constraint.Range, matches);
        }
    }

    private static void WriteConstrainedOutputItems(
        Utf8JsonWriter writer,
        JsonSchemaNode schema,
        IReadOnlyList<FerruleInstance> items,
        NodeBudget budget,
        int depth)
    {
        if (items.Count > MaximumNodes)
        {
            budget.MarkFatalTraversalLimit();
            throw Boundary(
                $"Normalized JSON array '{schema.Name}' exceeds the {MaximumNodes}-item limit.");
        }

        var documents = new List<JsonDocument>(items.Count);
        try
        {
            var totalBytes = 0;
            foreach (var item in items)
            {
                var document = CreateNormalizedOutputItem(
                    schema,
                    item,
                    budget,
                    depth);
                documents.Add(document);
                totalBytes = checked(
                    totalBytes +
                    Encoding.UTF8.GetByteCount(document.RootElement.GetRawText()));
                if (totalBytes > MaximumDocumentBytes)
                {
                    throw Boundary(
                        $"Normalized JSON items in array '{schema.Name}' exceed the {MaximumDocumentBytes}-byte limit.");
                }
            }

            var matcher = budget.Matcher();
            foreach (var constraint in schema.JsonContains)
            {
                var matches = 0;
                if (constraint.Predicate.Schema is { } predicate)
                {
                    foreach (var document in documents)
                    {
                        if (MatchesContainsPredicate(
                                predicate,
                                document.RootElement,
                                matcher,
                                depth))
                        {
                            matches = checked(matches + 1);
                            if (constraint.Range.Maximum is { } maximum &&
                                (ulong)matches > maximum)
                            {
                                break;
                            }
                        }
                    }
                }
                ValidateContainsCount(schema.Name, constraint.Range, matches);
            }

            if (schema.JsonUniqueItems)
            {
                var keys = new HashSet<byte[]>(UniqueItemKeyComparer.Instance);
                var uniqueBudget = new UniqueItemBudget();
                for (var index = 0; index < documents.Count; index++)
                {
                    var key = CreateUniqueItemKey(
                        documents[index].RootElement,
                        uniqueBudget);
                    if (!keys.Add(key))
                    {
                        throw Boundary(
                            $"Normalized JSON array '{schema.Name}' violates uniqueItems at item {index + 1}.");
                    }
                }
            }

            foreach (var document in documents)
            {
                document.RootElement.WriteTo(writer);
            }
        }
        finally
        {
            foreach (var document in documents)
            {
                document.Dispose();
            }
        }
    }

    private static bool MatchesContainsPredicate(
        JsonSchemaNode predicate,
        JsonElement value,
        NodeBudget matcher,
        int depth)
    {
        try
        {
            _ = ReadNode(predicate, value, matcher, depth);
            return true;
        }
        catch (FerruleRuntimeException) when (!matcher.HasFatalLimit)
        {
            return false;
        }
    }

    private static void ValidateContainsCount(
        string name,
        JsonItemCountRange range,
        int matches)
    {
        if (!range.Contains(matches))
        {
            throw Boundary(
                $"JSON array '{name}' has {matches} items matching its contains predicate outside the declared match-count range.");
        }
    }

    private sealed record JsonContainsConstraint(
        JsonContainsPredicate Predicate,
        JsonItemCountRange Range);

    private sealed record JsonContainsPredicate(JsonSchemaNode? Schema);
}
