namespace Ferrule.Runtime;

/// <summary>A prepared filename and early values, with a fresh deferred body
/// evaluation on each materialization. This does not open or publish a file.</summary>
public sealed class FerruleDeferredXmlDocument(string descriptor, FerruleDocument early, Func<FerruleInstance> late)
{
    public string Path => early.Path;
    public FerruleDocument Materialize()
    {
        ArgumentNullException.ThrowIfNull(early); ArgumentNullException.ThrowIfNull(late);
        var unwrapped = FerruleEmbeddedSchema.Unwrap(descriptor, 8 * 1024 * 1024);
        using var schema = System.Text.Json.JsonDocument.Parse(unwrapped.Payload, new System.Text.Json.JsonDocumentOptions { MaxDepth = 128 });
        int nodes = 0;
        var value = Merge(schema.RootElement, early.Value, late(), 0, ref nodes) ?? new FerruleGroup([]);
        return new FerruleDocument(Path, value);
    }
    private static FerruleInstance? Present(FerruleInstance? value) => value is FerruleScalar { Value.Kind: FerruleValueKind.Null } ? null : value;
    private static FerruleInstance? Merge(System.Text.Json.JsonElement schema, FerruleInstance? first, FerruleInstance? second, int depth, ref int nodes)
    {
        if (depth > 64 || ++nodes > 1000000) throw new InvalidOperationException("Deferred XML merge limit exceeded.");
        first = Present(first); second = Present(second);
        if (first is null && second is null) return null;
        static IReadOnlyList<FerruleInstance>? Sequence(FerruleInstance? value) => value switch { FerruleRepeated r => r.Items, FerruleMappedSequence r => r.Items, _ => null };
        var a = Sequence(first); var b = Sequence(second);
        if (a is not null || b is not null)
        {
            if ((first is not null && a is null) || (second is not null && b is null) || (a is not null && b is not null && (a.Count != b.Count || first!.GetType() != second!.GetType()))) throw new InvalidOperationException("XML sequence ownership differs between phases.");
            var items = new List<FerruleInstance>(); int count = (a ?? b)!.Count;
            if (count > 1000000 - nodes) throw new InvalidOperationException("Deferred XML merge limit exceeded.");
            for (int i = 0; i < count; i++) items.Add(Merge(schema, a?[i], b?[i], depth + 1, ref nodes) ?? new FerruleScalar(FerruleValue.Null));
            return (first ?? second) is FerruleMappedSequence ? new FerruleMappedSequence(items) : new FerruleRepeated(items);
        }
        var kind = schema.GetProperty("kind");
        if (kind.GetProperty("kind").GetString() == "scalar")
        {
            if (first is not null && second is not null) throw new InvalidOperationException("XML scalar belongs to both phases.");
            if ((first ?? second) is not FerruleScalar) throw new InvalidOperationException("Non-scalar XML leaf.");
            return first ?? second;
        }
        if (kind.GetProperty("kind").GetString() != "group") throw new InvalidOperationException("Unsupported deferred XML schema.");
        var children = kind.GetProperty("children").EnumerateArray().ToArray();
        foreach (var value in new[] { first, second }.Where(v => v is not null))
        {
            if (value is not FerruleGroup group || group.Fields.Select(f => f.Name).Distinct(StringComparer.Ordinal).Count() != group.Fields.Count || group.Fields.Any(f => !children.Any(c => c.GetProperty("name").GetString() == f.Name))) throw new InvalidOperationException("Unknown, duplicate or non-group XML field.");
        }
        static FerruleInstance? Field(FerruleInstance? value, string name) => (value as FerruleGroup)?.Fields.FirstOrDefault(f => f.Name == name)?.Value;
        var fields = new List<FerruleField>();
        foreach (var child in children)
        {
            string name = child.GetProperty("name").GetString()!;
            var value = Merge(child, Field(first, name), Field(second, name), depth + 1, ref nodes);
            if (value is not null) fields.Add(new(name, value));
        }
        return new FerruleGroup(fields);
    }
}

/// <summary>Constructs portable, mapping-produced document-set members.</summary>
public static class FerruleDynamicDocuments
{
    public static FerruleDocument Create(
        uint node,
        FerruleValue path,
        FerruleInstance value)
    {
        ArgumentNullException.ThrowIfNull(value);
        if (path.Kind != FerruleValueKind.String)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.DynamicTargetPath,
                $"Node {node}: dynamic target path expected a string, got {path.Kind}.",
                node: node,
                foundKind: path.Kind);
        }

        if (string.IsNullOrWhiteSpace(path.StringValue))
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.EmptyDynamicTargetPath,
                $"Node {node}: dynamic target path must not be empty.",
                node: node);
        }

        try
        {
            return new FerruleDocument(path.StringValue, value);
        }
        catch (FerruleRuntimeException error)
            when (error.Error is FerruleRuntimeError.InvalidDocumentPath
                or FerruleRuntimeError.NestedDocumentSet)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.EmptyDynamicTargetPath,
                $"Node {node}: dynamic target path must not be empty.",
                error,
                node: node);
        }
    }
}
