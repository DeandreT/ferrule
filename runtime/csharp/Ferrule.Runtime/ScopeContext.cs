using System.Collections.ObjectModel;
using System.Diagnostics.CodeAnalysis;

namespace Ferrule.Runtime;

/// <summary>
/// Immutable source frames and active collection identities for one scope item.
/// </summary>
public sealed partial class ScopeContext
{
    public FerruleValue ResolveSourceRootXmlTypeEquals(uint node, string identity) =>
        FerruleValue.FromBoolean(FerrulePrimaryRoot.XmlTypeEquals(_primarySource, identity, node));

    public FerruleValue ResolveSourceRootField(uint node, IReadOnlyList<string> path) =>
        FerrulePrimaryRoot.Scalar(_primarySource, path, node);
    public FerruleValue ResolveRequiredSourceRootField(uint node, IReadOnlyList<string> path) =>
        FerrulePrimaryRoot.RequiredScalar(_primarySource, path, node);
    private readonly FerruleInstance _primarySource;
    private readonly IReadOnlyList<FerruleInstance> _frames;
    private readonly IReadOnlyList<CollectionIdentity> _collections;
    private readonly FerruleExecutionContext? _executionContext;
    private readonly IFerruleDynamicSourceLoader? _dynamicSourceLoader;

    private ScopeContext(
        FerruleInstance primarySource,
        IReadOnlyList<FerruleInstance> frames,
        IReadOnlyList<CollectionIdentity> collections,
        FerruleExecutionContext? executionContext,
        IFerruleDynamicSourceLoader? dynamicSourceLoader = null,
        FerruleFilterMapRunState? filterMapRunState = null,
        FerruleFilterMapBoundary? filterMapBoundary = null,
        int filterMapCallDepth = 0)
    {
        _primarySource = primarySource;
        _frames = frames;
        _collections = collections;
        _executionContext = executionContext;
        _dynamicSourceLoader = dynamicSourceLoader;
        _filterMapRunState = filterMapRunState ?? new FerruleFilterMapRunState(executionContext);
        _filterMapBoundary = filterMapBoundary;
        _filterMapCallDepth = filterMapCallDepth;
    }

    public IReadOnlyList<FerruleInstance> Frames => _frames;

    internal IFerruleDynamicSourceLoader? DynamicSourceLoader => _dynamicSourceLoader;

    /// <summary>Adds the host boundary used to resolve per-driver sources.</summary>
    public ScopeContext WithDynamicSourceLoader(IFerruleDynamicSourceLoader loader)
    {
        ArgumentNullException.ThrowIfNull(loader);
        return new ScopeContext(_primarySource, _frames, _collections, _executionContext, loader,
            _filterMapRunState, _filterMapBoundary, _filterMapCallDepth);
    }

    public static ScopeContext FromSource(
        FerruleInstance source,
        FerruleExecutionContext? executionContext = null)
    {
        ArgumentNullException.ThrowIfNull(source);
        return new ScopeContext(
            source,
            new ReadOnlyCollection<FerruleInstance>(new[] { source }),
            Array.Empty<CollectionIdentity>(),
            executionContext);
    }

    /// <summary>
    /// Creates a root context with one outer group of named inputs followed by
    /// the primary source. Inner scope items and the primary source therefore
    /// take precedence during ordinary outward fallback.
    /// </summary>
    public static ScopeContext FromSources(
        FerruleInstance source,
        IEnumerable<FerruleField> extraSources,
        FerruleExecutionContext? executionContext = null)
    {
        ArgumentNullException.ThrowIfNull(source);
        ArgumentNullException.ThrowIfNull(extraSources);
        var extras = new FerruleGroup(extraSources);
        return new ScopeContext(
            source,
            new ReadOnlyCollection<FerruleInstance>(new FerruleInstance[] { extras, source }),
            Array.Empty<CollectionIdentity>(),
            executionContext);
    }

    /// <summary>Returns one exact host value or a typed missing-context error.</summary>
    public FerruleValue ResolveRuntimeValue(FerruleRuntimeValue value)
    {
        var resolved = _executionContext?.GetValue(value);
        if (resolved is null)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.MissingRuntimeValue,
                $"execution context does not provide {value}",
                runtimeValue: value);
        }
        return FerruleValue.FromString(resolved);
    }

    /// <summary>Returns one named host scalar after declared-type coercion.</summary>
    public FerruleValue ResolveRuntimeParameter(
        uint node,
        string name,
        FerruleScalarType expected)
    {
        return FerruleExecutionContext.ResolveParameter(_executionContext, node, name, expected);
    }

    /// <summary>Uses the default only when the host omits the parameter.</summary>
    public FerruleValue ResolveRuntimeParameterOrDefault(
        uint node,
        string name,
        FerruleScalarType expected,
        Func<FerruleValue> defaultValue)
    {
        ArgumentNullException.ThrowIfNull(name);
        ArgumentNullException.ThrowIfNull(defaultValue);
        var value = _executionContext is not null &&
            _executionContext.TryGetParameter(name, out var supplied)
            ? supplied
            : defaultValue();
        if (!FerruleValueMaps.TryCoerce(value, expected, out var coerced))
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.RuntimeParameterType,
                $"node {node}: runtime parameter '{name}' expected {expected}, got {value.Kind}",
                node: node,
                foundKind: value.Kind,
                expectedScalarType: expected,
                runtimeParameter: name);
        }
        return coerced;
    }

    /// <summary>Returns the resolved path of the nearest active source document.</summary>
    public FerruleValue ResolveSourceDocumentPath()
    {
        for (var index = _collections.Count - 1; index >= 0; index--)
        {
            var path = _collections[index].DocumentPath;
            if (path is not null)
            {
                return FerruleValue.FromString(path);
            }
        }
        for (var index = _frames.Count - 1; index >= 0; index--)
        {
            if (_frames[index] is FerruleDocumentSet { Documents.Count: > 0 } documents)
            {
                var document = documents.Documents[0];
                return FerruleValue.FromString(document.ResolvedSourcePath ?? document.Path);
            }
        }
        throw new FerruleRuntimeException(
            FerruleRuntimeError.MissingSourceField,
            "No source document is active in the scope context.",
            detail: "<document-path>");
    }

    /// <summary>
    /// Reads one runtime-named scalar field from an open source object.
    /// Missing objects/properties, structural values, inactive frames, and
    /// non-string keys produce Null; present JSON null remains distinct.
    /// </summary>
    public FerruleValue ResolveDynamicScalar(
        IReadOnlyList<string>? frame,
        IReadOnlyList<string> objectPath,
        FerruleValue key)
    {
        ArgumentNullException.ThrowIfNull(objectPath);
        ValidatePath(objectPath);
        if (frame is not null)
        {
            ValidatePath(frame);
        }
        if (key.Kind != FerruleValueKind.String)
        {
            return FerruleValue.Null;
        }

        if (frame is not null)
        {
            for (var index = _collections.Count - 1; index >= 0; index--)
            {
                var collection = _collections[index];
                if (FrameMatches(frame, collection.Path))
                {
                    return TryResolveDynamicScalar(
                        collection.Item,
                        objectPath,
                        key.StringValue,
                        out var framedValue)
                        ? framedValue
                        : FerruleValue.Null;
                }
            }
            return FerruleValue.Null;
        }

        for (var index = _frames.Count - 1; index >= 0; index--)
        {
            if (TryResolveDynamicScalar(
                _frames[index],
                objectPath,
                key.StringValue,
                out var value))
            {
                return value;
            }
        }
        return FerruleValue.Null;
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

        var prefix = path.Count == 0 && _collections.Count != 0 &&
            _collections[^1].Grouped &&
            ReferenceEquals(_collections[^1].Item, baseInstance)
                ? new List<string>(_collections[^1].Path)
                : new List<string>();
        var output = new List<ScopeContext>();
        Walk(
            baseInstance,
            path,
            0,
            prefix,
            new List<FerruleInstance>(),
            new List<CollectionIdentity>(),
            output);
        return new ReadOnlyCollection<ScopeContext>(output);
    }

    internal IReadOnlyList<ScopeContext> IterateLoadedSource(
        string source,
        FerruleInstance document,
        IReadOnlyList<string> tail)
    {
        ArgumentException.ThrowIfNullOrEmpty(source);
        ArgumentNullException.ThrowIfNull(document);
        ArgumentNullException.ThrowIfNull(tail);
        ValidatePath(tail);
        var output = new List<ScopeContext>();
        Walk(
            document,
            tail,
            0,
            new List<string> { source },
            new List<FerruleInstance>(),
            new List<CollectionIdentity>(),
            output);
        return new ReadOnlyCollection<ScopeContext>(output);
    }

    /// <summary>
    /// Appends generated scalar items as an empty-path collection while
    /// retaining every parent frame and collection identity.
    /// </summary>
    public IReadOnlyList<ScopeContext> IterateGenerated(IReadOnlyList<FerruleValue> values)
    {
        ArgumentNullException.ThrowIfNull(values);
        var output = new ScopeContext[values.Count];
        for (var index = 0; index < values.Count; index++)
        {
            output[index] = GeneratedContext(values[index], index + 1);
        }
        return new ReadOnlyCollection<ScopeContext>(output);
    }

    /// <summary>
    /// Lazily appends generated scalar items, allowing existential reducers to
    /// stop without constructing contexts for later values.
    /// </summary>
    public IEnumerable<ScopeContext> EnumerateGenerated(IReadOnlyList<FerruleValue> values)
    {
        ArgumentNullException.ThrowIfNull(values);
        return EnumerateGeneratedCore(values);
    }

    private IEnumerable<ScopeContext> EnumerateGeneratedCore(IReadOnlyList<FerruleValue> values)
    {
        for (var index = 0; index < values.Count; index++)
        {
            yield return GeneratedContext(values[index], index + 1);
        }
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
    /// Finds the first item whose exact scalar key equals <paramref name="matches"/>
    /// and returns its exact scalar value. Collection resolution falls back from
    /// innermost to outermost source frames. Repeated ancestors are flattened
    /// left-to-right.
    /// </summary>
    public FerruleValue Lookup(
        IReadOnlyList<string> collection,
        IReadOnlyList<string> key,
        FerruleValue matches,
        IReadOnlyList<string> value)
    {
        ArgumentNullException.ThrowIfNull(collection);
        ArgumentNullException.ThrowIfNull(key);
        ArgumentNullException.ThrowIfNull(value);
        ValidatePath(collection);
        ValidatePath(key);
        ValidatePath(value);

        foreach (var item in FindLookupItems(collection))
        {
            if (!TryResolveExactScalar(item, key, out var candidate) || candidate != matches)
            {
                continue;
            }

            return TryResolveExactScalar(item, value, out var result)
                ? result
                : FerruleValue.Null;
        }

        return FerruleValue.Null;
    }

    private IReadOnlyList<FerruleInstance> FindLookupItems(IReadOnlyList<string> path)
    {
        if (path.Count == 0)
        {
            for (var frameIndex = _frames.Count - 1; frameIndex >= 0; frameIndex--)
            {
                if (_frames[frameIndex] is FerruleRepeated repeated)
                {
                    return repeated.Items;
                }
            }
        }
        for (var frameIndex = _frames.Count - 1; frameIndex >= 0; frameIndex--)
        {
            if (path.Count == 0 ||
                !TryGetField(_frames[frameIndex], path[0], out var baseInstance))
            {
                continue;
            }
            var items = new List<FerruleInstance>();
            VisitLookupItems(baseInstance, path, 1, items);
            return items;
        }

        var display = path.Count == 0 ? "<current>" : string.Join('/', path);
        throw new FerruleRuntimeException(
            FerruleRuntimeError.MissingSourceField,
            $"Source collection '{display}' does not exist in the active scope context.");
    }

    private static void VisitLookupItems(
        FerruleInstance current,
        IReadOnlyList<string> path,
        int consumed,
        ICollection<FerruleInstance> output)
    {
        if (current is FerruleRepeated repeated)
        {
            if (consumed == path.Count)
            {
                foreach (var item in repeated.Items)
                {
                    output.Add(item);
                }
                return;
            }

            foreach (var item in repeated.Items)
            {
                VisitLookupItems(item, path, consumed, output);
            }
            return;
        }
        if (consumed < path.Count &&
            TryGetField(current, path[consumed], out var next))
        {
            VisitLookupItems(next, path, consumed + 1, output);
        }
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

    /// <summary>
    /// Reads a validated SourceField. The first matching active collection owns
    /// an absolute path even when an optional descendant is absent.
    /// Ordinary relative fields retain their existing outward broadcast lookup.
    /// </summary>
    public FerruleValue ResolveSourceField(IReadOnlyList<string> path)
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
            var resolved = TryResolveScalar(collection.Item, path, collection.Path.Count, true);
            if (resolved.Found)
            {
                return resolved.Value;
            }
            throw new FerruleRuntimeException(
                FerruleRuntimeError.MissingSourceField,
                $"Source scalar '{string.Join('/', path)}' does not exist in the active scope context.");
        }

        FerruleInstance? firstOwner = null;
        FerruleInstance? firstAbsence = null;
        for (var index = _frames.Count - 1; index >= 0; index--)
        {
            var instance = _frames[index];
            var resolved = TryResolveScalar(instance, path, 0);
            if (resolved.Found)
            {
                return resolved.Value;
            }
            var candidate = instance is FerruleRepeated { Items.Count: > 0 } repeated
                ? repeated.Items[0]
                : instance;
            if (firstOwner is null &&
                (path.Count == 0 || TryGetField(candidate, path[0], out _)))
            {
                firstOwner = instance;
            }
            if (firstAbsence is null && resolved.MissingField)
            {
                firstAbsence = instance;
            }
        }
        var selected = firstOwner ?? firstAbsence;
        if (selected is not null)
        {
            var resolved = TryResolveScalar(selected, path, 0, true);
            if (resolved.Found)
            {
                return resolved.Value;
            }
        }
        return ResolveScalar(path);
    }

    /// <summary>
    /// Reads a validated field in its exact frame. Missing groups and an
    /// intermediate Null/JsonNull yield Null; malformed values and inactive frames fail.
    /// </summary>
    public FerruleValue ResolveSourceFieldInFrame(
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
            var resolved = TryResolveScalar(collection.Item, path, 0, true);
            if (resolved.Found)
            {
                return resolved.Value;
            }
            break;
        }
        return ResolveScalarInFrame(frame, path);
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
        collections[^1] = collections[^1].JoinPosition.HasValue
            ? collections[^1] with { JoinPosition = index }
            : collections[^1] with { Index = index };
        return new ScopeContext(
            _primarySource,
            _frames,
            new ReadOnlyCollection<CollectionIdentity>(collections),
            _executionContext,
            _dynamicSourceLoader, _filterMapRunState, _filterMapBoundary, _filterMapCallDepth);
    }

    /// <summary>Clones the innermost source group for independent target ownership.</summary>
    public FerruleGroup CopyCurrentGroup()
    {
        if (_frames.Count == 0)
        {
            throw CopyCurrentGroupError("missing context");
        }

        return _frames[^1] switch
        {
            FerruleGroup group => (FerruleGroup)CloneInstance(group),
            FerruleScalar => throw CopyCurrentGroupError("scalar"),
            FerruleRepeated => throw CopyCurrentGroupError("repeated collection"),
            FerruleMappedSequence => throw CopyCurrentGroupError("mapped sequence"),
            FerruleDocumentSet => throw CopyCurrentGroupError("document set"),
            _ => throw CopyCurrentGroupError("unknown instance"),
        };
    }

    private static FerruleRuntimeException CopyCurrentGroupError(string found) =>
        new(
            FerruleRuntimeError.CopyCurrentSourceRequiresGroup,
            $"copy-current-source construction requires a group item, got {found}",
            detail: found);

    private static FerruleInstance CloneInstance(FerruleInstance instance) => instance switch
    {
        FerruleScalar scalar => new FerruleScalar(scalar.Value),
        FerruleGroup group => group.ExactCloneFields(group.Fields.Select(field =>
            new FerruleField(field.Name, CloneInstance(field.Value)))),
        FerruleRepeated repeated => new FerruleRepeated(repeated.Items.Select(CloneInstance)),
        FerruleMappedSequence mapped => new FerruleMappedSequence(mapped.Items.Select(CloneInstance)),
        FerruleDocumentSet documents => new FerruleDocumentSet(documents.Documents.Select(document =>
            new FerruleDocument(
                document.Path,
                CloneInstance(document.Value),
                document.ResolvedSourcePath))),
        _ => throw new InvalidOperationException("unknown Ferrule instance type"),
    };

    private FerruleInstance? FindIterationBase(IReadOnlyList<string> path, BasicWalkObserver? observed = null)
    {
        if (path.Count == 0)
        {
            if (_frames.Count == 0) return null;
            observed?.SelectedBase(_frames.Count - 1, _frames[^1]);
            return _frames[^1];
        }

        for (var index = _frames.Count - 1; index >= 0; index--)
        {
            observed?.ProbeBase(_frames[index]);
            if (TryGetField(_frames[index], path[0], out _))
            {
                observed?.SelectedBase(index, _frames[index]);
                return _frames[index];
            }
        }

        if (_frames.Count == 0) return null;
        observed?.SelectedBase(_frames.Count - 1, _frames[^1]);
        return _frames[^1];
    }

    private ScopeContext GeneratedContext(FerruleValue value, int index)
    {
        var item = new FerruleScalar(value);
        var frames = new List<FerruleInstance>(_frames.Count + 1);
        frames.AddRange(_frames);
        frames.Add(item);
        var collections = new List<CollectionIdentity>(_collections.Count + 1);
        collections.AddRange(_collections);
        collections.Add(new CollectionIdentity(Array.Empty<string>(), item, index));
        return new ScopeContext(
            _primarySource,
            new ReadOnlyCollection<FerruleInstance>(frames),
            new ReadOnlyCollection<CollectionIdentity>(collections),
            _executionContext,
            _dynamicSourceLoader, _filterMapRunState, _filterMapBoundary, _filterMapCallDepth);
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

    private static bool TryResolveExactScalar(
        FerruleInstance item,
        IReadOnlyList<string> path,
        out FerruleValue value)
    {
        var current = item;
        for (var index = 0; index < path.Count; index++)
        {
            if (!TryGetField(current, path[index], out var next))
            {
                value = FerruleValue.Null;
                return false;
            }
            current = next;
        }

        if (current is FerruleScalar scalar)
        {
            value = scalar.Value;
            return true;
        }

        value = FerruleValue.Null;
        return false;
    }

    private void Walk(
        FerruleInstance current,
        IReadOnlyList<string> path,
        int pathIndex,
        List<string> prefix,
        List<FerruleInstance> frames,
        List<CollectionIdentity> collections,
        ICollection<ScopeContext> output,
        BasicWalkObserver? observed = null)
    {
        observed?.Enter(current, pathIndex, prefix, frames.Count, collections.Count);
        try
        {
        if (pathIndex < path.Count && current is FerruleRepeated leadingRepeated)
        {
            for (var index = 0; index < leadingRepeated.Items.Count; index++)
            {
                var item = leadingRepeated.Items[index];
                PushCollection(item, prefix, index + 1, frames, collections, observed: observed);
                Walk(item, path, pathIndex, prefix, frames, collections, output, observed);
                PopCollection(frames, collections, observed);
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
                        PushCollection(
                            document.Value,
                            prefix,
                            index + 1,
                            frames,
                            collections,
                            document.ResolvedSourcePath ?? document.Path, observed);
                        AddCandidate(frames, collections, output, observed);
                        PopCollection(frames, collections, observed);
                    }
                    return;
                case FerruleRepeated repeated:
                    for (var index = 0; index < repeated.Items.Count; index++)
                    {
                        var item = repeated.Items[index];
                        PushCollection(item, prefix, index + 1, frames, collections, observed: observed);
                        AddCandidate(frames, collections, output, observed);
                        PopCollection(frames, collections, observed);
                    }
                    return;
                default:
                    observed?.BeforePlain();
                    if (observed is not null) GrowBasicList(frames, 1, observed);
                    observed?.Plain(current, prefix);
                    frames.Add(current);
                    AddCandidate(frames, collections, output, observed);
                    frames.RemoveAt(frames.Count - 1);
                    observed?.PlainPopped();
                    return;
            }
        }

        if (current is FerruleDocumentSet documentSet)
        {
            for (var index = 0; index < documentSet.Documents.Count; index++)
            {
                var document = documentSet.Documents[index];
                PushCollection(
                    document.Value,
                    prefix,
                    index + 1,
                    frames,
                    collections,
                    document.ResolvedSourcePath ?? document.Path, observed);
                Walk(document.Value, path, pathIndex, prefix, frames, collections, output, observed);
                PopCollection(frames, collections, observed);
            }
            return;
        }

        List<string> nextPrefix;
        if (observed is null)
            nextPrefix = new List<string>(prefix) { path[pathIndex] };
        else
        {
            var capacity = checked(prefix.Count + 1);
            observed.ReserveSlots((nuint)capacity + 1); // exact backing plus nextPrefix descriptor
            observed.ReservePath(prefix, path[pathIndex]);
            nextPrefix = new List<string>(capacity);
            nextPrefix.AddRange(prefix);
            nextPrefix.Add(path[pathIndex]);
        }
        try
        {
        if (!TryGetField(current, path[pathIndex], out var next))
        {
            return;
        }

        observed?.ConsumedField(path[pathIndex], next);

        if (next is FerruleRepeated repeatedField)
        {
            for (var index = 0; index < repeatedField.Items.Count; index++)
            {
                var item = repeatedField.Items[index];
                PushCollection(item, nextPrefix, index + 1, frames, collections, observed: observed);
                if (pathIndex + 1 == path.Count)
                {
                    AddCandidate(frames, collections, output, observed);
                }
                else
                {
                    Walk(item, path, pathIndex + 1, nextPrefix, frames, collections, output, observed);
                }
                PopCollection(frames, collections, observed);
            }
            return;
        }

        Walk(next, path, pathIndex + 1, nextPrefix, frames, collections, output, observed);
        }
        finally
        {
            if (observed is not null) observed.ReleaseSlots((nuint)nextPrefix.Capacity + 1);
        }
        }
        finally { observed?.Leave(); }
    }

    private void AddCandidate(
        IReadOnlyCollection<FerruleInstance> frames,
        IReadOnlyCollection<CollectionIdentity> collections,
        ICollection<ScopeContext> output,
        BasicWalkObserver? observed = null)
    {
        if (observed is not null)
        {
            observed.BeforeCandidate(frames.Count, collections.Count, output.Count);
            observed.ReserveSlots((nuint)checked(_frames.Count + frames.Count));
            observed.ReserveSlots((nuint)checked(_collections.Count + collections.Count));
            observed.ReserveSlots(5); // two Lists, two readonly wrappers and the exact legacy context
            observed.ReserveOutputCell();
        }
        var allFrames = new List<FerruleInstance>(_frames.Count + frames.Count);
        allFrames.AddRange(_frames);
        allFrames.AddRange(frames);
        var allCollections = new List<CollectionIdentity>(_collections.Count + collections.Count);
        allCollections.AddRange(_collections);
        allCollections.AddRange(collections);
        var candidate = new ScopeContext(
            _primarySource,
            new ReadOnlyCollection<FerruleInstance>(allFrames),
            new ReadOnlyCollection<CollectionIdentity>(allCollections),
            _executionContext,
            _dynamicSourceLoader, _filterMapRunState, _filterMapBoundary, _filterMapCallDepth);
        output.Add(candidate);
        observed?.Candidate(candidate, frames.Count, collections.Count);
    }

    private static void PushCollection(
        FerruleInstance item,
        IReadOnlyList<string> path,
        int index,
        ICollection<FerruleInstance> frames,
        ICollection<CollectionIdentity> collections,
        string? documentPath = null,
        BasicWalkObserver? observed = null)
    {
        if (observed is not null)
        {
            observed.BeforeCollectionPush();
            GrowBasicList((List<FerruleInstance>)frames, 1, observed);
            GrowBasicList((List<CollectionIdentity>)collections, 1, observed);
            observed.ReserveSlots((nuint)path.Count);
            observed.ReservePath(path, null);
            observed.ReserveSlots(1); // exact legacy collection identity object
        }
        frames.Add(item);
        collections.Add(new CollectionIdentity(
            path.ToArray(),
            item,
            index,
            DocumentPath: documentPath));
        observed?.CollectionPushed(item, path, index);
    }

    private static void PopCollection(
        List<FerruleInstance> frames,
        List<CollectionIdentity> collections,
        BasicWalkObserver? observed = null)
    {
        var releasedPathSlots = observed is null ? 0 : collections[^1].Path.Count;
        frames.RemoveAt(frames.Count - 1);
        collections.RemoveAt(collections.Count - 1);
        if (observed is not null)
        {
            observed.ReleaseSlots((nuint)releasedPathSlots + 1);
            observed.CollectionPopped();
        }
    }

    private static ScalarResolution TryResolveScalar(
        FerruleInstance source,
        IReadOnlyList<string> path,
        int pathIndex,
        bool absenceAsNull = false,
        bool basicOnly = false)
    {
        var current = source;
        for (var index = pathIndex; index < path.Count; index++)
        {
            if (basicOnly) RequireBasicScalarTraversal(current);
            if (current is FerruleRepeated repeated)
            {
                if (repeated.Items.Count == 0)
                {
                    return ScalarResolution.Resolved(FerruleValue.Null);
                }
                current = repeated.Items[0];
                if (basicOnly) RequireBasicScalarTraversal(current);
            }

            if (absenceAsNull && current is FerruleScalar { Value.Kind: FerruleValueKind.Null or FerruleValueKind.JsonNull })
            {
                return ScalarResolution.Resolved(FerruleValue.Null);
            }
            if (!TryGetField(current, path[index], out var next))
            {
                if (current is FerruleGroup or FerruleDocumentSet)
                {
                    return absenceAsNull
                        ? ScalarResolution.Resolved(FerruleValue.Null)
                        : ScalarResolution.AbsentField;
                }
                return ScalarResolution.Missing;
            }
            current = next;
        }

        if (basicOnly) RequireBasicScalarTraversal(current);
        if (current is FerruleRepeated terminalRepeated)
        {
            if (terminalRepeated.Items.Count == 0)
            {
                return ScalarResolution.Resolved(FerruleValue.Null);
            }
            current = terminalRepeated.Items[0];
            if (basicOnly) RequireBasicScalarTraversal(current);
        }

        return current is FerruleScalar scalar
            ? ScalarResolution.Resolved(scalar.Value)
            : ScalarResolution.Missing;
    }

    private static bool TryResolveDynamicScalar(
        FerruleInstance source,
        IReadOnlyList<string> objectPath,
        string key,
        out FerruleValue value)
    {
        var current = source;
        for (var index = 0; index < objectPath.Count; index++)
        {
            if (current is FerruleRepeated repeated)
            {
                if (repeated.Items.Count == 0)
                {
                    value = FerruleValue.Null;
                    return false;
                }
                current = repeated.Items[0];
            }
            if (!TryGetField(current, objectPath[index], out var next))
            {
                value = FerruleValue.Null;
                return false;
            }
            current = next;
        }
        if (current is FerruleRepeated terminalRepeated)
        {
            if (terminalRepeated.Items.Count == 0)
            {
                value = FerruleValue.Null;
                return false;
            }
            current = terminalRepeated.Items[0];
        }

        if (TryGetField(current, key, out var field))
        {
            value = field is FerruleScalar scalar
                ? scalar.Value
                : FerruleValue.Null;
            return true;
        }
        value = FerruleValue.Null;
        return true;
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

    internal sealed record CollectionIdentity(
        IReadOnlyList<string> Path,
        FerruleInstance Item,
        int Index,
        ulong? Join = null,
        int? JoinPosition = null,
        bool Grouped = false,
        string? DocumentPath = null,
        bool BasicGroupMembers = false)
    {
        private int _index = Index;
        public int Index { get => _index; init => _index = value; }
        // Used only on a private, unpublished group-operation selection clone.
        internal void SetGroupSelectionRank(int rank) => _index = rank;
    }

    private readonly record struct ScalarResolution(bool Found, FerruleValue Value, bool MissingField = false)
    {
        internal static ScalarResolution Missing => default;

        internal static ScalarResolution AbsentField => new(false, FerruleValue.Null, true);

        internal static ScalarResolution Resolved(FerruleValue value) => new(true, value);
    }
}

