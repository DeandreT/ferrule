using System.Collections.ObjectModel;

namespace Ferrule.Runtime;

/// <summary>Engine-compatible recursive same-shape group filtering.</summary>
public static class FerruleRecursiveFilter
{
    public const int MaximumDepth = 256;
    private const string OrderedXmlField = "\u001fferrule-xml-mixed-content";
    private const string OrderedXmlValueField = "\u001fferrule-xml-mixed-value";
    private const string XmlNodeNameField = "NodeName";

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

    /// <summary>
    /// Filters the current group of a scope iterating <paramref name="collection"/> of one
    /// explicitly owned endpoint. That selector is the only owner: every visited group keeps
    /// its collection identity and every item that collection plus the item field, so owned
    /// predicate reads and positions never fall back to another endpoint.
    /// </summary>
    public static FerruleInstance ApplyOwned(
        FerruleBasicScopeContext context,
        FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> collection,
        string children,
        string items,
        uint predicateNode,
        Func<FerruleBasicScopeContext, FerruleValue> predicate)
    {
        ArgumentNullException.ThrowIfNull(context);
        ArgumentNullException.ThrowIfNull(collection);
        ArgumentException.ThrowIfNullOrEmpty(children);
        ArgumentException.ThrowIfNullOrEmpty(items);
        ArgumentNullException.ThrowIfNull(predicate);
        var (key, current) = context.OwnedRecursiveFilterRoot(owner, collection);
        if (current is null)
        {
            throw RequiresGroup("missing context");
        }
        var itemPath = new string[collection.Count + 1];
        for (var index = 0; index < collection.Count; index++) itemPath[index] = collection[index];
        itemPath[^1] = items;
        var walk = new OwnedWalk(context.OwnedFrameCount, key, collection.ToArray(), itemPath,
            children, items, predicateNode, predicate);
        return walk.Group(context, current, 0);
    }

