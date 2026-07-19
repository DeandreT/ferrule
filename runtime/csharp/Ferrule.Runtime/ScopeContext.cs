using System.Collections.ObjectModel;
using System.Diagnostics.CodeAnalysis;

namespace Ferrule.Runtime;

/// <summary>
/// Immutable source frames and active collection identities for one scope item.
/// </summary>
public sealed class ScopeContext
{
    private readonly IReadOnlyList<FerruleInstance> _frames;
    private readonly IReadOnlyList<CollectionIdentity> _collections;

    private ScopeContext(
        IReadOnlyList<FerruleInstance> frames,
        IReadOnlyList<CollectionIdentity> collections)
    {
        _frames = frames;
        _collections = collections;
    }

    public IReadOnlyList<FerruleInstance> Frames => _frames;

    public static ScopeContext FromSource(FerruleInstance source)
    {
        ArgumentNullException.ThrowIfNull(source);
        return new ScopeContext(
            new ReadOnlyCollection<FerruleInstance>(new[] { source }),
            Array.Empty<CollectionIdentity>());
    }

    /// <summary>
    /// Follows a source path and returns one context per flattened candidate.
    /// Repeated and document-set boundaries retain their collection identity.
    /// </summary>
    public IReadOnlyList<ScopeContext> IterateSource(params string[] path) =>
        IterateSource((IReadOnlyList<string>)path);

    public IReadOnlyList<ScopeContext> IterateSource(IReadOnlyList<string> path)
    {
        ArgumentNullException.ThrowIfNull(path);
        ValidatePath(path);

        var baseInstance = FindIterationBase(path);
        if (baseInstance is null)
        {
            return Array.Empty<ScopeContext>();
        }

        var output = new List<ScopeContext>();
        Walk(
            baseInstance,
            path,
            0,
            new List<string>(),
            new List<FerruleInstance>(),
            new List<CollectionIdentity>(),
            output);
        return new ReadOnlyCollection<ScopeContext>(output);
    }

    /// <summary>
    /// Returns the flattened source items reduced by an aggregate expression.
    /// A named path starts at the nearest frame that owns its first field. An
    /// empty path instead starts at the nearest repeated or document-set frame.
    /// Missing collections produce no items.
    /// </summary>
    public IReadOnlyList<ScopeContext> AggregateItems(params string[] path) =>
        AggregateItems((IReadOnlyList<string>)path);

    public IReadOnlyList<ScopeContext> AggregateItems(IReadOnlyList<string> path)
    {
        ArgumentNullException.ThrowIfNull(path);
        ValidatePath(path);

        var baseInstance = FindAggregateBase(path);
        if (baseInstance is null)
        {
            return Array.Empty<ScopeContext>();
        }

        var output = new List<ScopeContext>();
        Walk(
            baseInstance,
            path,
            0,
            new List<string>(),
            new List<FerruleInstance>(),
            new List<CollectionIdentity>(),
            output);
        return new ReadOnlyCollection<ScopeContext>(output);
    }

    /// <summary>
    /// Reads a scalar relative only to the terminal item selected by
    /// <see cref="AggregateItems(IReadOnlyList{string})"/>. Missing fields and
    /// structural terminal values become Null; there is no outward fallback or
    /// implicit first-item traversal through a repeated value.
    /// </summary>
    public FerruleValue AggregateCurrentScalar(params string[] path) =>
        AggregateCurrentScalar((IReadOnlyList<string>)path);

    public FerruleValue AggregateCurrentScalar(IReadOnlyList<string> path)
    {
        ArgumentNullException.ThrowIfNull(path);
        ValidatePath(path);
        if (_frames.Count == 0)
        {
            return FerruleValue.Null;
        }

        var current = _frames[^1];
        for (var index = 0; index < path.Count; index++)
        {
            if (!TryGetField(current, path[index], out var next))
            {
                return FerruleValue.Null;
            }
            current = next;
        }

        return current is FerruleScalar scalar ? scalar.Value : FerruleValue.Null;
    }

