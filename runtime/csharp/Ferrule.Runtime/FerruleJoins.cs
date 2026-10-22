using System.Collections.ObjectModel;

namespace Ferrule.Runtime;

public enum FerruleJoinSourceCardinality
{
    Repeating,
    Singleton,
}

public sealed class FerruleJoinSource
{
    public FerruleJoinSource(
        IEnumerable<string> collection,
        FerruleJoinSourceCardinality cardinality = FerruleJoinSourceCardinality.Repeating)
    {
        ArgumentNullException.ThrowIfNull(collection);
        Collection = new ReadOnlyCollection<string>(collection.ToArray());
        ValidatePath(Collection, nameof(collection));
        Cardinality = cardinality;
    }

    public IReadOnlyList<string> Collection { get; }

    public FerruleJoinSourceCardinality Cardinality { get; }

    private static void ValidatePath(IReadOnlyList<string> path, string parameter)
    {
        for (var index = 0; index < path.Count; index++)
        {
            ArgumentNullException.ThrowIfNull(path[index], parameter);
        }
    }
}

public sealed class FerruleJoinKey
{
    public FerruleJoinKey(
        IEnumerable<string> leftCollection,
        IEnumerable<string> leftPath,
        IEnumerable<string> rightPath)
    {
        ArgumentNullException.ThrowIfNull(leftCollection);
        ArgumentNullException.ThrowIfNull(leftPath);
        ArgumentNullException.ThrowIfNull(rightPath);
        LeftCollection = new ReadOnlyCollection<string>(leftCollection.ToArray());
        LeftPath = new ReadOnlyCollection<string>(leftPath.ToArray());
        RightPath = new ReadOnlyCollection<string>(rightPath.ToArray());
        ValidatePath(LeftCollection, nameof(leftCollection));
        ValidatePath(LeftPath, nameof(leftPath));
        ValidatePath(RightPath, nameof(rightPath));
    }

    public IReadOnlyList<string> LeftCollection { get; }

    public IReadOnlyList<string> LeftPath { get; }

    public IReadOnlyList<string> RightPath { get; }

    private static void ValidatePath(IReadOnlyList<string> path, string parameter)
    {
        for (var index = 0; index < path.Count; index++)
        {
            ArgumentNullException.ThrowIfNull(path[index], parameter);
        }
    }
}

public sealed class FerruleJoinStage
{
    public FerruleJoinStage(FerruleJoinSource source, IEnumerable<FerruleJoinKey> conditions)
    {
        Source = source ?? throw new ArgumentNullException(nameof(source));
        ArgumentNullException.ThrowIfNull(conditions);
        var retained = conditions.ToArray();
        if (retained.Length == 0)
        {
            throw new ArgumentException(
                "A join stage requires at least one equality condition.",
                nameof(conditions));
        }
        if (retained.Any(condition => condition is null))
        {
            throw new ArgumentException("Join conditions cannot contain null.", nameof(conditions));
        }
        Conditions = new ReadOnlyCollection<FerruleJoinKey>(retained);
    }

    public FerruleJoinSource Source { get; }

    public IReadOnlyList<FerruleJoinKey> Conditions { get; }
}

public sealed class FerruleJoinPlan
{
    public FerruleJoinPlan(FerruleJoinSource first, IEnumerable<FerruleJoinStage> stages)
    {
        First = first ?? throw new ArgumentNullException(nameof(first));
        ArgumentNullException.ThrowIfNull(stages);
        var retained = stages.ToArray();
        if (retained.Length == 0)
        {
            throw new ArgumentException(
                "An inner join requires at least two sources.",
                nameof(stages));
        }
        if (retained.Any(stage => stage is null))
        {
            throw new ArgumentException("Join stages cannot contain null.", nameof(stages));
        }
        Stages = new ReadOnlyCollection<FerruleJoinStage>(retained);
    }

    public FerruleJoinSource First { get; }

    public IReadOnlyList<FerruleJoinStage> Stages { get; }
}

