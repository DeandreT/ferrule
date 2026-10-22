namespace Ferrule.Runtime;

/// <summary>
/// Every document one owned dynamic loading scope requested, each paired with
/// only the driver row that requested it.
/// </summary>
/// <remarks>
/// A loading scope walks the declared driver collection of exactly its
/// endpoint, evaluates the logical path once per driver row in driver order and
/// requests every document before its own walk. Each document roots its owner,
/// so nested loading and owned reads never borrow another driver, document or
/// endpoint. Generated code never caches: every reached path is one callback.
/// </remarks>
public sealed class FerruleOwnedDynamicDocuments
{
    private readonly int _key;
    private readonly FerruleRetainedSourceCohort _drivers;
    private readonly (int Driver, FerruleInstance Document)[] _documents;

    private FerruleOwnedDynamicDocuments(int key, FerruleRetainedSourceCohort drivers,
        (int Driver, FerruleInstance Document)[] documents)
    { _key = key; _drivers = drivers; _documents = documents; }

    internal static FerruleOwnedDynamicDocuments Load(FerruleBasicScopeContext context, string source,
        FerruleOwnedSourceOwner driver, IReadOnlyList<string> driverPath, uint node,
        Func<FerruleBasicScopeContext, FerruleValue> path)
    {
        ArgumentException.ThrowIfNullOrEmpty(source);
        ArgumentNullException.ThrowIfNull(driverPath);
        ArgumentNullException.ThrowIfNull(path);
        var loader = FerruleDynamicSourceItems.RequireLoader(context.Roots.DynamicSourceLoader, source);
        var roots = context.Roots.OwnedRoots;
        var key = -1;
        for (var i = 1; (nuint)i < roots.RegistryCount; i++)
            if (StringComparer.Ordinal.Equals(roots.Name(i), source)) { key = i; break; }
        if (key < 0)
            throw FerruleRuntimeException.FromOwnedSource(new FerruleOwnedSourceException(
                FerruleOwnedSourceErrorKind.UnknownOwner, new FerruleOwnedSourceIdentity(source), source));
        var drivers = context.WalkOwnedSourceCohort(driver, driverPath);
        FerruleDynamicSourceItems.RequireDriverCount(drivers.Count, source);
        var documents = new List<(int Driver, FerruleInstance Document)>(drivers.Count);
        for (var i = 0; i < drivers.Count; i++)
        {
            var logicalPath = FerruleDynamicSourceItems.LogicalPath(path(drivers[i].Context), source, node);
            if (logicalPath is null) continue;
            documents.Add((i, FerruleDynamicSourceItems.LoadDocument(loader, source, logicalPath)));
        }
        return new FerruleOwnedDynamicDocuments(key, drivers, documents.ToArray());
    }

    /// <summary>
    /// The receiving view of the loading scope. Its walk of the loaded owner
    /// reads every document under its own driver row; parent expressions keep
    /// the unchanged receiving context.
    /// </summary>
    public FerruleBasicScopeContext LoadingContext(FerruleBasicScopeContext parent)
    {
        ArgumentNullException.ThrowIfNull(parent);
        try
        {
            var entries = new Entry[_documents.Length];
            for (var i = 0; i < entries.Length; i++)
            {
                var driver = _drivers[_documents[i].Driver];
                entries[i] = new(driver.Context.WithDocument(_key, _documents[i].Document), driver.Cursor);
            }
            return parent.WithLoading(new Loaded(_key, entries));
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }

    internal readonly record struct Entry(FerruleBasicScopeContext Context, BasicOccurrenceCursor? Cursor);

    internal sealed class Loaded(int key, Entry[] entries)
    {
        internal IReadOnlyList<Entry> Entries => entries;
        internal bool Owns(FerruleOwnedSourceRoots roots, FerruleOwnedSourceOwner owner) =>
            !owner.IsPrimary && StringComparer.Ordinal.Equals(roots.Name(key), owner.Name);
    }
}

public sealed partial class FerruleBasicScopeContext
{
    /// <summary>
    /// Requests one owned dynamic endpoint's documents for every row of its exact
    /// driver collection, in driver order, before any is walked.
    /// </summary>
    /// <remarks>
    /// A missing loader fails before the driver walk, even for an empty
    /// collection. Absent and null paths skip their driver; other non-string
    /// paths, oversized paths and excessive drivers keep their typed failures,
    /// and each host failure keeps its source and logical path.
    /// </remarks>
    public FerruleOwnedDynamicDocuments LoadOwnedDynamicDocuments(string source,
        FerruleOwnedSourceOwner driver, IReadOnlyList<string> driverPath, uint node,
        Func<FerruleBasicScopeContext, FerruleValue> path) =>
        FerruleOwnedDynamicDocuments.Load(this, source, driver, driverPath, node, path);