    /// <summary>
    /// Resolves an unframed scalar through active collection owners first,
    /// then through source frames from innermost to outermost.
    /// </summary>
    public FerruleValue ResolveScalar(params string[] path) =>
        ResolveScalar((IReadOnlyList<string>)path);

    public FerruleValue ResolveScalar(IReadOnlyList<string> path)
    {
        ArgumentNullException.ThrowIfNull(path);
        ValidatePath(path);

        for (var index = _collections.Count - 1; index >= 0; index--)
        {
            var collection = _collections[index];
            if (collection.Path.Count == 0 || !StartsWith(path, collection.Path))
            {
                continue;
            }

            var resolved = TryResolveScalar(
                collection.Item,
                path,
                collection.Path.Count);
            if (resolved.Found)
            {
                return resolved.Value;
            }
        }

        for (var index = _frames.Count - 1; index >= 0; index--)
        {
            var resolved = TryResolveScalar(_frames[index], path, 0);
            if (resolved.Found)
            {
                return resolved.Value;
            }
        }

        var display = path.Count == 0 ? "<current>" : string.Join('/', path);
        throw new FerruleRuntimeException(
            FerruleRuntimeError.MissingSourceField,
            $"Source scalar '{display}' does not exist in the active scope context.");
    }

    /// <summary>
    /// Resolves a scalar only inside the nearest active collection matching
    /// <paramref name="frame"/>, without outward fallback.
    /// </summary>
    public FerruleValue ResolveScalarInFrame(
        IReadOnlyList<string> frame,
        IReadOnlyList<string> path)
    {
        ArgumentNullException.ThrowIfNull(frame);
        ArgumentNullException.ThrowIfNull(path);
        ValidatePath(frame);
        ValidatePath(path);

        for (var index = _collections.Count - 1; index >= 0; index--)
        {
            var collection = _collections[index];
            if (!FrameMatches(frame, collection.Path))
            {
                continue;
            }

            var resolved = TryResolveScalar(collection.Item, path, 0);
            if (resolved.Found)
            {
                return resolved.Value;
            }
            break;
        }

        var fullPath = frame.Concat(path);
        throw new FerruleRuntimeException(
            FerruleRuntimeError.MissingSourceField,
            $"Framed source scalar '{string.Join('/', fullPath)}' does not exist in the active scope context.");
    }

    /// <summary>Returns the active collection's 1-based position, or 1.</summary>
    public long Position(params string[] collection) =>
        Position((IReadOnlyList<string>)collection);

    public long Position(IReadOnlyList<string> collection)
    {
        ArgumentNullException.ThrowIfNull(collection);
        ValidatePath(collection);
        for (var index = _collections.Count - 1; index >= 0; index--)
        {
            var active = _collections[index];
            if (collection.Count == 0 || EndsWith(active.Path, collection))
            {
                return active.Index;
            }
        }
        return 1;
    }

    /// <summary>
    /// Returns a context whose innermost active collection has a compacted
    /// output position. Source instances and all outer positions are retained.
    /// </summary>
    public ScopeContext WithCompactedPosition(int index)
    {
        if (index < 1)
        {
            throw new ArgumentOutOfRangeException(nameof(index));
        }
        if (_collections.Count == 0)
        {
            return this;
        }

        var collections = _collections.ToArray();
        collections[^1] = collections[^1] with { Index = index };
        return new ScopeContext(_frames, new ReadOnlyCollection<CollectionIdentity>(collections));
    }

    private FerruleInstance? FindIterationBase(IReadOnlyList<string> path)
    {
        if (path.Count == 0)
        {
            return _frames.Count == 0 ? null : _frames[^1];
        }

        for (var index = _frames.Count - 1; index >= 0; index--)
        {
            if (TryGetField(_frames[index], path[0], out _))
            {
                return _frames[index];
            }
        }

        return _frames.Count == 0 ? null : _frames[^1];
    }

