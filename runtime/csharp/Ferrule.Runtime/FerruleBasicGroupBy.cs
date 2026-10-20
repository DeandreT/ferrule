using System.Collections.ObjectModel;

namespace Ferrule.Runtime;

/// <summary>Static controls shared by owned By, AdjacentBy and StartingWith.</summary>
public readonly record struct FerruleBasicGroupByMetadata(
    IReadOnlyList<bool> Descending, bool FilterBeforeSort, int ExplicitWindowCount,
    bool ImplicitFirst, bool HasFilter, bool HasPostFilter);

/// <summary>Candidate-phase controls shared by owned By and AdjacentBy.</summary>
public interface IFerruleBasicGroupByControls
{
    FerruleBasicGroupByMetadata Metadata();
    bool Filter(FerruleBasicScopeContext context);
    FerruleValue SortKey(int key, FerruleBasicScopeContext context);
    FerruleSequenceWindow Window(int bound, FerruleBasicScopeContext parent);
    FerruleValue GroupKey(FerruleBasicScopeContext context);
    bool RetainMember(FerruleBasicScopeContext context);
}

/// <summary>Candidate-phase Boolean boundary controls for owned StartingWith.</summary>
public interface IFerruleBasicGroupStartingWithControls
{
    FerruleBasicGroupByMetadata Metadata();
    bool Filter(FerruleBasicScopeContext context);
    FerruleValue SortKey(int key, FerruleBasicScopeContext context);
    FerruleSequenceWindow Window(int bound, FerruleBasicScopeContext parent);
    bool StartsGroup(FerruleBasicScopeContext context);
    bool RetainMember(FerruleBasicScopeContext context);
}

public sealed partial class FerruleBasicScopeContext
{
    /// <summary>Executes authenticated global, first-seen By grouping under a cumulative metadata reservation.</summary>
    /// <remarks>Scalar key strings are admitted when their callbacks return. Source/output
    /// payload allocation and caller side effects are not an RSS or transactional bound.</remarks>
    public IReadOnlyList<FerruleInstance> MapGroupBySourceCohort(
        FerruleRetainedSourceCohort original, IFerruleBasicGroupByControls controls,
        Func<FerruleBasicScopeContext, FerruleInstance> target) =>
        FerruleBasicGroupByOperation.Run(this, original, controls, target, BasicGroupPartition.By);

    /// <summary>Executes authenticated contiguous, exact-key AdjacentBy runs under a cumulative metadata reservation.</summary>
    /// <remarks>Only the last constructed run can acquire a member. Post predicates run
    /// for every candidate; dropping an all-false run never joins its neighbors.</remarks>
    public IReadOnlyList<FerruleInstance> MapGroupAdjacentBySourceCohort(
        FerruleRetainedSourceCohort original, IFerruleBasicGroupByControls controls,
        Func<FerruleBasicScopeContext, FerruleInstance> target) =>
        FerruleBasicGroupByOperation.Run(this, original, controls, target, BasicGroupPartition.AdjacentBy);

    /// <summary>Executes authenticated StartingWith grouping under a cumulative metadata reservation.</summary>
    /// <remarks>Each boundary runs before its member's post predicate, including the
    /// first candidate. A true member starts and belongs to a new run; an initial
    /// false prefix is retained as a real run. Boolean boundaries consume no key text.</remarks>
    public IReadOnlyList<FerruleInstance> MapGroupStartingWithSourceCohort(
        FerruleRetainedSourceCohort original, IFerruleBasicGroupStartingWithControls controls,
        Func<FerruleBasicScopeContext, FerruleInstance> target) =>
        FerruleBasicGroupByOperation.Run(this, original, controls, target);
}

// Minted inline by the actual walk. Paths borrow immutable candidate storage;
// a plain legacy terminal's raw identity is a suffix plus its consumed alias.
internal readonly record struct BasicGroupTerminal(int Owner, FerruleInstance Item,
    IReadOnlyList<string> Canonical, IReadOnlyList<string> Raw, int RawOffset, int RawCount,
    string? Alias, int OwnedCut, int LegacyCut, int OriginCut, int CollectionCut, string? Wrapper)
{
    internal string RawAt(int index) => Alias is not null && index == 0 ? Alias :
        Raw[RawOffset + index - (Alias is null ? 0 : 1)];
}

public sealed partial class FerruleOwnedScopeContext
{
    internal static FerruleOwnedScopeContext FromAdmittedGroup(FerruleOwnedSourceRoots roots, Occurrence[] frames) =>
        new(roots, frames);
    internal void SetAdmittedRank(int slot, nuint rank) => _frames[slot] = _frames[slot] with { Index = rank };
}

public sealed partial class ScopeContext
{
    internal CollectionIdentity BasicCollectionAt(int index) => _collections[index];
    internal ScopeContext FromAdmittedGroup(FerruleInstance[] frames, CollectionIdentity[] collections) =>
        new(_primarySource, new ReadOnlyCollection<FerruleInstance>(frames),
            new ReadOnlyCollection<CollectionIdentity>(collections), _executionContext,
            _dynamicSourceLoader, _filterMapRunState, _filterMapBoundary, _filterMapCallDepth);
    internal void SetAdmittedRank(int slot, int rank)
    {
        // Only called before a selection view is published to any callback.
        var collections = (ReadOnlyCollection<CollectionIdentity>)_collections;
        if (collections[slot] is not { } old) throw BasicPhaseAdmission.Invalid((nuint)slot);
        // Its association object was prepaid and may not be replaced per rank.
        old.SetGroupSelectionRank(rank);
    }
}