/// <summary>An exact static source-owner query; names are never normalized.</summary>
public readonly struct FerruleOwnedSourceOwner
{
    private FerruleOwnedSourceOwner(string name) { Name = name; }
    public static FerruleOwnedSourceOwner Primary => default;
    public static FerruleOwnedSourceOwner Named(string exactName)
    {
        ArgumentNullException.ThrowIfNull(exactName);
        FerruleUnicode.RequireWellFormed(exactName, nameof(exactName));
        return new(exactName);
    }
    public string? Name { get; }
    public bool IsPrimary => Name is null;
}

/// <summary>A declared name and optional supplied immutable instance.</summary>
public sealed class FerruleDeclaredNamedSourceRoot
{
    public FerruleDeclaredNamedSourceRoot(string exactName, FerruleInstance? instance)
    {
        ArgumentNullException.ThrowIfNull(exactName);
        FerruleUnicode.RequireWellFormed(exactName, nameof(exactName));
        Name = exactName; Instance = instance;
    }
    public string Name { get; }
    public FerruleInstance? Instance { get; }
}

public readonly record struct FerruleOwnedSourceLimits(
    nuint MaxRoots, nuint MaxActiveFrames, nuint MaxPathSegments,
    nuint MaxMetadataBytes, nuint MaxWalkContexts);

public enum FerruleOwnedSourceErrorKind
{
    InvalidNamedOwner, UnknownOwner, AmbiguousOwner, SourceUnavailable,
    InactiveSourceFrame, IncompatibleFramePath, UniteratedRepetition,
    UnsupportedSourceKind, Traversal, InvalidPosition, MetadataLimit,
    MetadataArithmeticOverflow,
    MissingRowDocumentPath,
}
public enum FerruleOwnedSourceLimitKind
{
    Roots, Frames, PathSegments, MetadataBytes, WalkContexts, MetadataSlots,
}
public enum FerruleOwnedSourceInstanceKind { Scalar, Group, Repeated, DocumentSet, MappedSequence }
public enum FerruleOwnedSourceTraversalKind { CannotTraverse, ExpectedScalar, MissingInstance }

public sealed class FerruleOwnedSourceIdentity
{
    internal FerruleOwnedSourceIdentity(string? name) { Name = name; }
    public bool IsPrimary => Name is null;
    public string? Name { get; }
}
public sealed class FerruleOwnedSourceTraversalException : Exception
{
    internal FerruleOwnedSourceTraversalException(FerruleOwnedSourceTraversalKind error,
        IReadOnlyList<string> path, nuint? segment, FerruleOwnedSourceInstanceKind found)
        : base("Owned source traversal failed.")
    { Error = error; Path = path; Segment = segment; Found = found; }
    public FerruleOwnedSourceTraversalKind Error { get; }
    public IReadOnlyList<string> Path { get; }
    public nuint? Segment { get; }
    public FerruleOwnedSourceInstanceKind Found { get; }
}
public sealed class FerruleOwnedSourceException : Exception
{
    internal FerruleOwnedSourceException(FerruleOwnedSourceErrorKind error,
        FerruleOwnedSourceIdentity? owner = null, string? requestedName = null,
        IReadOnlyList<int>? declarationIndices = null, IReadOnlyList<string>? path = null,
        IReadOnlyList<string>? frame = null, IReadOnlyList<string>? collection = null,
        nuint? segment = null, FerruleOwnedSourceInstanceKind? found = null,
        nuint? index = null, FerruleOwnedSourceLimitKind? limit = null,
        nuint? used = null, nuint? requested = null, nuint? maximum = null,
        FerruleOwnedSourceTraversalException? cause = null)
        : base("Owned source operation failed.", cause)
    {
        Error = error; Owner = owner; RequestedName = requestedName;
        DeclarationIndices = declarationIndices; Path = path; Frame = frame;
        Collection = collection; Segment = segment; FoundInstance = found;
        RequestedIndex = index; LimitKind = limit; Used = used;
        Requested = requested; Maximum = maximum;
    }
    public FerruleOwnedSourceErrorKind Error { get; }
    public FerruleOwnedSourceIdentity? Owner { get; }
    public string? RequestedName { get; }
    public IReadOnlyList<int>? DeclarationIndices { get; }
    public IReadOnlyList<string>? Path { get; }
    public IReadOnlyList<string>? Frame { get; }
    public IReadOnlyList<string>? Collection { get; }
    public nuint? Segment { get; }
    public FerruleOwnedSourceInstanceKind? FoundInstance { get; }
    public nuint? RequestedIndex { get; }
    public FerruleOwnedSourceLimitKind? LimitKind { get; }
    public nuint? Used { get; }
    public nuint? Requested { get; }
    public nuint? Maximum { get; }
}

// Counts are logical metadata slots and UTF-8 payload, never allocator/RSS bytes.
internal static class OwnedSourceAccounting
{
    internal static FerruleOwnedSourceException Overflow(FerruleOwnedSourceLimitKind kind) =>
        new(FerruleOwnedSourceErrorKind.MetadataArithmeticOverflow, limit: kind);
    internal static nuint Add(nuint left, nuint right, FerruleOwnedSourceLimitKind kind)
    {
        if (right > nuint.MaxValue - left) throw Overflow(kind);
        return left + right;
    }
    internal static nuint Multiply(nuint left, nuint right)
    {
        if (left != 0 && right > nuint.MaxValue / left)
            throw Overflow(FerruleOwnedSourceLimitKind.MetadataSlots);
        return left * right;
    }
    internal static nuint CoreSlots(FerruleOwnedSourceLimits l)
    {
        var k = FerruleOwnedSourceLimitKind.MetadataSlots;
        var wf = Multiply(l.MaxWalkContexts, l.MaxActiveFrames);
        var af = Add(wf, Multiply(2, l.MaxActiveFrames), k);
        var fs = Multiply(af, l.MaxPathSegments);
        var tasks = Multiply(2, Add(l.MaxActiveFrames, 1, k));
        var rr = Multiply(2, l.MaxRoots);
        var diagnostics = Add(Multiply(2, l.MaxPathSegments), 2, k);
        var total = Add(rr, 1, k);
        total = Add(total, af, k); total = Add(total, fs, k);
        total = Add(total, tasks, k); total = Add(total, l.MaxWalkContexts, k);
        return Add(total, diagnostics, k);
    }
    internal static nuint CSharpSlots(FerruleOwnedSourceLimits l)
    {
        var k = FerruleOwnedSourceLimitKind.MetadataSlots;
        var total = CoreSlots(l);
        total = Add(total, Multiply(2, l.MaxWalkContexts), k);
        total = Add(total, l.MaxActiveFrames, k);
        total = Add(total, Multiply(l.MaxActiveFrames, l.MaxPathSegments), k);
        return Add(total, Multiply(2, Add(l.MaxActiveFrames, 1, k)), k);
    }
    internal static nuint Tasks(FerruleOwnedSourceLimits l) =>
        Multiply(2, Add(l.MaxActiveFrames, 1, FerruleOwnedSourceLimitKind.MetadataSlots));
    internal static void Limit(nuint used, nuint requested, nuint maximum,
        FerruleOwnedSourceLimitKind kind)
    {
        if (Add(used, requested, kind) > maximum)
            throw new FerruleOwnedSourceException(FerruleOwnedSourceErrorKind.MetadataLimit,
                limit: kind, used: used, requested: requested, maximum: maximum);
    }
    internal static nuint Text(string text)
    {
        // Count without an int-sized encoded buffer or Encoding byte-count overflow.
        FerruleUnicode.RequireWellFormed(text, nameof(text));
        nuint bytes = 0;
        for (var i = 0; i < text.Length; i++)
        {
            var ch = text[i];
            nuint count = ch <= 0x7f ? (nuint)1 : ch <= 0x7ff ? (nuint)2 : (nuint)3;
            if (char.IsHighSurrogate(ch)) { count = 4; i++; }
            bytes = Add(bytes, count, FerruleOwnedSourceLimitKind.MetadataBytes);
        }
        return bytes;
    }
    internal static nuint PathText(IReadOnlyList<string> path)
    {
        nuint value = 0;
        for (var i = 0; i < path.Count; i++)
            value = Add(value, Text(path[i]), FerruleOwnedSourceLimitKind.MetadataBytes);
        return value;
    }
}
internal sealed class OwnedCounter
{
    internal OwnedCounter(nuint maximum, FerruleOwnedSourceLimitKind kind)
    { Maximum = maximum; Kind = kind; }
    internal nuint Used { get; private set; }
    internal nuint Maximum { get; }
    internal FerruleOwnedSourceLimitKind Kind { get; }
    internal void Charge(nuint count)
    {
        OwnedSourceAccounting.Limit(Used, count, Maximum, Kind);
        Used = OwnedSourceAccounting.Add(Used, count, Kind);
    }
    internal void Release(nuint count)
    {
        if (count > Used) throw OwnedSourceAccounting.Overflow(Kind);
        Used -= count;
    }
}

/// <summary>Immutable metadata snapshot sharing the supplied immutable source trees.</summary>
public sealed class FerruleOwnedSourceRoots
{
    private readonly Binding[] _named;
    private readonly FerruleInstance _primary;
    private readonly record struct Binding(string Name, FerruleInstance? Instance);
    internal FerruleOwnedSourceLimits Limits { get; }
    internal nuint SlotMaximum { get; }
    internal nuint RegistryCount => (nuint)_named.Length + 1;
    private readonly nuint _normalizationBackingSlots;
    internal nuint RegistryStorageCount => RegistryCount + _normalizationBackingSlots;
    internal nuint AmbiguityReservationCount => _normalizationBackingSlots == 0 ? RegistryCount : (nuint)_named.Length;
    internal nuint RegistryText { get; }
    private FerruleOwnedSourceRoots(FerruleInstance primary, Binding[] named,
        FerruleOwnedSourceLimits limits, nuint slots, nuint text, nuint normalizationBackingSlots = 0)
    { _primary = primary; _named = named; Limits = limits; SlotMaximum = slots; RegistryText = text; _normalizationBackingSlots = normalizationBackingSlots; }
    public static FerruleOwnedSourceRoots Create(FerruleInstance primary,
        IReadOnlyList<FerruleDeclaredNamedSourceRoot> named, FerruleOwnedSourceLimits limits)
    {
        ArgumentNullException.ThrowIfNull(primary); ArgumentNullException.ThrowIfNull(named);
        // All derived products precede registry allocation, even when actual inputs are small.
        var slots = OwnedSourceAccounting.CSharpSlots(limits);
        var roots = OwnedSourceAccounting.Add((nuint)named.Count, 1, FerruleOwnedSourceLimitKind.Roots);
        OwnedSourceAccounting.Limit(0, roots, limits.MaxRoots, FerruleOwnedSourceLimitKind.Roots);
        nuint bytes = 0;
        for (var i = 0; i < named.Count; i++)
        {
            ArgumentNullException.ThrowIfNull(named[i]);
            bytes = OwnedSourceAccounting.Add(bytes, OwnedSourceAccounting.Text(named[i].Name),
                FerruleOwnedSourceLimitKind.MetadataBytes);
        }
        OwnedSourceAccounting.Limit(0, bytes, limits.MaxMetadataBytes, FerruleOwnedSourceLimitKind.MetadataBytes);
        OwnedSourceAccounting.Limit(0, roots, slots, FerruleOwnedSourceLimitKind.MetadataSlots);
        if (named.Count > Array.MaxLength) throw OwnedSourceAccounting.Overflow(FerruleOwnedSourceLimitKind.MetadataSlots);
        var copy = new Binding[named.Count];
        for (var i = 0; i < copy.Length; i++) copy[i] = new(named[i].Name, named[i].Instance);
        return new(primary, copy, limits, slots, bytes);
    }

    // The final array is the registry table. No intermediate declaration list is made.
    internal static FerruleOwnedSourceRoots FromNamedInputs<T, TAccessor>(FerruleInstance primary,
        ReadOnlySpan<FerruleBasicNamedSourceDeclaration> catalogue, IReadOnlyList<T> supplied,
        TAccessor accessor, FerruleOwnedSourceLimits limits, out BasicSourceBudget budget)
        where TAccessor : struct, IFerruleNamedSourceInputAccessor<T>
    {
        Binding[] named;
        nuint core, text, backing;
        try
        {
            var policy = PreflightNamedInputs(catalogue, limits);
            core = policy.Core; text = policy.Text; backing = policy.Backing;
            budget = new BasicSourceBudget(limits, false);
            budget.Charge(policy.Peak); budget.Text(text);
            var length = catalogue.Length;
            named = length == 0 ? Array.Empty<Binding>() : new Binding[length];
            for (var i = 0; i < named.Length; i++) named[i] = new(catalogue[i].Name, null);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }

        // Caller code executes outside ownership lifting, including a caller-thrown typed cause.
        var suppliedCount = supplied.Count;
        if (suppliedCount < 0) throw new ArgumentOutOfRangeException(nameof(supplied), suppliedCount, "Count cannot be negative.");
        for (var ordinal = 0; ordinal < suppliedCount; ordinal++)
        {
            var item = supplied[ordinal];
            var input = accessor.Read(item);
            try
            {
                var name = input.Name; var instance = input.Instance;
                if (name is null || instance is null) throw new ArgumentNullException(nameof(supplied));
                FerruleUnicode.RequireWellFormed(name, nameof(supplied));
                var found = -1;
                for (var i = 0; i < named.Length; i++)
                    if (StringComparer.Ordinal.Equals(named[i].Name, name) && catalogue[i].Mode != FerruleBasicNamedSourceMode.ReservedUnavailable)
                    { found = i; break; }
                if (found < 0)
                {
                    budget.Text(OwnedSourceAccounting.Text(name));
                    throw new FerruleRuntimeException(FerruleRuntimeError.UnexpectedNamedSource,
                        "named source is not declared by this mapping", detail: name);
                }
                if (named[found].Instance is not null)
                    throw new FerruleRuntimeException(FerruleRuntimeError.DuplicateNamedSource,
                        "named source was supplied more than once", detail: named[found].Name);
                named[found] = new(named[found].Name, instance);
            }
            catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
            catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
        }
        for (var i = 0; i < named.Length; i++)
            if (catalogue[i].Mode == FerruleBasicNamedSourceMode.RequiredLegacy && named[i].Instance is null)
                throw new FerruleRuntimeException(FerruleRuntimeError.MissingNamedSource,
                    "named source is required by this mapping", detail: named[i].Name);
        // Only the two counters remain live during the shared projection factory.
        budget.Release(3);
        return new(primary, named, limits, core, text, backing);
    }

    internal readonly record struct NamedInputPreflight(nuint Core, nuint Paired, nuint Text, nuint Backing, nuint Peak);

    // Shared pure policy/catalogue proof. Preserve the typed factory's original
    // order and payloads; no roots, counters, projection or registry is created.
    internal static NamedInputPreflight PreflightNamedInputs(
        ReadOnlySpan<FerruleBasicNamedSourceDeclaration> catalogue, FerruleOwnedSourceLimits limits)
    {
        var core = OwnedSourceAccounting.CSharpSlots(limits);
        var paired = BasicSourceAccounting.Maximum(limits, false);
        var count = OwnedSourceAccounting.Add((nuint)catalogue.Length, 1, FerruleOwnedSourceLimitKind.Roots);
        OwnedSourceAccounting.Limit(0, count, limits.MaxRoots, FerruleOwnedSourceLimitKind.Roots);
        var length = BasicSourceAccounting.ArrayLength((nuint)catalogue.Length);
        var backing = length == 0 ? (nuint)0 : (nuint)1;
        var storage = OwnedSourceAccounting.Add(count, backing, FerruleOwnedSourceLimitKind.MetadataSlots);
        // One current value, two counters and two complete error envelopes.
        var peak = OwnedSourceAccounting.Add(storage, 5, FerruleOwnedSourceLimitKind.MetadataSlots);
        OwnedSourceAccounting.Limit(0, peak, core, FerruleOwnedSourceLimitKind.MetadataSlots);
        nuint text = 0;
        for (var i = 0; i < length; i++)
        {
            var declaration = catalogue[i]; var name = declaration.Name;
            if (name is null) throw new ArgumentNullException(nameof(catalogue));
            FerruleUnicode.RequireWellFormed(name, nameof(catalogue));
            if (name.Length == 0) throw new FerruleOwnedSourceException(FerruleOwnedSourceErrorKind.InvalidNamedOwner);
            if (declaration.Mode is not (FerruleBasicNamedSourceMode.Optional or
                FerruleBasicNamedSourceMode.RequiredLegacy or FerruleBasicNamedSourceMode.ReservedUnavailable))
                throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.InvalidDeclarationWitness,
                    declarationIndex: (nuint)i, declaredCount: (nuint)length);
            for (var j = 0; j < i; j++)
                if (StringComparer.Ordinal.Equals(catalogue[j].Name, name))
                    throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.InvalidDeclarationWitness,
                        declarationIndex: (nuint)i, declaredCount: (nuint)length);
            var size = OwnedSourceAccounting.Text(name);
            OwnedSourceAccounting.Limit(text, size, limits.MaxMetadataBytes, FerruleOwnedSourceLimitKind.MetadataBytes);
            text = OwnedSourceAccounting.Add(text, size, FerruleOwnedSourceLimitKind.MetadataBytes);
        }
        return new(core, paired, text, backing, peak);
    }

    public FerruleOwnedScopeContext RootContext() => FerruleOwnedScopeContext.CreateRoot(this);
    internal int Resolve(FerruleOwnedSourceOwner owner, OwnedQuery query)
    {
        if (owner.IsPrimary) return 0;
        var name = owner.Name!;
        if (name.Length == 0) throw new FerruleOwnedSourceException(FerruleOwnedSourceErrorKind.InvalidNamedOwner);
        var found = -1; var count = 0;
        for (var i = 0; i < _named.Length; i++)
            if (string.Equals(_named[i].Name, name, StringComparison.Ordinal)) { found = i; count++; }
        if (count == 0) throw query.Error(FerruleOwnedSourceErrorKind.UnknownOwner);
        if (count > 1)
        {
            // Query already reserved every possible matching declaration index.
            var indices = new int[count]; var target = 0;
            for (var i = 0; i < _named.Length; i++)
                if (string.Equals(_named[i].Name, name, StringComparison.Ordinal)) indices[target++] = i;
            throw query.Error(FerruleOwnedSourceErrorKind.AmbiguousOwner, declarations: Array.AsReadOnly(indices));
        }
        if (_named[found].Instance is null) throw query.Error(FerruleOwnedSourceErrorKind.SourceUnavailable);
        return found + 1;
    }
    /// <summary>Like <see cref="Resolve"/>, also returning a declared but unsupplied owner.</summary>
    internal int ResolveDeclared(FerruleOwnedSourceOwner owner, OwnedQuery query, out bool supplied)
    {
        supplied = true;
        if (owner.IsPrimary) return 0;
        var name = owner.Name!;
        if (name.Length == 0) return Resolve(owner, query);
        var found = -1;
        for (var i = 0; i < _named.Length; i++)
            if (string.Equals(_named[i].Name, name, StringComparison.Ordinal)) { found = i; break; }
        if (found < 0 || _named[found].Instance is not null) return Resolve(owner, query);
        // Ambiguity and empty names keep their exact original refusals.
        for (var i = found + 1; i < _named.Length; i++)
            if (string.Equals(_named[i].Name, name, StringComparison.Ordinal)) return Resolve(owner, query);
        supplied = false;
        return found + 1;
    }
    internal FerruleInstance Instance(int key) => key == 0 ? _primary : _named[key - 1].Instance!;
    internal FerruleInstance? SuppliedInstance(int key) => key == 0 ? _primary : _named[key - 1].Instance;
    internal string? Name(int key) => key == 0 ? null : _named[key - 1].Name;
}

