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
        ArgumentNullException.ThrowIfNull(name);
        if (_executionContext is null ||
            !_executionContext.TryGetParameter(name, out var value))
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.MissingRuntimeParameter,
                $"node {node}: execution context does not provide runtime parameter '{name}'",
                node: node,
                runtimeParameter: name);
        }
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
        FerruleGroup group => group.CloneFields(group.Fields.Select(field =>
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
                        PushCollection(
                            document.Value,
                            prefix,
                            index + 1,
                            frames,
                            collections,
                            document.ResolvedSourcePath ?? document.Path);
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
                PushCollection(
                    document.Value,
                    prefix,
                    index + 1,
                    frames,
                    collections,
                    document.ResolvedSourcePath ?? document.Path);
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
            _primarySource,
            new ReadOnlyCollection<FerruleInstance>(allFrames),
            new ReadOnlyCollection<CollectionIdentity>(allCollections),
            _executionContext,
            _dynamicSourceLoader, _filterMapRunState, _filterMapBoundary, _filterMapCallDepth));
    }

    private static void PushCollection(
        FerruleInstance item,
        IReadOnlyList<string> path,
        int index,
        ICollection<FerruleInstance> frames,
        ICollection<CollectionIdentity> collections,
        string? documentPath = null)
    {
        frames.Add(item);
        collections.Add(new CollectionIdentity(
            path.ToArray(),
            item,
            index,
            DocumentPath: documentPath));
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
        int pathIndex,
        bool absenceAsNull = false)
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

    private sealed record CollectionIdentity(
        IReadOnlyList<string> Path,
        FerruleInstance Item,
        int Index,
        ulong? Join = null,
        int? JoinPosition = null,
        bool Grouped = false,
        string? DocumentPath = null);

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
}
public enum FerruleOwnedSourceLimitKind
{
    Roots, Frames, PathSegments, MetadataBytes, WalkContexts, MetadataSlots,
}
public enum FerruleOwnedSourceInstanceKind { Scalar, Group, Repeated, DocumentSet, MappedSequence }
public enum FerruleOwnedSourceTraversalKind { CannotTraverse, ExpectedScalar }

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
    internal nuint RegistryText { get; }
    private FerruleOwnedSourceRoots(FerruleInstance primary, Binding[] named,
        FerruleOwnedSourceLimits limits, nuint slots, nuint text)
    { _primary = primary; _named = named; Limits = limits; SlotMaximum = slots; RegistryText = text; }
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
    internal FerruleInstance Instance(int key) => key == 0 ? _primary : _named[key - 1].Instance!;
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
        Slots.Charge(Roots.RegistryCount); Slots.Charge(1); Bytes.Charge(Roots.RegistryText);
        for (var i = 0; i < context.FrameCount; i++)
        {
            var item = context.FrameAt(i);
            Slots.Charge((nuint)item.Path.Count + 1); Bytes.Charge(OwnedSourceAccounting.PathText(item.Path));
        }
        CheckPath(path); if (frame is not null) CheckPath(frame);
        // Query/error/ambiguity reserves precede namespace resolution and diagnostic copies.
        Slots.Charge(Roots.RegistryCount);
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
    private void CheckPath(IReadOnlyList<string> path)
    {
        ArgumentNullException.ThrowIfNull(path);
        OwnedSourceAccounting.Limit(0, (nuint)path.Count, Roots.Limits.MaxPathSegments,
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
public sealed class FerruleOwnedScopeContext
{
    private readonly record struct Occurrence(int Owner, IReadOnlyList<string> Path, FerruleInstance Item, nuint Index);
    internal readonly record struct Observation(int Owner, IReadOnlyList<string> Path, FerruleInstance Item, nuint Index);
    private readonly Occurrence[] _frames;
    private FerruleOwnedScopeContext(FerruleOwnedSourceRoots roots, Occurrence[] frames) { Roots = roots; _frames = frames; }
    internal static FerruleOwnedScopeContext CreateRoot(FerruleOwnedSourceRoots roots)
    {
        var budget = new OwnedCounter(roots.SlotMaximum, FerruleOwnedSourceLimitKind.MetadataSlots);
        budget.Charge(roots.RegistryCount); budget.Charge(1);
        return new(roots, Array.Empty<Occurrence>());
    }
    internal FerruleOwnedSourceRoots Roots { get; }
    internal int FrameCount => _frames.Length;
    internal Observation FrameAt(int index)
    { var f = _frames[index]; return new(f.Owner, f.Path, f.Item, f.Index); }
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
            if (_frames[i].Owner == owner && (path is null ||
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
        IReadOnlyList<string>? frame = null)
    {
        var q = new OwnedQuery(this, owner, path, frame); var key = Roots.Resolve(owner, q);
        if (frame is not null && !Prefix(path, frame))
            throw q.Error(FerruleOwnedSourceErrorKind.IncompatibleFramePath, path, frame);
        var selected = Match(key, frame ?? path, frame is not null);
        if (frame is not null && selected < 0)
            throw q.Error(FerruleOwnedSourceErrorKind.InactiveSourceFrame, collection: frame);
        var current = selected < 0 ? Roots.Instance(key) : _frames[selected].Item;
        var consumed = selected < 0 ? 0 : _frames[selected].Path.Count;
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
    public nuint Position(FerruleOwnedSourceOwner owner, IReadOnlyList<string>? collection = null)
    {
        var q = new OwnedQuery(this, owner, collection ?? Array.Empty<string>(), null);
        var key = Roots.Resolve(owner, q); var selected = Match(key, collection, true);
        if (selected < 0) throw q.Error(FerruleOwnedSourceErrorKind.InactiveSourceFrame, collection: collection);
        return _frames[selected].Index;
    }
    public FerruleOwnedScopeContext WithPosition(FerruleOwnedSourceOwner owner,
        IReadOnlyList<string>? collection, nuint index)
    {
        var q = new OwnedQuery(this, owner, collection ?? Array.Empty<string>(), null);
        var key = Roots.Resolve(owner, q); var selected = Match(key, collection, true);
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
    private readonly record struct Task(FerruleInstance Instance, int Consumed, int Depth, int Next = -1);
    public IReadOnlyList<FerruleOwnedScopeContext> IterateSource(FerruleOwnedSourceOwner owner, IReadOnlyList<string> path)
    {
        var q = new OwnedQuery(this, owner, path, null); var key = Roots.Resolve(owner, q);
        var identity = Match(key, path, true);
        if (identity >= 0)
        {
            OwnedSourceAccounting.Limit(0, 1, Roots.Limits.MaxWalkContexts, FerruleOwnedSourceLimitKind.WalkContexts);
            var copy = CopyFrames(q, _frames); q.Slots.Charge(2);
            return Array.AsReadOnly(new[] { new FerruleOwnedScopeContext(Roots, copy) });
        }
        var selected = Match(key, path, false);
        var current = selected < 0 ? Roots.Instance(key) : _frames[selected].Item;
        var consumed = selected < 0 ? 0 : _frames[selected].Path.Count;
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
        void Emit()
        {
            OwnedSourceAccounting.Limit((nuint)output.Count, 1, Roots.Limits.MaxWalkContexts, FerruleOwnedSourceLimitKind.WalkContexts);
            nuint slots = (nuint)frames.Count; nuint text = 0;
            for (var i = 0; i < frames.Count; i++)
            { slots = OwnedSourceAccounting.Add(slots, (nuint)frames[i].Path.Count, FerruleOwnedSourceLimitKind.MetadataSlots);
              text = OwnedSourceAccounting.Add(text, OwnedSourceAccounting.PathText(frames[i].Path), FerruleOwnedSourceLimitKind.MetadataBytes); }
            q.Slots.Charge(slots); q.Bytes.Charge(text); q.Slots.Charge(1);
            var snapshot = new Occurrence[frames.Count];
            for (var i = 0; i < snapshot.Length; i++) snapshot[i] = frames[i];
            output.Push(new FerruleOwnedScopeContext(Roots, snapshot));
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
                    if (offset == path.Count) { Emit(); break; }
                    if (item is not FerruleGroup group)
                        throw q.Error(FerruleOwnedSourceErrorKind.Traversal, path, segment: (nuint)offset,
                            found: Kind(item), traversal: FerruleOwnedSourceTraversalKind.CannotTraverse);
                    if (!group.TryGetField(path[offset], out var next)) break;
                    item = next; offset++;
                }
            }
            return output.Retain();
        }
        finally { Truncate(0); frames.ReleaseStorage(); tasks.ReleaseStorage(); }
    }
}
