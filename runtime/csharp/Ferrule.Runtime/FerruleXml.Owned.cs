namespace Ferrule.Runtime;

public sealed partial class FerruleBasicScopeContext
{
    /// <summary>
    /// Reads the complete element an owned XML serializer writes. Selection
    /// matches owned fields; generated code supplies the exact declared root's
    /// repetition metadata for an explicit empty pin. A missing element is a
    /// typed failure and is never repaired from another owner or legacy frame.
    /// </summary>
    public FerruleInstance ResolveOwnedXmlInstance(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> path, IReadOnlyList<string>? frame, bool declaredNonrepeating)
    {
        try
        {
            var budget = Begin();
            return _owned.ResolveBasicXmlInstance(owner, path, frame,
                Query(budget, owner, path, frame), declaredNonrepeating);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
}

public sealed partial class FerruleOwnedScopeContext
{
    internal FerruleInstance ResolveBasicXmlInstance(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> path, IReadOnlyList<string>? frame, OwnedQuery q, bool declaredNonrepeating)
    {
        var key = Roots.Resolve(owner, q);
        if (frame is not null && !Prefix(path, frame))
            throw q.Error(FerruleOwnedSourceErrorKind.IncompatibleFramePath, path, frame);
        var rootGroup = Match(key, Array.Empty<string>(), true);
        var registeredRoot = frame is { Count: 0 } &&
            ((declaredNonrepeating && Roots.Instance(key) is not FerruleRepeated) ||
              (rootGroup >= 0 && _frames[rootGroup].GroupMembers));
        var selected = registeredRoot ? -1 : Match(key, frame ?? path, frame is not null);
        if (frame is not null && selected < 0 && !registeredRoot)
            throw q.Error(FerruleOwnedSourceErrorKind.InactiveSourceFrame, collection: frame);
        var current = selected < 0 ? Roots.Instance(key) : _frames[selected].Item;
        var consumed = selected < 0 ? 0 : _frames[selected].Path.Count;
        if (selected >= 0 && _frames[selected].GroupMembers)
        {
            var members = (FerruleRepeated)current;
            if (members.Items.Count == 0) throw MissingXmlInstance(q, path, consumed, current);
            current = members.Items[0];
        }
        while (true)
        {
            if (current is FerruleRepeated)
                throw q.Error(FerruleOwnedSourceErrorKind.UniteratedRepetition, path, segment: (nuint)consumed);
            if (Unsupported(current))
                throw q.Error(FerruleOwnedSourceErrorKind.UnsupportedSourceKind, path, found: Kind(current));
            // Present groups, scalars and XML nil values reach the writer.
            if (consumed == path.Count) return current;
            if (Absent(current)) throw MissingXmlInstance(q, path, consumed, current);
            if (current is not FerruleGroup group)
                throw q.Error(FerruleOwnedSourceErrorKind.Traversal, path, segment: (nuint)consumed,
                    found: Kind(current), traversal: FerruleOwnedSourceTraversalKind.CannotTraverse);
            if (!group.TryGetField(path[consumed], out var next))
                throw MissingXmlInstance(q, path, consumed, current);
            current = next; consumed++;
        }
    }

    private static FerruleOwnedSourceException MissingXmlInstance(OwnedQuery q,
        IReadOnlyList<string> path, int consumed, FerruleInstance at) =>
        q.Error(FerruleOwnedSourceErrorKind.Traversal, path, segment: (nuint)consumed,
            found: Kind(at), traversal: FerruleOwnedSourceTraversalKind.MissingInstance);
}
