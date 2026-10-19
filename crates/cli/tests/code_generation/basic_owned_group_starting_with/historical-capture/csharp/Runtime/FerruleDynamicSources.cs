using System.Collections.ObjectModel;

namespace Ferrule.Runtime;

/// <summary>Host boundary for already parsed dynamic source documents.</summary>
public interface IFerruleDynamicSourceLoader
{
    FerruleInstance Load(string sourceName, string logicalPath);
}

/// <summary>Host boundary for bounded schema-shaped JSON dynamic sources.</summary>
public interface IFerruleDynamicJsonSourceLoader
{
    byte[] Load(string sourceName, string logicalPath);
}

public static class FerruleDynamicSourceLimits
{
    public const int MaximumLoads = 1_000_000;
    public const int MaximumPathBytes = 4096;
    public const int MaximumDocumentBytes = 64 * 1024 * 1024;
    public const int MaximumTotalBytes = 256 * 1024 * 1024;
}

/// <summary>
/// Owns one source document per non-absent driver path and keeps each loaded
/// document paired with the exact driver context that requested it.
/// </summary>
public sealed class FerruleDynamicSourceItems
{
    private readonly string _source;
    private readonly IReadOnlyList<string> _tail;
    private readonly IReadOnlyList<ScopeContext> _drivers;
    private readonly IReadOnlyList<FerruleInstance> _documents;

    private FerruleDynamicSourceItems(
        string source,
        IReadOnlyList<string> tail,
        IReadOnlyList<ScopeContext> drivers,
        IReadOnlyList<FerruleInstance> documents)
    {
        _source = source;
        _tail = tail;
        _drivers = drivers;
        _documents = documents;
    }

    public static FerruleDynamicSourceItems Load(
        ScopeContext context,
        string source,
        IReadOnlyList<string> driver,
        IReadOnlyList<string> tail,
        uint node,
        Func<ScopeContext, FerruleValue> path)
    {
        ArgumentNullException.ThrowIfNull(context);
        ArgumentException.ThrowIfNullOrEmpty(source);
        ArgumentNullException.ThrowIfNull(driver);
        ArgumentNullException.ThrowIfNull(tail);
        ArgumentNullException.ThrowIfNull(path);

        var loader = context.DynamicSourceLoader;
        if (loader is null)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.MissingDynamicSourceLoader,
                $"dynamic source '{source}' requires a host source loader",
                sourceField: source);
        }
        var candidates = context.IterateSource(driver);
        if (candidates.Count > FerruleDynamicSourceLimits.MaximumLoads)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.DynamicSourceTooMany,
                $"dynamic source '{source}' requested more than " +
                    $"{FerruleDynamicSourceLimits.MaximumLoads} documents",
                maximumItems: (UInt128)FerruleDynamicSourceLimits.MaximumLoads,
                sourceField: source);
        }

        var drivers = new List<ScopeContext>(candidates.Count);
        var documents = new List<FerruleInstance>(candidates.Count);
        foreach (var candidate in candidates)
        {
            var pathValue = path(candidate);
            if (pathValue.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
            {
                continue;
            }
            if (pathValue.Kind != FerruleValueKind.String)
            {
                throw new FerruleRuntimeException(
                    FerruleRuntimeError.DynamicSourcePath,
                    $"node {node}: dynamic source '{source}' path expected a string " +
                        $"or absent value, got {pathValue.Kind}",
                    node: node,
                    foundKind: pathValue.Kind,
                    sourceField: source);
            }
            var logicalPath = pathValue.StringValue;
            if (global::System.Text.Encoding.UTF8.GetByteCount(logicalPath) >
                FerruleDynamicSourceLimits.MaximumPathBytes)
            {
                throw new FerruleRuntimeException(
                    FerruleRuntimeError.DynamicSourcePathTooLong,
                    $"dynamic source '{source}' path exceeds the " +
                        $"{FerruleDynamicSourceLimits.MaximumPathBytes}-byte limit",
                    maximumItems: (UInt128)FerruleDynamicSourceLimits.MaximumPathBytes,
                    sourceField: source);
            }

            FerruleInstance document;
            try
            {
                document = loader.Load(source, logicalPath);
                if (document is null)
                {
                    throw new InvalidOperationException("loader returned null");
                }
            }
            catch (Exception error)
            {
                throw new FerruleRuntimeException(
                    FerruleRuntimeError.DynamicSourceLoad,
                    $"dynamic source '{source}' could not load '{logicalPath}': {error.Message}",
                    error,
                    detail: logicalPath,
                    sourceField: source);
            }
            drivers.Add(candidate);
            documents.Add(document);
        }

        return new FerruleDynamicSourceItems(
            source,
            new ReadOnlyCollection<string>(tail.ToArray()),
            new ReadOnlyCollection<ScopeContext>(drivers),
            new ReadOnlyCollection<FerruleInstance>(documents));
    }

    public IReadOnlyList<ScopeContext> Contexts()
    {
        var contexts = new List<ScopeContext>();
        for (var index = 0; index < _documents.Count; index++)
        {
            contexts.AddRange(
                _drivers[index].IterateLoadedSource(_source, _documents[index], _tail));
        }
        return new ReadOnlyCollection<ScopeContext>(contexts);
    }
}