internal static class BasicGroupAccounting
{
    // Two explicit fixed setup records: the slot-ledger OwnedCounter and the
    // UTF8-ledger OwnedCounter. Neither is hidden in S0 or the operation's three.
    internal const int LedgerSetupRecords = 2;
    internal static void AdmitLedgerSetup(nuint maximum) => BasicSourceAccounting.Limit(
        0, (nuint)LedgerSetupRecords, maximum, FerruleBasicSourceBridgeComponent.MetadataSlots);
    internal static nuint Add(nuint a, nuint b) => BasicSourceAccounting.Add(a, b);
    internal static nuint Mul(nuint a, nuint b) => BasicSourceAccounting.Multiply(a, b);
    internal static nuint Sub(nuint a, nuint b) => a >= b ? a - b : throw BasicSourceAccounting.Overflow();
    internal static nuint Maximum(FerruleOwnedSourceLimits limits)
    {
        var f = limits.MaxActiveFrames; var p = limits.MaxPathSegments;
        var l = BasicSourceAccounting.LegacyFrames(limits); var q = Add(p, 1);
        var j = Add(Add(Mul(f, p), Mul(f, q)), Mul(l, p));
        var h = Add(Add(Add(7, Mul(3, f)), Mul(2, l)), j);
        var pair = Add(h, Sub(h, 1));
        return Add(Add(Add(BasicSourceAccounting.Maximum(limits, true), Mul(limits.MaxWalkContexts, pair)),
            Mul(14, limits.MaxWalkContexts)), Add(3, (nuint)LedgerSetupRecords));
    }
    internal static int Capacity<T>(nuint count)
    {
        var length = BasicSourceAccounting.ArrayLength(count);
        if (Mul(count, (nuint)System.Runtime.CompilerServices.Unsafe.SizeOf<T>()) > (nuint)nint.MaxValue)
            throw BasicSourceAccounting.Overflow();
        return length;
    }
    internal static string[] CopyPath(IReadOnlyList<string> path)
    {
        var copy = new string[path.Count];
        for (var i = 0; i < copy.Length; i++) copy[i] = path[i];
        return copy;
    }
    internal static nuint PathSlots(IReadOnlyList<string> path) { Capacity<string>((nuint)path.Count); return (nuint)path.Count; }
    internal static nuint Text(string value)
    {
        try { return OwnedSourceAccounting.Text(value); }
        catch (FerruleOwnedSourceException e) when (e.Error == FerruleOwnedSourceErrorKind.MetadataArithmeticOverflow)
        { throw BasicSourceAccounting.Overflow(); }
    }
    internal static nuint PathBytes(IReadOnlyList<string> path)
    {
        nuint bytes = 0;
        for (var i = 0; i < path.Count; i++) bytes = Add(bytes, Text(path[i]));
        return bytes;
    }
}

// Each template is a one-shot capability for its distinct future immutable context
// and exact payload. No ticket vector or per-group replenishment exists.
internal sealed class BasicGroupPrefixTemplate
{
    private readonly FerruleBasicScopeContext _source;
    private readonly BasicGroupTerminal _terminal;
    private readonly FerruleOwnedScopeContext.Occurrence[] _owned;
    private readonly FerruleInstance[] _legacy;
    private readonly BasicFrameOrigin[] _origins;
    private readonly ScopeContext.CollectionIdentity[] _collections;
    private readonly string[] _ownedPath, _membersPath, _rawPath;
    private readonly string[]? _wrapperPath;
    private bool _claimed, _membersAdopted, _wrapperAdopted, _finished;
    private int _memberCount;

    internal BasicGroupPrefixTemplate(FerruleBasicScopeContext source, BasicGroupTerminal terminal)
    {
        _source = source; _terminal = terminal;
        var w = terminal.Wrapper is null ? 0 : 1;
        _owned = new FerruleOwnedScopeContext.Occurrence[terminal.OwnedCut + 1];
        _legacy = new FerruleInstance[terminal.LegacyCut + 1 + w];
        _origins = new BasicFrameOrigin[terminal.OriginCut + 1 + w];
        _collections = new ScopeContext.CollectionIdentity[terminal.CollectionCut + 1];
        for (var i = 0; i < terminal.OwnedCut; i++)
        {
            var f = source.OwnedView.FrameAt(i);
            _owned[i] = new(f.Owner, BasicGroupAccounting.CopyPath(f.Path), f.Item, f.Index, f.GroupMembers);
        }
        for (var i = 0; i < terminal.LegacyCut; i++) _legacy[i] = source.LegacyView.Frames[i];
        for (var i = 0; i < terminal.OriginCut; i++)
        {
            var origin = source.OriginAt(i);
            _origins[i] = origin with { Canonical = BasicGroupAccounting.CopyPath(origin.Canonical) };
        }
        for (var i = 0; i < terminal.CollectionCut; i++)
        {
            var c = source.LegacyView.BasicCollectionAt(i);
            _collections[i] = c with { Path = BasicGroupAccounting.CopyPath(c.Path) };
        }
        _ownedPath = BasicGroupAccounting.CopyPath(terminal.Canonical);
        _membersPath = BasicGroupAccounting.CopyPath(terminal.Canonical);
        _wrapperPath = w == 0 ? null : BasicGroupAccounting.CopyPath(terminal.Canonical);
        _rawPath = new string[terminal.RawCount];
        for (var i = 0; i < _rawPath.Length; i++) _rawPath[i] = terminal.RawAt(i);
    }

