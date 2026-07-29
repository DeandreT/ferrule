using System.Text.Json;

namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    private const int MaximumJsonPatternPropertyNameSources = 32;

    private static JsonPatternPropertyNames? ReadJsonPatternPropertyNames(
        string name,
        JsonElement element,
        bool isGroup,
        JsonPatternSchemaContext context)
    {
        JsonElement? declared = null;
        foreach (var property in element.EnumerateObject())
        {
            if (!string.Equals(
                    property.Name,
                    "json_pattern_property_names",
                    StringComparison.Ordinal))
            {
                continue;
            }
            if (declared is not null)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has duplicate patternProperties selector metadata.");
            }
            declared = property.Value;
        }
        if (declared is not { } selectorsElement ||
            selectorsElement.ValueKind == JsonValueKind.Null)
        {
            return null;
        }
        if (!isGroup)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has patternProperties selectors without an object domain.");
        }

        RequireKind(
            selectorsElement,
            JsonValueKind.Object,
            $"schema node '{name}' patternProperties selectors",
            "object");
        var fields = new HashSet<string>(StringComparer.Ordinal);
        foreach (var property in selectorsElement.EnumerateObject())
        {
            if (property.Name is not "sources" || !fields.Add(property.Name))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' patternProperties selectors have an unknown or duplicate field '{property.Name}'.");
            }
        }
        var sourcesElement = RequiredProperty(selectorsElement, "sources");
        RequireKind(
            sourcesElement,
            JsonValueKind.Array,
            $"schema node '{name}' patternProperties selector sources",
            "array");

        var sources = new List<string>();
        var seenSources = new HashSet<string>(StringComparer.Ordinal);
        var compiled = new List<FerruleJsonPattern>();
        foreach (var sourceElement in sourcesElement.EnumerateArray())
        {
            if (sourceElement.ValueKind != JsonValueKind.String)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' patternProperties selectors must contain strings.");
            }
            if (compiled.Count == MaximumJsonPatternPropertyNameSources)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has more than {MaximumJsonPatternPropertyNameSources} patternProperties selectors.");
            }
            var source = sourceElement.GetString() ?? string.Empty;
            if (!seenSources.Add(source))
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' has duplicate patternProperties selectors.");
            }
            sources.Add(source);
            compiled.Add(context.GetOrCompile(name, source));
        }
        if (compiled.Count == 0)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has empty patternProperties selectors.");
        }
        if (seenSources.Contains(string.Empty) && compiled.Count != 1)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has noncanonical patternProperties selectors.");
        }
        return new JsonPatternPropertyNames(sources, compiled);
    }

    private static void ValidatePatternPropertyNameSchema(
        string name,
        JsonPatternPropertyNames? selectors,
        IReadOnlyList<JsonSchemaNode> children,
        JsonSchemaNode? dynamic,
        IReadOnlyList<JsonPropertyDependency> propertyDependencies,
        IReadOnlyList<string> required,
        IReadOnlyList<JsonAlternative> alternatives,
        JsonPatternSchemaContext patternContext)
    {
        if (selectors is null)
        {
            return;
        }
        if (dynamic is null || alternatives.Count != 0)
        {
            throw Boundary(
                $"Embedded JSON schema node '{name}' has patternProperties selectors without one open-object value schema.");
        }

        bool IsDeclared(string property) =>
            children.Any(child =>
                string.Equals(child.Name, property, StringComparison.Ordinal));
        var expectations = new SortedDictionary<string, bool>(
            Comparer<string>.Create(CompareUtf8Ordinal));
        foreach (var child in children)
        {
            if (!SchemaNodesEqualIgnoringRootName(child, dynamic))
            {
                expectations[child.Name] = false;
            }
        }
        void RequireSelected(string property)
        {
            if (!IsDeclared(property))
            {
                expectations[property] = true;
            }
        }
        foreach (var property in required)
        {
            RequireSelected(property);
        }
        foreach (var dependency in propertyDependencies)
        {
            RequireSelected(dependency.Trigger);
            foreach (var property in dependency.Required)
            {
                RequireSelected(property);
            }
        }

        foreach (var expectation in expectations)
        {
            var matches = patternContext.FixedMatches(selectors, expectation.Key);
            if (matches == expectation.Value)
            {
                continue;
            }
            if (expectation.Value)
            {
                throw Boundary(
                    $"Embedded JSON schema node '{name}' requires undeclared property '{expectation.Key}' that matches no patternProperties selector.");
            }
            throw Boundary(
                $"Embedded JSON schema node '{name}' has declared property '{expectation.Key}' matching a patternProperties selector with a schema that differs from the dynamic schema.");
        }
    }

    private static bool SchemaNodesEqualIgnoringRootName(
        JsonSchemaNode left,
        JsonSchemaNode right) =>
        SchemaNodesEqual(left, right, compareName: false);

    private static bool SchemaNodesEqual(
        JsonSchemaNode left,
        JsonSchemaNode right,
        bool compareName = true) =>
        (!compareName ||
         string.Equals(left.Name, right.Name, StringComparison.Ordinal)) &&
        left.Repeating == right.Repeating &&
        left.Nullable == right.Nullable &&
        left.ContainerNullable == right.ContainerNullable &&
        left.JsonAny == right.JsonAny &&
        left.JsonFormats.SequenceEqual(right.JsonFormats, StringComparer.Ordinal) &&
        left.ScalarDomain == right.ScalarDomain &&
        string.Equals(left.FixedLexical, right.FixedLexical, StringComparison.Ordinal) &&
        AllowedValuesEqual(left.JsonAllowedValues, right.JsonAllowedValues) &&
        Equals(left.NumericRange, right.NumericRange) &&
        MultipleOfEqual(left.JsonMultipleOf, right.JsonMultipleOf) &&
        Equals(left.StringLengthRange, right.StringLengthRange) &&
        PatternConstraintsEqual(left.JsonPatterns, right.JsonPatterns) &&
        Equals(left.ItemCountRange, right.ItemCountRange) &&
        ContainsEqual(left.JsonContains, right.JsonContains) &&
        Equals(left.PropertyCountRange, right.PropertyCountRange) &&
        PropertyDependenciesEqual(
            left.PropertyDependencies,
            right.PropertyDependencies) &&
        DependentSchemasEqual(left.DependentSchemas, right.DependentSchemas) &&
        PropertyNamesEqual(left.PropertyNames, right.PropertyNames) &&
        PatternPropertyNamesEqual(
            left.PatternPropertyNames,
            right.PatternPropertyNames) &&
        left.JsonUniqueItems == right.JsonUniqueItems &&
        SchemaNodeListsEqual(left.Children, right.Children) &&
        OptionalSchemaNodesEqual(left.Dynamic, right.Dynamic) &&
        left.Required.SequenceEqual(right.Required, StringComparer.Ordinal) &&
        AlternativesEqual(left.Alternatives, right.Alternatives) &&
        left.InclusiveAlternatives == right.InclusiveAlternatives;

    private static bool AllowedValuesEqual(
        JsonAllowedValues? left,
        JsonAllowedValues? right) =>
        left is null && right is null ||
        left is not null &&
        right is not null &&
        left.Values.SequenceEqual(right.Values);

    private static bool MultipleOfEqual(
        JsonMultipleOfConstraints? left,
        JsonMultipleOfConstraints? right) =>
        left is null && right is null ||
        left is not null &&
        right is not null &&
        left.AnyOf.Count == right.AnyOf.Count &&
        left.AnyOf.Zip(right.AnyOf).All(pair =>
            pair.First.Terms.SequenceEqual(pair.Second.Terms));

    private static bool PatternConstraintsEqual(
        JsonPatternConstraints? left,
        JsonPatternConstraints? right) =>
        left is null && right is null ||
        left is not null &&
        right is not null &&
        left.AnyOf.Count == right.AnyOf.Count &&
        left.AnyOf.Zip(right.AnyOf).All(pair =>
            pair.First.Sources.SequenceEqual(
                pair.Second.Sources,
                StringComparer.Ordinal));

    private static bool ContainsEqual(
        IReadOnlyList<JsonContainsConstraint> left,
        IReadOnlyList<JsonContainsConstraint> right) =>
        left.Count == right.Count &&
        left.Zip(right).All(pair =>
            Equals(pair.First.Range, pair.Second.Range) &&
            OptionalSchemaNodesEqual(
                pair.First.Predicate.Schema,
                pair.Second.Predicate.Schema));

    private static bool PropertyDependenciesEqual(
        IReadOnlyList<JsonPropertyDependency> left,
        IReadOnlyList<JsonPropertyDependency> right) =>
        left.Count == right.Count &&
        left.Zip(right).All(pair =>
            string.Equals(
                pair.First.Trigger,
                pair.Second.Trigger,
                StringComparison.Ordinal) &&
            pair.First.Required.SequenceEqual(
                pair.Second.Required,
                StringComparer.Ordinal));

    private static bool DependentSchemasEqual(
        IReadOnlyList<JsonDependentSchema> left,
        IReadOnlyList<JsonDependentSchema> right) =>
        left.Count == right.Count &&
        left.Zip(right).All(pair =>
            string.Equals(
                pair.First.Trigger,
                pair.Second.Trigger,
                StringComparison.Ordinal) &&
            OptionalSchemaNodesEqual(
                pair.First.Predicate.Schema,
                pair.Second.Predicate.Schema));

    private static bool PropertyNamesEqual(
        JsonPropertyNameConstraints? left,
        JsonPropertyNameConstraints? right) =>
        left is null && right is null ||
        left is not null &&
        right is not null &&
        left.RejectAll == right.RejectAll &&
        OptionalStringsEqual(left.Allowed, right.Allowed) &&
        OptionalStringsEqual(left.Excluded, right.Excluded) &&
        Equals(left.Length, right.Length) &&
        PatternConstraintsEqual(left.Patterns, right.Patterns) &&
        left.Formats.SequenceEqual(right.Formats, StringComparer.Ordinal);

    private static bool PatternPropertyNamesEqual(
        JsonPatternPropertyNames? left,
        JsonPatternPropertyNames? right) =>
        left is null && right is null ||
        left is not null &&
        right is not null &&
        left.Sources.SequenceEqual(right.Sources, StringComparer.Ordinal);

    private static bool SchemaNodeListsEqual(
        IReadOnlyList<JsonSchemaNode> left,
        IReadOnlyList<JsonSchemaNode> right) =>
        left.Count == right.Count &&
        left.Zip(right).All(pair =>
            SchemaNodesEqual(pair.First, pair.Second));

    private static bool OptionalSchemaNodesEqual(
        JsonSchemaNode? left,
        JsonSchemaNode? right) =>
        left is null && right is null ||
        left is not null &&
        right is not null &&
        SchemaNodesEqual(left, right);

    private static bool OptionalStringsEqual(
        IReadOnlyList<string>? left,
        IReadOnlyList<string>? right) =>
        left is null && right is null ||
        left is not null &&
        right is not null &&
        left.SequenceEqual(right, StringComparer.Ordinal);

    private static bool AlternativesEqual(
        IReadOnlyList<JsonAlternative> left,
        IReadOnlyList<JsonAlternative> right) =>
        left.Count == right.Count &&
        left.Zip(right).All(pair =>
            string.Equals(
                pair.First.Name,
                pair.Second.Name,
                StringComparison.Ordinal) &&
            pair.First.Members.SequenceEqual(
                pair.Second.Members,
                StringComparer.Ordinal) &&
            pair.First.Required.SequenceEqual(
                pair.Second.Required,
                StringComparer.Ordinal) &&
            ConstraintsEqual(
                pair.First.Constraints,
                pair.Second.Constraints));

    private static bool ConstraintsEqual(
        IReadOnlyList<JsonConstraint> left,
        IReadOnlyList<JsonConstraint> right) =>
        left.Count == right.Count &&
        left.Zip(right).All(pair =>
            string.Equals(
                pair.First.Member,
                pair.Second.Member,
                StringComparison.Ordinal) &&
            string.Equals(
                pair.First.Type,
                pair.Second.Type,
                StringComparison.Ordinal) &&
            JsonElement.DeepEquals(
                pair.First.Expected,
                pair.Second.Expected));

    private static void ValidatePatternProperties(
        JsonSchemaNode schema,
        IReadOnlyList<JsonProperty> properties,
        NodeBudget budget)
    {
        if (schema.PatternPropertyNames is not { } selectors)
        {
            return;
        }
        foreach (var property in properties)
        {
            if (schema.Child(property.Name) is null)
            {
                ValidatePatternPropertyName(
                    schema.Name,
                    property.Name,
                    selectors,
                    budget);
            }
        }
    }

    private static void ValidateOutputPatternProperties(
        JsonSchemaNode schema,
        FerruleGroup group,
        NodeBudget budget)
    {
        if (schema.PatternPropertyNames is not { } selectors ||
            schema.Dynamic is not { } dynamic)
        {
            return;
        }
        foreach (var field in group.Fields)
        {
            if (schema.Child(field.Name) is null &&
                !BoundaryAbsence(dynamic, field.Value))
            {
                ValidatePatternPropertyName(
                    schema.Name,
                    field.Name,
                    selectors,
                    budget);
            }
        }
    }

    private static void ValidatePatternPropertyName(
        string objectName,
        string propertyName,
        JsonPatternPropertyNames selectors,
        NodeBudget budget)
    {
        if (!selectors.Matches(propertyName, budget))
        {
            throw Boundary(
                $"JSON object '{objectName}' has dynamic property '{propertyName}' that matches no patternProperties selector.");
        }
    }

    private sealed record JsonPatternPropertyNames(
        IReadOnlyList<string> Sources,
        IReadOnlyList<FerruleJsonPattern> Selectors)
    {
        public bool IsMatch(string value, ref ulong remainingWork)
        {
            foreach (var selector in Selectors)
            {
                if (selector.IsMatch(value, ref remainingWork))
                {
                    return true;
                }
            }
            return false;
        }

        public bool Matches(string value, NodeBudget budget)
        {
            foreach (var selector in Selectors)
            {
                if (budget.Matches(selector, value))
                {
                    return true;
                }
            }
            return false;
        }
    }
}