public sealed partial class ScopeContext
{
    public IReadOnlyList<ScopeContext> InnerJoin(ulong join, FerruleJoinPlan plan)
    {
        ArgumentNullException.ThrowIfNull(plan);
        var rows = JoinSourceRows(join, plan.First);
        foreach (var stage in plan.Stages)
        {
            var rightRows = JoinSourceRows(join, stage.Source);
            var joined = new List<JoinRow>();
            foreach (var left in rows)
            {
                foreach (var right in rightRows)
                {
                    if (JoinConditionsMatch(left, right, stage.Conditions))
                    {
                        joined.Add(left.Append(right));
                    }
                }
            }
            rows = joined;
        }

        var contexts = new ScopeContext[rows.Count];
        for (var index = 0; index < rows.Count; index++)
        {
            var row = rows[index].WithJoinPosition(index + 1);
            var frames = new List<FerruleInstance>(_frames.Count + row.Frames.Count);
            frames.AddRange(_frames);
            frames.AddRange(row.Frames);
            var collections = new List<CollectionIdentity>(_collections.Count + row.Collections.Count);
            collections.AddRange(_collections);
            collections.AddRange(row.Collections);
            contexts[index] = new ScopeContext(
                _primarySource,
                new ReadOnlyCollection<FerruleInstance>(frames),
                new ReadOnlyCollection<CollectionIdentity>(collections),
                _executionContext,
                _dynamicSourceLoader, invocationCounters: _invocationCounters, documentFilenameClock: _documentFilenameClock);
        }
        return new ReadOnlyCollection<ScopeContext>(contexts);
    }

    public FerruleValue ResolveJoinScalar(
        ulong join,
        IReadOnlyList<string> collection,
        IReadOnlyList<string> path)
    {
        ArgumentNullException.ThrowIfNull(collection);
        ArgumentNullException.ThrowIfNull(path);
        ValidatePath(collection);
        ValidatePath(path);
        for (var index = _collections.Count - 1; index >= 0; index--)
        {
            var active = _collections[index];
            if (active.Join != join || !SamePath(collection, active.Path))
            {
                continue;
            }
            if (TryResolveExactScalar(active.Item, path, out var value))
            {
                return value;
            }
            break;
        }
        var display = collection.Count == 0 ? "<current>" : string.Join('/', collection);
        throw new FerruleRuntimeException(
            FerruleRuntimeError.MissingSourceField,
            $"Join {join} source scalar '{display}/{string.Join('/', path)}' does not exist.",
            join: join);
    }

    public long JoinPosition(ulong join)
    {
        for (var index = _collections.Count - 1; index >= 0; index--)
        {
            var active = _collections[index];
            if (active.Join == join && active.JoinPosition.HasValue)
            {
                return active.JoinPosition.Value;
            }
        }
        throw new FerruleRuntimeException(
            FerruleRuntimeError.MissingJoinContext,
            $"Join {join} is not active in this scope context.",
            detail: join.ToString(System.Globalization.CultureInfo.InvariantCulture),
            join: join);
    }

    private List<JoinRow> JoinSourceRows(ulong join, FerruleJoinSource source)
    {
        var candidates = IterateSource(source.Collection);
        var rows = new List<JoinRow>(candidates.Count);
        foreach (var candidate in candidates)
        {
            var addedFrames = candidate._frames.Skip(_frames.Count).ToArray();
            var addedCollections = candidate._collections
                .Skip(_collections.Count)
                .Select(collection => collection with { Join = join, JoinPosition = null })
                .ToArray();
            if (addedCollections.Length == 0 && addedFrames.Length != 0)
            {
                addedCollections = new[]
                {
                    new CollectionIdentity(source.Collection, addedFrames[^1], 1, join),
                };
            }
            rows.Add(new JoinRow(addedFrames, addedCollections));
        }
        return rows;
    }