    internal void Claim(int count)
    {
        if (_claimed || count <= 0) throw BasicPhaseAdmission.Invalid(0);
        _claimed = true; _memberCount = count;
    }
    internal void SetSelectionRank(BasicOccurrenceCursor cursor, int rank)
    {
        if (_claimed) throw BasicPhaseAdmission.Invalid(0);
        if (cursor.OwnedSlot < _terminal.OwnedCut)
            _owned[cursor.OwnedSlot] = _owned[cursor.OwnedSlot] with { Index = (nuint)rank };
        if (cursor.LegacySlot < _terminal.CollectionCut) _collections[cursor.LegacySlot].SetGroupSelectionRank(rank);
    }
    internal void ConsumeMembers(int count)
    {
        if (!_claimed || _membersAdopted || count != _memberCount) throw BasicPhaseAdmission.Invalid(0);
        _membersAdopted = true;
    }
    internal void ConsumeWrapper()
    {
        if (!_membersAdopted || _wrapperAdopted || _terminal.Wrapper is null) throw BasicPhaseAdmission.Invalid(0);
        _wrapperAdopted = true;
    }
    internal FerruleBasicScopeContext Finish(FerruleRepeated members, int ordinal)
    {
        if (!_membersAdopted || _finished || ordinal <= 0) throw BasicPhaseAdmission.Invalid(0);
        _finished = true;
        var w = _terminal.Wrapper is null ? 0 : 1;
        if (w != 0)
        {
            _legacy[_terminal.LegacyCut] = FerruleGroup.FromBasicGroupWrapper(_terminal.Wrapper!, members, this);
            _origins[_terminal.OriginCut] = new(_terminal.Owner, _wrapperPath!, Kind: BasicOriginKind.GroupWrapper,
                WrapperField: _terminal.Wrapper, GroupCollection: _terminal.CollectionCut);
        }
        _legacy[^1] = members;
        _origins[^1] = new(_terminal.Owner, _membersPath, (nuint)ordinal, Kind: BasicOriginKind.GroupMembers,
            GroupCollection: _terminal.CollectionCut);
        _owned[^1] = new(_terminal.Owner, _ownedPath, members, (nuint)ordinal, GroupMembers: true);
        _collections[^1] = new(_rawPath, members, ordinal, Grouped: true, BasicGroupMembers: true);
        var owned = FerruleOwnedScopeContext.FromAdmittedGroup(_source.Roots.OwnedRoots, _owned);
        var legacy = _source.LegacyView.FromAdmittedGroup(_legacy, _collections);
        return FerruleBasicScopeContext.Create(_source.Roots, legacy, owned, _origins, 2);
    }

