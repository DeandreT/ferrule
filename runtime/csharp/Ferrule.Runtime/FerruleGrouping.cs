using System.Collections.ObjectModel;

namespace Ferrule.Runtime;

public sealed partial class ScopeContext
{
    /// <summary>
    /// Partitions candidates by exact tagged scalar key in first-seen order.
    /// </summary>
    public IReadOnlyList<ScopeContext> GroupBy(
        IReadOnlyList<ScopeContext> candidates,
        IReadOnlyList<string> sourcePath,
        Func<ScopeContext, FerruleValue> keySelector)
    {
        ValidateGroupingArguments(candidates, sourcePath);
        ArgumentNullException.ThrowIfNull(keySelector);

        var groups = new List<GroupBucket>();
        foreach (var candidate in candidates)
        {
            ArgumentNullException.ThrowIfNull(candidate);
            var key = keySelector(candidate);
            var existing = groups.Find(group => group.Key.HasValue && group.Key.Value == key);
            if (existing is null)
            {
                existing = new GroupBucket(key, CandidateMember(candidate, sourcePath));
                groups.Add(existing);
            }
            else
            {
                existing.Add(CandidateMember(candidate, sourcePath));
            }
        }
        return BuildGroupedContexts(groups, sourcePath);
    }

    /// <summary>
    /// Partitions consecutive candidates with the same exact tagged scalar
    /// key. A later occurrence of a previous key starts a new group.
    /// </summary>
    public IReadOnlyList<ScopeContext> GroupAdjacentBy(
        IReadOnlyList<ScopeContext> candidates,
        IReadOnlyList<string> sourcePath,
        Func<ScopeContext, FerruleValue> keySelector)
    {
        ValidateGroupingArguments(candidates, sourcePath);
        ArgumentNullException.ThrowIfNull(keySelector);

        var groups = new List<GroupBucket>();
        foreach (var candidate in candidates)
        {
            ArgumentNullException.ThrowIfNull(candidate);
            var key = keySelector(candidate);
            var current = groups.Count == 0 ? null : groups[^1];
            if (current is null || !current.Key.HasValue || current.Key.Value != key)
            {
                groups.Add(new GroupBucket(key, CandidateMember(candidate, sourcePath)));
            }
            else
            {
                current.Add(CandidateMember(candidate, sourcePath));
            }
        }
        return BuildGroupedContexts(groups, sourcePath);
    }

    /// <summary>
    /// Starts a contiguous group at every candidate whose predicate is true.
    /// A leading false candidate still creates the first group.
    /// </summary>
    public IReadOnlyList<ScopeContext> GroupStartingWith(
        IReadOnlyList<ScopeContext> candidates,
        IReadOnlyList<string> sourcePath,
        Func<ScopeContext, bool> startsGroup)
    {
        ValidateGroupingArguments(candidates, sourcePath);
        ArgumentNullException.ThrowIfNull(startsGroup);

        var groups = new List<GroupBucket>();
        foreach (var candidate in candidates)
        {
            ArgumentNullException.ThrowIfNull(candidate);
            var member = CandidateMember(candidate, sourcePath);
            var starts = startsGroup(candidate);
            if (groups.Count == 0 || starts)
            {
                groups.Add(new GroupBucket(null, member));
            }
            else
            {
                groups[^1].Add(member);
            }
        }
        return BuildGroupedContexts(groups, sourcePath);
    }

    /// <summary>
    /// Ends the current contiguous group after every candidate whose predicate
    /// is true. A trailing false candidate remains in the final group.
    /// </summary>
    public IReadOnlyList<ScopeContext> GroupEndingWith(
        IReadOnlyList<ScopeContext> candidates,
        IReadOnlyList<string> sourcePath,
        Func<ScopeContext, bool> endsGroup)
    {
        ValidateGroupingArguments(candidates, sourcePath);
        ArgumentNullException.ThrowIfNull(endsGroup);

        var groups = new List<GroupBucket>();
        var previousEndedGroup = true;
        foreach (var candidate in candidates)
        {
            ArgumentNullException.ThrowIfNull(candidate);
            var member = CandidateMember(candidate, sourcePath);
            if (previousEndedGroup)
            {
                groups.Add(new GroupBucket(null, member));
            }
            else
            {
                groups[^1].Add(member);
            }
            previousEndedGroup = endsGroup(candidate);
        }
        return BuildGroupedContexts(groups, sourcePath);
    }