internal sealed class OwnedQuery
{
    internal OwnedQuery(FerruleOwnedScopeContext context, FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> path, IReadOnlyList<string>? frame)
    {
        Roots = context.Roots; Owner = owner;
        Slots = new(Roots.SlotMaximum, FerruleOwnedSourceLimitKind.MetadataSlots);
        Bytes = new(Roots.Limits.MaxMetadataBytes, FerruleOwnedSourceLimitKind.MetadataBytes);
        Slots.Charge(Roots.RegistryStorageCount); Slots.Charge(1); Bytes.Charge(Roots.RegistryText);
        for (var i = 0; i < context.FrameCount; i++)
        {
            var item = context.FrameAt(i);
            Slots.Charge((nuint)item.Path.Count + 1); Bytes.Charge(OwnedSourceAccounting.PathText(item.Path));
        }
        CheckPath(path); if (frame is not null) CheckPath(frame);
        // Query/error/ambiguity reserves precede namespace resolution and diagnostic copies.
        Slots.Charge(Roots.AmbiguityReservationCount);
        Slots.Charge(OwnedSourceAccounting.Add(OwnedSourceAccounting.Add((nuint)path.Count,
            (nuint)(frame?.Count ?? 0), FerruleOwnedSourceLimitKind.MetadataSlots),
            2, FerruleOwnedSourceLimitKind.MetadataSlots));
        var text = OwnedSourceAccounting.Add(OwnedSourceAccounting.PathText(path),
            frame is null ? 0 : OwnedSourceAccounting.PathText(frame), FerruleOwnedSourceLimitKind.MetadataBytes);
        if (owner.Name is not null) text = OwnedSourceAccounting.Add(text,
            OwnedSourceAccounting.Text(owner.Name), FerruleOwnedSourceLimitKind.MetadataBytes);
        Bytes.Charge(text); Bytes.Charge(text);
        nuint longest = 0;
        for (var i = 0; i < path.Count; i++)
        { var size = OwnedSourceAccounting.Text(path[i]); if (size > longest) longest = size; }
        Bytes.Charge(longest);
    }
    // The basic adapter admits its complete paired registry/current footprint
    // before allocating this query. Query and diagnostic payload reserves use
    // those same per-call counters; no per-view or per-output reset is allowed.
    internal OwnedQuery(FerruleOwnedScopeContext context, FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> path, IReadOnlyList<string>? frame,
        OwnedCounter slots, OwnedCounter bytes, nuint? legacyPathMaximum = null,
        nuint? canonicalSelectorLength = null)
    {
        Roots = context.Roots; Owner = owner; Slots = slots; Bytes = bytes;
        CheckPath(path, legacyPathMaximum);
        if (frame is not null) CheckPath(frame);
        if (canonicalSelectorLength.HasValue)
            BasicSourceAccounting.Limit(0, canonicalSelectorLength.Value, Roots.Limits.MaxPathSegments,
                FerruleBasicSourceBridgeComponent.PathSegments);
        Slots.Charge(Roots.AmbiguityReservationCount);
        Slots.Charge(OwnedSourceAccounting.Add(OwnedSourceAccounting.Add((nuint)path.Count,
            (nuint)(frame?.Count ?? 0), FerruleOwnedSourceLimitKind.MetadataSlots),
            2, FerruleOwnedSourceLimitKind.MetadataSlots));
        var text = OwnedSourceAccounting.Add(OwnedSourceAccounting.PathText(path),
            frame is null ? 0 : OwnedSourceAccounting.PathText(frame), FerruleOwnedSourceLimitKind.MetadataBytes);
        if (owner.Name is not null) text = OwnedSourceAccounting.Add(text,
            OwnedSourceAccounting.Text(owner.Name), FerruleOwnedSourceLimitKind.MetadataBytes);
        Bytes.Charge(text); Bytes.Charge(text);
        nuint longest = 0;
        for (var i = 0; i < path.Count; i++)
        { var size = OwnedSourceAccounting.Text(path[i]); if (size > longest) longest = size; }
        Bytes.Charge(longest);
    }
    internal FerruleOwnedSourceRoots Roots { get; }
    internal FerruleOwnedSourceOwner Owner { get; }
    internal OwnedCounter Slots { get; }
    internal OwnedCounter Bytes { get; }
    private void CheckPath(IReadOnlyList<string> path, nuint? maximum = null)
    {
        ArgumentNullException.ThrowIfNull(path);
        OwnedSourceAccounting.Limit(0, (nuint)path.Count, maximum ?? Roots.Limits.MaxPathSegments,
            FerruleOwnedSourceLimitKind.PathSegments);
        if (path.Count > Array.MaxLength) throw OwnedSourceAccounting.Overflow(FerruleOwnedSourceLimitKind.MetadataSlots);
        for (var i = 0; i < path.Count; i++)
        { ArgumentNullException.ThrowIfNull(path[i]); FerruleUnicode.RequireWellFormed(path[i], nameof(path)); }
    }
    internal static IReadOnlyList<string> Copy(IReadOnlyList<string> path)
    {
        var value = new string[path.Count];
        for (var i = 0; i < value.Length; i++) value[i] = path[i];
        return Array.AsReadOnly(value);
    }
    internal FerruleOwnedSourceException Error(FerruleOwnedSourceErrorKind error,
        IReadOnlyList<string>? path = null, IReadOnlyList<string>? frame = null,
        IReadOnlyList<string>? collection = null, nuint? segment = null,
        FerruleOwnedSourceInstanceKind? found = null, nuint? index = null,
        IReadOnlyList<int>? declarations = null, FerruleOwnedSourceTraversalKind? traversal = null)
    {
        var copiedPath = path is null ? null : Copy(path);
        var cause = traversal.HasValue ? new FerruleOwnedSourceTraversalException(
            traversal.Value, copiedPath!, segment, found!.Value) : null;
        return new(error, new FerruleOwnedSourceIdentity(Owner.Name), Owner.Name, declarations,
            copiedPath, frame is null ? null : Copy(frame), collection is null ? null : Copy(collection),
            segment, found, index, cause: cause);
    }
}

// Bounded storage counts capacity, including old/new arrays during growth.
internal sealed class OwnedBuffer<T>
{
    private T[] _items = Array.Empty<T>();
    private readonly OwnedCounter _slots;
    private readonly nuint _maximum;
    private readonly FerruleOwnedSourceLimitKind _kind;
    internal OwnedBuffer(OwnedCounter slots, nuint maximum, FerruleOwnedSourceLimitKind kind)
    { _slots = slots; _maximum = maximum; _kind = kind; }
    internal int Count { get; private set; }
    internal bool NeedsGrowth => Count == _items.Length;
    internal T this[int index] { get => _items[index]; set => _items[index] = value; }
    internal void Push(T item)
    {
        OwnedSourceAccounting.Limit((nuint)Count, 1, _maximum, _kind);
        if (NeedsGrowth)
        {
            var old = (nuint)_items.Length;
            var desired = old == 0 ? (nuint)1 : OwnedSourceAccounting.Multiply(old, 2);
            var capacity = desired < _maximum ? desired : _maximum;
            if (capacity > (nuint)Array.MaxLength) throw OwnedSourceAccounting.Overflow(FerruleOwnedSourceLimitKind.MetadataSlots);
            _slots.Charge(capacity);
            var replacement = new T[(int)capacity];
            Array.Copy(_items, replacement, Count);
            _items = replacement; _slots.Release(old);
        }
        _items[Count++] = item;
    }
    internal T Pop()
    {
        var item = _items[--Count]; _items[Count] = default!;
        // Cleared reusable cells remain charged until their backing storage is released.
        return item;
    }
    internal void ReleaseStorage()
    { _slots.Release((nuint)_items.Length); _items = Array.Empty<T>(); Count = 0; }
    internal IReadOnlyList<T> Retain() => new Result(_items, Count);
    private sealed class Result(T[] items, int count) : IReadOnlyList<T>
    {
        public int Count => count;
        public T this[int index] => (uint)index < (uint)count ? items[index] : throw new ArgumentOutOfRangeException(nameof(index));
        public IEnumerator<T> GetEnumerator() { for (var i = 0; i < count; i++) yield return items[i]; }
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    }
}

/// <summary>Separate direct owned core; never substitutes for legacy ScopeContext.</summary>
public sealed partial class FerruleOwnedScopeContext
{
    // Authenticated synthetic member containers minted only by paired By,
    // AdjacentBy, StartingWith, EndingWith and IntoBlocks. Nested ordinary occurrences remain Items.
    // A Document roots one loaded document of an owned dynamic endpoint inside
    // its loading scope; it is never a position or an exact occurrence.
    internal readonly record struct Occurrence(int Owner, IReadOnlyList<string> Path, FerruleInstance Item, nuint Index,
        bool GroupMembers = false, bool Document = false);
    internal readonly record struct Observation(int Owner, IReadOnlyList<string> Path, FerruleInstance Item, nuint Index,
        bool GroupMembers = false, bool Document = false);
    private readonly Occurrence[] _frames;
    private FerruleOwnedScopeContext(FerruleOwnedSourceRoots roots, Occurrence[] frames) { Roots = roots; _frames = frames; }
    internal static FerruleOwnedScopeContext CreateRoot(FerruleOwnedSourceRoots roots)
    {
        var budget = new OwnedCounter(roots.SlotMaximum, FerruleOwnedSourceLimitKind.MetadataSlots);
        budget.Charge(roots.RegistryStorageCount); budget.Charge(1);
        return new(roots, Array.Empty<Occurrence>());
    }
    internal FerruleOwnedSourceRoots Roots { get; }
    internal int FrameCount => _frames.Length;
    internal Observation FrameAt(int index)
    { var f = _frames[index]; return new(f.Owner, f.Path, f.Item, f.Index, f.GroupMembers, f.Document); }
    private static bool Prefix(IReadOnlyList<string> path, IReadOnlyList<string> prefix)
    {
        if (prefix.Count > path.Count) return false;
        for (var i = 0; i < prefix.Count; i++)
            if (!string.Equals(path[i], prefix[i], StringComparison.Ordinal)) return false;
        return true;
    }
    private int Match(int owner, IReadOnlyList<string>? path, bool exact)
    {
        for (var i = _frames.Length - 1; i >= 0; i--)
            if (_frames[i].Owner == owner && (!_frames[i].Document || (!exact && path is not null)) && (path is null ||
                (exact ? path.Count == _frames[i].Path.Count && Prefix(path, _frames[i].Path) : Prefix(path, _frames[i].Path))))
                return i;
        return -1;
    }
    private static FerruleOwnedSourceInstanceKind Kind(FerruleInstance instance) => instance switch
    {
        FerruleScalar => FerruleOwnedSourceInstanceKind.Scalar,
        FerruleGroup => FerruleOwnedSourceInstanceKind.Group,
        FerruleRepeated => FerruleOwnedSourceInstanceKind.Repeated,
        FerruleDocumentSet => FerruleOwnedSourceInstanceKind.DocumentSet,
        FerruleMappedSequence => FerruleOwnedSourceInstanceKind.MappedSequence,
        _ => throw new InvalidOperationException("Unknown immutable source instance kind."),
    };
    private static bool Absent(FerruleInstance value) => value is FerruleScalar
        { Value.Kind: FerruleValueKind.Null or FerruleValueKind.JsonNull };
    private static bool Unsupported(FerruleInstance value) => value is FerruleDocumentSet or FerruleMappedSequence;
    public FerruleValue ResolveField(FerruleOwnedSourceOwner owner, IReadOnlyList<string> path,
        IReadOnlyList<string>? frame = null) => ResolveFieldCore(owner, path, frame, null);
    internal FerruleValue ResolveBasicField(FerruleOwnedSourceOwner owner, IReadOnlyList<string> path,
        IReadOnlyList<string>? frame, OwnedQuery admittedQuery) =>
        ResolveFieldCore(owner, path, frame, admittedQuery);
    internal FerruleValue ResolveBasicFieldWithRootPin(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> path, IReadOnlyList<string>? frame, OwnedQuery admittedQuery,
        bool declaredNonrepeating) =>
        ResolveFieldCore(owner, path, frame, admittedQuery, declaredNonrepeating);
    private FerruleValue ResolveFieldCore(FerruleOwnedSourceOwner owner, IReadOnlyList<string> path,
        IReadOnlyList<string>? frame, OwnedQuery? admittedQuery, bool declaredNonrepeating = false)
    {
        var q = admittedQuery ?? new OwnedQuery(this, owner, path, frame); var key = Resolve(owner, q);
        if (frame is not null && !Prefix(path, frame))
            throw q.Error(FerruleOwnedSourceErrorKind.IncompatibleFramePath, path, frame);
        // The Basic path supplies the static exact-owner declaration proof.
        // An explicit empty pin selects the registered nonrepeating root, even
        // when a descendant or another owner has an active occurrence. At [] the
        // latest exact-owner occurrence alone decides member-container vs ordinary Item:
        // a newer member Item shadows its older root group, never another owner's.
        var rootGroup = Match(key, Array.Empty<string>(), true);
        var registeredRoot = frame is { Count: 0 } &&
            ((declaredNonrepeating && RootOf(key) is not FerruleRepeated) ||
              (rootGroup >= 0 && _frames[rootGroup].GroupMembers));
        var selected = registeredRoot ? -1 : Match(key, frame ?? path, frame is not null);
        if (frame is not null && selected < 0 && !registeredRoot)
            throw q.Error(FerruleOwnedSourceErrorKind.InactiveSourceFrame, collection: frame);
        var current = selected < 0 ? RootOf(key) : _frames[selected].Item;
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
                throw q.Error(FerruleOwnedSourceErrorKind.UniteratedRepetition, path, segment: (nuint)consumed);
            if (Unsupported(current))
                throw q.Error(FerruleOwnedSourceErrorKind.UnsupportedSourceKind, path, found: Kind(current));
            if (consumed == path.Count)
            {
                if (current is FerruleScalar scalar) return scalar.Value;
                throw q.Error(FerruleOwnedSourceErrorKind.Traversal, path, found: Kind(current),
                    traversal: FerruleOwnedSourceTraversalKind.ExpectedScalar);
            }
            if (Absent(current)) return FerruleValue.Null;
            if (current is not FerruleGroup group)
                throw q.Error(FerruleOwnedSourceErrorKind.Traversal, path, segment: (nuint)consumed,
                    found: Kind(current), traversal: FerruleOwnedSourceTraversalKind.CannotTraverse);
            if (!group.TryGetField(path[consumed], out var next)) return FerruleValue.Null;
            current = next; consumed++;
        }
    }
    public nuint Position(FerruleOwnedSourceOwner owner, IReadOnlyList<string>? collection = null) =>
        PositionCore(owner, collection, null);
    internal nuint BasicPosition(FerruleOwnedSourceOwner owner, IReadOnlyList<string>? collection,
        OwnedQuery admittedQuery) => PositionCore(owner, collection, admittedQuery);
    private nuint PositionCore(FerruleOwnedSourceOwner owner, IReadOnlyList<string>? collection,
        OwnedQuery? admittedQuery)
    {
        var q = admittedQuery ?? new OwnedQuery(this, owner, collection ?? Array.Empty<string>(), null);
        var key = Resolve(owner, q); var selected = Match(key, collection, true);
        if (selected < 0) throw q.Error(FerruleOwnedSourceErrorKind.InactiveSourceFrame, collection: collection);
        return _frames[selected].Index;
    }

    /// <summary>Reads exact-owner physical row metadata without ancestor or member fallback.</summary>
    public FerruleValue RowDocumentPath(FerruleOwnedSourceOwner owner, IReadOnlyList<string>? collection = null) =>
        RowDocumentPathCore(owner, collection, null);

    internal FerruleValue BasicRowDocumentPath(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string>? collection, OwnedQuery admittedQuery) =>
        RowDocumentPathCore(owner, collection, admittedQuery);

    private FerruleValue RowDocumentPathCore(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string>? collection, OwnedQuery? admittedQuery)
    {
        var query = admittedQuery ?? new OwnedQuery(this, owner, collection ?? Array.Empty<string>(), null);
        var key = Roots.Resolve(owner, query);
        var selected = collection is { Count: 0 } ? -1 : Match(key, collection, true);
        if (collection is { Count: > 0 } && selected < 0)
            throw query.Error(FerruleOwnedSourceErrorKind.InactiveSourceFrame, collection: collection);
        var current = selected < 0 ? Roots.Instance(key) : _frames[selected].Item;
        if (current is not FerruleGroup group || group.SourceOrigin is null)
            throw query.Error(FerruleOwnedSourceErrorKind.MissingRowDocumentPath, collection: collection);
        return FerruleValue.FromString(group.SourceOrigin.Document.SourcePath);
    }
    public FerruleOwnedScopeContext WithPosition(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string>? collection, nuint index)
    {
        var q = new OwnedQuery(this, owner, collection ?? Array.Empty<string>(), null);
        var key = Resolve(owner, q); var selected = Match(key, collection, true);
        if (selected < 0) throw q.Error(FerruleOwnedSourceErrorKind.InactiveSourceFrame, collection: collection);
        if (index == 0) throw q.Error(FerruleOwnedSourceErrorKind.InvalidPosition, index: index);
        var copy = CopyFrames(q, _frames); copy[selected] = copy[selected] with { Index = index };
        q.Slots.Charge(1); return new(Roots, copy);
    }
    public FerruleOwnedScopeContext CloneView()
    {
        var q = new OwnedQuery(this, FerruleOwnedSourceOwner.Primary, Array.Empty<string>(), null);
        var copy = CopyFrames(q, _frames); q.Slots.Charge(1); return new(Roots, copy);
    }
    private static Occurrence[] CopyFrames(OwnedQuery q, IReadOnlyList<Occurrence> frames)
    {
        nuint slots = (nuint)frames.Count; nuint text = 0;
        for (var i = 0; i < frames.Count; i++)
        {
            slots = OwnedSourceAccounting.Add(slots, (nuint)frames[i].Path.Count, FerruleOwnedSourceLimitKind.MetadataSlots);
            text = OwnedSourceAccounting.Add(text, OwnedSourceAccounting.PathText(frames[i].Path), FerruleOwnedSourceLimitKind.MetadataBytes);
        }
        q.Slots.Charge(slots); q.Bytes.Charge(text);
        var copy = new Occurrence[frames.Count];
        for (var i = 0; i < copy.Length; i++) copy[i] = frames[i];
        return copy;
    }
    internal static FerruleOwnedScopeContext FromBasicObserved(FerruleOwnedSourceRoots roots,
        ReadOnlySpan<Observation> observed, OwnedQuery query)
    {
        if (!ReferenceEquals(roots, query.Roots))
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.RegistryMismatch);
        OwnedSourceAccounting.Limit(0, (nuint)observed.Length, roots.Limits.MaxActiveFrames, FerruleOwnedSourceLimitKind.Frames);
        nuint cells = (nuint)observed.Length;
        for (var i = 0; i < observed.Length; i++)
        {
            var frame = observed[i];
            if (frame.Owner < 0 || (nuint)frame.Owner >= roots.RegistryCount)
                throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.InvalidDeclarationWitness,
                    declarationIndex: frame.Owner > 0 ? (nuint?)frame.Owner - 1 : null,
                    declaredCount: roots.RegistryCount - 1);
            OwnedSourceAccounting.Limit(0, (nuint)frame.Path.Count, roots.Limits.MaxPathSegments, FerruleOwnedSourceLimitKind.PathSegments);
            cells = BasicSourceAccounting.Add(cells, (nuint)frame.Path.Count);
            query.Bytes.Charge(OwnedSourceAccounting.PathText(frame.Path));
        }
        query.Slots.Charge(BasicSourceAccounting.Add(cells, 1));
        var copy = new Occurrence[observed.Length];
        for (var i = 0; i < copy.Length; i++)
        {
            var f = observed[i];
            // The scratch witness can be dropped after this publication. Each
            // output owns its separately admitted immutable path snapshot.
            var path = new string[f.Path.Count];
            for (var j = 0; j < path.Length; j++) path[j] = f.Path[j];
            copy[i] = new(f.Owner, path, f.Item, f.Index, f.GroupMembers, f.Document);
        }
        return new FerruleOwnedScopeContext(roots, copy);
    }
    internal FerruleOwnedScopeContext CloneBasicObserved(OwnedQuery query, int? slot = null, nuint index = 0)
    {
        if (!ReferenceEquals(Roots, query.Roots))
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.RegistryMismatch);
        if (slot.HasValue && (uint)slot.Value >= (uint)_frames.Length)
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness);
        var copy = CopyFrames(query, _frames);
        if (slot.HasValue) copy[slot.Value] = copy[slot.Value] with { Index = index };
        query.Slots.Charge(1);
        return new FerruleOwnedScopeContext(Roots, copy);
    }
    private readonly record struct Task(FerruleInstance Instance, int Consumed, int Depth, int Next = -1);
    public IReadOnlyList<FerruleOwnedScopeContext> IterateSource(FerruleOwnedSourceOwner owner, IReadOnlyList<string> path) =>
        IterateSourceCore(owner, path, null);

    internal IReadOnlyList<FerruleOwnedScopeContext> IterateBasicSource(
        FerruleOwnedSourceOwner owner, IReadOnlyList<string> path, BasicOwnedWalkObserver observed) =>
        IterateSourceCore(owner, path, observed);

    private IReadOnlyList<FerruleOwnedScopeContext> IterateSourceCore(
        FerruleOwnedSourceOwner owner, IReadOnlyList<string> path, BasicOwnedWalkObserver? observed)
    {
        var q = observed is null ? new OwnedQuery(this, owner, path, null) : observed.CreateQuery(this, owner, path);
        var key = Resolve(owner, q);
        var identity = Match(key, path, true);
        if (identity >= 0 && !_frames[identity].GroupMembers)
        {
            var identityItem = _frames[identity].Item;
            observed?.SelectedBase(key, identityItem, _frames[identity].Path.Count, true, identity);
            // Legacy source walking can witness null occurrences. Only the paired
            // bridge can transfer those; it must prune before the identity shortcut.
            if (observed is not null && Absent(identityItem))
                return Array.Empty<FerruleOwnedScopeContext>();
            observed?.BeforePublication(_frames.Length, identityItem, path, true);
            OwnedSourceAccounting.Limit(0, 1, Roots.Limits.MaxWalkContexts, FerruleOwnedSourceLimitKind.WalkContexts);
            var copy = CopyFrames(q, _frames); q.Slots.Charge(observed is null ? (nuint)2 : 1);
            var identityContext = new FerruleOwnedScopeContext(Roots, copy);
            observed?.Published(identityContext, identityItem, path, true);
            return observed is null ? Array.AsReadOnly(new[] { identityContext }) : Array.Empty<FerruleOwnedScopeContext>();
        }
        var selected = Match(key, path, false);
        var current = selected < 0 ? RootOf(key) : _frames[selected].Item;
        var consumed = selected < 0 ? 0 : _frames[selected].Path.Count;
        observed?.SelectedBase(key, current, consumed, false, selected);
        observed?.BeforeWalkBuffers();
        var frames = new OwnedBuffer<Occurrence>(q.Slots, Roots.Limits.MaxActiveFrames, FerruleOwnedSourceLimitKind.Frames);
        var tasks = new OwnedBuffer<Task>(q.Slots, OwnedSourceAccounting.Tasks(Roots.Limits), FerruleOwnedSourceLimitKind.MetadataSlots);
        var output = new OwnedBuffer<FerruleOwnedScopeContext>(q.Slots, Roots.Limits.MaxWalkContexts, FerruleOwnedSourceLimitKind.WalkContexts);
        void PushFrame(Occurrence item)
        {
            OwnedSourceAccounting.Limit((nuint)frames.Count, 1, Roots.Limits.MaxActiveFrames, FerruleOwnedSourceLimitKind.Frames);
            if (frames.NeedsGrowth)
                for (var i = 0; i < frames.Count; i++) q.Bytes.Charge(OwnedSourceAccounting.PathText(frames[i].Path));
            q.Slots.Charge((nuint)item.Path.Count); q.Bytes.Charge(OwnedSourceAccounting.PathText(item.Path));
            frames.Push(item);
        }
        void Truncate(int depth)
        {
            while (frames.Count > depth) { var item = frames.Pop(); q.Slots.Release((nuint)item.Path.Count); }
        }
        void Emit(FerruleInstance actualTerminal)
        {
            observed?.BeforePublication(frames.Count, actualTerminal, path, false);
            OwnedSourceAccounting.Limit((nuint)output.Count, 1, Roots.Limits.MaxWalkContexts, FerruleOwnedSourceLimitKind.WalkContexts);
            nuint slots = (nuint)frames.Count; nuint text = 0;
            for (var i = 0; i < frames.Count; i++)
            { slots = OwnedSourceAccounting.Add(slots, (nuint)frames[i].Path.Count, FerruleOwnedSourceLimitKind.MetadataSlots);
              text = OwnedSourceAccounting.Add(text, OwnedSourceAccounting.PathText(frames[i].Path), FerruleOwnedSourceLimitKind.MetadataBytes); }
            q.Slots.Charge(slots); q.Bytes.Charge(text); q.Slots.Charge(1);
            var snapshot = new Occurrence[frames.Count];
            for (var i = 0; i < snapshot.Length; i++) snapshot[i] = frames[i];
            var actualContext = new FerruleOwnedScopeContext(Roots, snapshot);
            output.Push(actualContext);
            observed?.Published(actualContext, actualTerminal, path, false);
        }
        try
        {
            for (var i = 0; i < _frames.Length; i++) PushFrame(_frames[i]);
            tasks.Push(new(current, consumed, frames.Count));
            while (tasks.Count != 0)
            {
                var task = tasks.Pop(); Truncate(task.Depth);
                var item = task.Instance; var offset = task.Consumed;
                if (task.Next >= 0)
                {
                    var repeated = (FerruleRepeated)item;
                    if (task.Next == repeated.Items.Count) continue;
                    tasks.Push(task with { Next = task.Next + 1 });
                    // Frame/path admission precedes the new retained path array.
                    OwnedSourceAccounting.Limit((nuint)frames.Count, 1, Roots.Limits.MaxActiveFrames, FerruleOwnedSourceLimitKind.Frames);
                    q.Slots.Charge((nuint)offset); nuint text = 0;
                    for (var i = 0; i < offset; i++) text = OwnedSourceAccounting.Add(text,
                        OwnedSourceAccounting.Text(path[i]), FerruleOwnedSourceLimitKind.MetadataBytes);
                    q.Bytes.Charge(text);
                    var prefix = new string[offset]; for (var i = 0; i < offset; i++) prefix[i] = path[i];
                    // Prefix slots/text already charged; only capacity/growth costs remain.
                    if (frames.NeedsGrowth)
                        for (var i = 0; i < frames.Count; i++) q.Bytes.Charge(OwnedSourceAccounting.PathText(frames[i].Path));
                    frames.Push(new(key, Array.AsReadOnly(prefix), repeated.Items[task.Next], (nuint)task.Next + 1));
                    tasks.Push(new(repeated.Items[task.Next], offset, frames.Count));
                    continue;
                }
                while (true)
                {
                    if (Absent(item)) break;
                    if (Unsupported(item)) throw q.Error(FerruleOwnedSourceErrorKind.UnsupportedSourceKind, path, found: Kind(item));
                    if (item is FerruleRepeated) { tasks.Push(new(item, offset, frames.Count, 0)); break; }
                    if (offset == path.Count) { Emit(item); break; }
                    if (item is not FerruleGroup group)
                        throw q.Error(FerruleOwnedSourceErrorKind.Traversal, path, segment: (nuint)offset,
                            found: Kind(item), traversal: FerruleOwnedSourceTraversalKind.CannotTraverse);
                    if (!group.TryGetField(path[offset], out var next)) break;
                    item = next; offset++;
                }
            }
            return observed is null ? output.Retain() : Array.Empty<FerruleOwnedScopeContext>();
        }
        finally
        {
            Truncate(0); frames.ReleaseStorage(); tasks.ReleaseStorage();
            if (observed is not null) output.ReleaseStorage();
        }
    }
    internal interface BasicOwnedWalkObserver
    {
        OwnedQuery CreateQuery(FerruleOwnedScopeContext current,
            FerruleOwnedSourceOwner owner, IReadOnlyList<string> path);
        void BeforeWalkBuffers();
        void SelectedBase(int ownerKey, FerruleInstance actual,
            int consumed, bool identity, int selectedSlot);
        void BeforePublication(int frameCount, FerruleInstance actualTerminal,
            IReadOnlyList<string> fullCanonicalPath, bool identity);
        void Published(FerruleOwnedScopeContext actualContext,
            FerruleInstance actualTerminal, IReadOnlyList<string> fullCanonicalPath, bool identity);
    }

}


