namespace Ferrule.Runtime;

public static partial class FerruleJson
{
    private sealed class JsonSchemaNode
    {
        public JsonSchemaNode(
            string name,
            bool repeating,
            bool nullable,
            bool containerNullable,
            bool? jsonObjectItemNullable,
            bool jsonArrayWrapper,
            bool jsonAny,
            IReadOnlyList<string> jsonFormats,
            JsonScalarDomain scalarDomain,
            string? fixedLexical,
            FerruleValue? fixedValue,
            JsonAllowedValues? jsonAllowedValues,
            JsonNumericRange? numericRange,
            JsonMultipleOfConstraints? jsonMultipleOf,
            JsonStringLengthRange? stringLengthRange,
            JsonPatternConstraints? jsonPatterns,
            JsonItemCountRange? itemCountRange,
            IReadOnlyList<JsonContainsConstraint> jsonContains,
            JsonPropertyCountRange? propertyCountRange,
            IReadOnlyList<JsonPropertyDependency> propertyDependencies,
            IReadOnlyList<JsonDependentSchema> dependentSchemas,
            JsonPropertyNameConstraints? propertyNames,
            JsonPatternPropertyNames? patternPropertyNames,
            bool jsonUniqueItems,
            IReadOnlyList<JsonSchemaNode> children,
            JsonSchemaNode? dynamic,
            IReadOnlyList<string> required,
            IReadOnlyList<JsonAlternative> alternatives,
            bool inclusiveAlternatives,
            bool xmlTypeAlternatives,
            string? recursiveReference)
        {
            Name = name;
            Repeating = repeating;
            Nullable = nullable;
            ContainerNullable = containerNullable;
            JsonObjectItemNullable = jsonObjectItemNullable;
            JsonArrayWrapper = jsonArrayWrapper;
            JsonAny = jsonAny;
            JsonFormats = jsonFormats;
            ScalarDomain = scalarDomain;
            FixedLexical = fixedLexical;
            Fixed = fixedValue;
            JsonAllowedValues = jsonAllowedValues;
            NumericRange = numericRange;
            JsonMultipleOf = jsonMultipleOf;
            StringLengthRange = stringLengthRange;
            JsonPatterns = jsonPatterns;
            ItemCountRange = itemCountRange;
            JsonContains = jsonContains;
            PropertyCountRange = propertyCountRange;
            PropertyDependencies = propertyDependencies;
            DependentSchemas = dependentSchemas;
            PropertyNames = propertyNames;
            PatternPropertyNames = patternPropertyNames;
            JsonUniqueItems = jsonUniqueItems;
            Children = children;
            DuplicateChildren = children.Select(child => child.Name).Distinct(StringComparer.Ordinal).Count() != children.Count;
            Dynamic = dynamic;
            Required = required;
            Alternatives = alternatives;
            InclusiveAlternatives = inclusiveAlternatives;
            XmlTypeAlternatives = xmlTypeAlternatives;
            RecursiveReference = recursiveReference;
        }

        public string Name { get; private set; }

        public bool Repeating { get; private set; }

        public bool Nullable { get; }

        public bool ContainerNullable { get; private set; }
        public bool? JsonObjectItemNullable { get; private set; }
        // Scalar items admit null only through their own Nullable value domain.
        public bool SingleContainerNullable => !IsScalar && (JsonObjectItemNullable ?? ContainerNullable);
        public bool JsonArrayWrapper { get; }

        public bool JsonAny { get; }

        public IReadOnlyList<string> JsonFormats { get; }

        public JsonScalarDomain ScalarDomain { get; }

        public string? FixedLexical { get; }

        public FerruleValue? Fixed { get; }

        public JsonAllowedValues? JsonAllowedValues { get; }

        public JsonNumericRange? NumericRange { get; }

        public JsonMultipleOfConstraints? JsonMultipleOf { get; }

        public JsonStringLengthRange? StringLengthRange { get; }

        public JsonPatternConstraints? JsonPatterns { get; }

        public JsonItemCountRange? ItemCountRange { get; private set; }

        public IReadOnlyList<JsonContainsConstraint> JsonContains { get; private set; }

        public JsonPropertyCountRange? PropertyCountRange { get; }

        public IReadOnlyList<JsonPropertyDependency> PropertyDependencies { get; }

        public IReadOnlyList<JsonDependentSchema> DependentSchemas { get; }

        public JsonPropertyNameConstraints? PropertyNames { get; }

        public JsonPatternPropertyNames? PatternPropertyNames { get; }

        public bool JsonUniqueItems { get; private set; }

        public string? RecursiveReference { get; }

        public JsonSchemaNode? ResolvedRecursiveSchema { get; private set; }

        public bool IsScalar => ScalarDomain != JsonScalarDomain.None;

        public bool IsScalarUnion => IsScalar && !IsSingleScalar(ScalarDomain);

        public IReadOnlyList<JsonSchemaNode> Children { get; }

        public bool DuplicateChildren { get; }

        public JsonSchemaNode? Dynamic { get; }

        public IReadOnlyList<string> Required { get; }

        public IReadOnlyList<JsonAlternative> Alternatives { get; }

        public bool InclusiveAlternatives { get; }

        public bool XmlTypeAlternatives { get; }

        public JsonSchemaNode? Child(string name) =>
            Children.FirstOrDefault(child =>
                string.Equals(child.Name, name, StringComparison.Ordinal));

        public void BindRecursiveAnchor(JsonSchemaNode anchor)
        {
            var resolved = (JsonSchemaNode)anchor.MemberwiseClone();
            resolved.Name = Name;
            resolved.Repeating = Repeating;
            resolved.ContainerNullable = ContainerNullable;
            resolved.JsonObjectItemNullable = JsonObjectItemNullable;
            resolved.ItemCountRange = ItemCountRange;
            resolved.JsonContains = JsonContains;
            resolved.JsonUniqueItems = JsonUniqueItems;
            ResolvedRecursiveSchema = resolved;
        }
    }
}