    /// <summary>Partitions candidates into positive-sized contiguous blocks.</summary>
    public IReadOnlyList<ScopeContext> GroupIntoBlocks(
        IReadOnlyList<ScopeContext> candidates,
        IReadOnlyList<string> sourcePath,
        ulong size)
    {
        ValidateGroupingArguments(candidates, sourcePath);
        if (size == 0)
        {
            throw new ArgumentOutOfRangeException(nameof(size));
        }

        var groups = new List<GroupBucket>();
        foreach (var candidate in candidates)
        {
            ArgumentNullException.ThrowIfNull(candidate);
            var member = CandidateMember(candidate, sourcePath);
            if (groups.Count == 0 || (ulong)groups[^1].Members.Count >= size)
            {
                groups.Add(new GroupBucket(null, member));
            }
            else
            {
                groups[^1].Add(member);
            }
        }
        return BuildGroupedContexts(groups, sourcePath);
    }

    private void ValidateGroupingArguments(
        IReadOnlyList<ScopeContext> candidates,
        IReadOnlyList<string> sourcePath)
    {
        ArgumentNullException.ThrowIfNull(candidates);
        ArgumentNullException.ThrowIfNull(sourcePath);
        ValidatePath(sourcePath);
    }

    private GroupMember CandidateMember(
        ScopeContext candidate,
        IReadOnlyList<string> sourcePath)
    {
        if (candidate._frames.Count <= _frames.Count ||
            candidate._collections.Count <= _collections.Count)
        {
            throw new ArgumentException(
                "Grouped candidates must extend their parent scope context.",
                nameof(candidate));
        }

        var frames = candidate._frames.Skip(_frames.Count).ToArray();
        var collections = candidate._collections.Skip(_collections.Count).ToArray();
        var terminal = collections[^1];
        if (sourcePath.Count != 0 && !SamePath(terminal.Path, sourcePath))
        {
            throw new ArgumentException(
                "Grouped candidates must end in the declared source collection.",
                nameof(candidate));
        }
        return new GroupMember(
            frames[^1],
            frames[..^1],
            collections[..^1],
            terminal.Path,
            terminal.DocumentPath);
    }

    private IReadOnlyList<ScopeContext> BuildGroupedContexts(
        IReadOnlyList<GroupBucket> groups,
        IReadOnlyList<string> sourcePath)
    {
        var contexts = new ScopeContext[groups.Count];
        for (var index = 0; index < groups.Count; index++)
        {
            var group = groups[index];
            var members = new FerruleRepeated(group.Members);
            var frames = new List<FerruleInstance>(
                _frames.Count + group.IntermediateFrames.Count + 2);
            frames.AddRange(_frames);
            frames.AddRange(group.IntermediateFrames);
            if (sourcePath.Count != 0)
            {
                frames.Add(new FerruleGroup(new[]
                {
                    new FerruleField(sourcePath[^1], members),
                }));
            }
            frames.Add(members);

            var collections = new List<CollectionIdentity>(
                _collections.Count + group.IntermediateCollections.Count + 1);
            collections.AddRange(_collections);
            collections.AddRange(group.IntermediateCollections);
            collections.Add(new CollectionIdentity(
                group.CollectionPath,
                members,
                index + 1,
                Grouped: sourcePath.Count != 0,
                DocumentPath: group.DocumentPath));
            contexts[index] = new ScopeContext(
                new ReadOnlyCollection<FerruleInstance>(frames),
                new ReadOnlyCollection<CollectionIdentity>(collections),
                _executionContext);
        }
        return new ReadOnlyCollection<ScopeContext>(contexts);
    }

    private sealed class GroupBucket
    {
        internal GroupBucket(FerruleValue? key, GroupMember first)
        {
            Key = key;
            Members = new List<FerruleInstance> { first.Member };
            IntermediateFrames = first.IntermediateFrames;
            IntermediateCollections = first.IntermediateCollections;
            CollectionPath = first.CollectionPath;
            DocumentPath = first.DocumentPath;
        }

        internal FerruleValue? Key { get; }

        internal List<FerruleInstance> Members { get; }

        internal IReadOnlyList<FerruleInstance> IntermediateFrames { get; }

        internal IReadOnlyList<CollectionIdentity> IntermediateCollections { get; }

        internal IReadOnlyList<string> CollectionPath { get; }

        internal string? DocumentPath { get; }

        internal void Add(GroupMember member) => Members.Add(member.Member);
    }

    private sealed record GroupMember(
        FerruleInstance Member,
        IReadOnlyList<FerruleInstance> IntermediateFrames,
        IReadOnlyList<CollectionIdentity> IntermediateCollections,
        IReadOnlyList<string> CollectionPath,
        string? DocumentPath);
}
