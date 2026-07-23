using System.Collections.ObjectModel;

namespace Ferrule.Runtime;

/// <summary>Engine-compatible recursive same-shape group filtering.</summary>
public static class FerruleRecursiveFilter
{
    public const int MaximumDepth = 256;

    public static FerruleInstance Apply(
        ScopeContext context,
        string children,
        string items,
        uint predicateNode,
        Func<ScopeContext, FerruleValue> predicate)
    {
        ArgumentNullException.ThrowIfNull(context);
        ArgumentException.ThrowIfNullOrEmpty(children);
        ArgumentException.ThrowIfNullOrEmpty(items);
        ArgumentNullException.ThrowIfNull(predicate);
        return FilterGroup(context, children, items, predicateNode, predicate, 0);
    }

    private static FerruleGroup FilterGroup(
        ScopeContext context,
        string children,
        string items,
        uint predicateNode,
        Func<ScopeContext, FerruleValue> predicate,
        int depth)
    {
        if (depth >= MaximumDepth)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.RecursiveFilterDepth,
                $"recursive filter exceeds the {MaximumDepth}-group depth limit",
                maximumDepth: MaximumDepth);
        }
        if (context.Frames.Count == 0)
        {
            throw RequiresGroup("missing context");
        }
        if (context.Frames[^1] is not FerruleGroup current)
        {
            throw RequiresGroup(InstanceKind(context.Frames[^1]));
        }

        var output = new List<FerruleField>(current.Fields.Count);
        foreach (var field in current.Fields)
        {
            FerruleInstance value;
            if (string.Equals(field.Name, items, StringComparison.Ordinal))
            {
                value = FilterItems(context, field.Value, items, predicateNode, predicate);
            }
            else if (string.Equals(field.Name, children, StringComparison.Ordinal))
            {
                value = FilterChildren(
                    context,
                    field.Value,
                    children,
                    items,
                    predicateNode,
                    predicate,
                    depth);
            }
            else
            {
                value = CloneInstance(field.Value);
            }
            output.Add(new FerruleField(field.Name, value));
        }
        return new FerruleGroup(output);
    }

    private static FerruleRepeated FilterItems(
        ScopeContext context,
        FerruleInstance collection,
        string items,
        uint predicateNode,
        Func<ScopeContext, FerruleValue> predicate)
    {
        if (collection is not FerruleRepeated values)
        {
            throw RequiresCollection(items, collection);
        }
        var output = new List<FerruleInstance>(values.Items.Count);
        for (var index = 0; index < values.Items.Count; index++)
        {
            var item = values.Items[index];
            var itemContext = context.WithRecursiveFilterItem(item, items, index + 1);
            if (FerruleFunctions.RequireBoolean(predicate(itemContext), predicateNode))
            {
                output.Add(CloneInstance(item));
            }
        }
        return new FerruleRepeated(output);
    }

    private static FerruleRepeated FilterChildren(
        ScopeContext context,
        FerruleInstance collection,
        string children,
        string items,
        uint predicateNode,
        Func<ScopeContext, FerruleValue> predicate,
        int depth)
    {
        if (collection is not FerruleRepeated values)
        {
            throw RequiresCollection(children, collection);
        }
        var output = new List<FerruleInstance>(values.Items.Count);
        for (var index = 0; index < values.Items.Count; index++)
        {
            var child = values.Items[index];
            var childContext = context.WithRecursiveFilterItem(child, children, index + 1);
            output.Add(FilterGroup(
                childContext,
                children,
                items,
                predicateNode,
                predicate,
                depth + 1));
        }
        return new FerruleRepeated(output);
    }

    private static FerruleRuntimeException RequiresGroup(string found) =>
        new(
            FerruleRuntimeError.RecursiveFilterRequiresGroup,
            $"recursive filter requires a group item, got {found}",
            foundInstance: found);

    private static FerruleRuntimeException RequiresCollection(
        string field,
        FerruleInstance value)
    {
        var found = InstanceKind(value);
        return new FerruleRuntimeException(
            FerruleRuntimeError.RecursiveFilterRequiresCollection,
            $"recursive filter field '{field}' must be a repeated collection, got {found}",
            sourceField: field,
            foundInstance: found);
    }

    private static string InstanceKind(FerruleInstance instance) => instance switch
    {
        FerruleScalar => "scalar",
        FerruleGroup => "group",
        FerruleRepeated => "repeated collection",
        FerruleMappedSequence => "mapped sequence",
        FerruleDocumentSet => "document set",
        _ => "unknown instance",
    };

    private static FerruleInstance CloneInstance(FerruleInstance instance) => instance switch
    {
        FerruleScalar scalar => new FerruleScalar(scalar.Value),
        FerruleGroup group => new FerruleGroup(group.Fields.Select(field =>
            new FerruleField(field.Name, CloneInstance(field.Value)))),
        FerruleRepeated repeated => new FerruleRepeated(repeated.Items.Select(CloneInstance)),
        FerruleMappedSequence mapped => new FerruleMappedSequence(mapped.Items.Select(CloneInstance)),
        FerruleDocumentSet documents => new FerruleDocumentSet(documents.Documents.Select(document =>
            new FerruleDocument(
                document.Path,
                CloneInstance(document.Value),
                document.ResolvedSourcePath))),
        _ => throw new InvalidOperationException("unknown Ferrule instance type"),
    };
}

public sealed partial class ScopeContext
{
    internal ScopeContext WithRecursiveFilterItem(
        FerruleInstance item,
        string collection,
        int index)
    {
        ArgumentNullException.ThrowIfNull(item);
        ArgumentException.ThrowIfNullOrEmpty(collection);
        if (index <= 0)
        {
            throw new ArgumentOutOfRangeException(nameof(index));
        }

        var frames = new List<FerruleInstance>(_frames.Count + 1);
        frames.AddRange(_frames);
        frames.Add(item);
        var collections = new List<CollectionIdentity>(_collections.Count + 1);
        collections.AddRange(_collections);
        collections.Add(new CollectionIdentity(new[] { collection }, item, index));
        return new ScopeContext(
            new ReadOnlyCollection<FerruleInstance>(frames),
            new ReadOnlyCollection<CollectionIdentity>(collections),
            _executionContext);
    }
}
