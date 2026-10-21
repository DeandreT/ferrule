using System.Collections.ObjectModel;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>
/// One statically validated replacement of owned mixed-content atomization. An
/// occurrence replacement evaluates in an owned occurrence of the group's own
/// owner; otherwise it evaluates in the receiving context.
/// </summary>
public sealed class FerruleOwnedXmlMixedContentReplacement
{
    public FerruleOwnedXmlMixedContentReplacement(
        string element,
        bool occurrence,
        Func<FerruleBasicScopeContext, FerruleValue> expression)
    {
        if (string.IsNullOrEmpty(element))
        {
            throw new ArgumentException("Element name cannot be empty.", nameof(element));
        }
        ArgumentNullException.ThrowIfNull(expression);
        Element = element;
        Occurrence = occurrence;
        Expression = expression;
    }

    public string Element { get; }

    public bool Occurrence { get; }

    public Func<FerruleBasicScopeContext, FerruleValue> Expression { get; }
}

public sealed partial class FerruleBasicScopeContext
{
    /// <summary>
    /// Atomizes one owned mixed group. The group is read with the owned element
    /// rules, so a missing group never borrows another owner or legacy frame.
    /// Failures of the group read and of an occurrence frame are attributed to
    /// <paramref name="node"/>; replacement failures keep their own nodes.
    /// </summary>
    public FerruleValue OwnedXmlMixedContent(uint node, FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> path, IReadOnlyList<string>? frame, bool declaredNonrepeating,
        IReadOnlyList<FerruleOwnedXmlMixedContentReplacement> replacements)
    {
        ArgumentNullException.ThrowIfNull(replacements);
        FerruleInstance source;
        try { source = ResolveOwnedXmlInstance(owner, path, frame, declaredNonrepeating); }
        catch (FerruleRuntimeException error) { error.WithOwnedSourceNode(node); throw; }
        if (source is not FerruleGroup group)
        {
            return FerruleValue.Null;
        }
        if (!group.TryGetField(FerruleXmlMixedContent.MixedContentField, out var retained) ||
            retained is not FerruleRepeated items)
        {
            return group.TryGetField(FerruleXmlMixedContent.TextField, out var text) &&
                text is FerruleScalar scalar
                    ? scalar.Value
                    : FerruleValue.Null;
        }

        var output = new StringBuilder();
        var occurrences = new Dictionary<string, int>(StringComparer.Ordinal);
        foreach (var item in items.Items)
        {
            var name = FerruleXmlMixedContent.StringField(item, FerruleXmlMixedContent.NodeNameField);
            var text = FerruleXmlMixedContent.StringField(item, FerruleXmlMixedContent.TextField);
            FerruleOwnedXmlMixedContentReplacement? replacement = null;
            foreach (var rule in replacements)
            {
                if (string.Equals(rule.Element, name, StringComparison.Ordinal))
                {
                    replacement = rule;
                    break;
                }
            }
            if (replacement is null)
            {
                output.Append(text);
                continue;
            }
            if (!replacement.Occurrence)
            {
                FerruleXmlMixedContent.Append(output, replacement.Expression(this));
                continue;
            }
            // Readers retain every element's value; an item without one is no
            // occurrence and keeps its text.
            if (item is not FerruleGroup itemGroup ||
                !itemGroup.TryGetField(FerruleXmlMixedContent.MixedValueField, out var value) ||
                value is null)
            {
                output.Append(text);
                continue;
            }
            occurrences.TryGetValue(name, out var occurrence);
            occurrence++;
            occurrences[name] = occurrence;
            FerruleBasicScopeContext occurrenceContext;
            try { occurrenceContext = WithOwnedMixedOccurrence(owner, path, name, value, occurrence); }
            catch (FerruleRuntimeException error) { error.WithOwnedSourceNode(node); throw; }
            FerruleXmlMixedContent.Append(output, replacement.Expression(occurrenceContext));
        }
        return FerruleValue.FromString(output.ToString());
    }

    /// <summary>
    /// Attaches the ordered stream of the owner's current occurrence of
    /// <paramref name="collection"/> (the scope's own owned selector) to a
    /// constructed group: its latest exact frame, a group-member frame's first
    /// member, or the registry root of an endpoint-root selection. Another
    /// owner's frame and the innermost legacy frame are never consulted.
    /// </summary>
    public FerruleGroup PreserveOwnedXmlMixedContent(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> collection, FerruleGroup output,
        IReadOnlyList<FerruleXmlMixedContentElement> elements)
    {
        ArgumentNullException.ThrowIfNull(output);
        ArgumentNullException.ThrowIfNull(elements);
        try
        {
            var budget = Begin();
            var source = _owned.ConstructionItem(owner, collection, Query(budget, owner, collection));
            return FerruleXmlMixedContent.PreserveFrom(source, output, elements);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }

    // Stacks one occurrence of `element` under the owner's group at `path` on
    // both views, keeping every existing owned frame.
    internal FerruleBasicScopeContext WithOwnedMixedOccurrence(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> path, string element, FerruleInstance item, int index)
    {
        var collection = new string[path.Count + 1];
        for (var i = 0; i < path.Count; i++) collection[i] = path[i];
        collection[^1] = element;
        try
        {
            var budget = Begin();
            var query = Query(budget, owner, collection);
            var key = _owned.ResolveOwnerKey(owner, query);
            BasicSourceAccounting.Limit((nuint)LegacyView.Frames.Count, 1,
                BasicSourceAccounting.LegacyFrames(Roots.Limits), FerruleBasicSourceBridgeComponent.LegacyFrames);
            var owned = _owned.WithRecursiveOccurrence(query, _owned.FrameCount, key, collection, item, (nuint)index);
            var legacy = LegacyView.WithBasicMixedContentItem(budget, item, collection, index);
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

public sealed partial class FerruleOwnedScopeContext
{
    internal int ResolveOwnerKey(FerruleOwnedSourceOwner owner, OwnedQuery query) =>
        Roots.Resolve(owner, query);

    internal FerruleInstance? ConstructionItem(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> collection, OwnedQuery query)
    {
        var key = Roots.Resolve(owner, query);
        var selected = Match(key, collection, true);
        if (selected >= 0)
        {
            var frame = _frames[selected];
            if (!frame.GroupMembers) return frame.Item;
            var members = ((FerruleRepeated)frame.Item).Items;
            return members.Count == 0 ? null : members[0];
        }
        if (collection.Count == 0) return Roots.Instance(key);
        throw query.Error(FerruleOwnedSourceErrorKind.InactiveSourceFrame, collection: collection);
    }
}

public sealed partial class ScopeContext
{
    // The legacy view stacks the occurrence at its complete endpoint-local path.
    internal ScopeContext WithBasicMixedContentItem(
        BasicSourceBudget budget,
        FerruleInstance item,
        IReadOnlyList<string> collection,
        int index)
    {
        RequireBasicContext();
        budget.Charge(BasicSourceAccounting.Add((nuint)_frames.Count + 1, (nuint)_collections.Count + 1));
        budget.Charge(4); // two read-only wrappers, the association and the new legacy view
        for (var i = 0; i < _collections.Count; i++) budget.Path(_collections[i].Path);
        var path = collection.ToArray();
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
