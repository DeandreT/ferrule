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
                new ReadOnlyCollection<FerruleInstance>(frames),
                new ReadOnlyCollection<CollectionIdentity>(collections),
                _executionContext);
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
        value.Kind is FerruleValueKind.Null or FerruleValueKind.XmlNil;

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