    internal static BasicSortCopyCost Cost(FerruleBasicScopeContext source, BasicGroupTerminal t)
    {
        var limits = source.Roots.Limits;
        BasicSourceAccounting.Limit((nuint)t.OwnedCut, 1, limits.MaxActiveFrames, FerruleBasicSourceBridgeComponent.ActiveFrames);
        var d = t.Wrapper is null ? (nuint)1 : (nuint)2;
        BasicSourceAccounting.Limit((nuint)t.LegacyCut, d, BasicSourceAccounting.LegacyFrames(limits), FerruleBasicSourceBridgeComponent.LegacyFrames);
        BasicSourceAccounting.Limit((nuint)t.OriginCut, d, BasicSourceAccounting.LegacyFrames(limits), FerruleBasicSourceBridgeComponent.LegacyFrames);
        BasicSourceAccounting.Limit((nuint)t.CollectionCut, 1, limits.MaxActiveFrames, FerruleBasicSourceBridgeComponent.ActiveFrames);
        BasicSourceAccounting.Limit(0, (nuint)t.Canonical.Count, limits.MaxPathSegments, FerruleBasicSourceBridgeComponent.PathSegments);
        BasicSourceAccounting.Limit(0, (nuint)t.RawCount, BasicGroupAccounting.Add(limits.MaxPathSegments, 1), FerruleBasicSourceBridgeComponent.PathSegments);
        _ = BasicGroupAccounting.Capacity<FerruleOwnedScopeContext.Occurrence>(BasicGroupAccounting.Add((nuint)t.OwnedCut, 1));
        _ = BasicGroupAccounting.Capacity<FerruleInstance>(BasicGroupAccounting.Add((nuint)t.LegacyCut, d));
        _ = BasicGroupAccounting.Capacity<BasicFrameOrigin>(BasicGroupAccounting.Add((nuint)t.OriginCut, d));
        _ = BasicGroupAccounting.Capacity<ScopeContext.CollectionIdentity>(BasicGroupAccounting.Add((nuint)t.CollectionCut, 1));
        nuint slots = BasicGroupAccounting.Add(7, BasicGroupAccounting.Add(BasicGroupAccounting.Add((nuint)t.OwnedCut, 1),
            BasicGroupAccounting.Add(BasicGroupAccounting.Add((nuint)t.LegacyCut, d),
                BasicGroupAccounting.Add(BasicGroupAccounting.Add((nuint)t.OriginCut, d), BasicGroupAccounting.Mul(2, BasicGroupAccounting.Add((nuint)t.CollectionCut, 1))))));
        nuint bytes = 0;
        for (var i = 0; i < t.OwnedCut; i++) AddPath(source.OwnedView.FrameAt(i).Path, ref slots, ref bytes);
        for (var i = 0; i < t.OriginCut; i++) AddPath(source.OriginAt(i).Canonical, ref slots, ref bytes);
        for (var i = 0; i < t.CollectionCut; i++) AddPath(source.LegacyView.BasicCollectionPath(i), ref slots, ref bytes);
        var copies = t.Wrapper is null ? 2 : 3;
        for (var i = 0; i < copies; i++) AddPath(t.Canonical, ref slots, ref bytes);
        _ = BasicGroupAccounting.Capacity<string>((nuint)t.RawCount);
        slots = BasicGroupAccounting.Add(slots, (nuint)t.RawCount);
        for (var i = 0; i < t.RawCount; i++) bytes = BasicGroupAccounting.Add(bytes, BasicGroupAccounting.Text(t.RawAt(i)));
        if (t.Wrapper is not null) bytes = BasicGroupAccounting.Add(bytes, BasicGroupAccounting.Text(t.Wrapper));
        return new(slots, bytes);
    }
    private static void AddPath(IReadOnlyList<string> path, ref nuint slots, ref nuint bytes)
    {
        slots = BasicGroupAccounting.Add(slots, BasicGroupAccounting.PathSlots(path));
        bytes = BasicGroupAccounting.Add(bytes, BasicGroupAccounting.PathBytes(path));
    }
    internal static BasicSortCopyCost SelectionCost(FerruleBasicScopeContext source)
    {
        nuint slots = BasicGroupAccounting.Add(6, BasicGroupAccounting.Add((nuint)source.OwnedView.FrameCount,
            BasicGroupAccounting.Add((nuint)source.LegacyView.Frames.Count,
                BasicGroupAccounting.Add((nuint)source.OriginCount, BasicGroupAccounting.Mul(2, (nuint)source.LegacyView.BasicCollectionCount)))));
        nuint bytes = 0;
        for (var i = 0; i < source.OwnedView.FrameCount; i++) AddPath(source.OwnedView.FrameAt(i).Path, ref slots, ref bytes);
        for (var i = 0; i < source.OriginCount; i++) AddPath(source.OriginAt(i).Canonical, ref slots, ref bytes);
        for (var i = 0; i < source.LegacyView.BasicCollectionCount; i++) AddPath(source.LegacyView.BasicCollectionPath(i), ref slots, ref bytes);
        _ = BasicGroupAccounting.Capacity<FerruleOwnedScopeContext.Occurrence>((nuint)source.OwnedView.FrameCount);
        _ = BasicGroupAccounting.Capacity<FerruleInstance>((nuint)source.LegacyView.Frames.Count);
        _ = BasicGroupAccounting.Capacity<BasicFrameOrigin>((nuint)source.OriginCount);
        _ = BasicGroupAccounting.Capacity<ScopeContext.CollectionIdentity>((nuint)source.LegacyView.BasicCollectionCount);
        return new(slots, bytes);
    }
    internal static FerruleBasicScopeContext Selection(FerruleBasicScopeContext source)
    {
        var owned = new FerruleOwnedScopeContext.Occurrence[source.OwnedView.FrameCount];
        for (var i = 0; i < owned.Length; i++)
        {
            var f = source.OwnedView.FrameAt(i);
            owned[i] = new(f.Owner, BasicGroupAccounting.CopyPath(f.Path), f.Item, f.Index, f.GroupMembers);
        }
        var frames = new FerruleInstance[source.LegacyView.Frames.Count];
        for (var i = 0; i < frames.Length; i++) frames[i] = source.LegacyView.Frames[i];
        var origins = new BasicFrameOrigin[source.OriginCount];
        for (var i = 0; i < origins.Length; i++)
        { var f = source.OriginAt(i); origins[i] = f with { Canonical = BasicGroupAccounting.CopyPath(f.Canonical) }; }
        var collections = new ScopeContext.CollectionIdentity[source.LegacyView.BasicCollectionCount];
        for (var i = 0; i < collections.Length; i++)
        { var c = source.LegacyView.BasicCollectionAt(i); collections[i] = c with { Path = BasicGroupAccounting.CopyPath(c.Path) }; }
        return FerruleBasicScopeContext.Create(source.Roots, source.LegacyView.FromAdmittedGroup(frames, collections),
            FerruleOwnedScopeContext.FromAdmittedGroup(source.Roots.OwnedRoots, owned), origins, 2);
    }
}

// This private partition choice grants no new public policy or container kind.
// Only these three paired operations mint authenticated member containers.
internal enum BasicGroupPartition { By, AdjacentBy, StartingWith }

internal sealed class FerruleBasicGroupByOperation
{
    private struct Bucket
    {
        internal int Head, Tail, Count;
        internal bool Retained;
    }
    private readonly FerruleRetainedSourceCohort _original;
    private readonly BasicSourceBudget _budget;
    private readonly bool[] _descending;
    private readonly FerruleValue[] _sortKeys, _groupKeys;
    private readonly int[] _order, _scratch, _next;
    private readonly Bucket[] _buckets;
    private readonly FerruleSequenceWindow[] _windows;
    private readonly FerruleInstance[] _outputs;
    private readonly BasicGroupTerminal[] _terminals;
    private readonly BasicGroupPrefixTemplate[] _templates;
    private readonly FerruleBasicScopeContext[] _selection;
    private readonly FerruleBasicGroupByMetadata _metadata;
    private int _remainingMembers;
    private FerruleBasicGroupByOperation(FerruleRetainedSourceCohort original, BasicSourceBudget budget,
        FerruleBasicGroupByMetadata metadata, int keys, int keyCells, int windows)
    {
        _original = original; _budget = budget; _metadata = metadata;
        var n = original.Count; _remainingMembers = n;
        _descending = new bool[keys]; _sortKeys = new FerruleValue[keyCells];
        _order = new int[n]; _scratch = keys == 0 ? Array.Empty<int>() : new int[n];
        _next = new int[n]; _buckets = new Bucket[n]; _groupKeys = new FerruleValue[n];
        _windows = new FerruleSequenceWindow[windows]; _outputs = new FerruleInstance[n];
        _terminals = new BasicGroupTerminal[n]; _templates = new BasicGroupPrefixTemplate[n];
        _selection = keys == 0 ? Array.Empty<FerruleBasicScopeContext>() : new FerruleBasicScopeContext[n];
        for (var i = 0; i < n; i++)
        {
            _next[i] = -1; _terminals[i] = original[i].GroupTerminal!.Value;
            _templates[i] = new BasicGroupPrefixTemplate(original[i].Context, _terminals[i]);
            if (keys != 0) _selection[i] = BasicGroupPrefixTemplate.Selection(original[i].Context);
        }
    }

