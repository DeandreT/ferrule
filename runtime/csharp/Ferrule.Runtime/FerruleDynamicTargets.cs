namespace Ferrule.Runtime;

/// <summary>Ordered computed-property construction shared by generated mappings.</summary>
public static class FerruleDynamicTargets
{
    public static string PropertyName(uint node, FerruleValue value)
    {
        if (value.Kind == FerruleValueKind.String)
        {
            return value.StringValue;
        }

        throw new FerruleRuntimeException(
            FerruleRuntimeError.DynamicPropertyName,
            $"Node {node}: dynamic target property name must be a string, got {value.Kind}.",
            node: node,
            foundKind: value.Kind);
    }

    public static void Insert(
        List<FerruleField> fields,
        IReadOnlyList<string> fixedFields,
        string name,
        FerruleInstance value)
    {
        ArgumentNullException.ThrowIfNull(fields);
        ArgumentNullException.ThrowIfNull(fixedFields);
        ArgumentNullException.ThrowIfNull(name);
        ArgumentNullException.ThrowIfNull(value);
        if (fixedFields.Contains(name, StringComparer.Ordinal) ||
            fields.Any(field => string.Equals(field.Name, name, StringComparison.Ordinal)))
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.DuplicateDynamicProperty,
                $"Dynamic target object contains duplicate or fixed-colliding property '{name}'.",
                detail: name);
        }

        fields.Add(new FerruleField(name, value));
    }

    public static FerruleGroup Merge(IEnumerable<FerruleInstance> fragments)
    {
        ArgumentNullException.ThrowIfNull(fragments);
        var fields = new List<FerruleField>();
        foreach (var fragment in fragments)
        {
            if (fragment is not FerruleGroup group)
            {
                throw new FerruleRuntimeException(
                    FerruleRuntimeError.InvalidDynamicPropertyFragment,
                    "A dynamic object merge can contain only object property fragments.");
            }

            foreach (var field in group.Fields)
            {
                Insert(fields, Array.Empty<string>(), field.Name, field.Value);
            }
        }

        return new FerruleGroup(fields);
    }
}
