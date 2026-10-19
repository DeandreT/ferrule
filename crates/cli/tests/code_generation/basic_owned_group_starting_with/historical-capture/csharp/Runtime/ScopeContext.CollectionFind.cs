using System.Collections.ObjectModel;

namespace Ferrule.Runtime;

public sealed partial class ScopeContext
{
    /// <summary>
    /// Enumerates the exact scalar or group candidates visited by a
    /// collection-find expression. Repeated values crossed at any depth
    /// retain their source positions. Document sets are not flattened.
    /// </summary>
    public IReadOnlyList<ScopeContext> CollectionFindItems(params string[] path) =>
        CollectionFindItems((IReadOnlyList<string>)path);

    public IReadOnlyList<ScopeContext> CollectionFindItems(IReadOnlyList<string> path)
    {
        ArgumentNullException.ThrowIfNull(path);
        ValidatePath(path);

        var root = FindCollectionFindRoot(path);
        if (root is null)
        {
            var display = path.Count == 0 ? "<current>" : string.Join('/', path);
            throw new FerruleRuntimeException(
                FerruleRuntimeError.MissingSourceField,
                $"Source collection '{display}' does not exist in the active scope context.");
        }

        var output = new List<ScopeContext>();
        VisitCollectionFind(
            root,
            path,
            0,
            new List<FerruleInstance>(),
            new List<CollectionIdentity>(),
            output);
        return new ReadOnlyCollection<ScopeContext>(output);
    }

    private FerruleInstance? FindCollectionFindRoot(IReadOnlyList<string> path)
    {
        for (var index = _frames.Count - 1; index >= 0; index--)
        {
            var candidate = _frames[index];
            if (path.Count == 0)
            {
                if (candidate is FerruleRepeated)
                {
                    return candidate;
                }
            }
            else if (TryGetField(candidate, path[0], out _))
            {
                return candidate;
            }
        }
        return null;
    }

    private void VisitCollectionFind(
        FerruleInstance current,
        IReadOnlyList<string> path,
        int consumed,
        List<FerruleInstance> frames,
        List<CollectionIdentity> collections,
        ICollection<ScopeContext> output)
    {
        if (current is FerruleRepeated repeated)
        {
            var collection = path.Take(consumed).ToArray();
            for (var index = 0; index < repeated.Items.Count; index++)
            {
                var item = repeated.Items[index];
                PushCollection(item, collection, index + 1, frames, collections);
                VisitCollectionFind(item, path, consumed, frames, collections, output);
                PopCollection(frames, collections);
            }
            return;
        }

        if (consumed < path.Count)
        {
            if (TryGetField(current, path[consumed], out var next))
            {
                VisitCollectionFind(next, path, consumed + 1, frames, collections, output);
            }
            return;
        }

        AddCandidate(frames, collections, output);
    }
}