    internal static IReadOnlyList<FerruleInstance> Run(FerruleBasicScopeContext parent,
        FerruleRetainedSourceCohort original, IFerruleBasicGroupByControls controls,
        Func<FerruleBasicScopeContext, FerruleInstance> target, BasicGroupPartition partition)
    {
        ArgumentNullException.ThrowIfNull(original); ArgumentNullException.ThrowIfNull(controls); ArgumentNullException.ThrowIfNull(target);
        if (partition is not (BasicGroupPartition.By or BasicGroupPartition.AdjacentBy))
            throw new ArgumentOutOfRangeException(nameof(partition));
        var operation = PrepareOperation(parent, original, controls, null);
        return operation.Execute(parent, controls, null, target, partition);
    }
    internal static IReadOnlyList<FerruleInstance> Run(FerruleBasicScopeContext parent,
        FerruleRetainedSourceCohort original, IFerruleBasicGroupStartingWithControls controls,
        Func<FerruleBasicScopeContext, FerruleInstance> target)
    {
        ArgumentNullException.ThrowIfNull(original); ArgumentNullException.ThrowIfNull(controls); ArgumentNullException.ThrowIfNull(target);
        var operation = PrepareOperation(parent, original, null, controls);
        return operation.Execute(parent, null, controls, target, BasicGroupPartition.StartingWith);
    }
    // Borrow exactly one existing controls interface. No adapter, boxing, retained
    // callbacks or new operation fields are needed to share authentication/storage.
    private static FerruleBasicGroupByOperation PrepareOperation(FerruleBasicScopeContext parent,
        FerruleRetainedSourceCohort original, IFerruleBasicGroupByControls? keyed,
        IFerruleBasicGroupStartingWithControls? starting)
    {
        BasicSourceBudget budget;
        try { budget = Seed(parent, original); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
        // Caller metadata/accessors and semantic callbacks remain outside lifting.
        var metadata = keyed is not null ? keyed.Metadata() : starting!.Metadata();
        ArgumentNullException.ThrowIfNull(metadata.Descending);
        var keyCount = metadata.Descending.Count;
        FerruleBasicGroupByOperation operation;
        try { operation = Prepare(original, budget, metadata, keyCount); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
        for (var i = 0; i < keyCount; i++) operation._descending[i] = metadata.Descending[i];
        return operation;
    }
#if FERRULE_COMPONENT_ORACLES
    internal static (
        IReadOnlyList<FerruleInstance> Output,
        (int, int, int, int, int, int, int, int, int, int, int, int) Capacities,
        bool KeysInitiallyNull, bool KeysFinallyNull)
    RunStartingWithSnapshotForTests(FerruleBasicScopeContext parent,
        FerruleRetainedSourceCohort original, IFerruleBasicGroupStartingWithControls controls,
        Func<FerruleBasicScopeContext, FerruleInstance> target)
    {
        ArgumentNullException.ThrowIfNull(original); ArgumentNullException.ThrowIfNull(controls); ArgumentNullException.ThrowIfNull(target);
        // This is the public Starting route's real preparation and execution on
        // ONE object. The tuple observes actual backing, not recomputed accounting.
        var operation = PrepareOperation(parent, original, null, controls);
        var initiallyNull = operation.GroupKeysNullForTests();
        var output = operation.Execute(parent, null, controls, target, BasicGroupPartition.StartingWith);
        return (output, (operation._descending.Length, operation._sortKeys.Length, operation._windows.Length,
            operation._order.Length, operation._scratch.Length, operation._next.Length, operation._buckets.Length,
            operation._groupKeys.Length, operation._outputs.Length, operation._terminals.Length,
            operation._templates.Length, operation._selection.Length), initiallyNull, operation.GroupKeysNullForTests());
    }
    private bool GroupKeysNullForTests()
    {
        for (var i = 0; i < _groupKeys.Length; i++)
            if (_groupKeys[i] != FerruleValue.Null) return false;
        return true;
    }
#endif
    internal static BasicSourceBudget Seed(FerruleBasicScopeContext parent, FerruleRetainedSourceCohort original)
    {
        var n = original.Count; var limits = parent.Roots.Limits;
        BasicSourceAccounting.Limit(0, (nuint)n, limits.MaxWalkContexts, FerruleBasicSourceBridgeComponent.WalkContexts);
        var maximum = BasicGroupAccounting.Maximum(limits);
        // Stateless admission precedes allocation of BOTH ledger counter objects.
        BasicGroupAccounting.AdmitLedgerSetup(maximum);
        var budget = new BasicSourceBudget(limits, maximum);
        budget.Charge((nuint)BasicGroupAccounting.LedgerSetupRecords);
        parent.Roots.Seed(budget); parent.Seed(budget);
        var witness = original.SourceWitness;
        if (!ReferenceEquals(parent.Roots, witness.Parent.Roots))
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.RegistryMismatch);
        if (!ReferenceEquals(parent, witness.Parent) || !witness.IsComplete) throw BasicPhaseAdmission.Invalid(0);
        FerruleBasicSortOperation.ValidateOriginalIds(witness.OriginalCount, original);
        for (var i = 0; i < n; i++)
        {
            var c = original[i];
            if (!ReferenceEquals(c.SourceWitness, witness) || !ReferenceEquals(c.Context.Roots, parent.Roots))
                throw BasicPhaseAdmission.Invalid((nuint)i);
            ValidateTerminal(c, (nuint)i);
        }
        budget.Charge(BasicGroupAccounting.Add((nuint)original.RetainedCapacity, 2));
        for (var i = 0; i < n; i++) { original[i].Context.Seed(budget); budget.Charge(1); }
        return budget;
    }
    internal static void ValidateTerminal(FerruleBasicSourceCandidate candidate, nuint input)
    {
        if (candidate.GroupTerminal is not { } t) throw BasicPhaseAdmission.Invalid(input);
        var c = candidate.Context;
        if (t.Item is null || t.Canonical is null || t.Raw is null ||
            t.Owner < 0 || (nuint)t.Owner >= c.Roots.OwnedRoots.RegistryCount || t.RawCount < 0 || t.RawOffset < 0 ||
            t.OwnedCut < 0 || t.OwnedCut > c.OwnedView.FrameCount || c.OwnedView.FrameCount - t.OwnedCut > 1 ||
            t.LegacyCut < 0 || t.LegacyCut > c.LegacyView.Frames.Count || c.LegacyView.Frames.Count - t.LegacyCut > 1 ||
            t.OriginCut != t.LegacyCut || c.OriginCount != c.LegacyView.Frames.Count ||
            t.CollectionCut < 0 || t.CollectionCut > c.LegacyView.BasicCollectionCount || c.LegacyView.BasicCollectionCount - t.CollectionCut > 1 ||
            (t.RawCount == 0) != (t.Wrapper is null) ||
            (t.Alias is not null && t.RawCount == 0) ||
            (nuint)t.RawOffset + (nuint)t.RawCount - (t.Alias is null ? (nuint)0 : 1) > (nuint)t.Raw.Count)
            throw BasicPhaseAdmission.Invalid(input);
        if (t.RawCount != 0 && !StringComparer.Ordinal.Equals(t.Wrapper, t.RawAt(t.RawCount - 1)))
            throw BasicPhaseAdmission.Invalid(input);
        var replacesOwned = t.OwnedCut < c.OwnedView.FrameCount;
        var replacesLegacy = t.LegacyCut < c.LegacyView.Frames.Count;
        var replacesCollection = t.CollectionCut < c.LegacyView.BasicCollectionCount;
        var parent = candidate.SourceWitness.Parent;
        if (replacesOwned != replacesCollection || (replacesOwned && !replacesLegacy))
            throw BasicPhaseAdmission.Invalid(input);
        if (t.OwnedCut < parent.OwnedView.FrameCount || t.LegacyCut < parent.LegacyView.Frames.Count ||
            t.OriginCut < parent.OriginCount || t.CollectionCut < parent.LegacyView.BasicCollectionCount ||
            (!replacesLegacy && !ReferenceEquals(c.LegacyView, parent.LegacyView)))
            throw BasicPhaseAdmission.Invalid(input);
        if (!replacesLegacy && (t.Alias is not null || t.RawOffset != 0 ||
            t.RawCount != t.Canonical.Count || !SamePath(t.Raw, t.Canonical)))
            throw BasicPhaseAdmission.Invalid(input);
        if (t.LegacyCut < c.LegacyView.Frames.Count && !ReferenceEquals(c.LegacyView.Frames[t.LegacyCut], t.Item))
            throw BasicPhaseAdmission.Invalid(input);
        if (replacesLegacy)
        {
            var origin = c.OriginAt(t.OriginCut);
            if (origin.OwnerKey != t.Owner || origin.Projection || !SamePath(origin.Canonical, t.Canonical))
                throw BasicPhaseAdmission.Invalid(input);
        }
        if (t.OwnedCut < c.OwnedView.FrameCount)
        {
            var f = c.OwnedView.FrameAt(t.OwnedCut);
            if (f.Owner != t.Owner || !ReferenceEquals(f.Item, t.Item) || !SamePath(f.Path, t.Canonical))
                throw BasicPhaseAdmission.Invalid(input);
        }
        if (replacesCollection)
        {
            var association = c.LegacyView.BasicCollectionAt(t.CollectionCut);
            if (!ReferenceEquals(association.Item, t.Item) || association.Path.Count != t.RawCount)
                throw BasicPhaseAdmission.Invalid(input);
            for (var i = 0; i < t.RawCount; i++)
                if (!StringComparer.Ordinal.Equals(association.Path[i], t.RawAt(i))) throw BasicPhaseAdmission.Invalid(input);
        }
        else
        {
            // A plain/identity raw path is a canonical suffix, optionally preceded
            // by the actual exact named-root alias, never an ownership guess.
            var alias = t.Alias is null ? 0 : 1;
            if (alias != 0 && (t.Owner == 0 || !StringComparer.Ordinal.Equals(t.Alias, c.Roots.OwnedRoots.Name(t.Owner))))
                throw BasicPhaseAdmission.Invalid(input);
            var body = t.RawCount - alias;
            if (body > t.Canonical.Count) throw BasicPhaseAdmission.Invalid(input);
            for (var i = 0; i < body; i++)
                if (!StringComparer.Ordinal.Equals(t.RawAt(i + alias), t.Canonical[t.Canonical.Count - body + i]))
                    throw BasicPhaseAdmission.Invalid(input);
        }
    }
    private static bool SamePath(IReadOnlyList<string> a, IReadOnlyList<string> b)
    {
        if (a.Count != b.Count) return false;
        for (var i = 0; i < a.Count; i++) if (!StringComparer.Ordinal.Equals(a[i], b[i])) return false;
        return true;
    }
    private static FerruleBasicGroupByOperation Prepare(FerruleRetainedSourceCohort original, BasicSourceBudget budget,
        FerruleBasicGroupByMetadata metadata, int keys)
    {
        var cost = Footprint(original, metadata, keys, out var keyCapacity, out var windowCapacity);
        budget.Charge(cost.Slots); // slots win over known UTF8 at this new frontier
        budget.Text(cost.Bytes);
        return new(original, budget, metadata, keys, keyCapacity, windowCapacity);
    }
    internal static BasicSortCopyCost Footprint(FerruleRetainedSourceCohort original,
        FerruleBasicGroupByMetadata metadata, int keys, out int keyCapacity, out int windowCapacity)
    {
        if (keys < 0 || metadata.ExplicitWindowCount < 0) throw BasicSourceAccounting.Overflow();
        var n = (nuint)original.Count; var s = keys == 0 ? (nuint)0 : (nuint)1;
        var h = BasicGroupAccounting.Add((nuint)metadata.ExplicitWindowCount, metadata.ImplicitFirst ? (nuint)1 : 0);
        var cells = BasicGroupAccounting.Mul(n, (nuint)keys);
        keyCapacity = BasicGroupAccounting.Capacity<FerruleValue>(cells);
        windowCapacity = BasicGroupAccounting.Capacity<FerruleSequenceWindow>(h);
        _ = BasicGroupAccounting.Capacity<bool>((nuint)keys);
        _ = BasicGroupAccounting.Capacity<int>(n);
        _ = BasicGroupAccounting.Capacity<Bucket>(n);
        _ = BasicGroupAccounting.Capacity<FerruleValue>(n);
        _ = BasicGroupAccounting.Capacity<FerruleInstance>(n);
        _ = BasicGroupAccounting.Capacity<BasicGroupTerminal>(n);
        _ = BasicGroupAccounting.Capacity<BasicGroupPrefixTemplate>(n);
        if (s != 0) _ = BasicGroupAccounting.Capacity<FerruleBasicScopeContext>(n);
        // k+nk+3h +(5+s)n + n +(3n+4a) +3 +sum T +s sum U.
        nuint slots = BasicGroupAccounting.Add(BasicGroupAccounting.Add((nuint)keys, cells), BasicGroupAccounting.Mul(3, h));
        slots = BasicGroupAccounting.Add(slots, BasicGroupAccounting.Mul(BasicGroupAccounting.Add(9, s), n));
        slots = BasicGroupAccounting.Add(slots, 3);
        nuint bytes = 0;
        for (var i = 0; i < original.Count; i++)
        {
            var c = original[i]; var t = c.GroupTerminal!.Value;
            var cost = BasicGroupPrefixTemplate.Cost(c.Context, t);
            slots = BasicGroupAccounting.Add(slots, cost.Slots);
            bytes = BasicGroupAccounting.Add(bytes, cost.Bytes);
            if (t.Wrapper is not null) slots = BasicGroupAccounting.Add(slots, 4);
            if (s != 0)
            {
                cost = BasicGroupPrefixTemplate.SelectionCost(c.Context);
                slots = BasicGroupAccounting.Add(slots, cost.Slots);
                bytes = BasicGroupAccounting.Add(bytes, cost.Bytes);
            }
        }
        return new(slots, bytes);
    }
    private void AdmitKey(FerruleValue value)
    {
        if (value.Kind != FerruleValueKind.String) return;
        try { _budget.Text(BasicGroupAccounting.Text(value.StringValue)); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
    private IReadOnlyList<FerruleInstance> Execute(FerruleBasicScopeContext parent,
        IFerruleBasicGroupByControls? keyed, IFerruleBasicGroupStartingWithControls? starting,
        Func<FerruleBasicScopeContext, FerruleInstance> target, BasicGroupPartition partition)
    {
        var sorted = _descending.Length != 0;
        var prefilter = sorted && _metadata.FilterBeforeSort;
        var count = 0;
        for (var i = 0; i < _original.Count; i++)
            if (!prefilter || !_metadata.HasFilter || (keyed is not null ? keyed.Filter(_original[i].Context) :
                starting!.Filter(_original[i].Context))) _order[count++] = i;
        if (sorted)
        {
            for (var row = 0; row < count; row++)
            {
                var id = _order[row];
                for (var key = 0; key < _descending.Length; key++)
                {
                    var value = keyed is not null ? keyed.SortKey(key, _original[id].Context) : starting!.SortKey(key, _original[id].Context);
                    AdmitKey(value);
                    _sortKeys[checked(id * _descending.Length + key)] = value;
                }
            }
            Sort(count);
            for (var rank = 0; rank < count; rank++)
            {
                var id = _order[rank]; var cursor = _original[id].Cursor;
                if (cursor.HasValue)
                {
                    _selection[id].OwnedView.SetAdmittedRank(cursor.Value.OwnedSlot, (nuint)rank + 1);
                    _selection[id].LegacyView.SetAdmittedRank(cursor.Value.LegacySlot, rank + 1);
                    _templates[id].SetSelectionRank(cursor.Value, rank + 1);
                }
            }
        }
        for (var i = 0; i < _metadata.ExplicitWindowCount; i++)
            _windows[i] = keyed is not null ? keyed.Window(i, parent) : starting!.Window(i, parent);
        if (_metadata.ImplicitFirst) _windows[^1] = FerruleSequenceWindow.First(1);
        var buckets = 0;
        for (var row = 0; row < count; row++)
        {
            var id = _order[row]; var c = sorted ? _selection[id] : _original[id].Context;
            if (!prefilter && _metadata.HasFilter && !(keyed is not null ? keyed.Filter(c) : starting!.Filter(c))) continue;
            var key = FerruleValue.Null;
            var starts = false;
            if (partition == BasicGroupPartition.StartingWith)
                starts = starting!.StartsGroup(c); // including the first candidate; before post
            else
            {
                key = keyed!.GroupKey(c); AdmitKey(key);
            }
            var retain = !_metadata.HasPostFilter || (keyed is not null ? keyed.RetainMember(c) : starting!.RetainMember(c));
            var bucket = buckets;
            if (partition == BasicGroupPartition.StartingWith)
            {
                // False keeps the last constructed run (or creates the initial
                // false prefix); true starts a new run containing this member.
                if (!starts && buckets != 0) bucket = buckets - 1;
            }
            else if (partition == BasicGroupPartition.AdjacentBy)
            {
                // Compare the last CREATED run, including an all-false run. Retention
                // is compacted only after partitioning, so dropped separators survive
                // as boundaries. FerruleValue equality preserves scalar tags and NaN.
                if (buckets != 0 && _groupKeys[buckets - 1] == key) bucket = buckets - 1;
            }
            else
            {
                bucket = 0;
                while (bucket < buckets && _groupKeys[bucket] != key) bucket++;
            }
            if (bucket == buckets)
            {
                // Starting retains the same prepaid n-key backing, Null-filled
                // and unused: no boundary vector, key comparison or key text.
                if (partition != BasicGroupPartition.StartingWith) _groupKeys[bucket] = key;
                _buckets[bucket] = new() { Head = id, Tail = id, Count = 1, Retained = retain };
                buckets++;
            }
            else
            {
                ref var b = ref _buckets[bucket]; _next[b.Tail] = id; b.Tail = id; b.Count++; b.Retained |= retain;
            }
        }
        var retained = 0;
        for (var i = 0; i < buckets; i++) if (_buckets[i].Retained) _order[retained++] = i;
        var start = 0; var length = retained;
        for (var i = 0; i < _windows.Length; i++) ApplyWindow(_windows[i], ref start, ref length);
        for (var i = 0; i < length; i++)
        {
            var b = _buckets[_order[start + i]]; var template = _templates[b.Head];
            if (b.Count > _remainingMembers) throw FerruleRuntimeException.FromBasicSourceBridge(BasicPhaseAdmission.Invalid((nuint)b.Head));
            _remainingMembers -= b.Count; template.Claim(b.Count);
            // At most n runs and sum(member counts) <= n, even for A,B,A,... .
            // Each run claims its first candidate's distinct prepaid template;
            // 3n member payload/view cells and 4a wrapper cells already cover
            // singleton worst case. No array is republished or appended afterward.
            var members = new FerruleInstance[b.Count]; var id = b.Head;
            for (var j = 0; j < members.Length; j++) { members[j] = _terminals[id].Item; id = _next[id]; }
            if (id != -1) throw FerruleRuntimeException.FromBasicSourceBridge(BasicPhaseAdmission.Invalid((nuint)b.Head));
            var payload = FerruleRepeated.FromBasicGroupMembers(members, template);
            var context = template.Finish(payload, i + 1);
            _outputs[i] = target(context) ?? throw new InvalidOperationException("Group target returned null.");
        }
        GC.KeepAlive(_original);
        return new OutputPrefix(_outputs, length);
    }
    private int Compare(int a, int b)
    {
        for (var k = 0; k < _descending.Length; k++)
        {
            var compared = FerruleSequences.CompareSortValues(_sortKeys[a * _descending.Length + k], _sortKeys[b * _descending.Length + k]);
            if (compared != 0) return _descending[k] ? -Math.Sign(compared) : Math.Sign(compared);
        }
        return 0;
    }
    private void Sort(int length)
    {
        var input = _order; var output = _scratch;
        for (var width = 1; width < length; width = width >= length - width ? length : width + width)
        {
            var offset = 0;
            while (offset < length)
            {
                var middle = offset + Math.Min(width, length - offset);
                var end = middle + Math.Min(width, length - middle);
                var left = offset; var right = middle;
                for (var i = offset; i < end; i++)
                    output[i] = left < middle && (right == end || Compare(input[left], input[right]) <= 0) ? input[left++] : input[right++];
                offset = end;
            }
            (input, output) = (output, input);
        }
        if (!ReferenceEquals(input, _order)) Array.Copy(input, _order, length);
    }
    private static void ApplyWindow(FerruleSequenceWindow window, ref int start, ref int length)
    {
        var size = (ulong)length;
        var skip = window.Kind switch
        {
            FerruleSequenceWindowKind.SkipFirst => Math.Min(window.FirstValue, size),
            FerruleSequenceWindowKind.First => 0UL,
            FerruleSequenceWindowKind.From or FerruleSequenceWindowKind.FromTo => Math.Min(window.FirstValue == 0 ? 0UL : window.FirstValue - 1, size),
            FerruleSequenceWindowKind.Last => size - Math.Min(window.FirstValue, size),
            _ => throw new ArgumentOutOfRangeException(nameof(window)),
        };
        var available = size - skip;
        var take = window.Kind switch
        {
            FerruleSequenceWindowKind.SkipFirst or FerruleSequenceWindowKind.From => available,
            FerruleSequenceWindowKind.First or FerruleSequenceWindowKind.Last => Math.Min(window.FirstValue, size),
            FerruleSequenceWindowKind.FromTo => Math.Min(window.LastValue > skip ? window.LastValue - skip : 0UL, available),
            _ => throw new ArgumentOutOfRangeException(nameof(window)),
        };
        start += (int)skip; length = (int)take;
    }
    private sealed class OutputPrefix(FerruleInstance[] items, int count) : IReadOnlyList<FerruleInstance>
    {
        public int Count => count;
        public FerruleInstance this[int index] => (uint)index < (uint)count ? items[index] : throw new ArgumentOutOfRangeException(nameof(index));
        public IEnumerator<FerruleInstance> GetEnumerator() { for (var i = 0; i < count; i++) yield return items[i]; }
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    }
}