    private FerruleInstance? FindAggregateBase(IReadOnlyList<string> path)
    {
        if (path.Count == 0)
        {
            for (var index = _frames.Count - 1; index >= 0; index--)
            {
                if (_frames[index] is FerruleRepeated or FerruleDocumentSet)
                {
                    return _frames[index];
                }
            }
            return null;
        }

        for (var index = _frames.Count - 1; index >= 0; index--)
        {
            if (TryGetField(_frames[index], path[0], out _))
            {
                return _frames[index];
            }
        }
        return null;
    }

    private void Walk(
        FerruleInstance current,
        IReadOnlyList<string> path,
        int pathIndex,
        List<string> prefix,
        List<FerruleInstance> frames,
        List<CollectionIdentity> collections,
        List<ScopeContext> output)
    {
        if (pathIndex < path.Count && current is FerruleRepeated leadingRepeated)
        {
            for (var index = 0; index < leadingRepeated.Items.Count; index++)
            {
                var item = leadingRepeated.Items[index];
                PushCollection(item, prefix, index + 1, frames, collections);
                Walk(item, path, pathIndex, prefix, frames, collections, output);
                PopCollection(frames, collections);
            }
            return;
        }

        if (pathIndex == path.Count)
        {
            switch (current)
            {
                case FerruleDocumentSet documents:
                    for (var index = 0; index < documents.Documents.Count; index++)
                    {
                        var document = documents.Documents[index];
                        PushCollection(document.Value, prefix, index + 1, frames, collections);
                        AddCandidate(frames, collections, output);
                        PopCollection(frames, collections);
                    }
                    return;
                case FerruleRepeated repeated:
                    for (var index = 0; index < repeated.Items.Count; index++)
                    {
                        var item = repeated.Items[index];
                        PushCollection(item, prefix, index + 1, frames, collections);
                        AddCandidate(frames, collections, output);
                        PopCollection(frames, collections);
                    }
                    return;
                default:
                    frames.Add(current);
                    AddCandidate(frames, collections, output);
                    frames.RemoveAt(frames.Count - 1);
                    return;
            }
        }

        if (current is FerruleDocumentSet documentSet)
        {
            for (var index = 0; index < documentSet.Documents.Count; index++)
            {
                var document = documentSet.Documents[index];
                PushCollection(document.Value, prefix, index + 1, frames, collections);
                Walk(document.Value, path, pathIndex, prefix, frames, collections, output);
                PopCollection(frames, collections);
            }
            return;
        }

        var nextPrefix = new List<string>(prefix) { path[pathIndex] };
        if (!TryGetField(current, path[pathIndex], out var next))
        {
            return;
        }

        if (next is FerruleRepeated repeatedField)
        {
            for (var index = 0; index < repeatedField.Items.Count; index++)
            {
                var item = repeatedField.Items[index];
                PushCollection(item, nextPrefix, index + 1, frames, collections);
                if (pathIndex + 1 == path.Count)
                {
                    AddCandidate(frames, collections, output);
                }
                else
                {
                    Walk(item, path, pathIndex + 1, nextPrefix, frames, collections, output);
                }
                PopCollection(frames, collections);
            }
            return;
        }

        Walk(next, path, pathIndex + 1, nextPrefix, frames, collections, output);
    }

    private void AddCandidate(
        IReadOnlyCollection<FerruleInstance> frames,
        IReadOnlyCollection<CollectionIdentity> collections,
        ICollection<ScopeContext> output)
    {
        var allFrames = new List<FerruleInstance>(_frames.Count + frames.Count);
        allFrames.AddRange(_frames);
        allFrames.AddRange(frames);
        var allCollections = new List<CollectionIdentity>(_collections.Count + collections.Count);
        allCollections.AddRange(_collections);
        allCollections.AddRange(collections);
        output.Add(new ScopeContext(
            new ReadOnlyCollection<FerruleInstance>(allFrames),
            new ReadOnlyCollection<CollectionIdentity>(allCollections)));
    }