public sealed partial class ScopeContext
{
    // One allocation-free selection, retained by the authentic walk witness and
    // reused after query admission. Unsupported probes are deferred to their old
    // post-query frontier, without selecting a different frame a second time.
    internal readonly record struct BasicSourceSelection(int Frame, int ProbeFloor,
        bool HasField, int Count, string? First);
    internal BasicSourceSelection PreselectBasicSource(IReadOnlyList<string> path)
    {
        ArgumentNullException.ThrowIfNull(path);
        var count = path.Count;
        var first = count > 0 ? path[0] : null;
        if (count <= 0 || _frames.Count == 0)
            return new(_frames.Count - 1, _frames.Count, false, count, first);
        for (var i = _frames.Count - 1; i >= 0; i--)
        {
            if (_frames[i] is FerruleDocumentSet or FerruleMappedSequence)
                return new(i, i, false, count, first);
            if (first is not null && TryGetField(_frames[i], first, out _))
                return new(i, i, true, count, first);
        }
        return new(_frames.Count - 1, 0, false, count, first);
    }
    internal abstract class BasicWalkObserver
    {
        internal abstract nuint LegacyCapacityMaximum { get; }
        internal abstract void ReserveSlots(nuint slots);
        internal abstract void ReserveOutputCell();
        internal abstract void ReleaseSlots(nuint slots);
        internal abstract void ReservePath(IReadOnlyList<string> path, string? appended);
        internal abstract void ProbeBase(FerruleInstance current);
        internal abstract void SelectedBase(int frameIndex, FerruleInstance current);
        internal virtual IReadOnlyList<string> InitialLegacyPrefix => Array.Empty<string>();
        internal virtual int InitialLegacyPrefixCount => 0;
        internal abstract void Enter(FerruleInstance current, int consumed,
            IReadOnlyList<string> relativePrefix, int frameCount, int collectionCount);
        internal abstract void Leave();
        internal abstract void ConsumedField(string actualField, FerruleInstance actualChild);
        internal abstract void BeforeCollectionPush();
        internal abstract void BeforePlain();
        internal abstract void CollectionPushed(FerruleInstance item, IReadOnlyList<string> legacyPath, int rawPosition);
        internal abstract void CollectionPopped();
        internal abstract void Plain(FerruleInstance item, IReadOnlyList<string> rawPath);
        internal abstract void PlainPopped();
        internal abstract void BeforeCandidate(int newFrames, int newCollections, int count);
        internal abstract void Candidate(ScopeContext actual, int newFrames, int newCollections);
    }

    private static void GrowBasicList<T>(List<T> list, int additional, BasicWalkObserver observed)
    {
        var required = (nuint)list.Count + (nuint)additional;
        OwnedSourceAccounting.Limit(0, required, observed.LegacyCapacityMaximum,
            FerruleOwnedSourceLimitKind.MetadataSlots);
        if (required > (nuint)Array.MaxLength)
            throw OwnedSourceAccounting.Overflow(FerruleOwnedSourceLimitKind.MetadataSlots);
        if ((nuint)list.Capacity >= required) return;
        var oldCapacity = list.Capacity;
        observed.ReserveSlots(required);
        list.Capacity = (int)required;
        observed.ReleaseSlots((nuint)oldCapacity);
    }

    internal void IterateBasicSource(IReadOnlyList<string> path, BasicWalkObserver observed,
        ICollection<ScopeContext> output, BasicSourceSelection selection)
    {
        ArgumentNullException.ThrowIfNull(path);
        ArgumentNullException.ThrowIfNull(observed);
        ValidatePath(path);
        if (selection.Frame < 0) return;
        for (var i = _frames.Count - 1; i >= selection.ProbeFloor; i--) observed.ProbeBase(_frames[i]);
        var current = _frames[selection.Frame];
        observed.SelectedBase(selection.Frame, current);
        // The opaque holder is the output sink; it never publishes a mutable array.
        observed.ReserveSlots(3);
        var prefixCount = observed.InitialLegacyPrefixCount;
        observed.ReserveSlots((nuint)prefixCount);
        var prefix = new List<string>(prefixCount);
        for (var i = 0; i < prefixCount; i++)
        {
            observed.ReservePath(Array.Empty<string>(), observed.InitialLegacyPrefix[i]);
            prefix.Add(observed.InitialLegacyPrefix[i]);
        }
        var frames = new List<FerruleInstance>();
        var collections = new List<CollectionIdentity>();
        try { Walk(current, path, 0, prefix, frames, collections, output, observed); }
        finally
        {
            observed.ReleaseSlots((nuint)prefix.Capacity + 1);
            observed.ReleaseSlots((nuint)frames.Capacity + 1);
            observed.ReleaseSlots((nuint)collections.Capacity + 1);
        }
    }

}

public enum FerruleBasicSourceBridgeErrorKind
{
    RegistryMismatch,
    InvalidDeclarationWitness,
    UnsupportedBasicContext,
    PairedMetadataLimit,
    PairedMetadataArithmeticOverflow,
    LegacyPositionRange,
    InvalidCohortWitness,
    DuplicatePhaseCandidate,
    UnsupportedPrimaryFailureContext,
}

public enum FerruleBasicSourceBridgeComponent
{
    MetadataSlots,
    LegacyFrames,
    CanonicalPaths,
    PathSegments,
    RecursionDepth,
    ActiveFrames,
    WalkContexts,
    RootProjection,
    PhaseInput,
    MetadataBytes,
}

public sealed class FerruleBasicSourceBridgeException : Exception
{
    internal FerruleBasicSourceBridgeException(
        FerruleBasicSourceBridgeErrorKind error,
        FerruleBasicSourceBridgeComponent? component = null,
        nuint? used = null,
        nuint? requested = null,
        nuint? maximum = null,
        nuint? inputOrdinal = null,
        nuint? candidateOrdinal = null,
        nuint? declarationIndex = null,
        nuint? declaredCount = null,
        FerruleOwnedSourceInstanceKind? found = null)
        : base("basic source bridge refused")
    {
        Error = error;
        Component = component;
        Used = used;
        Requested = requested;
        Maximum = maximum;
        InputOrdinal = inputOrdinal;
        CandidateOrdinal = candidateOrdinal;
        DeclarationIndex = declarationIndex;
        DeclaredCount = declaredCount;
        Found = found;
    }

    public FerruleBasicSourceBridgeErrorKind Error { get; }
    public FerruleBasicSourceBridgeComponent? Component { get; }
    public nuint? Used { get; }
    public nuint? Requested { get; }
    public nuint? Maximum { get; }
    public nuint? InputOrdinal { get; }
    public nuint? CandidateOrdinal { get; }
    public nuint? DeclarationIndex { get; }
    public nuint? DeclaredCount { get; }
    public FerruleOwnedSourceInstanceKind? Found { get; }
}


internal static class BasicSourceAccounting
{
    internal static FerruleBasicSourceBridgeException Overflow() => new(
        FerruleBasicSourceBridgeErrorKind.PairedMetadataArithmeticOverflow,
        FerruleBasicSourceBridgeComponent.MetadataSlots);
    internal static nuint Add(nuint left, nuint right)
    {
        if (right > nuint.MaxValue - left) throw Overflow();
        return left + right;
    }
    internal static nuint Multiply(nuint left, nuint right)
    {
        if (left != 0 && right > nuint.MaxValue / left) throw Overflow();
        return left * right;
    }
    internal static nuint ThreeSets(nuint maximum) => Multiply(3, maximum);
    internal static void Limit(nuint used, nuint requested, nuint maximum,
        FerruleBasicSourceBridgeComponent component)
    {
        if (Add(used, requested) > maximum)
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit,
                component, used, requested, maximum);
    }
    internal static int ArrayLength(nuint count)
    {
        if (count > (nuint)Array.MaxLength) throw Overflow();
        return (int)count;
    }
    internal static nuint LegacyFrames(FerruleOwnedSourceLimits limits) =>
        Add(Add(Multiply(2, limits.MaxActiveFrames), Multiply(2, limits.MaxPathSegments)), 4);
    internal static nuint Recursion(FerruleOwnedSourceLimits limits) =>
        Add(Add(limits.MaxActiveFrames, limits.MaxPathSegments), 1);
    internal static nuint Maximum(FerruleOwnedSourceLimits limits, bool fullPhase)
    {
        var k = fullPhase ? ThreeSets(limits.MaxWalkContexts) : limits.MaxWalkContexts;
        var f = limits.MaxActiveFrames; var s = limits.MaxPathSegments; var r = limits.MaxRoots;
        var wf = Multiply(k, f); var af = Add(wf, Multiply(2, f)); var fs = Multiply(af, s);
        var tasks = Multiply(2, Add(f, 1)); var rr = Multiply(2, r); var d = Add(Multiply(2, s), 2);
        var core = Add(Add(Add(Add(Add(Add(rr, 1), af), fs), tasks), k), d);
        var csharpCore = Add(Add(Add(Add(core, Multiply(2, k)), f), Multiply(f, s)), tasks);
        var l = LegacyFrames(limits); var a = Recursion(limits); var n = Add(k, 1);
        var bl = Multiply(n, l); var bo = bl; var bp = Multiply(bo, s);
        var lc = Multiply(n, f); var lp = Multiply(lc, Add(s, 1)); var bt = Multiply(5, r);
        var q = Multiply(Multiply(Multiply(2, a), Add(Add(l, f), 2)), Add(s, 2));
        var bc = Multiply(2, n);
        var m = Add(Add(Add(Add(Add(Add(Add(bl, bo), bp), lc), lp), bt), q), bc);
        var j = Add(Add(Add(k, 1), Multiply(4, n)), Add(a, 4));
        var result = Add(Add(Add(csharpCore, m), d), Multiply(2, m));
        if (fullPhase) result = Add(Add(result, Multiply(2, limits.MaxWalkContexts)), 5);
        return Add(result, j);
    }
    internal static void Charge(OwnedCounter slots, nuint count)
    {
        try { slots.Charge(count); }
        catch (FerruleOwnedSourceException e) when (e.LimitKind == FerruleOwnedSourceLimitKind.MetadataSlots)
        {
            if (e.Error == FerruleOwnedSourceErrorKind.MetadataArithmeticOverflow) throw Overflow();
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit,
                FerruleBasicSourceBridgeComponent.MetadataSlots, e.Used, e.Requested, e.Maximum);
        }
    }
}

internal readonly struct BasicSourceBudget
{
    internal BasicSourceBudget(FerruleOwnedSourceLimits limits, nuint maximum)
    {
        Slots = new OwnedCounter(maximum, FerruleOwnedSourceLimitKind.MetadataSlots);
        Bytes = new OwnedCounter(limits.MaxMetadataBytes, FerruleOwnedSourceLimitKind.MetadataBytes);
        Phase = true;
    }
    internal BasicSourceBudget(FerruleOwnedSourceLimits limits, bool phase)
    {
        Slots = new OwnedCounter(BasicSourceAccounting.Maximum(limits, phase), FerruleOwnedSourceLimitKind.MetadataSlots);
        Bytes = new OwnedCounter(limits.MaxMetadataBytes, FerruleOwnedSourceLimitKind.MetadataBytes);
        Phase = phase;
    }
    internal OwnedCounter Slots { get; }
    internal OwnedCounter Bytes { get; }
    internal bool Phase { get; }
    internal void Charge(nuint slots) => BasicSourceAccounting.Charge(Slots, slots);
    internal void Release(nuint slots) => Slots.Release(slots);
    internal void Path(IReadOnlyList<string> path) => Text(OwnedSourceAccounting.PathText(path));
    internal void Text(nuint bytes)
    {
        if (!Phase) { Bytes.Charge(bytes); return; }
        try { Bytes.Charge(bytes); }
        catch (FerruleOwnedSourceException e) when (e.LimitKind == FerruleOwnedSourceLimitKind.MetadataBytes)
        {
            if (e.Error == FerruleOwnedSourceErrorKind.MetadataArithmeticOverflow) throw BasicSourceAccounting.Overflow();
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.PairedMetadataLimit,
                FerruleBasicSourceBridgeComponent.MetadataBytes, e.Used, e.Requested, e.Maximum);
        }
    }
}

internal enum BasicOriginKind { Ordinary, GroupWrapper, GroupMembers }
internal readonly record struct BasicFrameOrigin(int OwnerKey, IReadOnlyList<string> Canonical,
    nuint? RawOrdinal = null, bool Projection = false, BasicOriginKind Kind = BasicOriginKind.Ordinary,
    string? WrapperField = null, int GroupCollection = -1);
internal readonly record struct BasicOccurrenceCursor(int OwnedSlot, int LegacySlot, int OwnerKey,
    IReadOnlyList<string> Canonical, nuint RawOrdinal);


public sealed partial class ScopeContext
{
    internal int BasicCollectionCount => _collections.Count;
    internal object BasicRunStateIdentity => _filterMapRunState;
    internal IReadOnlyList<string> BasicCollectionPath(int index) => _collections[index].Path;
    internal int BasicCollectionIndex(int index) => _collections[index].Index;
    internal bool IsPrimaryFailureRoot(FerruleInstance primary, FerruleGroup projection) =>
        ReferenceEquals(_primarySource, primary) && _frames.Count == 2 &&
        ReferenceEquals(_frames[0], projection) && ReferenceEquals(_frames[1], primary) &&
        _collections.Count == 0 && _dynamicSourceLoader is null &&
        _filterMapBoundary is null && _filterMapCallDepth == 0;
    internal void RequireBasicContext()
    {
        if (_dynamicSourceLoader is not null || _filterMapBoundary is not null || _filterMapCallDepth != 0)
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.UnsupportedBasicContext);
        for (var i = 0; i < _collections.Count; i++)
            if (_collections[i].Join.HasValue || (_collections[i].Grouped && !_collections[i].BasicGroupMembers) || _collections[i].DocumentPath is not null)
                throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.UnsupportedBasicContext);
    }
    internal void SeedBasicLegacy(BasicSourceBudget budget)
    {
        budget.Charge((nuint)_frames.Count);
        budget.Charge((nuint)_collections.Count * 2);
        for (var i = 0; i < _collections.Count; i++)
        {
            budget.Charge((nuint)_collections[i].Path.Count);
            budget.Path(_collections[i].Path);
        }
        // The calling paired view supplies its exact container-descriptor count.
        budget.Charge(1);
    }
    internal static ScopeContext CreateBasicRoot(FerruleInstance primary, FerruleGroup projection,
        FerruleExecutionContext? execution, BasicSourceBudget budget)
    {
        budget.Charge(5); // two frame cells, read-only wrapper, legacy view and root helper state
        var frames = new FerruleInstance[] { projection, primary };
        return new ScopeContext(primary, new ReadOnlyCollection<FerruleInstance>(frames),
            Array.Empty<CollectionIdentity>(), execution);
    }
    internal ScopeContext CloneBasicLegacy(BasicSourceBudget budget, int? slot = null, int index = 0)
    {
        RequireBasicContext();
        if (slot.HasValue && (uint)slot.Value >= (uint)_collections.Count)
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness);
        budget.Charge((nuint)_frames.Count);
        budget.Charge((nuint)_collections.Count);
        budget.Charge(3); // two exact read-only wrappers and new legacy context
        if (slot.HasValue) budget.Charge(1); // replacement association record
        for (var i = 0; i < _collections.Count; i++) budget.Path(_collections[i].Path);
        var frames = new FerruleInstance[_frames.Count];
        var collections = new CollectionIdentity[_collections.Count];
        for (var i = 0; i < frames.Length; i++) frames[i] = _frames[i];
        for (var i = 0; i < collections.Length; i++) collections[i] = _collections[i];
        if (slot.HasValue) collections[slot.Value] = collections[slot.Value] with { Index = index };
        return new ScopeContext(_primarySource, new ReadOnlyCollection<FerruleInstance>(frames),
            new ReadOnlyCollection<CollectionIdentity>(collections), _executionContext,
            _dynamicSourceLoader, _filterMapRunState, _filterMapBoundary, _filterMapCallDepth);
    }
    internal ScopeContext ProjectBasicOwned(FerruleOwnedScopeContext observed, int inheritedOwnedFrames,
        FerruleInstance terminal, IReadOnlyList<string> canonical, bool identity, BasicSourceBudget budget)
    {
        RequireBasicContext();
        if (identity) return this;
        var extra = observed.FrameCount - inheritedOwnedFrames;
        if (extra < 0) throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness);
        var plain = extra == 0 || observed.FrameAt(observed.FrameCount - 1).Path.Count != canonical.Count;
        var totalFrames = BasicSourceAccounting.Add((nuint)_frames.Count, (nuint)extra + (plain ? (nuint)1 : 0));
        var totalCollections = BasicSourceAccounting.Add((nuint)_collections.Count, (nuint)extra);
        BasicSourceAccounting.Limit(0, totalFrames, BasicSourceAccounting.LegacyFrames(observed.Roots.Limits),
            FerruleBasicSourceBridgeComponent.LegacyFrames);
        BasicSourceAccounting.Limit(0, totalCollections, observed.Roots.Limits.MaxActiveFrames,
            FerruleBasicSourceBridgeComponent.ActiveFrames);
        for (var i = inheritedOwnedFrames; i < observed.FrameCount; i++)
            if (observed.FrameAt(i).Index > int.MaxValue)
                throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.LegacyPositionRange,
                    requested: observed.FrameAt(i).Index, maximum: int.MaxValue);
        budget.Charge(BasicSourceAccounting.Add(totalFrames, totalCollections));
        budget.Charge(3);
        for (var i = 0; i < _collections.Count; i++) budget.Path(_collections[i].Path);
        // New collection path cells and association objects are admitted individually below.
        var frames = new FerruleInstance[BasicSourceAccounting.ArrayLength(totalFrames)];
        var collections = new CollectionIdentity[BasicSourceAccounting.ArrayLength(totalCollections)];
        for (var i = 0; i < _frames.Count; i++) frames[i] = _frames[i];
        for (var i = 0; i < _collections.Count; i++) collections[i] = _collections[i];
        for (var i = 0; i < extra; i++)
        {
            var f = observed.FrameAt(inheritedOwnedFrames + i);
            // Owned walks publish the actual owner-relative collection, as native does.
            // Ordinary named wrapper paths are retained by the separate legacy walker.
            var length = (nuint)f.Path.Count;
            budget.Charge(BasicSourceAccounting.Add(length, 1));
            budget.Path(f.Path);
            var path = new string[BasicSourceAccounting.ArrayLength(length)];
            for (var j = 0; j < f.Path.Count; j++) path[j] = f.Path[j];
            frames[_frames.Count + i] = f.Item;
            collections[_collections.Count + i] = new CollectionIdentity(path, f.Item, checked((int)f.Index));
        }
        if (plain) frames[^1] = terminal;
        return new ScopeContext(_primarySource, new ReadOnlyCollection<FerruleInstance>(frames),
            new ReadOnlyCollection<CollectionIdentity>(collections), _executionContext,
            _dynamicSourceLoader, _filterMapRunState, _filterMapBoundary, _filterMapCallDepth);
    }
}