    private static bool JoinConditionsMatch(
        JoinRow left,
        JoinRow right,
        IReadOnlyList<FerruleJoinKey> conditions)
    {
        foreach (var condition in conditions)
        {
            if (!left.TryScalar(condition.LeftCollection, condition.LeftPath, out var leftValue) ||
                !right.TryTerminalScalar(condition.RightPath, out var rightValue) ||
                IsNullLike(leftValue) ||
                IsNullLike(rightValue))
            {
                return false;
            }
            var equal = FerruleFunctions.Call("equal", new[] { leftValue, rightValue });
            if (equal.Kind != FerruleValueKind.Bool || !equal.BooleanValue)
            {
                return false;
            }
        }
        return true;
    }

    private static bool IsNullLike(FerruleValue value) =>
        value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull or FerruleValueKind.XmlNil;

    private sealed record JoinRow(
        IReadOnlyList<FerruleInstance> Frames,
        IReadOnlyList<CollectionIdentity> Collections)
    {
        internal JoinRow Append(JoinRow right) =>
            new(Frames.Concat(right.Frames).ToArray(), Collections.Concat(right.Collections).ToArray());

        internal JoinRow WithJoinPosition(int position)
        {
            var collections = Collections.ToArray();
            if (collections.Length != 0)
            {
                collections[^1] = collections[^1] with { JoinPosition = position };
            }
            return new JoinRow(Frames, collections);
        }

        internal bool TryScalar(
            IReadOnlyList<string> collection,
            IReadOnlyList<string> path,
            out FerruleValue value)
        {
            for (var index = Collections.Count - 1; index >= 0; index--)
            {
                if (SamePath(collection, Collections[index].Path))
                {
                    return TryResolveExactScalar(Collections[index].Item, path, out value);
                }
            }
            value = FerruleValue.Null;
            return false;
        }

        internal bool TryTerminalScalar(IReadOnlyList<string> path, out FerruleValue value)
        {
            if (Frames.Count != 0)
            {
                return TryResolveExactScalar(Frames[^1], path, out value);
            }
            value = FerruleValue.Null;
            return false;
        }
    }
}

/// <summary>One source of an owned join plan, in plan order.</summary>
public sealed class FerruleOwnedJoinSource
{
    public FerruleOwnedJoinSource(FerruleOwnedSourceOwner owner, IEnumerable<string> collection, bool singleton = false)
    {
        ArgumentNullException.ThrowIfNull(collection);
        Owner = owner;
        Collection = new ReadOnlyCollection<string>(collection.ToArray());
        if (Collection.Any(segment => segment is null))
            throw new ArgumentException("Owned join collection segments cannot be null.", nameof(collection));
        Singleton = singleton;
    }

    public FerruleOwnedSourceOwner Owner { get; }

    public IReadOnlyList<string> Collection { get; }

    public bool Singleton { get; }
}

/// <summary>An equality between an earlier member, by plan index, and a stage's new member.</summary>
public sealed class FerruleOwnedJoinKey
{
    public FerruleOwnedJoinKey(int left, IEnumerable<string> leftPath, IEnumerable<string> rightPath)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(left);
        ArgumentNullException.ThrowIfNull(leftPath);
        ArgumentNullException.ThrowIfNull(rightPath);
        Left = left;
        LeftPath = new ReadOnlyCollection<string>(leftPath.ToArray());
        RightPath = new ReadOnlyCollection<string>(rightPath.ToArray());
    }

    public int Left { get; }

    public IReadOnlyList<string> LeftPath { get; }

    public IReadOnlyList<string> RightPath { get; }
}

/// <summary>One source after the first in a left-deep owned join.</summary>
public sealed class FerruleOwnedJoinStage
{
    public FerruleOwnedJoinStage(FerruleOwnedJoinSource source, IEnumerable<FerruleOwnedJoinKey> keys)
    {
        Source = source ?? throw new ArgumentNullException(nameof(source));
        ArgumentNullException.ThrowIfNull(keys);
        var retained = keys.ToArray();
        if (retained.Length == 0 || retained.Any(key => key is null))
            throw new ArgumentException("An owned join stage requires non-null equality keys.", nameof(keys));
        Keys = new ReadOnlyCollection<FerruleOwnedJoinKey>(retained);
    }