    internal FerruleOwnedDynamicDocuments.Loaded? LoadingFor(FerruleOwnedSourceOwner owner) =>
        _loading is { } loading && loading.Owns(Roots.OwnedRoots, owner) ? loading : null;

    internal FerruleBasicScopeContext WithLoading(FerruleOwnedDynamicDocuments.Loaded loading) =>
        new(Roots, LegacyView, _owned, _origins, _legacyContainers) { _loading = loading };

    internal FerruleBasicScopeContext WithDocument(int key, FerruleInstance document) =>
        new(Roots, LegacyView, _owned.WithDocument(key, document), _origins, _legacyContainers);
}

public sealed partial class FerruleOwnedScopeContext
{
    /// <summary>Resolves an exact owner; a declared dynamic owner resolves only
    /// inside its loading scope, to its active loaded document.</summary>
    internal int Resolve(FerruleOwnedSourceOwner owner, OwnedQuery query)
    {
        var key = Roots.ResolveDeclared(owner, query, out var supplied);
        if (!supplied && LoadedDocument(key) < 0)
            throw query.Error(FerruleOwnedSourceErrorKind.SourceUnavailable);
        return key;
    }

    /// <summary>The owner's supplied root, or its active loaded document.</summary>
    internal FerruleInstance RootOf(int key) =>
        Roots.SuppliedInstance(key) ?? _frames[LoadedDocument(key)].Item;

    private int LoadedDocument(int key)
    {
        for (var i = _frames.Length - 1; i >= 0; i--)
            if (_frames[i].Owner == key && _frames[i].Document) return i;
        return -1;
    }

    internal FerruleOwnedScopeContext WithDocument(int key, FerruleInstance document)
    {
        OwnedSourceAccounting.Limit((nuint)_frames.Length, 1, Roots.Limits.MaxActiveFrames,
            FerruleOwnedSourceLimitKind.Frames);
        var copy = new Occurrence[_frames.Length + 1];
        Array.Copy(_frames, copy, _frames.Length);
        copy[^1] = new(key, Array.Empty<string>(), document, 0, Document: true);
        return new(Roots, copy);
    }
}

public sealed partial class FerruleRetainedSourceCohort
{
    /// <summary>One walk of every loaded document, in driver order, as a single
    /// cohort of the loading scope's receiving view.</summary>
    private static FerruleRetainedSourceCohort WalkLoaded(FerruleBasicScopeContext parent,
        FerruleOwnedDynamicDocuments.Loaded loading, FerruleOwnedSourceOwner owner, IReadOnlyList<string> path)
    {
        var budget = parent.Begin();
        budget.Charge(2); // original holder and shared per-walk witness
        var result = new FerruleRetainedSourceCohort(parent, budget);
        try
        {
            foreach (var entry in loading.Entries)
            {
                var rows = Walk(entry.Context, path, owner);
                for (var i = 0; i < rows.Count; i++)
                {
                    var row = rows[i];
                    result.ReserveOutput(budget);
                    // A row's newest repetition is its document walk's own, or its driver row's.
                    result.Append(row.Context, row.Cursor ?? entry.Cursor, budget,
                        row.GroupTerminal ?? throw BasicPhaseAdmission.Invalid((nuint)i));
                }
            }
            result._sealed = true;
            result._witness.Seal(result._count);
            return result;
        }
        finally { result._witness.ReleaseScratch(); }
    }
}
