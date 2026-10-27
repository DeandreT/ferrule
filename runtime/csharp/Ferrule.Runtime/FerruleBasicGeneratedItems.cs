namespace Ferrule.Runtime;

/// <summary>
/// Generated scalar items for predicates evaluated in a paired Basic view.
/// An item is a lexical empty-path legacy collection frame. It never becomes
/// an owned occurrence or origin, so exact endpoint reads keep resolving in the
/// receiving context while ordinary empty-path reads see the item first.
/// </summary>
public sealed partial class FerruleBasicScopeContext
{
    /// <summary>
    /// Lazily copies this view once per generated scalar and appends the item
    /// with its raw one-based position. Each copy is admitted against the
    /// caller's policy only when reached, so an existential consumer can stop
    /// before later items allocate any view metadata.
    /// </summary>
    public IEnumerable<FerruleBasicScopeContext> EnumerateGenerated(IReadOnlyList<FerruleValue> values, uint? owner = null)
    {
        ArgumentNullException.ThrowIfNull(values);
        return EnumerateGeneratedCore(values, owner);
    }

    private IEnumerable<FerruleBasicScopeContext> EnumerateGeneratedCore(IReadOnlyList<FerruleValue> values, uint? owner)
    {
        for (var index = 0; index < values.Count; index++)
        {
            yield return GeneratedItemContext(values[index], index + 1, owner);
        }
    }

    public FerruleValue GeneratedPosition(uint node, uint item) => LegacyView.GeneratedPosition(node, item);

    private FerruleBasicScopeContext GeneratedItemContext(FerruleValue value, int position, uint? owner)
    {
        try
        {
            var budget = Begin();
            var copy = CloneBudgeted(budget);
            budget.Charge(2); // the appended generated frame and its collection identity
            return new FerruleBasicScopeContext(copy.Roots, copy.LegacyView.BasicGeneratedItem(value, position, owner),
                copy._owned, copy._origins, copy._legacyContainers);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
}

public sealed partial class ScopeContext
{
    internal ScopeContext BasicGeneratedItem(FerruleValue value, int position, uint? owner = null)
    {
        RequireBasicContext();
        return GeneratedContext(value, position, owner);
    }
}