    public FerruleOwnedJoinSource Source { get; }

    public IReadOnlyList<FerruleOwnedJoinKey> Keys { get; }
}

/// <summary>
/// Owned inner joins over the paired Basic context. Each source walks exactly
/// its declared endpoint, anchored by the latest active same-owner frame, so a
/// source equal to an active owned row is that row. Tuples keep left-major
/// order and duplicate multiplicity; members keep raw source positions while
/// the tuple carries its separately renumbered flattened position.
/// </summary>
public sealed partial class FerruleBasicScopeContext
{
    private sealed class OwnedJoinMember(FerruleOwnedJoinSource source, FerruleBasicScopeContext? item,
        FerruleValue value, nuint raw)
    {
        internal FerruleOwnedJoinSource Source { get; } = source;
        internal nuint Raw { get; } = raw;

        internal bool Is(FerruleOwnedSourceOwner owner, IReadOnlyList<string> collection) =>
            Source.Owner.Name == owner.Name && Source.Collection.SequenceEqual(collection, StringComparer.Ordinal);

        /// <summary>A singleton member is read with an empty path.</summary>
        internal FerruleValue Value(IReadOnlyList<string> relative)
        {
            if (item is null) return relative.Count == 0 ? value : FerruleValue.Null;
            var path = new string[Source.Collection.Count + relative.Count];
            for (var i = 0; i < Source.Collection.Count; i++) path[i] = Source.Collection[i];
            for (var i = 0; i < relative.Count; i++) path[Source.Collection.Count + i] = relative[i];
            return item.ResolveOwnedField(Source.Owner, path, null);
        }
    }

    private sealed class OwnedJoinTuple(ulong join, OwnedJoinMember[] members, int position)
    {
        internal ulong Join { get; } = join;
        internal OwnedJoinMember[] Members { get; } = members;
        internal int Position { get; } = position;
    }

    private OwnedJoinTuple? _ownedJoin;

    private List<OwnedJoinMember> OwnedJoinMembers(FerruleOwnedJoinSource source)
    {
        if (source.Singleton)
        {
            var value = ResolveOwnedField(source.Owner, source.Collection, null);
            return [new OwnedJoinMember(source, null, value, 1)];
        }
        var cohort = WalkOwnedSourceCohort(source.Owner, source.Collection);
        var members = new List<OwnedJoinMember>(cohort.Count);
        nuint? identity = null;
        foreach (var candidate in cohort)
        {
            // An identity selection is the active occurrence at its current
            // position; an endpoint root identity is position one.
            var raw = candidate.RawSourceOrdinal ?? (identity ??= IdentityPosition(source));
            members.Add(new OwnedJoinMember(source, candidate.Context, FerruleValue.Null, raw));
        }
        return members;
    }

    private nuint IdentityPosition(FerruleOwnedJoinSource source)
    {
        try { return OwnedPosition(source.Owner, source.Collection); }
        catch (FerruleRuntimeException) { return 1; }
    }

    private static bool NullLike(FerruleValue value) => value.Kind is FerruleValueKind.Null
        or FerruleValueKind.JsonNull or FerruleValueKind.XmlNil;

    private static bool KeysMatch(IReadOnlyList<OwnedJoinMember> left, OwnedJoinMember right,
        IReadOnlyList<FerruleOwnedJoinKey> keys)
    {
        foreach (var key in keys)
        {
            // Emitters index earlier members in plan order.
            if (key.Left >= left.Count) return false;
            var leftValue = left[key.Left].Value(key.LeftPath);
            var rightValue = right.Value(key.RightPath);
            if (NullLike(leftValue) || NullLike(rightValue)) return false;
            var equal = FerruleFunctions.Call("equal", new[] { leftValue, rightValue });
            if (equal.Kind != FerruleValueKind.Bool || !equal.BooleanValue) return false;
        }
        return true;
    }