    private sealed record OwnedWalk(
        int Retain,
        int Key,
        IReadOnlyList<string> GroupPath,
        IReadOnlyList<string> ItemPath,
        string Children,
        string Items,
        uint PredicateNode,
        Func<FerruleBasicScopeContext, FerruleValue> Predicate)
    {
        internal FerruleGroup Group(FerruleBasicScopeContext context, FerruleInstance current, int depth)
        {
            if (depth >= MaximumDepth)
            {
                throw new FerruleRuntimeException(
                    FerruleRuntimeError.RecursiveFilterDepth,
                    $"recursive filter exceeds the {MaximumDepth}-group depth limit",
                    maximumDepth: MaximumDepth);
            }
            if (current is not FerruleGroup group)
            {
                throw RequiresGroup(InstanceKind(current));
            }
            var output = new List<FerruleField>(group.Fields.Count);
            List<bool>? keptItems = null;
            var filteredChildren = false;
            foreach (var field in group.Fields)
            {
                FerruleInstance value;
                if (string.Equals(field.Name, Items, StringComparison.Ordinal))
                {
                    value = FilterItems(context, field.Value, out keptItems);
                }
                else if (string.Equals(field.Name, Children, StringComparison.Ordinal))
                {
                    filteredChildren = true;
                    value = FilterChildren(context, field.Value, depth);
                }
                else
                {
                    value = CloneInstance(field.Value);
                }
                output.Add(new FerruleField(field.Name, value));
            }
            if (keptItems is not null || filteredChildren)
            {
                RebuildOrderedXml(group, output, Items, Children, keptItems ?? []);
            }
            return group.RebuildFields(output);
        }

        private FerruleRepeated FilterItems(
            FerruleBasicScopeContext context, FerruleInstance collection, out List<bool> kept)
        {
            if (collection is not FerruleRepeated values)
            {
                throw RequiresCollection(Items, collection);
            }
            var output = new List<FerruleInstance>(values.Items.Count);
            kept = new List<bool>(values.Items.Count);
            for (var index = 0; index < values.Items.Count; index++)
            {
                var item = values.Items[index];
                var itemContext = context.WithOwnedRecursiveFrame(
                    context.OwnedFrameCount, Key, ItemPath, item, Items, index + 1);
                var keep = FerruleFunctions.RequireBoolean(Predicate(itemContext), PredicateNode);
                kept.Add(keep);
                if (keep)
                {
                    output.Add(CloneInstance(item));
                }
            }
            return new FerruleRepeated(output);
        }

        private FerruleRepeated FilterChildren(
            FerruleBasicScopeContext context, FerruleInstance collection, int depth)
        {
            if (collection is not FerruleRepeated values)
            {
                throw RequiresCollection(Children, collection);
            }
            var output = new List<FerruleInstance>(values.Items.Count);
            for (var index = 0; index < values.Items.Count; index++)
            {
                var child = values.Items[index];
                var childContext = context.WithOwnedRecursiveFrame(
                    Retain, Key, GroupPath, child, Children, index + 1);
                output.Add(Group(childContext, child, depth + 1));
            }
            return new FerruleRepeated(output);
        }
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
        List<bool>? keptItems = null;
        var filteredChildren = false;
        foreach (var field in current.Fields)
        {
            FerruleInstance value;
            if (string.Equals(field.Name, items, StringComparison.Ordinal))
            {
                value = FilterItems(
                    context, field.Value, items, predicateNode, predicate, out keptItems);
            }
            else if (string.Equals(field.Name, children, StringComparison.Ordinal))
            {
                filteredChildren = true;
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
        if (keptItems is not null || filteredChildren)
        {
            RebuildOrderedXml(current, output, items, children, keptItems ?? []);
        }
        return current.RebuildFields(output);
    }

    // The XML choice reader keeps a private ordered stream of typed children.
    // The XML writer consumes that stream, so it must follow the filtered fields.
    private static void RebuildOrderedXml(
        FerruleGroup source,
        List<FerruleField> output,
        string items,
        string children,
        IReadOnlyList<bool> keptItems)
    {
        if (!source.TryGetField(OrderedXmlField, out var ordered) ||
            ordered is not FerruleRepeated entries)
        {
            return;
        }
        var filteredItems = output.FirstOrDefault(field => field.Name == items)?.Value
            as FerruleRepeated;
        var filteredChildren = output.FirstOrDefault(field => field.Name == children)?.Value
            as FerruleRepeated;
        var rebuilt = new List<FerruleInstance>(entries.Items.Count);
        var itemIndex = 0;
        var keptIndex = 0;
        var childIndex = 0;
        foreach (var entry in entries.Items)
        {
            if (entry is not FerruleGroup group ||
                !group.TryGetField(XmlNodeNameField, out var nodeName) ||
                nodeName is not FerruleScalar
                {
                    Value.Kind: FerruleValueKind.String,
                } name)
            {
                rebuilt.Add(CloneInstance(entry));
                continue;
            }
            FerruleInstance? replacement = null;
            if (string.Equals(name.Value.StringValue, items, StringComparison.Ordinal))
            {
                var keep = itemIndex < keptItems.Count && keptItems[itemIndex];
                itemIndex++;
                if (!keep)
                {
                    continue;
                }
                if (filteredItems is not null && keptIndex < filteredItems.Items.Count)
                {
                    replacement = filteredItems.Items[keptIndex];
                }
                keptIndex++;
            }
            else if (string.Equals(name.Value.StringValue, children, StringComparison.Ordinal))
            {
                if (filteredChildren is not null && childIndex < filteredChildren.Items.Count)
                {
                    replacement = filteredChildren.Items[childIndex];
                }
                childIndex++;
            }
            if ((name.Value.StringValue == items || name.Value.StringValue == children) &&
                replacement is null)
            {
                continue;
            }
            if (replacement is null)
            {
                // No data in this unrelated occurrence changed: retain its own
                // and its nested occurrences' facts through an exact clone.
                rebuilt.Add(CloneInstance(entry));
                continue;
            }
            rebuilt.Add(group.RebuildFields(group.Fields.Select(field =>
                new FerruleField(
                    field.Name,
                    field.Name == OrderedXmlValueField && replacement is not null
                        ? CloneInstance(replacement)
                        : CloneInstance(field.Value)))));
        }
        var orderedIndex = output.FindIndex(field => field.Name == OrderedXmlField);
        if (orderedIndex >= 0)
        {
            output[orderedIndex] = new FerruleField(
                OrderedXmlField, new FerruleRepeated(rebuilt));
        }
    }

    private static FerruleRepeated FilterItems(
        ScopeContext context,
        FerruleInstance collection,
        string items,
        uint predicateNode,
        Func<ScopeContext, FerruleValue> predicate,
        out List<bool> kept)
    {
        if (collection is not FerruleRepeated values)
        {
            throw RequiresCollection(items, collection);
        }
        var output = new List<FerruleInstance>(values.Items.Count);
        kept = new List<bool>(values.Items.Count);
        for (var index = 0; index < values.Items.Count; index++)
        {
            var item = values.Items[index];
            var itemContext = context.WithRecursiveFilterItem(item, items, index + 1);
            var keep = FerruleFunctions.RequireBoolean(predicate(itemContext), predicateNode);
            kept.Add(keep);
            if (keep)
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
        FerruleGroup group => group.CloneFields(group.Fields.Select(field =>
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
            _primarySource,
            new ReadOnlyCollection<FerruleInstance>(frames),
            new ReadOnlyCollection<CollectionIdentity>(collections),
            _executionContext,
            _dynamicSourceLoader);
    }
}

public sealed partial class ScopeContext
{
    // One recursive owned-filter level keeps every Basic legacy fact and stacks
    // the visited item exactly like the legacy filter.
    internal ScopeContext WithBasicRecursiveFilterItem(
        BasicSourceBudget budget,
        FerruleInstance item,
        string collection,
        int index)
    {
        RequireBasicContext();
        budget.Charge(BasicSourceAccounting.Add((nuint)_frames.Count + 1, (nuint)_collections.Count + 1));
        budget.Charge(4); // two read-only wrappers, the association and the new legacy view
        for (var i = 0; i < _collections.Count; i++) budget.Path(_collections[i].Path);
        var path = new[] { collection };
        budget.Path(path);
        var frames = new FerruleInstance[_frames.Count + 1];
        var collections = new CollectionIdentity[_collections.Count + 1];
        for (var i = 0; i < _frames.Count; i++) frames[i] = _frames[i];
        for (var i = 0; i < _collections.Count; i++) collections[i] = _collections[i];
        frames[^1] = item;
        collections[^1] = new CollectionIdentity(path, item, index);
        return new ScopeContext(_primarySource, new ReadOnlyCollection<FerruleInstance>(frames),
            new ReadOnlyCollection<CollectionIdentity>(collections), _executionContext,
            _dynamicSourceLoader, _filterMapRunState, _filterMapBoundary, _filterMapCallDepth);
    }
}

public sealed partial class FerruleOwnedScopeContext
{
    // The scope's current group: the owner's latest exact occurrence, or its
    // registry root for an endpoint root selected without an occurrence.
    internal (int Key, FerruleInstance? Current) RecursiveFilterRoot(
        FerruleOwnedSourceOwner owner, IReadOnlyList<string> collection, OwnedQuery query)
    {
        var key = Roots.Resolve(owner, query);
        var selected = Match(key, collection, true);
        if (selected >= 0) return (key, _frames[selected].Item);
        return (key, collection.Count == 0 ? Roots.Instance(key) : null);
    }

    // Keep the first retained frames and add one same-shape occurrence; a newer
    // occurrence with the same owner and collection shadows an older one.
    internal FerruleOwnedScopeContext WithRecursiveOccurrence(OwnedQuery query, int retain, int key,
        IReadOnlyList<string> collection, FerruleInstance item, nuint index)
    {
        if ((uint)retain > (uint)_frames.Length)
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness);
        OwnedSourceAccounting.Limit(0, (nuint)retain + 1, Roots.Limits.MaxActiveFrames,
            FerruleOwnedSourceLimitKind.Frames);
        var copy = CopyFrames(query, new ArraySegment<Occurrence>(_frames, 0, retain));
        query.Slots.Charge(OwnedSourceAccounting.Add((nuint)collection.Count, 2,
            FerruleOwnedSourceLimitKind.MetadataSlots));
        query.Bytes.Charge(OwnedSourceAccounting.PathText(collection));
        var path = new string[collection.Count];
        for (var i = 0; i < path.Length; i++) path[i] = collection[i];
        var frames = new Occurrence[retain + 1];
        Array.Copy(copy, frames, retain);
        frames[retain] = new Occurrence(key, path, item, index);
        return new FerruleOwnedScopeContext(Roots, frames);
    }
}

public sealed partial class FerruleBasicScopeContext
{
    internal int OwnedFrameCount => _owned.FrameCount;

    internal (int Key, FerruleInstance? Current) OwnedRecursiveFilterRoot(
        FerruleOwnedSourceOwner owner, IReadOnlyList<string> collection)
    {
        try
        {
            var budget = Begin();
            return _owned.RecursiveFilterRoot(owner, collection, Query(budget, owner, collection));
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }

    // One recursive level: the legacy view stacks the visited item while the
    // owned view keeps the first retained frames plus the owner's occurrence.
    internal FerruleBasicScopeContext WithOwnedRecursiveFrame(int retain, int key,
        IReadOnlyList<string> collection, FerruleInstance item, string legacyCollection, int index)
    {
        try
        {
            var budget = Begin();
            BasicSourceAccounting.Limit((nuint)LegacyView.Frames.Count, 1,
                BasicSourceAccounting.LegacyFrames(Roots.Limits), FerruleBasicSourceBridgeComponent.LegacyFrames);
            var query = Query(budget, FerruleOwnedSourceOwner.Primary, Array.Empty<string>());
            var owned = _owned.WithRecursiveOccurrence(query, retain, key, collection, item, (nuint)index);
            var legacy = LegacyView.WithBasicRecursiveFilterItem(budget, item, legacyCollection, index);
            budget.Charge((nuint)_origins.Length + 1);
            var origins = new BasicFrameOrigin[_origins.Length];
            for (var i = 0; i < origins.Length; i++)
            {
                budget.Charge((nuint)_origins[i].Canonical.Count);
                budget.Path(_origins[i].Canonical);
                origins[i] = _origins[i];
            }
            return new FerruleBasicScopeContext(Roots, legacy, owned, origins, 2);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
}
