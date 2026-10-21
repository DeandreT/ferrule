namespace Ferrule.Runtime;

public sealed partial class FerruleBasicScopeContext
{
    /// <summary>
    /// Reads one runtime-named scalar property from the open object of exactly
    /// one owner and frame. The caller evaluates the key first; a non-string key
    /// reads nothing. Absent and structural properties are Null, and a present
    /// JSON null stays distinct. No other owner or frame supplies the object.
    /// </summary>
    public FerruleValue ResolveOwnedDynamicField(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> objectPath, IReadOnlyList<string>? frame, FerruleValue key,
        bool declaredNonrepeating)
    {
        ArgumentNullException.ThrowIfNull(objectPath);
        if (key.Kind != FerruleValueKind.String) return FerruleValue.Null;
        try
        {
            var budget = Begin();
            return _owned.ResolveBasicDynamicField(owner, objectPath, frame, key.StringValue,
                Query(budget, owner, objectPath, frame), declaredNonrepeating);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
}

public sealed partial class FerruleOwnedScopeContext
{
    // Selection mirrors ResolveFieldCore with the object path in place of the
    // scalar path; only the final property lookup keeps dynamic results.
    internal FerruleValue ResolveBasicDynamicField(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> objectPath, IReadOnlyList<string>? frame, string key,
        OwnedQuery q, bool declaredNonrepeating)
    {
        var ownerKey = Roots.Resolve(owner, q);
        if (frame is not null && !Prefix(objectPath, frame))
            throw q.Error(FerruleOwnedSourceErrorKind.IncompatibleFramePath, objectPath, frame);
        var rootGroup = Match(ownerKey, Array.Empty<string>(), true);
        var registeredRoot = frame is { Count: 0 } &&
            ((declaredNonrepeating && Roots.Instance(ownerKey) is not FerruleRepeated) ||
              (rootGroup >= 0 && _frames[rootGroup].GroupMembers));
        var selected = registeredRoot ? -1 : Match(ownerKey, frame ?? objectPath, frame is not null);
        if (frame is not null && selected < 0 && !registeredRoot)
            throw q.Error(FerruleOwnedSourceErrorKind.InactiveSourceFrame, collection: frame);
        var current = selected < 0 ? Roots.Instance(ownerKey) : _frames[selected].Item;
        var consumed = selected < 0 ? 0 : _frames[selected].Path.Count;
        if (selected >= 0 && _frames[selected].GroupMembers)
        {
            var members = (FerruleRepeated)current;
            if (members.Items.Count == 0) return FerruleValue.Null;
            current = members.Items[0];
        }
        while (true)
        {
            if (current is FerruleRepeated)
                throw q.Error(FerruleOwnedSourceErrorKind.UniteratedRepetition, objectPath, segment: (nuint)consumed);
            if (Unsupported(current))
                throw q.Error(FerruleOwnedSourceErrorKind.UnsupportedSourceKind, objectPath, found: Kind(current));
            if (Absent(current)) return FerruleValue.Null;
            if (current is not FerruleGroup group)
                throw q.Error(FerruleOwnedSourceErrorKind.Traversal, objectPath, segment: (nuint)consumed,
                    found: Kind(current), traversal: FerruleOwnedSourceTraversalKind.CannotTraverse);
            if (consumed == objectPath.Count)
                return group.TryGetField(key, out var property) && property is FerruleScalar scalar
                    ? scalar.Value
                    : FerruleValue.Null;
            if (!group.TryGetField(objectPath[consumed], out var next)) return FerruleValue.Null;
            current = next; consumed++;
        }
    }
}