    private static void PushCollection(
        FerruleInstance item,
        IReadOnlyList<string> path,
        int index,
        ICollection<FerruleInstance> frames,
        ICollection<CollectionIdentity> collections)
    {
        frames.Add(item);
        collections.Add(new CollectionIdentity(path.ToArray(), item, index));
    }

    private static void PopCollection(
        List<FerruleInstance> frames,
        List<CollectionIdentity> collections)
    {
        frames.RemoveAt(frames.Count - 1);
        collections.RemoveAt(collections.Count - 1);
    }

    private static ScalarResolution TryResolveScalar(
        FerruleInstance source,
        IReadOnlyList<string> path,
        int pathIndex)
    {
        var current = source;
        for (var index = pathIndex; index < path.Count; index++)
        {
            if (current is FerruleRepeated repeated)
            {
                if (repeated.Items.Count == 0)
                {
                    return ScalarResolution.Resolved(FerruleValue.Null);
                }
                current = repeated.Items[0];
            }

            if (!TryGetField(current, path[index], out var next))
            {
                return ScalarResolution.Missing;
            }
            current = next;
        }

        if (current is FerruleRepeated terminalRepeated)
        {
            if (terminalRepeated.Items.Count == 0)
            {
                return ScalarResolution.Resolved(FerruleValue.Null);
            }
            current = terminalRepeated.Items[0];
        }

        return current is FerruleScalar scalar
            ? ScalarResolution.Resolved(scalar.Value)
            : ScalarResolution.Missing;
    }

    private static bool TryGetField(
        FerruleInstance source,
        string name,
        [NotNullWhen(true)] out FerruleInstance? value)
    {
        switch (source)
        {
            case FerruleGroup group:
                return group.TryGetField(name, out value);
            case FerruleDocumentSet { Documents.Count: > 0 } documents:
                return TryGetField(documents.Documents[0].Value, name, out value);
            default:
                value = null;
                return false;
        }
    }

    private static bool StartsWith(
        IReadOnlyList<string> path,
        IReadOnlyList<string> prefix)
    {
        if (prefix.Count > path.Count)
        {
            return false;
        }
        for (var index = 0; index < prefix.Count; index++)
        {
            if (!string.Equals(path[index], prefix[index], StringComparison.Ordinal))
            {
                return false;
            }
        }
        return true;
    }

    private static bool FrameMatches(
        IReadOnlyList<string> frame,
        IReadOnlyList<string> active) =>
        SamePath(frame, active) || active.Count > 0 && EndsWith(frame, active);

    private static bool SamePath(
        IReadOnlyList<string> left,
        IReadOnlyList<string> right) =>
        left.Count == right.Count && EndsWith(left, right);

    private static bool EndsWith(
        IReadOnlyList<string> path,
        IReadOnlyList<string> suffix)
    {
        if (suffix.Count > path.Count)
        {
            return false;
        }
        var offset = path.Count - suffix.Count;
        for (var index = 0; index < suffix.Count; index++)
        {
            if (!string.Equals(path[offset + index], suffix[index], StringComparison.Ordinal))
            {
                return false;
            }
        }
        return true;
    }

    private static void ValidatePath(IReadOnlyList<string> path)
    {
        for (var index = 0; index < path.Count; index++)
        {
            ArgumentNullException.ThrowIfNull(path[index]);
        }
    }

    private sealed record CollectionIdentity(
        IReadOnlyList<string> Path,
        FerruleInstance Item,
        int Index);

    private readonly record struct ScalarResolution(bool Found, FerruleValue Value)
    {
        internal static ScalarResolution Missing => default;

        internal static ScalarResolution Resolved(FerruleValue value) => new(true, value);
    }
}