/// <summary>Static roots for the paired basic source view; limits are supplied by the caller.</summary>
public sealed class FerruleBasicSourceRoots
{
    private readonly FerruleOwnedSourceRoots _owned;
    private readonly FerruleGroup _projection;
    private readonly int[] _supplied;
    private readonly int[] _unique;
    private readonly nuint _retainedSlots;
    private FerruleBasicSourceRoots(FerruleOwnedSourceRoots owned, FerruleGroup projection,
        int[] supplied, int[] unique, nuint retainedSlots, IFerruleDynamicSourceLoader? dynamicSourceLoader = null)
    { _owned = owned; _projection = projection; _supplied = supplied; _unique = unique; _retainedSlots = retainedSlots; DynamicSourceLoader = dynamicSourceLoader; }
    /// <summary>The host boundary owned dynamic loading scopes request documents from.</summary>
    internal IFerruleDynamicSourceLoader? DynamicSourceLoader { get; }
    /// <summary>These roots with a host loader for owned dynamic loading scopes.
    /// Generated mappings never access a filesystem or cache documents.</summary>
    public FerruleBasicSourceRoots WithDynamicSourceLoader(IFerruleDynamicSourceLoader loader)
    {
        ArgumentNullException.ThrowIfNull(loader);
        return new(_owned, _projection, _supplied, _unique, _retainedSlots, loader);
    }
    internal FerruleOwnedSourceRoots OwnedRoots => _owned;
    internal FerruleGroup Projection => _projection;
    internal FerruleOwnedSourceLimits Limits => _owned.Limits;
    internal int SuppliedCount => _supplied.Length;
    internal int SuppliedIndex(int index) => _supplied[index];
    internal int UniqueCount => _unique.Length;
    internal int UniqueIndex(int index) => _unique[index];
    public static FerruleBasicSourceRoots Create(FerruleInstance primary,
        IReadOnlyList<FerruleDeclaredNamedSourceRoot> declared, FerruleOwnedSourceLimits limits)
    {
        ArgumentNullException.ThrowIfNull(primary); ArgumentNullException.ThrowIfNull(declared);
        try
        {
            // Preserve the direct-core arithmetic precedence before new bridge derivation.
            _ = OwnedSourceAccounting.CSharpSlots(limits);
            var budget = new BasicSourceBudget(limits, false);
            var count = OwnedSourceAccounting.Add((nuint)declared.Count, 1, FerruleOwnedSourceLimitKind.Roots);
            OwnedSourceAccounting.Limit(0, count, limits.MaxRoots, FerruleOwnedSourceLimitKind.Roots);
            budget.Charge(count); // owned immutable binding cells and its roots descriptor
            var owned = FerruleOwnedSourceRoots.Create(primary, declared, limits);
            var suppliedCount = 0; var uniqueCount = 0;
            for (var i = 1; (nuint)i < owned.RegistryCount; i++)
            {
                if (owned.Instance(i) is null) continue;
                suppliedCount++;
                var first = true;
                for (var j = 1; j < i; j++)
                    if (owned.Instance(j) is not null && StringComparer.Ordinal.Equals(owned.Name(j), owned.Name(i)))
                    { first = false; break; }
                if (first) uniqueCount++;
            }
            // All arrays have exact capacities. The temporary fields coexist with
            // the factory snapshot; field objects reuse admitted immutable names.
            budget.Charge((nuint)suppliedCount);
            budget.Charge((nuint)uniqueCount * 2);
            var supplied = new int[suppliedCount]; var unique = new int[uniqueCount];
            var fields = new FerruleField[uniqueCount];
            var a = 0; var u = 0;
            for (var i = 1; (nuint)i < owned.RegistryCount; i++)
            {
                var value = owned.Instance(i); if (value is null) continue;
                supplied[a++] = i - 1;
                var first = true;
                for (var j = 0; j < u; j++)
                    if (StringComparer.Ordinal.Equals(owned.Name(unique[j] + 1), owned.Name(i)))
                    { first = false; break; }
                if (!first) continue;
                unique[u] = i - 1;
                budget.Charge(1);
                fields[u++] = new FerruleField(owned.Name(i)!, value);
            }
            var projection = FerruleGroup.FromBasicSourceProjection(fields, count - 1, budget.Slots);
            fields = Array.Empty<FerruleField>();
            budget.Release((nuint)uniqueCount); // the sole temporary array is no longer retained
            budget.Charge(1);
            return new FerruleBasicSourceRoots(owned, projection, supplied, unique, budget.Slots.Used);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }

    /// <summary>Checks one supplied list against immutable complete program declarations.</summary>
    /// <remarks>The catalogue names, modes and order must remain immutable for this call;
    /// ReadOnlySpan alone does not guarantee immutability. The supplied Count and each reached
    /// item/accessor are consumed once. Limits cover library metadata, not caller allocations,
    /// source trees, execution time or total process memory.</remarks>
    public static FerruleBasicSourceRoots CreateFromNamedInputs<T, TAccessor>(FerruleInstance primary,
        ReadOnlySpan<FerruleBasicNamedSourceDeclaration> catalogue, IReadOnlyList<T> supplied,
        TAccessor accessor, FerruleOwnedSourceLimits limits)
        where TAccessor : struct, IFerruleNamedSourceInputAccessor<T>
    {
        ArgumentNullException.ThrowIfNull(primary); ArgumentNullException.ThrowIfNull(supplied);
        var owned = FerruleOwnedSourceRoots.FromNamedInputs(primary, catalogue, supplied, accessor, limits, out var budget);
        return FromAdmittedInputs(owned, budget);
    }
    private static FerruleBasicSourceRoots FromAdmittedInputs(FerruleOwnedSourceRoots owned, BasicSourceBudget budget)
    {
        try
        {
            var count = owned.RegistryCount;
            var suppliedCount = 0; var uniqueCount = 0;
            for (var i = 1; (nuint)i < owned.RegistryCount; i++)
            {
                if (owned.Instance(i) is null) continue;
                suppliedCount++;
                var first = true;
                for (var j = 1; j < i; j++)
                    if (owned.Instance(j) is not null && StringComparer.Ordinal.Equals(owned.Name(j), owned.Name(i)))
                    { first = false; break; }
                if (first) uniqueCount++;
            }
            // All arrays have exact capacities. The temporary fields coexist with
            // the factory snapshot; field objects reuse admitted immutable names.
            budget.Charge((nuint)suppliedCount);
            budget.Charge((nuint)uniqueCount * 2);
            var supplied = new int[suppliedCount]; var unique = new int[uniqueCount];
            var fields = new FerruleField[uniqueCount];
            var a = 0; var u = 0;
            for (var i = 1; (nuint)i < owned.RegistryCount; i++)
            {
                var value = owned.Instance(i); if (value is null) continue;
                supplied[a++] = i - 1;
                var first = true;
                for (var j = 0; j < u; j++)
                    if (StringComparer.Ordinal.Equals(owned.Name(unique[j] + 1), owned.Name(i)))
                    { first = false; break; }
                if (!first) continue;
                unique[u] = i - 1;
                budget.Charge(1);
                fields[u++] = new FerruleField(owned.Name(i)!, value);
            }
            var projection = FerruleGroup.FromBasicSourceProjection(fields, count - 1, budget.Slots);
            fields = Array.Empty<FerruleField>();
            budget.Release((nuint)uniqueCount); // the sole temporary array is no longer retained
            budget.Charge(1);
            // Counters stay admitted through allocation but are not part of returned roots.
            var retained = budget.Slots.Used;
            if (retained < 2) throw BasicSourceAccounting.Overflow();
            return new FerruleBasicSourceRoots(owned, projection, supplied, unique, retained - 2);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }

    public FerruleBasicScopeContext RootContext(FerruleExecutionContext? execution = null)
    {
        try
        {
            var budget = new BasicSourceBudget(Limits, false);
            Seed(budget, includeRootState: false);
            var legacy = ScopeContext.CreateBasicRoot(_owned.Instance(0), _projection, execution, budget);
            budget.Charge(3); // two origin cells and one paired view
            var origins = new[] { new BasicFrameOrigin(-1, Array.Empty<string>(), Projection: true),
                new BasicFrameOrigin(0, Array.Empty<string>()) };
            budget.Charge(1);
            var owned = FerruleOwnedScopeContext.CreateRoot(_owned);
            return FerruleBasicScopeContext.Create(this, legacy, owned, origins, legacyContainers: 1);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
    internal void Seed(BasicSourceBudget budget, bool includeRootState = true)
    {
        budget.Charge(_retainedSlots);
        budget.Text(_owned.RegistryText);
        if (includeRootState) budget.Charge(1); // the one shared root filter-map state, never charged from projection slack
    }
    internal int ProjectionKey(string actualField)
    {
        for (var i = 0; i < _unique.Length; i++)
            if (StringComparer.Ordinal.Equals(_owned.Name(_unique[i] + 1), actualField)) return _unique[i] + 1;
        throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.InvalidDeclarationWitness,
            declaredCount: _owned.RegistryCount - 1);
    }
}


/// <summary>An immutable paired basic view; it cannot be reconstructed from an arbitrary legacy view.</summary>
public sealed partial class FerruleBasicScopeContext
{
    private readonly BasicFrameOrigin[] _origins;
    // The documents a loading scope requested; only that scope's own walk of
    // their owner consults them.
    private FerruleOwnedDynamicDocuments.Loaded? _loading;
    private readonly int _legacyContainers;
    private readonly FerruleOwnedScopeContext _owned;
    private FerruleBasicScopeContext(FerruleBasicSourceRoots roots, ScopeContext legacy,
        FerruleOwnedScopeContext owned, BasicFrameOrigin[] origins, int legacyContainers)
    { Roots = roots; LegacyView = legacy; _owned = owned; _origins = origins; _legacyContainers = legacyContainers; }
    internal static FerruleBasicScopeContext Create(FerruleBasicSourceRoots roots, ScopeContext legacy,
        FerruleOwnedScopeContext owned, BasicFrameOrigin[] origins, int legacyContainers) =>
        new(roots, legacy, owned, origins, legacyContainers);
    public ScopeContext LegacyView { get; }
    internal FerruleBasicSourceRoots Roots { get; }
    internal FerruleOwnedScopeContext OwnedView => _owned;
    internal int OriginCount => _origins.Length;
    internal int LegacyContainerCost => _legacyContainers;
    internal BasicFrameOrigin OriginAt(int index) => _origins[index];
    internal bool IsPrimaryFailureRoot()
    {
        var primary = Roots.OwnedRoots.Instance(0);
        return primary is FerruleGroup && ReferenceEquals(_owned.Roots, Roots.OwnedRoots) &&
            _owned.FrameCount == 0 && LegacyView.IsPrimaryFailureRoot(primary, Roots.Projection) &&
            _origins.Length == 2 && _origins[0].Projection && _origins[0].OwnerKey == -1 &&
            _origins[0].Canonical.Count == 0 && _origins[0].RawOrdinal is null &&
            !_origins[1].Projection && _origins[1].OwnerKey == 0 &&
            _origins[1].Canonical.Count == 0 && _origins[1].RawOrdinal is null;
    }
    internal bool HasPrimaryFailureNamedFallback(string collection) =>
        Roots.OwnedRoots.Instance(0) is FerruleGroup primary &&
        !primary.TryGetField(collection, out _) && Roots.Projection.TryGetField(collection, out _);
    internal void Seed(BasicSourceBudget budget)
    {
        LegacyView.RequireBasicContext();
        budget.Charge(2); // paired and owned view descriptors
        budget.Charge((nuint)_owned.FrameCount);
        for (var i = 0; i < _owned.FrameCount; i++)
        { var f = _owned.FrameAt(i); budget.Charge((nuint)f.Path.Count); budget.Path(f.Path); }
        budget.Charge((nuint)_origins.Length);
        for (var i = 0; i < _origins.Length; i++)
        { budget.Charge((nuint)_origins[i].Canonical.Count); budget.Path(_origins[i].Canonical); }
        LegacyView.SeedBasicLegacy(budget);
        budget.Charge((nuint)_legacyContainers);
    }
    internal BasicSourceBudget Begin(bool phase = false)
    {
        var budget = new BasicSourceBudget(Roots.Limits, phase);
        Roots.Seed(budget); Seed(budget); return budget;
    }
    internal OwnedQuery Query(BasicSourceBudget budget, FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> path, IReadOnlyList<string>? frame = null, nuint? legacyPathMaximum = null,
        nuint? canonicalSelectorLength = null) =>
        new(_owned, owner, path, frame, budget.Slots, budget.Bytes, legacyPathMaximum, canonicalSelectorLength);
    public FerruleValue ResolveOwnedField(FerruleOwnedSourceOwner owner, IReadOnlyList<string> path,
        IReadOnlyList<string>? frame = null)
    {
        try { var b = Begin(); return _owned.ResolveBasicField(owner, path, frame, Query(b, owner, path, frame)); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
    /// <summary>Reads an explicit root pin using the generated exact-owner schema proof.</summary>
    public FerruleValue ResolveOwnedFieldWithRootPin(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> path, IReadOnlyList<string>? frame, bool declaredNonrepeating)
    {
        try
        {
            var budget = Begin();
            return _owned.ResolveBasicFieldWithRootPin(owner, path, frame,
                Query(budget, owner, path, frame), declaredNonrepeating);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
    public nuint OwnedPosition(FerruleOwnedSourceOwner owner, IReadOnlyList<string>? collection = null)
    {
        try { var b = Begin(); return _owned.BasicPosition(owner, collection,
            Query(b, owner, collection ?? Array.Empty<string>())); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }

    public FerruleValue OwnedRowDocumentPath(FerruleOwnedSourceOwner owner, IReadOnlyList<string>? collection = null)
    {
        try
        {
            var budget = Begin();
            return _owned.BasicRowDocumentPath(owner, collection,
                Query(budget, owner, collection ?? Array.Empty<string>()));
        }
        catch (FerruleOwnedSourceException error) { throw FerruleRuntimeException.FromOwnedSource(error); }
        catch (FerruleBasicSourceBridgeException error) { throw FerruleRuntimeException.FromBasicSourceBridge(error); }
    }
    public FerruleValue ResolveLegacyField(IReadOnlyList<string> path, IReadOnlyList<string>? frame = null)
    {
        try
        {
            var b = Begin(); _ = Query(b, FerruleOwnedSourceOwner.Primary, path, frame);
            return frame is null ? LegacyView.ResolveBasicSourceField(path) : LegacyView.ResolveBasicSourceFieldInFrame(frame, path);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
    public long LegacyPosition(IReadOnlyList<string> collection)
    {
        try { var b = Begin(); _ = Query(b, FerruleOwnedSourceOwner.Primary, collection); return LegacyView.Position(collection); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
    public IReadOnlyList<FerruleBasicSourceCandidate> IterateSource(IReadOnlyList<string> path) => WalkSourceCohort(path);
    public IReadOnlyList<FerruleBasicSourceCandidate> IterateOwnedSource(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string> path) => WalkOwnedSourceCohort(owner, path);
    public FerruleRetainedSourceCohort WalkSourceCohort(IReadOnlyList<string> path)
    {
        try { return FerruleRetainedSourceCohort.Walk(this, path, null); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
    public FerruleRetainedSourceCohort WalkOwnedSourceCohort(FerruleOwnedSourceOwner owner, IReadOnlyList<string> path)
    {
        try { return FerruleRetainedSourceCohort.Walk(this, path, owner); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
    /// <summary>
    /// Visits authenticated static primary failure rows after complete walk admission.
    /// Unsupported terminal rows are refused only when reached, before the callback.
    /// </summary>
    public void VisitPrimaryFailureRows(IReadOnlyList<string> path, Action<FerruleBasicScopeContext> visit)
    {
        ArgumentNullException.ThrowIfNull(path); ArgumentNullException.ThrowIfNull(visit);
        try { FerruleRetainedSourceCohort.VisitPrimaryFailureRows(this, path, visit); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
    internal FerruleBasicScopeContext CloneBudgeted(BasicSourceBudget budget, BasicOccurrenceCursor? cursor = null,
        nuint index = 0)
    {
        var owner = cursor.HasValue && cursor.Value.OwnerKey != 0 ?
            FerruleOwnedSourceOwner.Named(Roots.OwnedRoots.Name(cursor.Value.OwnerKey)!) : FerruleOwnedSourceOwner.Primary;
        var q = Query(budget, owner, Array.Empty<string>());
        var owned = _owned.CloneBasicObserved(q, cursor?.OwnedSlot, index);
        var legacy = LegacyView.CloneBasicLegacy(budget, cursor?.LegacySlot, cursor.HasValue ? (int)index : 0);
        budget.Charge((nuint)_origins.Length + 1);
        var origins = new BasicFrameOrigin[_origins.Length];
        for (var i = 0; i < origins.Length; i++)
        { budget.Charge((nuint)_origins[i].Canonical.Count); budget.Path(_origins[i].Canonical); origins[i] = _origins[i]; }
        return new FerruleBasicScopeContext(Roots, legacy, owned, origins, 2) { _loading = _loading };
    }
    public FerruleBasicScopeContext CloneView()
    {
        try { var budget = Begin(); return CloneBudgeted(budget); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
    internal BasicSortCopyCost SortCopyCost(bool hasCursor, string? ownerName)
    {
        var slots = BasicSourceAccounting.Add(Roots.OwnedRoots.AmbiguityReservationCount, 2);
        var ownerBytes = ownerName is null ? (nuint)0 : OwnedSourceAccounting.Text(ownerName);
        var bytes = OwnedSourceAccounting.Add(ownerBytes, ownerBytes, FerruleOwnedSourceLimitKind.MetadataBytes);
        slots = BasicSourceAccounting.Add(slots, (nuint)_owned.FrameCount + 1);
        for (var i = 0; i < _owned.FrameCount; i++)
        {
            var path = _owned.FrameAt(i).Path;
            slots = BasicSourceAccounting.Add(slots, (nuint)path.Count);
            bytes = OwnedSourceAccounting.Add(bytes, OwnedSourceAccounting.PathText(path), FerruleOwnedSourceLimitKind.MetadataBytes);
        }
        slots = BasicSourceAccounting.Add(slots, (nuint)LegacyView.Frames.Count);
        slots = BasicSourceAccounting.Add(slots, (nuint)LegacyView.BasicCollectionCount + 3 + (hasCursor ? (nuint)1 : 0));
        for (var i = 0; i < LegacyView.BasicCollectionCount; i++)
            bytes = OwnedSourceAccounting.Add(bytes, OwnedSourceAccounting.PathText(LegacyView.BasicCollectionPath(i)), FerruleOwnedSourceLimitKind.MetadataBytes);
        slots = BasicSourceAccounting.Add(slots, (nuint)_origins.Length + 1);
        for (var i = 0; i < _origins.Length; i++)
        {
            slots = BasicSourceAccounting.Add(slots, (nuint)_origins[i].Canonical.Count);
            bytes = OwnedSourceAccounting.Add(bytes, OwnedSourceAccounting.PathText(_origins[i].Canonical), FerruleOwnedSourceLimitKind.MetadataBytes);
        }
        return new(BasicSourceAccounting.Add(slots, 1), bytes); // the copied candidate
    }
    /// <summary>Sorts one authentic original cohort under one bounded operation.</summary>
    public IReadOnlyList<FerruleInstance> SortRetainedSource(FerruleRetainedSourceCohort original,
        IReadOnlyList<FerruleSortKey<FerruleBasicScopeContext>> keys, bool filterBeforeSort,
        Func<FerruleBasicScopeContext, bool>? predicate, Func<FerruleBasicScopeContext, FerruleInstance> target)
    {
        try { return FerruleBasicSortOperation.Run(this, original, keys, filterBeforeSort, predicate, target); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
    /// <summary>Applies ordered windows to an authentic cohort under one cumulative phase ledger.</summary>
    public IReadOnlyList<FerruleInstance> VisitWindowedSourceCohort(FerruleRetainedSourceCohort original,
        IReadOnlyList<FerruleSortKey<FerruleBasicScopeContext>> keys, bool filterBeforeSort,
        Func<FerruleBasicScopeContext, bool>? predicate,
        IReadOnlyList<Func<FerruleBasicScopeContext, FerruleSequenceWindow>> bounds, bool implicitFirst,
        Func<FerruleBasicScopeContext, FerruleInstance> target)
    {
        try { return FerruleBasicWindowOperation.Run(this, original, keys, filterBeforeSort, predicate, bounds, implicitFirst, target); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
    public FerruleCompletePhaseCohort ReindexRetainedPhase(FerruleRetainedSourceCohort original,
        FerrulePhaseSelection selection)
    {
        try { return FerruleCompletePhaseCohort.Reindex(this, original, selection); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
    public FerruleBasicFilterPhase BeginRetainedFilterPhase(FerruleRetainedSourceCohort original)
    {
        try { return FerruleBasicFilterPhase.Begin(this, original); }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
}


internal interface BasicSourceIds
{
    int Count { get; }
    nuint SourceIdAt(int index);
}
internal interface BasicPhaseIndices
{
    int Count { get; }
    nuint IndexAt(int index);
}
internal interface BasicCompleteRows : BasicSourceIds
{
    FerruleBasicSourceCandidate At(int index);
    FerruleRetainedSourceCohort.Witness SourceWitness { get; }
}

/// <summary>A complete original source walk with read-only candidate access.</summary>
public sealed partial class FerruleRetainedSourceCohort : IReadOnlyList<FerruleBasicSourceCandidate>, BasicCompleteRows
{
    private FerruleBasicSourceCandidate[] _items = Array.Empty<FerruleBasicSourceCandidate>();
    private int _count;
    private bool _sealed;
    private readonly Witness _witness;
    private FerruleRetainedSourceCohort(FerruleBasicScopeContext parent, BasicSourceBudget budget,
        bool primaryFailureRows = false)
    { _witness = new Witness(this, parent, budget, primaryFailureRows); }
    public int Count { get { RequireSealed(); return _count; } }
    public FerruleBasicSourceCandidate this[int index]
    { get { RequireSealed(); return (uint)index < (uint)_count ? _items[index] : throw new ArgumentOutOfRangeException(nameof(index)); } }
    private void RequireSealed()
    { if (!_sealed) throw new InvalidOperationException("Source cohort is not published."); }
#if FERRULE_COMPONENT_ORACLES
    // CR-REQUEST ONLY: guarded negative-fixture seam, not a production API.
    // Corrupt a fresh three-row walk without updating its authenticated witness.
    internal void StartingWitnessFaultForTests(int fault, FerruleBasicSourceCandidate? donor = null)
    {
        RequireSealed();
        if (_count != 3) throw new InvalidOperationException("Expected the literal three-row fixture.");
        var c = _items[1];
        switch (fault)
        {
            case 0: _items[1] = new(c.Context, c.SourceWitness, _items[0].SourceId, c.Cursor, c.GroupTerminal); break;
            case 1: _count--; break;
            case 2: (_items[0], _items[1]) = (_items[1], _items[0]); break;
            case 3: _items[1] = new(c.Context, c.SourceWitness, c.SourceId, c.Cursor,
                c.GroupTerminal!.Value with { OriginCut = int.MaxValue }); break;
            case 4: _items[1] = new(c.Context, c.SourceWitness, c.SourceId, c.Cursor); break;
            case 5: _items[1] = new(c.Context, donor!.SourceWitness, c.SourceId, c.Cursor, c.GroupTerminal); break;
            default: throw new ArgumentOutOfRangeException(nameof(fault));
        }
    }
#endif
    FerruleBasicSourceCandidate BasicCompleteRows.At(int index) => this[index];
    nuint BasicSourceIds.SourceIdAt(int index) => this[index].SourceId;
    Witness BasicCompleteRows.SourceWitness => _witness;
    public IEnumerator<FerruleBasicSourceCandidate> GetEnumerator()
    { RequireSealed(); for (var i = 0; i < _count; i++) yield return _items[i]; }
    System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    internal Witness SourceWitness => _witness;
    internal int RetainedCapacity => _items.Length;
    internal static FerruleRetainedSourceCohort Walk(FerruleBasicScopeContext parent,
        IReadOnlyList<string> path, FerruleOwnedSourceOwner? owner) => Walk(parent, path, owner, false);
    private static FerruleRetainedSourceCohort Walk(FerruleBasicScopeContext parent,
        IReadOnlyList<string> path, FerruleOwnedSourceOwner? owner, bool primaryFailureRows)
    {
        if (owner.HasValue && parent.LoadingFor(owner.Value) is { } loading)
            return WalkLoaded(parent, loading, owner.Value, path);
        var budget = parent.Begin();
        budget.Charge(2); // original holder and shared per-walk witness, replacing the native output containers
        var result = new FerruleRetainedSourceCohort(parent, budget, primaryFailureRows);
        try
        {
            if (owner.HasValue)
                _ = parent.OwnedView.IterateBasicSource(owner.Value, path, result._witness);
            else
            {
                result._witness.SetLegacyQuery(path);
                parent.LegacyView.IterateBasicSource(path, result._witness, result._witness, result._witness.LegacySelection);
            }
            result._sealed = true;
            result._witness.Seal(result._count);
            return result;
        }
        finally { result._witness.ReleaseScratch(); }
    }
    internal static void VisitPrimaryFailureRows(FerruleBasicScopeContext parent,
        IReadOnlyList<string> path, Action<FerruleBasicScopeContext> visit)
    {
        if (path.Count != 1 || string.IsNullOrEmpty(path[0]) || !parent.IsPrimaryFailureRoot() ||
            parent.HasPrimaryFailureNamedFallback(path[0]))
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.UnsupportedPrimaryFailureContext);
        // Refuse an actually supplied named fallback before acquiring work, as
        // native does. This negative guard is not a provenance witness: the
        // observed legacy SelectedBase still independently authenticates Primary.
        // This purpose-bound holder never escapes. In particular a specialized
        // repeated terminal can exist in its complete metadata but is never
        // published through a public cohort or passed to caller code.
        var rows = Walk(parent, path, null, true);
        try
        {
            for (var i = 0; i < rows.Count; i++)
            {
                var context = rows[i].Context;
                var terminal = context.LegacyView.Frames[^1];
                if (terminal is FerruleDocumentSet or FerruleMappedSequence)
                    throw new FerruleBasicSourceBridgeException(
                        FerruleBasicSourceBridgeErrorKind.UnsupportedBasicContext,
                        found: terminal is FerruleDocumentSet ? FerruleOwnedSourceInstanceKind.DocumentSet :
                            FerruleOwnedSourceInstanceKind.MappedSequence);
                visit(context);
            }
        }
        finally { GC.KeepAlive(rows); }
    }
    private void ReserveOutput(BasicSourceBudget budget)
    {
        BasicSourceAccounting.Limit(0, (nuint)_count + 1, _witness.Parent.Roots.Limits.MaxWalkContexts,
            FerruleBasicSourceBridgeComponent.WalkContexts);
        if (_count < _items.Length) return;
        var capacity = (nuint)_count + 1;
        budget.Charge(capacity);
        var next = new FerruleBasicSourceCandidate[BasicSourceAccounting.ArrayLength(capacity)];
        Array.Copy(_items, next, _count);
        var old = _items.Length; _items = next; budget.Release((nuint)old);
    }
    private void Append(FerruleBasicScopeContext context, BasicOccurrenceCursor? cursor, BasicSourceBudget budget,
        BasicGroupTerminal terminal)
    {
        budget.Charge(1);
        _items[_count] = new FerruleBasicSourceCandidate(context, _witness, (nuint)_count, cursor, terminal);
        _count++;
    }

    internal sealed class Witness : ScopeContext.BasicWalkObserver,
        FerruleOwnedScopeContext.BasicOwnedWalkObserver, ICollection<ScopeContext>
    {
        private FerruleRetainedSourceCohort? _building;
        private readonly BasicSourceBudget _budget;
        private readonly record struct State(int Owner, IReadOnlyList<string> Canonical, bool CanonicalOwned, bool Projection,
            FerruleInstance? Plain, int ScratchDepth, string? Redirect);
        private State[] _states = Array.Empty<State>();
        private int _stateDepth;
        private FerruleOwnedScopeContext.Observation[] _scratch = Array.Empty<FerruleOwnedScopeContext.Observation>();
        private int _scratchCount;
        private int _inheritedOwned;
        private int _owner;
        private IReadOnlyList<string> _canonical = Array.Empty<string>();
        private bool _projection;
        private bool _canonicalOwned;
        private FerruleInstance? _plain;
        private ScopeContext? _pendingLegacy;
        private OwnedQuery? _query;
        private bool _finished;
        private readonly bool _primaryFailureRows;
        private IReadOnlyList<string> _initialPrefix = Array.Empty<string>();
        private int _initialPrefixCount, _plainRawCount, _identityOwnedSlot;
        private string? _plainRawFirst, _redirect;
        private ScopeContext.BasicSourceSelection _legacySelection;
        private int _selectedAliasOwner = -1;
        internal ScopeContext.BasicSourceSelection LegacySelection => _legacySelection;
        internal override IReadOnlyList<string> InitialLegacyPrefix => _initialPrefix;
        internal override int InitialLegacyPrefixCount => _initialPrefixCount;
        internal Witness(FerruleRetainedSourceCohort building, FerruleBasicScopeContext parent, BasicSourceBudget budget,
            bool primaryFailureRows)
        { _building = building; Parent = parent; _budget = budget; _primaryFailureRows = primaryFailureRows; }
        internal FerruleBasicScopeContext Parent { get; }
        internal int OriginalCount { get; private set; }
        internal int SelectionCalls { get; private set; }
        internal bool IsComplete => _finished;
        internal void Seal(int count) { OriginalCount = count; _finished = true; _building = null; }
        internal void SetLegacyQuery(IReadOnlyList<string> path)
        {
            _legacySelection = Parent.LegacyView.PreselectBasicSource(path);
            var suffix = _legacySelection.Count < 0 ? 0 : _legacySelection.Count;
            nuint canonical = 0;
            var redirects = false;
            if (_legacySelection.Frame >= 0)
            {
                var origin = Parent.OriginAt(_legacySelection.Frame);
                canonical = (nuint)origin.Canonical.Count;
                if (_legacySelection.HasField && _legacySelection.First is not null)
                {
                    if (origin.Projection && ReferenceEquals(Parent.LegacyView.Frames[_legacySelection.Frame], Parent.Roots.Projection))
                    {
                        _selectedAliasOwner = Parent.Roots.ProjectionKey(_legacySelection.First);
                        canonical = 0; redirects = true;
                    }
                    else redirects = origin.Kind == BasicOriginKind.GroupWrapper &&
                        StringComparer.Ordinal.Equals(_legacySelection.First, origin.WrapperField);
                }
            }
            if (redirects) suffix--;
            var rawMaximum = redirects ? BasicSourceAccounting.Add(Parent.Roots.Limits.MaxPathSegments, 1) : Parent.Roots.Limits.MaxPathSegments;
            var completeCanonical = BasicSourceAccounting.Add(canonical, (nuint)suffix);
            // Preserve original OwnedSource raw-P and Unicode admission first.
            // Only a real preselected alias/redirect receives Q; full canonical
            // prefix admission then precedes all query byte charges.
            _query = Parent.Query(_budget, FerruleOwnedSourceOwner.Primary, path,
                legacyPathMaximum: rawMaximum, canonicalSelectorLength: completeCanonical);
            _inheritedOwned = Parent.OwnedView.FrameCount;
            _budget.Charge((nuint)_inheritedOwned);
            for (var i = 0; i < _inheritedOwned; i++)
            { var f = Parent.OwnedView.FrameAt(i); _budget.Charge((nuint)f.Path.Count); _budget.Path(f.Path); }
            _scratch = new FerruleOwnedScopeContext.Observation[_inheritedOwned];
            for (var i = 0; i < _scratch.Length; i++) _scratch[i] = Parent.OwnedView.FrameAt(i);
            _scratchCount = _inheritedOwned;
        }
        internal override nuint LegacyCapacityMaximum => BasicSourceAccounting.LegacyFrames(Parent.Roots.Limits);
        internal override void ReserveSlots(nuint slots) => _budget.Charge(slots);
        internal override void ReleaseSlots(nuint slots) => _budget.Release(slots);
        internal override void ReserveOutputCell() => _building!.ReserveOutput(_budget);
        internal override void ReservePath(IReadOnlyList<string> path, string? appended)
        { _budget.Path(path); if (appended is not null) _budget.Text(OwnedSourceAccounting.Text(appended)); }
        internal override void ProbeBase(FerruleInstance current)
        {
            if (current is FerruleDocumentSet or FerruleMappedSequence)
                throw new FerruleBasicSourceBridgeException(_primaryFailureRows ?
                    FerruleBasicSourceBridgeErrorKind.UnsupportedPrimaryFailureContext : FerruleBasicSourceBridgeErrorKind.UnsupportedBasicContext,
                    found: _primaryFailureRows ? null : InstanceKind(current));
        }
        internal override void SelectedBase(int frameIndex, FerruleInstance current)
        {
            SelectionCalls++;
            var origin = Parent.OriginAt(frameIndex);
            // The selected legacy BASE, not its child payload, must be the
            // actual Primary Group. A named projection cannot authenticate it,
            // even when the named payload aliases the primary source tree.
            if (_primaryFailureRows && (current is not FerruleGroup ||
                !ReferenceEquals(current, Parent.Roots.OwnedRoots.Instance(0)) ||
                origin.OwnerKey != 0 || origin.Canonical.Count != 0 || origin.Projection))
                throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.UnsupportedPrimaryFailureContext);
            _owner = origin.OwnerKey; _canonical = origin.Canonical; _canonicalOwned = false; _projection = origin.Projection;
            if (origin.Kind != BasicOriginKind.Ordinary)
            {
                _initialPrefix = Parent.LegacyView.BasicCollectionPath(origin.GroupCollection);
                _initialPrefixCount = _initialPrefix.Count - (origin.Kind == BasicOriginKind.GroupWrapper ? 1 : 0);
                _redirect = origin.Kind == BasicOriginKind.GroupWrapper ? origin.WrapperField : null;
            }
        }
        private static FerruleOwnedSourceInstanceKind InstanceKind(FerruleInstance current) => current switch
        {
            FerruleScalar => FerruleOwnedSourceInstanceKind.Scalar,
            FerruleGroup => FerruleOwnedSourceInstanceKind.Group,
            FerruleRepeated => FerruleOwnedSourceInstanceKind.Repeated,
            FerruleDocumentSet => FerruleOwnedSourceInstanceKind.DocumentSet,
            FerruleMappedSequence => FerruleOwnedSourceInstanceKind.MappedSequence,
            _ => throw new InvalidOperationException("Unknown immutable source kind."),
        };
        internal override void Enter(FerruleInstance current, int consumed,
            IReadOnlyList<string> relativePrefix, int frameCount, int collectionCount)
        {
            if (current is FerruleDocumentSet or FerruleMappedSequence)
                throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.UnsupportedBasicContext,
                    found: InstanceKind(current));
            BasicSourceAccounting.Limit((nuint)_stateDepth, 1, BasicSourceAccounting.Recursion(Parent.Roots.Limits),
                FerruleBasicSourceBridgeComponent.RecursionDepth);
            if (_stateDepth == _states.Length)
            {
                var capacity = (nuint)_stateDepth + 1; _budget.Charge(capacity);
                var next = new State[BasicSourceAccounting.ArrayLength(capacity)];
                Array.Copy(_states, next, _stateDepth); var old = _states.Length; _states = next; _budget.Release((nuint)old);
            }
            _states[_stateDepth++] = new(_owner, _canonical, _canonicalOwned, _projection, _plain, _scratchCount, _redirect);
        }
        internal override void Leave()
        {
            var state = _states[--_stateDepth]; _states[_stateDepth] = default;
            var released = _canonicalOwned && !ReferenceEquals(_canonical, state.Canonical) ? (nuint)_canonical.Count : 0;
            _owner = state.Owner; _canonical = state.Canonical; _canonicalOwned = state.CanonicalOwned;
            _projection = state.Projection; _plain = state.Plain;
            _redirect = state.Redirect;
            if (released != 0) _budget.Release(released);
            // The collection push/pop callbacks own occurrence lifetime; this is the recursion state only.
        }
        private IReadOnlyList<string> CopyCanonical(IReadOnlyList<string> path)
        {
            BasicSourceAccounting.Limit(0, (nuint)path.Count, Parent.Roots.Limits.MaxPathSegments,
                FerruleBasicSourceBridgeComponent.PathSegments);
            _budget.Charge((nuint)path.Count); _budget.Path(path);
            var copy = new string[path.Count]; for (var i = 0; i < copy.Length; i++) copy[i] = path[i];
            return copy;
        }
        internal override void ConsumedField(string actualField, FerruleInstance actualChild)
        {
            if (_redirect is not null)
            {
                if (!StringComparer.Ordinal.Equals(actualField, _redirect)) throw BasicPhaseAdmission.Invalid(0);
                _redirect = null; return;
            }
            if (_projection)
            {
                if (_selectedAliasOwner <= 0 || !StringComparer.Ordinal.Equals(actualField, _legacySelection.First))
                    throw BasicPhaseAdmission.Invalid(0);
                _owner = _selectedAliasOwner; _canonical = Array.Empty<string>(); _canonicalOwned = false; _projection = false; return;
            }
            var count = BasicSourceAccounting.Add((nuint)_canonical.Count, 1);
            BasicSourceAccounting.Limit(0, count, Parent.Roots.Limits.MaxPathSegments, FerruleBasicSourceBridgeComponent.PathSegments);
            _budget.Charge(count); _budget.Path(_canonical); _budget.Text(OwnedSourceAccounting.Text(actualField));
            var path = new string[BasicSourceAccounting.ArrayLength(count)];
            for (var i = 0; i < _canonical.Count; i++) path[i] = _canonical[i];
            path[^1] = actualField; _canonical = path; _canonicalOwned = true;
        }
        internal override void BeforeCollectionPush()
        {
            BasicSourceAccounting.Limit((nuint)_scratchCount, 1, Parent.Roots.Limits.MaxActiveFrames,
                FerruleBasicSourceBridgeComponent.ActiveFrames);
            BasicSourceAccounting.Limit(0, (nuint)Parent.OriginCount + (nuint)(_scratchCount - _inheritedOwned) + 1,
                LegacyCapacityMaximum, FerruleBasicSourceBridgeComponent.LegacyFrames);
        }
        internal override void BeforePlain()
        {
            BasicSourceAccounting.Limit(0, (nuint)Parent.OriginCount + (nuint)(_scratchCount - _inheritedOwned) + 1,
                LegacyCapacityMaximum, FerruleBasicSourceBridgeComponent.LegacyFrames);
        }
        internal override void CollectionPushed(FerruleInstance item, IReadOnlyList<string> legacyPath, int rawPosition)
        {
            BasicSourceAccounting.Limit((nuint)_scratchCount, 1, Parent.Roots.Limits.MaxActiveFrames,
                FerruleBasicSourceBridgeComponent.ActiveFrames);
            if (_scratchCount == _scratch.Length)
            {
                var capacity = (nuint)_scratchCount + 1; _budget.Charge(capacity);
                var next = new FerruleOwnedScopeContext.Observation[BasicSourceAccounting.ArrayLength(capacity)];
                Array.Copy(_scratch, next, _scratchCount); var old = _scratch.Length; _scratch = next; _budget.Release((nuint)old);
            }
            _scratch[_scratchCount++] = new(_owner, CopyCanonical(_canonical), item, (nuint)rawPosition);
        }
        internal override void CollectionPopped()
        {
            var f = _scratch[--_scratchCount]; _scratch[_scratchCount] = default;
            _budget.Release((nuint)f.Path.Count);
        }
        internal override void Plain(FerruleInstance item, IReadOnlyList<string> rawPath)
        { _plain = item; _plainRawCount = rawPath.Count; _plainRawFirst = rawPath.Count == 0 ? null : rawPath[0]; }
        internal override void PlainPopped() => _plain = null;
        internal override void BeforeCandidate(int newFrames, int newCollections, int count)
        {
            BasicSourceAccounting.Limit(0, (nuint)count + 1, Parent.Roots.Limits.MaxWalkContexts,
                FerruleBasicSourceBridgeComponent.WalkContexts);
            BasicSourceAccounting.Limit(0, (nuint)Parent.OriginCount + (nuint)newFrames,
                LegacyCapacityMaximum, FerruleBasicSourceBridgeComponent.LegacyFrames);
        }
        internal override void Candidate(ScopeContext actual, int newFrames, int newCollections)
        {
            if (!ReferenceEquals(_pendingLegacy, actual)) throw BasicPhaseAdmission.Invalid(0);
            var q = _query!;
            var owned = FerruleOwnedScopeContext.FromBasicObserved(Parent.Roots.OwnedRoots,
                _scratch.AsSpan(0, _scratchCount), q);
            var origins = CopyOrigins(newFrames, owned, _plain is not null, _canonical);
            _budget.Charge(1);
            var paired = FerruleBasicScopeContext.Create(Parent.Roots, actual, owned, origins, 4);
            BasicOccurrenceCursor? cursor = newCollections == 0 ? null : new(
                owned.FrameCount - 1, actual.BasicCollectionCount - 1, _owner,
                owned.FrameAt(owned.FrameCount - 1).Path, owned.FrameAt(owned.FrameCount - 1).Index);
            var terminalPath = _plain is not null ? origins[^1].Canonical : owned.FrameAt(owned.FrameCount - 1).Path;
            var terminal = _plain ?? owned.FrameAt(owned.FrameCount - 1).Item;
            var terminalFrame = _plain is null && newCollections > 0;
            var raw = terminalFrame ? actual.BasicCollectionPath(actual.BasicCollectionCount - 1) : terminalPath;
            var rawCount = terminalFrame ? raw.Count : _plainRawCount;
            var alias = !terminalFrame && rawCount > terminalPath.Count ? _plainRawFirst : null;
            var offset = terminalFrame ? 0 : terminalPath.Count - (rawCount - (alias is null ? 0 : 1));
            var proof = new BasicGroupTerminal(_owner, terminal, terminalPath, raw, offset, rawCount, alias,
                owned.FrameCount - (terminalFrame ? 1 : 0), actual.Frames.Count - 1,
                origins.Length - 1, actual.BasicCollectionCount - (terminalFrame ? 1 : 0),
                rawCount == 0 ? null : (terminalFrame ? raw[^1] : terminalPath.Count == 0 ? alias : terminalPath[^1]));
            _building!.Append(paired, cursor, _budget, proof); _pendingLegacy = null;
        }
        private BasicFrameOrigin[] CopyOrigins(int extraFrames, FerruleOwnedScopeContext owned,
            bool plain, IReadOnlyList<string> terminal)
        {
            var total = BasicSourceAccounting.Add((nuint)Parent.OriginCount, (nuint)extraFrames);
            _budget.Charge(total); var result = new BasicFrameOrigin[BasicSourceAccounting.ArrayLength(total)];
            for (var i = 0; i < Parent.OriginCount; i++)
            { var o = Parent.OriginAt(i); _budget.Charge((nuint)o.Canonical.Count); _budget.Path(o.Canonical); result[i] = o; }
            var extraOwned = owned.FrameCount - Parent.OwnedView.FrameCount;
            for (var i = 0; i < extraOwned; i++)
            {
                var f = owned.FrameAt(Parent.OwnedView.FrameCount + i);
                result[Parent.OriginCount + i] = new(f.Owner, CopyCanonical(f.Path), f.Index);
            }
            if (plain) result[^1] = new(_owner, CopyCanonical(terminal));
            return result;
        }
        OwnedQuery FerruleOwnedScopeContext.BasicOwnedWalkObserver.CreateQuery(FerruleOwnedScopeContext current,
            FerruleOwnedSourceOwner owner, IReadOnlyList<string> path)
        { _inheritedOwned = current.FrameCount; return _query = Parent.Query(_budget, owner, path); }
        void FerruleOwnedScopeContext.BasicOwnedWalkObserver.BeforeWalkBuffers() => _budget.Charge(3);
        void FerruleOwnedScopeContext.BasicOwnedWalkObserver.SelectedBase(int ownerKey, FerruleInstance actual,
            int consumed, bool identity, int selectedSlot)
        { SelectionCalls++; _owner = ownerKey; _identityOwnedSlot = selectedSlot; }
        void FerruleOwnedScopeContext.BasicOwnedWalkObserver.BeforePublication(int frameCount,
            FerruleInstance terminal, IReadOnlyList<string> canonical, bool identity) => _building!.ReserveOutput(_budget);
        void FerruleOwnedScopeContext.BasicOwnedWalkObserver.Published(FerruleOwnedScopeContext actual,
            FerruleInstance terminal, IReadOnlyList<string> canonical, bool identity)
        {
            var extraOwned = actual.FrameCount - _inheritedOwned;
            // A fresh named singleton root does not introduce a legacy traversal
            // frame. Its owned root stays authentic; relative reads retain the parent.
            var suppressFreshNamedRoot = !identity && _owner != 0 && canonical.Count == 0 &&
                extraOwned == 0 && terminal is FerruleGroup;
            var legacyProjectionIdentity = identity || suppressFreshNamedRoot;
            var legacy = Parent.LegacyView.ProjectBasicOwned(actual, _inheritedOwned, terminal, canonical, legacyProjectionIdentity, _budget);
            var plain = !legacyProjectionIdentity && (extraOwned == 0 || actual.FrameAt(actual.FrameCount - 1).Path.Count != canonical.Count);
            var origins = legacyProjectionIdentity ? CopyOrigins(0, actual, false, canonical) :
                CopyOrigins(extraOwned + (plain ? 1 : 0), actual, plain, canonical);
            _budget.Charge(1);
            var paired = FerruleBasicScopeContext.Create(Parent.Roots, legacy, actual, origins,
                legacyProjectionIdentity ? Parent.LegacyContainerCost : 2);
            BasicOccurrenceCursor? cursor = identity || extraOwned == 0 ? null : new(
                actual.FrameCount - 1, legacy.BasicCollectionCount - 1, _owner,
                actual.FrameAt(actual.FrameCount - 1).Path, actual.FrameAt(actual.FrameCount - 1).Index);
            var newTerminal = !identity && extraOwned > 0 && actual.FrameAt(actual.FrameCount - 1).Path.Count == canonical.Count;
            var terminalPath = newTerminal ? actual.FrameAt(actual.FrameCount - 1).Path :
                !legacyProjectionIdentity ? origins[^1].Canonical : canonical.Count == 0 ? Array.Empty<string>() :
                actual.FrameAt(_identityOwnedSlot).Path;
            var proof = new BasicGroupTerminal(_owner, terminal, terminalPath, terminalPath, 0, terminalPath.Count, null,
                actual.FrameCount - (newTerminal ? 1 : 0),
                legacy.Frames.Count - (legacyProjectionIdentity ? 0 : 1),
                origins.Length - (legacyProjectionIdentity ? 0 : 1),
                legacy.BasicCollectionCount - (newTerminal ? 1 : 0),
                terminalPath.Count == 0 ? null : terminalPath[^1]);
            _building!.Append(paired, cursor, _budget, proof);
        }
        int ICollection<ScopeContext>.Count => _building?._count ?? OriginalCount;
        bool ICollection<ScopeContext>.IsReadOnly => true;
        void ICollection<ScopeContext>.Add(ScopeContext item)
        { if (_finished) throw new NotSupportedException("Source cohort is immutable."); _pendingLegacy = item; }
        void ICollection<ScopeContext>.Clear() => throw new NotSupportedException("Source cohort is immutable.");
        bool ICollection<ScopeContext>.Remove(ScopeContext item) => throw new NotSupportedException("Source cohort is immutable.");
        bool ICollection<ScopeContext>.Contains(ScopeContext item) => false;
        void ICollection<ScopeContext>.CopyTo(ScopeContext[] array, int arrayIndex) => throw new NotSupportedException("Source cohort is opaque.");
        IEnumerator<ScopeContext> IEnumerable<ScopeContext>.GetEnumerator() => throw new NotSupportedException("Source cohort is opaque.");
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => throw new NotSupportedException("Source cohort is opaque.");
        internal void ReleaseScratch()
        {
            nuint released = (nuint)_states.Length + (nuint)_scratch.Length;
            for (var i = 0; i < _scratchCount; i++) released = BasicSourceAccounting.Add(released, (nuint)_scratch[i].Path.Count);
            if (_canonicalOwned) released = BasicSourceAccounting.Add(released, (nuint)_canonical.Count);
            _pendingLegacy = null; _query = null; _canonical = Array.Empty<string>(); _canonicalOwned = false;
            _plain = null; _states = Array.Empty<State>(); _scratch = Array.Empty<FerruleOwnedScopeContext.Observation>();
            _stateDepth = 0; _scratchCount = 0;
            if (released != 0) _budget.Release(released);
        }
    }
}


/// <summary>One witnessed source candidate; source ordinals are separate from effective phase positions.</summary>
public sealed class FerruleBasicSourceCandidate
{
    private readonly BasicOccurrenceCursor? _cursor;
    internal FerruleBasicSourceCandidate(FerruleBasicScopeContext context,
        FerruleRetainedSourceCohort.Witness witness, nuint sourceId, BasicOccurrenceCursor? cursor)
        : this(context, witness, sourceId, cursor, null) { }
    internal FerruleBasicSourceCandidate(FerruleBasicScopeContext context,
        FerruleRetainedSourceCohort.Witness witness, nuint sourceId, BasicOccurrenceCursor? cursor,
        BasicGroupTerminal? terminal)
    { Context = context; SourceWitness = witness; SourceId = sourceId; _cursor = cursor; GroupTerminal = terminal; }
    public FerruleBasicScopeContext Context { get; }
    internal FerruleRetainedSourceCohort.Witness SourceWitness { get; }
    internal nuint SourceId { get; }
    internal nuint? RawSourceOrdinal => _cursor?.RawOrdinal;
    internal bool HasNewCursor => _cursor.HasValue;
    internal BasicOccurrenceCursor? Cursor => _cursor;
    internal BasicGroupTerminal? GroupTerminal { get; }
    internal FerruleBasicSourceCandidate Copy(BasicSourceBudget budget, nuint position)
    {
        if (position == 0)
        {
            var owner = _cursor.HasValue && _cursor.Value.OwnerKey != 0 ?
                FerruleOwnedSourceOwner.Named(Context.Roots.OwnedRoots.Name(_cursor.Value.OwnerKey)!) : FerruleOwnedSourceOwner.Primary;
            throw Context.Query(budget, owner, Array.Empty<string>()).Error(
                FerruleOwnedSourceErrorKind.InvalidPosition, index: position);
        }
        if (position > int.MaxValue)
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.LegacyPositionRange,
                requested: position, maximum: int.MaxValue);
        var context = Context.CloneBudgeted(budget, _cursor, position);
        budget.Charge(1);
        return new FerruleBasicSourceCandidate(context, SourceWitness, SourceId, _cursor, GroupTerminal);
    }
    internal BasicSortCopyCost SortCopyCost()
    {
        var name = _cursor.HasValue && _cursor.Value.OwnerKey != 0 ? Context.Roots.OwnedRoots.Name(_cursor.Value.OwnerKey) : null;
        return Context.SortCopyCost(_cursor.HasValue, name);
    }
    /// <summary>
    /// Copies this explicitly supplied candidate and its parent. This legacy
    /// convenience does not account for source or prior cohorts retained elsewhere.
    /// Use ReindexRetainedPhase when the complete original/current holders coexist.
    /// </summary>
    public FerruleBasicSourceCandidate WithPhasePosition(nuint oneBased)
    {
        try
        {
            var budget = SourceWitness.Parent.Begin(); Context.Seed(budget);
            return Copy(budget, oneBased);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
}

internal static class BasicPhaseAdmission
{
    internal static void ValidateIds(int originalCount, BasicSourceIds original,
        BasicSourceIds current, BasicPhaseIndices indices)
    {
        // This is also the strict internal corrupted-ID test boundary. Production
        // uses allocation-free accessors over its private complete candidate arrays.
        for (var i = 0; i < original.Count; i++)
        {
            var id = original.SourceIdAt(i);
            for (var j = 0; j < i; j++)
                if (original.SourceIdAt(j) == id) throw Duplicate((nuint)i, id);
            if (id != (nuint)i || id >= (nuint)originalCount) throw Invalid((nuint)i);
        }
        if (original.Count != originalCount) throw Invalid((nuint)original.Count);
        for (var i = 0; i < current.Count; i++)
        {
            var id = current.SourceIdAt(i);
            for (var j = 0; j < i; j++) if (current.SourceIdAt(j) == id) throw Duplicate((nuint)i, id);
            if (id >= (nuint)originalCount) throw Invalid((nuint)i);
        }
        for (var i = 0; i < indices.Count; i++)
        {
            var index = indices.IndexAt(i);
            if (index >= (nuint)current.Count) throw Invalid((nuint)i);
            var id = current.SourceIdAt((int)index);
            for (var j = 0; j < i; j++)
                if (current.SourceIdAt((int)indices.IndexAt(j)) == id) throw Duplicate((nuint)i, id);
        }
    }
    internal static FerruleBasicSourceBridgeException Invalid(nuint input) =>
        new(FerruleBasicSourceBridgeErrorKind.InvalidCohortWitness, inputOrdinal: input);
    internal static FerruleBasicSourceBridgeException Duplicate(nuint input, nuint candidate) =>
        new(FerruleBasicSourceBridgeErrorKind.DuplicatePhaseCandidate,
            inputOrdinal: input, candidateOrdinal: candidate);

}

/// <summary>Immutable ordered indices into one complete original or current phase holder.</summary>
public sealed class FerrulePhaseSelection : BasicPhaseIndices
{
    private readonly nuint[] _indices;
    private readonly BasicCompleteRows _current;
    private FerrulePhaseSelection(BasicCompleteRows current, nuint[] indices)
    { _current = current; _indices = indices; }
    internal BasicCompleteRows Current => _current;
    internal int Count => _indices.Length;
    internal nuint Index(int index) => _indices[index];
    int BasicPhaseIndices.Count => _indices.Length;
    nuint BasicPhaseIndices.IndexAt(int index) => _indices[index];
    public static FerrulePhaseSelection FromOriginal(FerruleRetainedSourceCohort current, ReadOnlySpan<nuint> indices) =>
        Capture(current, indices);
    public static FerrulePhaseSelection FromPhase(FerruleCompletePhaseCohort current, ReadOnlySpan<nuint> indices) =>
        Capture(current, indices);
    private static FerrulePhaseSelection Capture(BasicCompleteRows current, ReadOnlySpan<nuint> indices)
    {
        ArgumentNullException.ThrowIfNull(current);
        try
        {
            var witness = current.SourceWitness;
            var budget = new BasicSourceBudget(witness.Parent.Roots.Limits, true);
            var maximum = witness.Parent.Roots.Limits.MaxWalkContexts;
            BasicSourceAccounting.Limit(0, (nuint)indices.Length, maximum, FerruleBasicSourceBridgeComponent.WalkContexts);
            BasicSourceAccounting.Limit(0, (nuint)current.Count, maximum, FerruleBasicSourceBridgeComponent.WalkContexts);
            if (!witness.IsComplete) throw BasicPhaseAdmission.Invalid(0);
            witness.Parent.Roots.Seed(budget); witness.Parent.Seed(budget);
            budget.Charge((nuint)current.Count + 2); // current backing, witness and current holder
            for (var i = 0; i < current.Count; i++) { current.At(i).Context.Seed(budget); budget.Charge(1); }
            budget.Charge(BasicSourceAccounting.Multiply(2, (nuint)indices.Length));
            budget.Charge(1);
            var snapshot = new nuint[BasicSourceAccounting.ArrayLength((nuint)indices.Length)]; indices.CopyTo(snapshot);
            // Validate exactly the completed private snapshot, never the mutable caller's span.
            for (var i = 0; i < snapshot.Length; i++)
            {
                if (snapshot[i] >= (nuint)current.Count) throw BasicPhaseAdmission.Invalid((nuint)i);
                var id = current.At((int)snapshot[i]).SourceId;
                for (var j = 0; j < i; j++)
                    if (current.At((int)snapshot[j]).SourceId == id)
                        throw BasicPhaseAdmission.Duplicate((nuint)i, id);
            }
            return new FerrulePhaseSelection(current, snapshot);
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
}

/// <summary>One complete immutable phase output, retaining its original source witness.</summary>
public sealed class FerruleCompletePhaseCohort : IReadOnlyList<FerruleBasicSourceCandidate>, BasicCompleteRows
{
    private readonly FerruleBasicSourceCandidate[] _items;
    private readonly FerruleRetainedSourceCohort.Witness _witness;
    private FerruleCompletePhaseCohort(FerruleBasicSourceCandidate[] items, FerruleRetainedSourceCohort.Witness witness)
    { _items = items; _witness = witness; }
    public int Count => _items.Length;
    public FerruleBasicSourceCandidate this[int index] => _items[index];
    FerruleBasicSourceCandidate BasicCompleteRows.At(int index) => _items[index];
    nuint BasicSourceIds.SourceIdAt(int index) => _items[index].SourceId;
    FerruleRetainedSourceCohort.Witness BasicCompleteRows.SourceWitness => _witness;
    public IEnumerator<FerruleBasicSourceCandidate> GetEnumerator()
    { for (var i = 0; i < _items.Length; i++) yield return _items[i]; }
    System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    internal static FerruleCompletePhaseCohort Reindex(FerruleBasicScopeContext parent,
        FerruleRetainedSourceCohort original, FerrulePhaseSelection selection)
    {
        ArgumentNullException.ThrowIfNull(original); ArgumentNullException.ThrowIfNull(selection);
        var current = selection.Current; var witness = original.SourceWitness;
        var limits = parent.Roots.Limits;
        var budget = new BasicSourceBudget(limits, true);
        if (!ReferenceEquals(parent.Roots, witness.Parent.Roots) ||
            !ReferenceEquals(parent.Roots, current.SourceWitness.Parent.Roots))
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.RegistryMismatch);
        if (!ReferenceEquals(parent, witness.Parent) || !ReferenceEquals(witness, current.SourceWitness))
            throw BasicPhaseAdmission.Invalid(0);
        BasicSourceAccounting.Limit(0, (nuint)original.Count, limits.MaxWalkContexts, FerruleBasicSourceBridgeComponent.WalkContexts);
        BasicSourceAccounting.Limit(0, (nuint)current.Count, limits.MaxWalkContexts, FerruleBasicSourceBridgeComponent.WalkContexts);
        BasicSourceAccounting.Limit(0, (nuint)selection.Count, limits.MaxWalkContexts, FerruleBasicSourceBridgeComponent.WalkContexts);
        // Upfront opaque checks require no membership set or second ID snapshot.
        for (var i = 0; i < original.Count; i++)
            if (!ReferenceEquals(original[i].SourceWitness, witness)) throw BasicPhaseAdmission.Invalid((nuint)i);
        for (var i = 0; i < current.Count; i++)
            if (!ReferenceEquals(current.At(i).SourceWitness, witness)) throw BasicPhaseAdmission.Invalid((nuint)i);
        if (!witness.IsComplete) throw BasicPhaseAdmission.Invalid(0);
        BasicPhaseAdmission.ValidateIds(witness.OriginalCount, original, current, selection);
        parent.Roots.Seed(budget); parent.Seed(budget);
        budget.Charge(5); // complete original/current/output holders, witness and selection
        budget.Charge((nuint)original.Count + (nuint)current.Count + (nuint)selection.Count);
        for (var i = 0; i < original.Count; i++) { original[i].Context.Seed(budget); budget.Charge(1); }
        for (var i = 0; i < current.Count; i++) { current.At(i).Context.Seed(budget); budget.Charge(1); }
        // Both explicitly held logical sets are charged, even when their immutable backing aliases.
        budget.Charge((nuint)selection.Count);
        var output = new FerruleBasicSourceCandidate[selection.Count];
        for (var i = 0; i < output.Length; i++) output[i] = current.At((int)selection.Index(i)).Copy(budget, (nuint)i + 1);
        return new FerruleCompletePhaseCohort(output, witness);
    }
}

/// <summary>
/// One streaming filter operation retaining its complete original source cohort.
/// Predicate contexts remain original; Produce returns the next compact output
/// context without retaining accepted candidates or resetting the copy budget.
/// </summary>
public sealed class FerruleBasicFilterPhase
{
    private readonly FerruleRetainedSourceCohort _original;
    private readonly BasicSourceBudget _budget;
    private nuint _accepted;
    private nuint? _last;
    private FerruleBasicFilterPhase(FerruleRetainedSourceCohort original, BasicSourceBudget budget)
    { _original = original; _budget = budget; }
    internal static FerruleBasicFilterPhase Begin(FerruleBasicScopeContext parent, FerruleRetainedSourceCohort original)
    {
        ArgumentNullException.ThrowIfNull(original);
        var witness = original.SourceWitness;
        if (!ReferenceEquals(parent.Roots, witness.Parent.Roots))
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.RegistryMismatch);
        if (!ReferenceEquals(parent, witness.Parent) || !witness.IsComplete)
            throw BasicPhaseAdmission.Invalid(0);
        var count = original.Count;
        if (count != witness.OriginalCount) throw BasicPhaseAdmission.Invalid((nuint)count);
        BasicSourceAccounting.Limit(0, (nuint)count, parent.Roots.Limits.MaxWalkContexts,
            FerruleBasicSourceBridgeComponent.WalkContexts);
        for (var i = 0; i < count; i++)
        {
            var candidate = original[i];
            if (!ReferenceEquals(candidate.SourceWitness, witness) ||
                !ReferenceEquals(candidate.Context.Roots, parent.Roots) || candidate.SourceId != (nuint)i)
                throw BasicPhaseAdmission.Invalid((nuint)i);
        }
        var budget = new BasicSourceBudget(parent.Roots.Limits, true);
        parent.Roots.Seed(budget); parent.Seed(budget);
        budget.Charge(3); // original holder, its witness, and this streaming phase
        budget.Charge((nuint)count); // one complete original backing, no current/selection holder
        for (var i = 0; i < count; i++) { original[i].Context.Seed(budget); budget.Charge(1); }
        return new FerruleBasicFilterPhase(original, budget);
    }
    public FerruleBasicSourceCandidate Produce(nuint originalIndex)
    {
        try
        {
            if (originalIndex >= (nuint)_original.Count) throw BasicPhaseAdmission.Invalid(originalIndex);
            if (_last.HasValue && originalIndex == _last.Value)
                throw BasicPhaseAdmission.Duplicate(originalIndex, _original[(int)originalIndex].SourceId);
            if (_last.HasValue && originalIndex < _last.Value) throw BasicPhaseAdmission.Invalid(originalIndex);
            BasicSourceAccounting.Limit(_accepted, 1, _original.SourceWitness.Parent.Roots.Limits.MaxWalkContexts,
                FerruleBasicSourceBridgeComponent.WalkContexts);
            var position = BasicSourceAccounting.Add(_accepted, 1);
            // Copy changes only this source walk's introduced cursor. Its original
            // owner/raw ordinal and every enclosing phase remain authenticated.
            var result = _original[(int)originalIndex].Copy(_budget, position);
            _accepted = position; _last = originalIndex;
            return result;
        }
        catch (FerruleOwnedSourceException e) { throw FerruleRuntimeException.FromOwnedSource(e); }
        catch (FerruleBasicSourceBridgeException e) { throw FerruleRuntimeException.FromBasicSourceBridge(e); }
    }
}


public sealed partial class ScopeContext
{
    private static void RequireBasicScalarTraversal(FerruleInstance current)
    {
        if (current is FerruleDocumentSet or FerruleMappedSequence)
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.UnsupportedBasicContext,
                found: current is FerruleDocumentSet ? FerruleOwnedSourceInstanceKind.DocumentSet :
                    FerruleOwnedSourceInstanceKind.MappedSequence);
    }
    internal FerruleValue ResolveBasicSourceField(IReadOnlyList<string> path)
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
            var resolved = TryResolveScalar(collection.Item, path, collection.Path.Count, true, basicOnly: true);
            if (resolved.Found)
            {
                return resolved.Value;
            }
            throw new FerruleRuntimeException(
                FerruleRuntimeError.MissingSourceField,
                $"Source scalar '{string.Join('/', path)}' does not exist in the active scope context.");
        }

        FerruleInstance? firstOwner = null;
        FerruleInstance? firstAbsence = null;
        for (var index = _frames.Count - 1; index >= 0; index--)
        {
            var instance = _frames[index];
            var resolved = TryResolveScalar(instance, path, 0, basicOnly: true);
            if (resolved.Found)
            {
                return resolved.Value;
            }
            var candidate = instance is FerruleRepeated { Items.Count: > 0 } repeated
                ? repeated.Items[0]
                : instance;
            if (firstOwner is null &&
                (path.Count == 0 || TryGetField(candidate, path[0], out _)))
            {
                firstOwner = instance;
            }
            if (firstAbsence is null && resolved.MissingField)
            {
                firstAbsence = instance;
            }
        }
        var selected = firstOwner ?? firstAbsence;
        if (selected is not null)
        {
            var resolved = TryResolveScalar(selected, path, 0, true, basicOnly: true);
            if (resolved.Found)
            {
                return resolved.Value;
            }
        }
        return ResolveScalar(path);
    }

    /// <summary>
    /// Reads a validated field in its exact frame. Missing groups and an
    /// intermediate Null/JsonNull yield Null; malformed values and inactive frames fail.
    /// </summary>
    internal FerruleValue ResolveBasicSourceFieldInFrame(
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
            var resolved = TryResolveScalar(collection.Item, path, 0, true, basicOnly: true);
            if (resolved.Found)
            {
                return resolved.Value;
            }
            break;
        }
        return ResolveScalarInFrame(frame, path);
    }

}


/// <summary>The availability policy of one exact program declaration.</summary>
public enum FerruleBasicNamedSourceMode { Optional, RequiredLegacy, ReservedUnavailable }
public readonly record struct FerruleBasicNamedSourceDeclaration(string Name, FerruleBasicNamedSourceMode Mode);
public readonly record struct FerruleNamedSourceInput(string? Name, FerruleInstance? Instance);
public interface IFerruleNamedSourceInputAccessor<T>
{
    FerruleNamedSourceInput Read(T item);
}

public enum FerruleJsonDocumentKind { Text, Utf8 }

/// <summary>A closed borrowed document handle. Admission validates only its selected reference.</summary>
public readonly struct FerruleJsonDocumentRef
{
    private FerruleJsonDocumentRef(FerruleJsonDocumentKind kind, string? text, byte[]? utf8)
    { Kind = kind; Text = text; Utf8 = utf8; }
    public FerruleJsonDocumentKind Kind { get; }
    public string? Text { get; }
    public byte[]? Utf8 { get; }
    public static FerruleJsonDocumentRef FromText(string? document) => new(FerruleJsonDocumentKind.Text, document, null);
    public static FerruleJsonDocumentRef FromUtf8(byte[]? document) => new(FerruleJsonDocumentKind.Utf8, null, document);
    internal bool HasDocument => Kind == FerruleJsonDocumentKind.Text ? Text is not null :
        Kind == FerruleJsonDocumentKind.Utf8 && Utf8 is not null;
}

public readonly record struct FerruleNamedJsonDocumentRef(string? Name, FerruleJsonDocumentRef Document);
public interface IFerruleNamedJsonDocumentAccessor<T> where T : class
{
    FerruleNamedJsonDocumentRef Read(T? item);
}

internal readonly record struct BasicJsonInputSnapshot(int DeclarationIndex, FerruleJsonDocumentRef Document);
internal readonly record struct BasicJsonInputParsed(int DeclarationIndex, FerruleInstance? Instance);

// A and P are disjoint reservations, not a shared mutable counter. The existing
// typed factory receives the original L and never sees A or the larger envelope E.
internal sealed class BasicJsonInputEnvelope
{
    private readonly ReadOnlyMemory<FerruleBasicNamedSourceDeclaration> _catalogue;
    private readonly FerruleOwnedSourceLimits _limits;
    private readonly nuint _core, _paired, _adapter, _total;
    private BasicJsonInputEnvelope(ReadOnlyMemory<FerruleBasicNamedSourceDeclaration> catalogue,
        FerruleOwnedSourceLimits limits, nuint core, nuint paired, nuint adapter, nuint total)
    { _catalogue = catalogue; _limits = limits; _core = core; _paired = paired; _adapter = adapter; _total = total; }
    internal ReadOnlyMemory<FerruleBasicNamedSourceDeclaration> Catalogue => _catalogue;
    internal FerruleOwnedSourceLimits Limits => _limits;
    internal (nuint Core, nuint Paired, nuint Adapter, nuint Total) Proof => (_core, _paired, _adapter, _total);
    internal static BasicJsonInputEnvelope Create(ReadOnlyMemory<FerruleBasicNamedSourceDeclaration> catalogue,
        FerruleOwnedSourceLimits limits, FerruleOwnedSourceRoots.NamedInputPreflight policy)
    {
        var (adapter, total) = Bounds((nuint)catalogue.Length, policy.Paired);
        RequireSnapshotCapacity((nuint)catalogue.Length);
        return new(catalogue, limits, policy.Core, policy.Paired, adapter, total);
    }
    internal static (nuint Adapter, nuint Total) Bounds(nuint count, nuint paired)
    {
        try
        {
            var adapter = BasicSourceAccounting.Add(BasicSourceAccounting.Multiply(2, count), 16);
            return (adapter, BasicSourceAccounting.Add(paired, adapter));
        }
        catch (FerruleBasicSourceBridgeException error) { throw FerruleRuntimeException.FromBasicSourceBridge(error); }
    }
    internal static void RequireSnapshotCapacity(nuint count) => RequireSnapshotCapacity(count, (nuint)Array.MaxLength, (nuint)nint.MaxValue);
    internal static void RequireParsedCapacity(nuint count) => RequireParsedCapacity(count, (nuint)Array.MaxLength, (nuint)nint.MaxValue);
    // Pure internal capacity seams retain the actual two closed record layouts;
    // tests can exercise native refusal frontiers without impossible allocations.
    internal static void RequireSnapshotCapacity(nuint count, nuint arrayMaximum, nuint dataMaximum) =>
        RequireCapacity(count, (nuint)System.Runtime.CompilerServices.Unsafe.SizeOf<BasicJsonInputSnapshot>(), arrayMaximum, dataMaximum);
    internal static void RequireParsedCapacity(nuint count, nuint arrayMaximum, nuint dataMaximum) =>
        RequireCapacity(count, (nuint)System.Runtime.CompilerServices.Unsafe.SizeOf<BasicJsonInputParsed>(), arrayMaximum, dataMaximum);
    private static void RequireCapacity(nuint count, nuint recordBytes, nuint arrayMaximum, nuint dataMaximum)
    {
        try
        {
            if (count > arrayMaximum || BasicSourceAccounting.Multiply(count, recordBytes) > dataMaximum)
                throw BasicSourceAccounting.Overflow();
        }
        catch (FerruleBasicSourceBridgeException error) { throw FerruleRuntimeException.FromBasicSourceBridge(error); }
    }
}

/// <summary>One complete name-admission pass retaining only canonical ordinals and borrowed document handles.</summary>
/// <remarks>The catalogue backing must remain immutable through parsing and binding.
/// The root-derived adapter reservation is separate from codec/source-tree storage and
/// the existing control-free Basic primitive allowance; it is not an RSS bound.</remarks>
public sealed class FerruleBasicJsonInputAdmission
{
    private readonly BasicJsonInputEnvelope _ticket;
    private BasicJsonInputSnapshot[]? _snapshots;
    private BasicJsonInputParsed[]? _parsed;
    private int _parseClaimed;
    private FerruleBasicJsonInputAdmission(BasicJsonInputEnvelope ticket,
        BasicJsonInputSnapshot[] snapshots, BasicJsonInputParsed[] parsed)
    { _ticket = ticket; _snapshots = snapshots; _parsed = parsed; }
    internal (int Snapshots, int Parsed) OwnedCapacities =>
        (System.Threading.Volatile.Read(ref _snapshots)?.Length ?? 0, System.Threading.Volatile.Read(ref _parsed)?.Length ?? 0);
    internal (nuint Core, nuint Paired, nuint Adapter, nuint Total) EnvelopeProof => _ticket.Proof;

    public static FerruleBasicJsonInputAdmission Admit<T>(
        ReadOnlyMemory<FerruleBasicNamedSourceDeclaration> catalogue, IReadOnlyList<T> supplied,
        IFerruleNamedJsonDocumentAccessor<T> accessor, FerruleOwnedSourceLimits limits) where T : class
    {
        ArgumentNullException.ThrowIfNull(supplied); ArgumentNullException.ThrowIfNull(accessor);
        BasicJsonInputEnvelope ticket;
        BasicJsonInputSnapshot[] snapshots;
        nuint catalogueBytes;
        try
        {
            var policy = FerruleOwnedSourceRoots.PreflightNamedInputs(catalogue.Span, limits);
            catalogueBytes = policy.Text;
            ticket = BasicJsonInputEnvelope.Create(catalogue, limits, policy);
            snapshots = catalogue.Length == 0 ? Array.Empty<BasicJsonInputSnapshot>() : new BasicJsonInputSnapshot[catalogue.Length];
        }
        catch (FerruleOwnedSourceException error) { throw FerruleRuntimeException.FromOwnedSource(error); }
        catch (FerruleBasicSourceBridgeException error) { throw FerruleRuntimeException.FromBasicSourceBridge(error); }
        try
        {
            // Count, indexer and accessor are caller code: never place them inside
            // owned-error lifting, even when they throw a typed owned/bridge cause.
            var suppliedCount = supplied.Count;
            if (suppliedCount < 0) throw new ArgumentOutOfRangeException(nameof(supplied), suppliedCount, "Count cannot be negative.");
            var accepted = 0;
            for (var ordinal = 0; ordinal < suppliedCount; ordinal++)
            {
                var item = supplied[ordinal];
                var input = accessor.Read(item);
                try
                {
                    var name = input.Name; var document = input.Document;
                    if (name is null || !document.HasDocument) throw new ArgumentNullException(nameof(supplied));
                    FerruleUnicode.RequireWellFormed(name, nameof(supplied));
                    var found = -1;
                    for (var i = 0; i < catalogue.Length; i++)
                        if (StringComparer.Ordinal.Equals(catalogue.Span[i].Name, name) &&
                            catalogue.Span[i].Mode != FerruleBasicNamedSourceMode.ReservedUnavailable)
                        { found = i; break; }
                    if (found < 0)
                    {
                        // C# retains the diagnostic string reference; it allocates
                        // no encoded-name buffer. Keep its existing byte accounting.
                        OwnedSourceAccounting.Limit(catalogueBytes, OwnedSourceAccounting.Text(name),
                            limits.MaxMetadataBytes, FerruleOwnedSourceLimitKind.MetadataBytes);
                        throw new FerruleRuntimeException(FerruleRuntimeError.UnexpectedNamedSource,
                            "named source is not declared by this mapping", detail: name);
                    }
                    for (var i = 0; i < accepted; i++)
                        if (snapshots[i].DeclarationIndex == found)
                            throw new FerruleRuntimeException(FerruleRuntimeError.DuplicateNamedSource,
                                "named source was supplied more than once", detail: catalogue.Span[found].Name);
                    snapshots[accepted++] = new(found, document);
                }
                catch (FerruleOwnedSourceException error) { throw FerruleRuntimeException.FromOwnedSource(error); }
                catch (FerruleBasicSourceBridgeException error) { throw FerruleRuntimeException.FromBasicSourceBridge(error); }
            }
            for (var i = 0; i < catalogue.Length; i++)
            {
                if (catalogue.Span[i].Mode != FerruleBasicNamedSourceMode.RequiredLegacy) continue;
                var present = false;
                for (var j = 0; j < accepted; j++) if (snapshots[j].DeclarationIndex == i) { present = true; break; }
                if (!present) throw new FerruleRuntimeException(FerruleRuntimeError.MissingNamedSource,
                    "named source is required by this mapping", detail: catalogue.Span[i].Name);
            }
            // The C# projection has no extra name-byte charge. M <= N proves its
            // existing array-count checks; check the closed parsed layout at M,
            // only after every name/required-binding error has had precedence.
            BasicJsonInputEnvelope.RequireParsedCapacity((nuint)accepted);
            var parsed = accepted == 0 ? Array.Empty<BasicJsonInputParsed>() : new BasicJsonInputParsed[accepted];
            return new FerruleBasicJsonInputAdmission(ticket, snapshots, parsed);
        }
        catch { Array.Clear(snapshots); throw; }
    }

    /// <summary>Consumes this owner once; all callbacks see original declaration ordinals in supplied order.</summary>
    public FerruleBasicJsonParsedInputs ParseNamedWith(Func<int, FerruleJsonDocumentRef, FerruleInstance> parse)
    {
        if (System.Threading.Interlocked.CompareExchange(ref _parseClaimed, 1, 0) != 0)
            throw new InvalidOperationException("JSON input admission has already been consumed.");
        // Detach both arrays before even the first callback. Losing/reentrant
        // attempts cannot see or clear buffers owned by the winning parse.
        var snapshots = System.Threading.Interlocked.Exchange(ref _snapshots, null)!;
        var parsed = System.Threading.Interlocked.Exchange(ref _parsed, null)!;
        try
        {
            ArgumentNullException.ThrowIfNull(parse);
            for (var i = 0; i < parsed.Length; i++)
            {
                var snapshot = snapshots[i];
                var instance = parse(snapshot.DeclarationIndex, snapshot.Document);
                if (instance is null) throw new InvalidOperationException("JSON input parser returned null.");
                parsed[i] = new(snapshot.DeclarationIndex, instance);
            }
            return FerruleBasicJsonParsedInputs.Create(_ticket, parsed);
        }
        catch { Array.Clear(parsed); throw; }
        finally { Array.Clear(snapshots); }
    }
}

/// <summary>Completed parsed inputs carrying a disjoint reservation and a one-shot Basic binding ticket.</summary>
/// <remarks>Generated callers keep this owner live through complete serialization,
/// including error paths. The reservation covers control-free companion bookkeeping,
/// not codec/source-tree storage or retained filter/sort/window phases.</remarks>
public sealed class FerruleBasicJsonParsedInputs
{
    private readonly BasicJsonInputEnvelope _ticket;
    private BasicJsonInputParsed[]? _parsed;
    private int _bindingClaimed;
    private FerruleBasicJsonParsedInputs(BasicJsonInputEnvelope ticket, BasicJsonInputParsed[] parsed)
    { _ticket = ticket; _parsed = parsed; }
    internal static FerruleBasicJsonParsedInputs Create(BasicJsonInputEnvelope ticket, BasicJsonInputParsed[] parsed) => new(ticket, parsed);
    internal int OwnedCapacity => _parsed?.Length ?? 0;
    internal (nuint Core, nuint Paired, nuint Adapter, nuint Total) EnvelopeProof => _ticket.Proof;
    private sealed class FactoryInputView(ReadOnlyMemory<FerruleBasicNamedSourceDeclaration> catalogue,
        BasicJsonInputParsed[] parsed) : IReadOnlyList<FerruleNamedSourceInput>
    {
        public int Count => parsed.Length;
        public FerruleNamedSourceInput this[int index] =>
            new(catalogue.Span[parsed[index].DeclarationIndex].Name, parsed[index].Instance);
        public IEnumerator<FerruleNamedSourceInput> GetEnumerator() => throw new NotSupportedException("Factory input is indexed.");
        System.Collections.IEnumerator System.Collections.IEnumerable.GetEnumerator() => GetEnumerator();
    }
    private readonly struct FactoryAccessor : IFerruleNamedSourceInputAccessor<FerruleNamedSourceInput>
    { public FerruleNamedSourceInput Read(FerruleNamedSourceInput item) => item; }

    public FerruleBasicSourceRoots BasicRoots(FerruleInstance primary)
    {
        ArgumentNullException.ThrowIfNull(primary);
        if (System.Threading.Interlocked.CompareExchange(ref _bindingClaimed, 1, 0) != 0)
            throw new InvalidOperationException("Parsed JSON inputs have already been bound.");
        var parsed = _parsed!;
        try
        {
            var view = new FactoryInputView(_ticket.Catalogue, parsed);
            return FerruleBasicSourceRoots.CreateFromNamedInputs(primary, _ticket.Catalogue.Span,
                view, new FactoryAccessor(), _ticket.Limits);
        }
        catch { Array.Clear(parsed); _parsed = null; throw; }
        finally { GC.KeepAlive(_ticket); GC.KeepAlive(parsed); }
    }
}


internal readonly record struct BasicSortCopyCost(nuint Slots, nuint Bytes);

// Sort owns all tickets; none escape or transfer admission to another row.
internal sealed class FerruleBasicSortOperation
{
    private readonly FerruleRetainedSourceCohort _original;
    private readonly BasicSourceBudget _budget;
    private readonly byte[] _copies;
    private FerruleBasicSortOperation(FerruleRetainedSourceCohort original, BasicSourceBudget budget, byte copies)
    {
        _original = original; _budget = budget; _copies = new byte[original.Count];
        for (var i = 0; i < _copies.Length; i++) _copies[i] = copies;
    }
    private FerruleBasicSourceCandidate Copy(int originalId, nuint position)
    {
        if ((uint)originalId >= (uint)_copies.Length || _copies[originalId] == 0)
            throw BasicPhaseAdmission.Invalid((nuint)originalId);
        var original = _original[originalId]; var cost = original.SortCopyCost();
        _copies[originalId]--;
        var beforeSlots = _budget.Slots.Used; var beforeBytes = _budget.Bytes.Used;
        // Consume only this exact prepaid copy. Normal Copy keeps all its guards
        // and charges the same counters before any predicate or target resumes.
        _budget.Slots.Release(cost.Slots); _budget.Bytes.Release(cost.Bytes);
        var result = original.Copy(_budget, position);
        if (_budget.Slots.Used != beforeSlots || _budget.Bytes.Used != beforeBytes)
            throw BasicPhaseAdmission.Invalid((nuint)originalId);
        return result;
    }
    internal static void ValidateOriginalIds(int originalCount, BasicSourceIds original)
    {
        for (var i = 0; i < original.Count; i++)
        {
            var id = original.SourceIdAt(i);
            if (id < (nuint)i) throw BasicPhaseAdmission.Duplicate((nuint)i, id);
            if (id != (nuint)i || id >= (nuint)originalCount) throw BasicPhaseAdmission.Invalid((nuint)i);
        }
        if (original.Count != originalCount) throw BasicPhaseAdmission.Invalid((nuint)original.Count);
    }
    internal static IReadOnlyList<FerruleInstance> Run(FerruleBasicScopeContext parent,
        FerruleRetainedSourceCohort original, IReadOnlyList<FerruleSortKey<FerruleBasicScopeContext>> keys,
        bool filterBeforeSort, Func<FerruleBasicScopeContext, bool>? predicate,
        Func<FerruleBasicScopeContext, FerruleInstance> target)
    {
        ArgumentNullException.ThrowIfNull(original); ArgumentNullException.ThrowIfNull(keys);
        ArgumentNullException.ThrowIfNull(target);
        var witness = original.SourceWitness;
        if (!ReferenceEquals(parent.Roots, witness.Parent.Roots))
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.RegistryMismatch);
        if (!ReferenceEquals(parent, witness.Parent) || !witness.IsComplete) throw BasicPhaseAdmission.Invalid(0);
        var count = original.Count; var keyCount = keys.Count;
        if (keyCount < 0) throw BasicSourceAccounting.Overflow();
        BasicSourceAccounting.Limit(0, (nuint)count, parent.Roots.Limits.MaxWalkContexts,
            FerruleBasicSourceBridgeComponent.WalkContexts);
        for (var i = 0; i < count; i++)
            if (!ReferenceEquals(original[i].SourceWitness, witness) || !ReferenceEquals(original[i].Context.Roots, parent.Roots))
                throw BasicPhaseAdmission.Invalid((nuint)i);
        ValidateOriginalIds(witness.OriginalCount, original);
        var budget = new BasicSourceBudget(parent.Roots.Limits, true);
        parent.Roots.Seed(budget); parent.Seed(budget);
        budget.Charge((nuint)count + 3); // complete original backing, holder, witness and operation
        for (var i = 0; i < count; i++) { original[i].Context.Seed(budget); budget.Charge(1); }
        var cells = BasicSourceAccounting.Multiply((nuint)count, (nuint)keyCount);
        var keyCapacity = BasicSourceAccounting.ArrayLength(cells);
        _ = BasicSourceAccounting.ArrayLength((nuint)count); _ = BasicSourceAccounting.ArrayLength((nuint)keyCount);
        // Cache plus StableSort's duplicate key cells; bounded IDs/output/tickets,
        // decorated records, backing, scratch, snapshots, closures and wrappers.
        var workspace = BasicSourceAccounting.Add(BasicSourceAccounting.Multiply(2, cells),
            BasicSourceAccounting.Add(BasicSourceAccounting.Multiply(12, (nuint)count),
                BasicSourceAccounting.Add(BasicSourceAccounting.Multiply(10, (nuint)keyCount), 16)));
        budget.Charge(workspace);
        var copies = filterBeforeSort ? (byte)1 : (byte)2;
        for (var i = 0; i < count; i++)
        {
            var cost = original[i].SortCopyCost();
            for (var j = 0; j < copies; j++) { budget.Charge(cost.Slots); budget.Text(cost.Bytes); }
        }
        // Caller key collection is read only once before any callback can mutate it.
        var snapshot = new FerruleSortKey<FerruleBasicScopeContext>[keyCount];
        for (var i = 0; i < snapshot.Length; i++)
        {
            snapshot[i] = keys[i]; ArgumentNullException.ThrowIfNull(snapshot[i].Selector);
        }
        var cached = new FerruleValue[keyCapacity];
        var acceptedIds = new List<int>(count);
        var operation = new FerruleBasicSortOperation(original, budget, copies);
        var output = new List<FerruleInstance>(count);
        if (filterBeforeSort)
        {
            // This whole predicate pass must precede every key and target.
            for (var i = 0; i < count; i++)
                if (predicate is null || predicate(original[i].Context)) acceptedIds.Add(i);
        }
        else
            for (var i = 0; i < count; i++) acceptedIds.Add(i);
        for (var row = 0; row < acceptedIds.Count; row++)
        {
            var id = acceptedIds[row];
            for (var key = 0; key < snapshot.Length; key++)
            {
                var value = snapshot[key].Selector(original[id].Context);
                if (value.Kind == FerruleValueKind.String) budget.Text(OwnedSourceAccounting.Text(value.StringValue));
                cached[checked(id * keyCount + key)] = value;
            }
        }
        var cachedKeys = new FerruleSortKey<int>[keyCount];
        for (var key = 0; key < cachedKeys.Length; key++)
        {
            var capturedKey = key; // each selector owns this immutable key index
            cachedKeys[key] = new(id => cached[checked(id * keyCount + capturedKey)], snapshot[key].Descending);
        }
        var sortedIds = FerruleSequences.StableSort(acceptedIds, cachedKeys);
        for (var rank = 0; rank < sortedIds.Count; rank++)
        {
            var id = sortedIds[rank];
            var sorted = operation.Copy(id, (nuint)rank + 1);
            if (!filterBeforeSort && predicate is not null && !predicate(sorted.Context)) continue;
            var produced = filterBeforeSort ? sorted : operation.Copy(id, (nuint)output.Count + 1);
            output.Add(target(produced.Context)); // complete before the next predicate
        }
        GC.KeepAlive(original);
        return output;
    }
}

// Windows own their complete original cohort and every phase under one ledger.
// No input tree is copied and no phase helper can reset admission mid-operation.
internal sealed class FerruleBasicWindowOperation
{
    private readonly FerruleRetainedSourceCohort _original;
    private readonly BasicSourceBudget _budget;
    private readonly byte[] _copies;
    private FerruleBasicWindowOperation(FerruleRetainedSourceCohort original, BasicSourceBudget budget, byte copies)
    {
        _original = original; _budget = budget; _copies = new byte[original.Count];
        for (var i = 0; i < _copies.Length; i++) _copies[i] = copies;
    }
    private FerruleBasicSourceCandidate Copy(int id, nuint position)
    {
        if ((uint)id >= (uint)_copies.Length || _copies[id] == 0) throw BasicPhaseAdmission.Invalid((nuint)id);
        var original = _original[id]; var cost = original.SortCopyCost();
        _copies[id]--;
        var slots = _budget.Slots.Used; var bytes = _budget.Bytes.Used;
        _budget.Slots.Release(cost.Slots); _budget.Bytes.Release(cost.Bytes);
        var result = original.Copy(_budget, position);
        if (_budget.Slots.Used != slots || _budget.Bytes.Used != bytes) throw BasicPhaseAdmission.Invalid((nuint)id);
        return result;
    }
    internal static IReadOnlyList<FerruleInstance> Run(FerruleBasicScopeContext parent,
        FerruleRetainedSourceCohort original, IReadOnlyList<FerruleSortKey<FerruleBasicScopeContext>> keys,
        bool filterBeforeSort, Func<FerruleBasicScopeContext, bool>? predicate,
        IReadOnlyList<Func<FerruleBasicScopeContext, FerruleSequenceWindow>> bounds, bool implicitFirst,
        Func<FerruleBasicScopeContext, FerruleInstance> target)
    {
        ArgumentNullException.ThrowIfNull(original); ArgumentNullException.ThrowIfNull(keys);
        ArgumentNullException.ThrowIfNull(bounds); ArgumentNullException.ThrowIfNull(target);
        var witness = original.SourceWitness;
        if (!ReferenceEquals(parent.Roots, witness.Parent.Roots))
            throw new FerruleBasicSourceBridgeException(FerruleBasicSourceBridgeErrorKind.RegistryMismatch);
        if (!ReferenceEquals(parent, witness.Parent) || !witness.IsComplete) throw BasicPhaseAdmission.Invalid(0);
        var count = original.Count; var keyCount = keys.Count; var boundCount = bounds.Count;
        if (keyCount < 0 || boundCount < 0) throw BasicSourceAccounting.Overflow();
        BasicSourceAccounting.Limit(0, (nuint)count, parent.Roots.Limits.MaxWalkContexts,
            FerruleBasicSourceBridgeComponent.WalkContexts);
        for (var i = 0; i < count; i++)
            if (!ReferenceEquals(original[i].SourceWitness, witness) || !ReferenceEquals(original[i].Context.Roots, parent.Roots))
                throw BasicPhaseAdmission.Invalid((nuint)i);
        FerruleBasicSortOperation.ValidateOriginalIds(witness.OriginalCount, original);
        var windowCount = BasicSourceAccounting.Add((nuint)boundCount, implicitFirst ? (nuint)1 : 0);
        var keyCells = BasicSourceAccounting.Multiply((nuint)count, (nuint)keyCount);
        var keyCapacity = BasicSourceAccounting.ArrayLength(keyCells);
        var windowCapacity = BasicSourceAccounting.ArrayLength(windowCount);
        _ = BasicSourceAccounting.ArrayLength((nuint)count); _ = BasicSourceAccounting.ArrayLength((nuint)keyCount);
        var hasSort = keyCount != 0;
        var preFilter = hasSort && filterBeforeSort;
        var sortedPredicates = hasSort && !preFilter && predicate is not null;
        var copies = sortedPredicates ? (byte)2 : (byte)1;
        var budget = new BasicSourceBudget(parent.Roots.Limits, true);
        parent.Roots.Seed(budget); parent.Seed(budget);
        budget.Charge((nuint)count + 3); // original backing, holder, witness, operation
        for (var i = 0; i < count; i++) { original[i].Context.Seed(budget); budget.Charge(1); }
        // Complete bounded ID/key/sort/output/ticket storage, callback snapshots,
        // evaluated windows, and the maximum backing produced by each window.
        var workspace = BasicSourceAccounting.Add(BasicSourceAccounting.Multiply(2, keyCells),
            BasicSourceAccounting.Add(BasicSourceAccounting.Multiply(14, (nuint)count),
                BasicSourceAccounting.Add(BasicSourceAccounting.Multiply(10, (nuint)keyCount),
                    BasicSourceAccounting.Add(BasicSourceAccounting.Multiply(3, windowCount), 20))));
        workspace = BasicSourceAccounting.Add(workspace, BasicSourceAccounting.Multiply((nuint)count, windowCount));
        budget.Charge(workspace);
        for (var i = 0; i < count; i++)
        {
            var cost = original[i].SortCopyCost();
            for (var copy = 0; copy < copies; copy++) { budget.Charge(cost.Slots); budget.Text(cost.Bytes); }
        }
        // Snapshot every caller-owned descriptor before invoking any effectful callback.
        var keySnapshot = new FerruleSortKey<FerruleBasicScopeContext>[keyCount];
        for (var i = 0; i < keyCount; i++)
        { keySnapshot[i] = keys[i]; ArgumentNullException.ThrowIfNull(keySnapshot[i].Selector); }
        var boundSnapshot = new Func<FerruleBasicScopeContext, FerruleSequenceWindow>[boundCount];
        for (var i = 0; i < boundCount; i++)
        { boundSnapshot[i] = bounds[i]; ArgumentNullException.ThrowIfNull(boundSnapshot[i]); }
        var cached = new FerruleValue[keyCapacity];
        var ids = new List<int>(count);
        var operation = new FerruleBasicWindowOperation(original, budget, copies);
        var output = new List<FerruleInstance>(count);
        if (preFilter)
        {
            for (var i = 0; i < count; i++)
                if (predicate is null || predicate(original[i].Context)) ids.Add(i);
        }
        else
            for (var i = 0; i < count; i++) ids.Add(i);
        if (hasSort)
        {
            for (var row = 0; row < ids.Count; row++)
            {
                var id = ids[row];
                for (var key = 0; key < keyCount; key++)
                {
                    var value = keySnapshot[key].Selector(original[id].Context);
                    if (value.Kind == FerruleValueKind.String) budget.Text(OwnedSourceAccounting.Text(value.StringValue));
                    cached[checked(id * keyCount + key)] = value;
                }
            }
            var cachedKeys = new FerruleSortKey<int>[keyCount];
            for (var key = 0; key < keyCount; key++)
            {
                var captured = key;
                cachedKeys[key] = new(id => cached[checked(id * keyCount + captured)], keySnapshot[key].Descending);
            }
            ids = new List<int>(FerruleSequences.StableSort(ids, cachedKeys));
        }
        // This frontier is after the pre-filter/keys/sort and before the selection
        // predicate batch. Every bound is evaluated once in the authentic parent,
        // even on an empty cohort or after an earlier First(0).
        var evaluated = new FerruleSequenceWindow[windowCapacity];
        for (var i = 0; i < boundCount; i++) evaluated[i] = boundSnapshot[i](parent);
        if (implicitFirst) evaluated[^1] = FerruleSequenceWindow.First(1);
        if (!preFilter && predicate is not null)
        {
            var selected = new List<int>(count);
            for (var rank = 0; rank < ids.Count; rank++)
            {
                var id = ids[rank];
                var context = hasSort ? operation.Copy(id, (nuint)rank + 1).Context : original[id].Context;
                if (predicate(context)) selected.Add(id);
            }
            ids = selected;
        }
        var finalIds = FerruleSequences.ApplyWindows(ids, evaluated);
        for (var i = 0; i < finalIds.Count; i++)
            output.Add(target(operation.Copy(finalIds[i], (nuint)i + 1).Context));
        GC.KeepAlive(original);
        return output;
    }
}
