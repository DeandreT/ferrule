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
    public IEnumerable<FerruleBasicScopeContext> EnumerateGenerated(IReadOnlyList<FerruleValue> values)
    {
        ArgumentNullException.ThrowIfNull(values);
        return EnumerateGeneratedCore(values);
    }

    private IEnumerable<FerruleBasicScopeContext> EnumerateGeneratedCore(IReadOnlyList<FerruleValue> values)
    {
        for (var index = 0; index < values.Count; index++)
        {
            yield return GeneratedItemContext(values[index], index + 1);
        }
    }

    private FerruleBasicScopeContext GeneratedItemContext(FerruleValue value, int position)
    {
        try
        {
            var budget = Begin();
            var copy = CloneBudgeted(budget);
            budget.Charge(2); // the appended generated frame and its collection identity
            return new FerruleBasicScopeContext(copy.Roots, copy.LegacyView.BasicGeneratedItem(value, position),
                copy._owned, copy._origins, copy._legacyContainers);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
}

public sealed partial class ScopeContext
{
    internal ScopeContext BasicGeneratedItem(FerruleValue value, int position)
    {
        RequireBasicContext();
        return GeneratedContext(value, position);
    }
}