    /// <summary>
    /// Executes one owned join in this receiving context. Every source of a stage
    /// is walked before its first comparison; the tuple count shares the walk-context limit.
    /// </summary>
    public IReadOnlyList<FerruleBasicScopeContext> OwnedInnerJoin(ulong join, FerruleOwnedJoinSource first,
        IReadOnlyList<FerruleOwnedJoinStage> stages)
    {
        ArgumentNullException.ThrowIfNull(first);
        ArgumentNullException.ThrowIfNull(stages);
        var rows = OwnedJoinMembers(first).Select(member => new List<OwnedJoinMember> { member }).ToList();
        nuint produced = 0;
        foreach (var stage in stages)
        {
            var right = OwnedJoinMembers(stage.Source);
            var joined = new List<List<OwnedJoinMember>>();
            foreach (var left in rows)
            {
                foreach (var member in right)
                {
                    if (!KeysMatch(left, member, stage.Keys)) continue;
                    try
                    {
                        OwnedSourceAccounting.Limit(produced, 1, Roots.Limits.MaxWalkContexts,
                            FerruleOwnedSourceLimitKind.WalkContexts);
                    }
                    catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
                    produced++;
                    joined.Add(new List<OwnedJoinMember>(left) { member });
                }
            }
            rows = joined;
        }
        var contexts = new FerruleBasicScopeContext[rows.Count];
        for (var index = 0; index < rows.Count; index++)
        {
            var context = CloneView();
            context._ownedJoin = new OwnedJoinTuple(join, rows[index].ToArray(), index + 1);
            contexts[index] = context;
        }
        return new ReadOnlyCollection<FerruleBasicScopeContext>(contexts);
    }

    private OwnedJoinMember FindOwnedJoinMember(ulong join, FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> collection)
    {
        ArgumentNullException.ThrowIfNull(collection);
        if (_ownedJoin is { } tuple && tuple.Join == join)
        {
            foreach (var member in tuple.Members)
                if (member.Is(owner, collection)) return member;
        }
        var display = collection.Count == 0 ? "<current>" : string.Join('/', collection);
        throw new FerruleRuntimeException(
            FerruleRuntimeError.MissingSourceField,
            $"Owned join {join} member '{display}' is not active in this scope context.",
            join: join);
    }

    /// <summary>Reads one exact tuple member, bound by its owner and collection.</summary>
    public FerruleValue OwnedJoinField(ulong join, FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> collection, IReadOnlyList<string> path)
    {
        ArgumentNullException.ThrowIfNull(path);
        return FindOwnedJoinMember(join, owner, collection).Value(path);
    }

    /// <summary>The raw one-based source position of one exact tuple member.</summary>
    public nuint OwnedJoinSourcePosition(ulong join, FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> collection) => FindOwnedJoinMember(join, owner, collection).Raw;

    /// <summary>The flattened one-based tuple position of the active owned join.</summary>
    public long OwnedJoinPosition(ulong join)
    {
        if (_ownedJoin is { } tuple && tuple.Join == join) return tuple.Position;
        throw new FerruleRuntimeException(
            FerruleRuntimeError.MissingJoinContext,
            $"Join {join} is not active in this scope context.",
            detail: join.ToString(System.Globalization.CultureInfo.InvariantCulture),
            join: join);
    }

    /// <summary>A view whose flattened tuple position is replaced; members are shared.</summary>
    public FerruleBasicScopeContext WithOwnedJoinPosition(ulong join, int position)
    {
        if (_ownedJoin is not { } tuple || tuple.Join != join)
        {
            _ = OwnedJoinPosition(join);
            throw new InvalidOperationException("Unreachable owned join position.");
        }
        var context = CloneView();
        context._ownedJoin = new OwnedJoinTuple(join, tuple.Members, position);
        return context;
    }
}
