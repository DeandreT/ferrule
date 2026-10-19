namespace Ferrule.Runtime;

/// <summary>Bounded recursive construction from flat string-keyed adjacency rows.</summary>
public static class FerruleAdjacencyTree
{
    public static FerruleInstance Build(
        ScopeContext context,
        IReadOnlyList<string> collectionPath,
        IReadOnlyList<string> keyPath,
        IReadOnlyList<string> parentPath,
        string targetKey,
        string targetChildren,
        Func<ScopeContext, FerruleValue>? root)
    {
        ArgumentNullException.ThrowIfNull(context);
        ArgumentNullException.ThrowIfNull(collectionPath);
        ArgumentNullException.ThrowIfNull(keyPath);
        ArgumentNullException.ThrowIfNull(parentPath);
        ArgumentException.ThrowIfNullOrEmpty(targetKey);
        ArgumentException.ThrowIfNullOrEmpty(targetChildren);

        var collection = context.RepeatedSource(collectionPath);
        if (collection is null)
        {
            var path = string.Join('/', collectionPath);
            throw new FerruleRuntimeException(
                FerruleRuntimeError.MissingAdjacencyCollection,
                $"adjacency-tree collection '{path}' is missing",
                sourceField: path);
        }
        if ((ulong)collection.Items.Count > FerruleSequences.MaximumGeneratedSequenceItems)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.AdjacencyTreeTooLarge,
                $"adjacency tree produced more than {FerruleSequences.MaximumGeneratedSequenceItems} items",
                maximumItems: FerruleSequences.MaximumGeneratedSequenceItems);
        }

        var rows = new List<Row>(collection.Items.Count);
        var byKey = new Dictionary<string, int>(StringComparer.Ordinal);
        var roots = new List<int>();
        var byParent = new Dictionary<string, List<int>>(StringComparer.Ordinal);
        for (var index = 0; index < collection.Items.Count; index++)
        {
            var instance = collection.Items[index];
            var key = StringField(instance, keyPath, "key");
            if (!byKey.TryAdd(key, index))
            {
                throw new FerruleRuntimeException(
                    FerruleRuntimeError.DuplicateAdjacencyKey,
                    $"adjacency-tree key '{key}' occurs more than once",
                    detail: key);
            }
            var parent = OptionalStringField(instance, parentPath, "parent");
            if (parent is null)
            {
                roots.Add(index);
            }
            else
            {
                if (!byParent.TryGetValue(parent, out var children))
                {
                    children = new List<int>();
                    byParent.Add(parent, children);
                }
                children.Add(index);
            }
            rows.Add(new Row(key));
        }

        var selectedRoot = root is null ? null : RootValue(root(context));
        IReadOnlyList<int> selectedRoots;
        if (selectedRoot is null)
        {
            selectedRoots = roots;
        }
        else if (byParent.TryGetValue(selectedRoot, out var matchingRoots))
        {
            selectedRoots = matchingRoots;
        }
        else
        {
            selectedRoots = Array.Empty<int>();
        }
        if (selectedRoots.Count != 1)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.AdjacencyRootCount,
                $"adjacency tree requires exactly one selected root row, got {selectedRoots.Count}",
                detail: selectedRoots.Count.ToString(
                    System.Globalization.CultureInfo.InvariantCulture));
        }
        return BuildRow(
            selectedRoots[0],
            rows,
            byParent,
            targetKey,
            targetChildren,
            0,
            new List<int>());
    }

    private static string? RootValue(FerruleValue value) => value.Kind switch
    {
        FerruleValueKind.Null or FerruleValueKind.JsonNull => null,
        FerruleValueKind.String => value.StringValue,
        _ => throw new FerruleRuntimeException(
            FerruleRuntimeError.InvalidAdjacencyRoot,
            $"adjacency-tree root requires a string or absent value, got {value.Kind}",
            foundKind: value.Kind),
    };

    private static FerruleGroup BuildRow(
        int index,
        IReadOnlyList<Row> rows,
        IReadOnlyDictionary<string, List<int>> byParent,
        string targetKey,
        string targetChildren,
        int depth,
        List<int> active)
    {
        if (depth >= FerruleSequences.MaximumRecursiveSequenceDepth)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.AdjacencyTreeDepth,
                $"adjacency tree exceeds the {FerruleSequences.MaximumRecursiveSequenceDepth}-group depth limit",
                maximumDepth: FerruleSequences.MaximumRecursiveSequenceDepth);
        }
        if (active.Contains(index))
        {
            var key = rows[index].Key;
            throw new FerruleRuntimeException(
                FerruleRuntimeError.AdjacencyCycle,
                $"adjacency tree contains a cycle at key '{key}'",
                detail: key);
        }
        active.Add(index);
        var row = rows[index];
        var childIndices = byParent.GetValueOrDefault(row.Key) ?? new List<int>();
        var children = childIndices.Select(child =>
            (FerruleInstance)BuildRow(
                child,
                rows,
                byParent,
                targetKey,
                targetChildren,
                depth + 1,
                active)).ToList();
        active.RemoveAt(active.Count - 1);
        return new FerruleGroup(new FerruleField[]
        {
            new(targetKey, new FerruleScalar(FerruleValue.FromString(row.Key))),
            new(targetChildren, new FerruleRepeated(children)),
        });
    }

    private static string StringField(
        FerruleInstance instance,
        IReadOnlyList<string> path,
        string role)
    {
        var value = FieldScalar(instance, path);
        if (value is { Kind: FerruleValueKind.String })
        {
            return value.Value.StringValue;
        }
        throw InvalidField(role, path, value?.Kind.ToString() ?? "missing value");
    }

    private static string? OptionalStringField(
        FerruleInstance instance,
        IReadOnlyList<string> path,
        string role)
    {
        var value = FieldScalar(instance, path);
        if (value is null ||
            value.Value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
        {
            return null;
        }
        if (value.Value.Kind == FerruleValueKind.String)
        {
            return value.Value.StringValue;
        }
        throw InvalidField(role, path, value.Value.Kind.ToString());
    }

    private static FerruleValue? FieldScalar(
        FerruleInstance instance,
        IReadOnlyList<string> path)
    {
        FerruleInstance? current = instance;
        foreach (var segment in path)
        {
            current = current is null ? null : Field(current, segment);
            if (current is null)
            {
                return null;
            }
        }
        return current is FerruleScalar scalar ? scalar.Value : null;
    }

    private static FerruleInstance? Field(FerruleInstance instance, string name) =>
        instance switch
        {
            FerruleGroup group when group.TryGetField(name, out var value) => value,
            FerruleDocumentSet { Documents.Count: > 0 } documents =>
                Field(documents.Documents[0].Value, name),
            _ => null,
        };

    private static FerruleRuntimeException InvalidField(
        string role,
        IReadOnlyList<string> path,
        string found)
    {
        var joined = string.Join('/', path);
        return new FerruleRuntimeException(
            FerruleRuntimeError.InvalidAdjacencyField,
            $"adjacency-tree {role} field '{joined}' requires a string or absent value, got {found}",
            detail: role,
            sourceField: joined,
            foundInstance: found);
    }

    private sealed record Row(string Key);
}

public sealed partial class ScopeContext
{
    internal FerruleRepeated? RepeatedSource(IReadOnlyList<string> path)
    {
        for (var frameIndex = _frames.Count - 1; frameIndex >= 0; frameIndex--)
        {
            FerruleInstance? current = _frames[frameIndex];
            foreach (var segment in path)
            {
                current = current switch
                {
                    FerruleGroup group when group.TryGetField(segment, out var value) => value,
                    FerruleDocumentSet { Documents.Count: > 0 } documents =>
                        FollowField(documents.Documents[0].Value, segment),
                    _ => null,
                };
                if (current is null)
                {
                    break;
                }
            }
            if (current is FerruleRepeated repeated)
            {
                return repeated;
            }
        }
        return null;
    }

    private static FerruleInstance? FollowField(FerruleInstance instance, string name) =>
        instance switch
        {
            FerruleGroup group when group.TryGetField(name, out var value) => value,
            FerruleDocumentSet { Documents.Count: > 0 } documents =>
                FollowField(documents.Documents[0].Value, name),
            _ => null,
        };
}
