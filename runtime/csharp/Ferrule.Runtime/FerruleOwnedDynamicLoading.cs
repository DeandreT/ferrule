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
    // Each document with its driver row and the logical path it was loaded by.
    private readonly (int Driver, string Path, FerruleInstance Document)[] _documents;

    private FerruleOwnedDynamicDocuments(int key, FerruleRetainedSourceCohort drivers,
        (int Driver, string Path, FerruleInstance Document)[] documents)
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
        var documents = new List<(int Driver, string Path, FerruleInstance Document)>(drivers.Count);
        for (var i = 0; i < drivers.Count; i++)
        {
            var logicalPath = FerruleDynamicSourceItems.LogicalPath(path(drivers[i].Context), source, node);
            if (logicalPath is null) continue;
            documents.Add((i, logicalPath, FerruleDynamicSourceItems.LoadDocument(loader, source, logicalPath)));
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
                entries[i] = new(driver.Context.WithDocument(_key, _documents[i].Document, _documents[i].Path),
                    driver.Cursor);
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

    internal FerruleBasicScopeContext WithDocument(int key, FerruleInstance document, string location) =>
        new(Roots, LegacyView, _owned.WithDocument(key, document, location), _origins, _legacyContainers);
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

    /// <summary>The owner's active document, or else its supplied root.</summary>
    internal FerruleInstance RootOf(int key)
    {
        var document = LoadedDocument(key);
        return document >= 0 ? _frames[document].Item : Roots.Instance(key);
    }

    internal int LoadedDocument(int key)
    {
        for (var i = _frames.Length - 1; i >= 0; i--)
            if (_frames[i].Owner == key && _frames[i].Document) return i;
        return -1;
    }

    internal FerruleOwnedScopeContext WithDocument(int key, FerruleInstance document, string location)
    {
        OwnedSourceAccounting.Limit((nuint)_frames.Length, 1, Roots.Limits.MaxActiveFrames,
            FerruleOwnedSourceLimitKind.Frames);
        var copy = new Occurrence[_frames.Length + 1];
        Array.Copy(_frames, copy, _frames.Length);
        copy[^1] = new(key, Array.Empty<string>(), document, 0, Document: true, Location: location);
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

// Owned scopes over one document-set endpoint. An ungrouped owned scope
// selecting a document-set endpoint with no active document walks each member
// in set order. The member's document roots its owner, with the member's
// location, and the selector path is walked from it as from a supplied root.
// Documents are not positions; nested scopes and reads of the same owner see
// the current document and never re-enter the set, while other owners keep
// their own roots and documents.

public sealed partial class FerruleBasicScopeContext
{
    /// <summary>
    /// The location of the selected owner's latest active document-set member;
    /// another owner's document never supplies it.
    /// </summary>
    public FerruleValue OwnedDocumentPath(FerruleOwnedSourceOwner owner)
    {
        try
        {
            var budget = Begin();
            return _owned.BasicDocumentPath(owner, Query(budget, owner, Array.Empty<string>()));
        }
        catch (FerruleOwnedSourceException error) { throw FerruleRuntimeException.FromOwnedSource(error); }
        catch (FerruleBasicSourceBridgeException error) { throw FerruleRuntimeException.FromBasicSourceBridge(error); }
    }

    /// <summary>
    /// Walks an owned scope selector like <see cref="WalkOwnedSourceCohort"/>,
    /// except that a document-set endpoint with no active document walks each
    /// member as its owner's root, in set order, as one cohort of this view.
    /// </summary>
    public FerruleRetainedSourceCohort WalkOwnedDocumentSourceCohort(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> path)
    {
        try { return FerruleRetainedSourceCohort.WalkDocuments(this, owner, path); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }

    /// <summary>The document set this view's walk of <paramref name="owner"/> enters, if any.</summary>
    internal (int Key, FerruleDocumentSet Set)? DocumentSetFor(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> path)
    {
        if (LoadingFor(owner) is not null) return null;
        var budget = Begin();
        var key = _owned.Resolve(owner, Query(budget, owner, path));
        return _owned.UnenteredDocumentSet(key, path) is { } set ? (key, set) : null;
    }
}

public sealed partial class FerruleOwnedScopeContext
{
    internal FerruleValue BasicDocumentPath(FerruleOwnedSourceOwner owner, OwnedQuery admittedQuery)
    {
        var key = Resolve(owner, admittedQuery);
        var document = LoadedDocument(key);
        if (document < 0 || _frames[document].Location is not { } location)
            throw admittedQuery.Error(FerruleOwnedSourceErrorKind.MissingDocumentPath);
        return FerruleValue.FromString(location);
    }

    /// <summary>The owner's supplied document set when no document or
    /// same-owner occurrence of <paramref name="path"/> is active.</summary>
    internal FerruleDocumentSet? UnenteredDocumentSet(int key, IReadOnlyList<string> path) =>
        LoadedDocument(key) < 0 && Match(key, path, false) < 0 ? Roots.SuppliedInstance(key) as FerruleDocumentSet : null;
}

public sealed partial class FerruleRetainedSourceCohort
{
    /// <summary>One walk of every member of a document-set endpoint, in set
    /// order, as a single cohort of the receiving view.</summary>
    internal static FerruleRetainedSourceCohort WalkDocuments(FerruleBasicScopeContext parent,
        FerruleOwnedSourceOwner owner, IReadOnlyList<string> path)
    {
        if (parent.DocumentSetFor(owner, path) is not { } documents) return Walk(parent, path, owner);
        var budget = parent.Begin();
        budget.Charge(2); // original holder and shared per-walk witness
        var result = new FerruleRetainedSourceCohort(parent, budget);
        try
        {
            foreach (var member in documents.Set.Documents)
            {
                var context = parent.WithDocument(documents.Key, member.Value,
                    member.ResolvedSourcePath ?? member.Path);
                var rows = Walk(context, path, owner);
                for (var i = 0; i < rows.Count; i++)
                {
                    var row = rows[i];
                    result.ReserveOutput(budget);
                    result.Append(row.Context, row.Cursor, budget,
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
