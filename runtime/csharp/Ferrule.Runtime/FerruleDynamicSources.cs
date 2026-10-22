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

        var loader = RequireLoader(context.DynamicSourceLoader, source);
        var candidates = context.IterateSource(driver);
        RequireDriverCount(candidates.Count, source);

        var drivers = new List<ScopeContext>(candidates.Count);
        var documents = new List<FerruleInstance>(candidates.Count);
        foreach (var candidate in candidates)
        {
            var logicalPath = LogicalPath(path(candidate), source, node);
            if (logicalPath is null)
            {
                continue;
            }
            var document = LoadDocument(loader, source, logicalPath);
            drivers.Add(candidate);
            documents.Add(document);
        }

        return new FerruleDynamicSourceItems(
            source,
            new ReadOnlyCollection<string>(tail.ToArray()),
            new ReadOnlyCollection<ScopeContext>(drivers),
            new ReadOnlyCollection<FerruleInstance>(documents));
    }

    /// <summary>A reached dynamic scope needs a loader, even for an empty driver walk.</summary>
    internal static IFerruleDynamicSourceLoader RequireLoader(IFerruleDynamicSourceLoader? loader, string source) =>
        loader ?? throw new FerruleRuntimeException(
            FerruleRuntimeError.MissingDynamicSourceLoader,
            $"dynamic source '{source}' requires a host source loader",
            sourceField: source);

    internal static void RequireDriverCount(int count, string source)
    {
        if (count > FerruleDynamicSourceLimits.MaximumLoads)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.DynamicSourceTooMany,
                $"dynamic source '{source}' requested more than " +
                    $"{FerruleDynamicSourceLimits.MaximumLoads} documents",
                maximumItems: (UInt128)FerruleDynamicSourceLimits.MaximumLoads,
                sourceField: source);
        }
    }

    /// <summary>The logical path of one driver, or null when it is absent.</summary>
    internal static string? LogicalPath(FerruleValue pathValue, string source, uint node)
    {
        if (pathValue.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
        {
            return null;
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
        return logicalPath;
    }

    /// <summary>One host callback; every failure keeps its source and path.</summary>
    internal static FerruleInstance LoadDocument(IFerruleDynamicSourceLoader loader, string source, string logicalPath)
    {
        try
        {
            var document = loader.Load(source, logicalPath);
            if (document is null)
            {
                throw new InvalidOperationException("loader returned null");
            }
            return document;
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

/// <summary>Where a dynamic-source request failed. Tokens match the Rust runtime.</summary>
public enum FerruleDynamicSourcePhase
{
    /// <summary>Before any host callback: loader, path or request-count checks.</summary>
    Request,
    /// <summary>The host could not acquire the document.</summary>
    Acquisition,
    /// <summary>The returned document failed its endpoint codec.</summary>
    Codec,
    /// <summary>A generated boundary resource limit refused the document.</summary>
    Resource,
}

/// <summary>The specific failure within its phase.</summary>
public enum FerruleDynamicSourceCategory
{
    MissingLoader,
    PathType,
    PathTooLong,
    TooManyRequests,
    Host,
    Encoding,
    Document,
    UndeclaredSource,
    DocumentBytes,
    TotalBytes,
    RequestCount,
}

/// <summary>How an acquired document failed its endpoint codec.</summary>
public enum FerruleDynamicSourceCodecKind
{
    Encoding,
    Document,
    UndeclaredSource,
}

/// <summary>A generated boundary resource that refused a dynamic document.</summary>
public enum FerruleDynamicSourceResource
{
    DocumentBytes,
    TotalBytes,
    RequestCount,
}

public sealed record FerruleDynamicSourceResourceLimit(
    FerruleDynamicSourceResource Resource, ulong Observed, ulong Limit);

/// <summary>Tagged failure a host or generated adapter throws for one dynamic document.</summary>
/// <remarks>
/// Acquisition belongs to the host; codec and resource faults belong to the
/// generated boundary. The original cause is the inner exception. Any other
/// host exception remains a legacy acquisition failure and is retained as is.
/// </remarks>
public sealed class FerruleDynamicSourceFault : InvalidOperationException
{
    private FerruleDynamicSourceFault(
        FerruleDynamicSourcePhase phase,
        FerruleDynamicSourceCodecKind? codecKind,
        FerruleDynamicSourceResourceLimit? resourceLimit,
        string message,
        Exception? cause)
        : base(message, cause)
    {
        Phase = phase;
        CodecKind = codecKind;
        ResourceLimit = resourceLimit;
    }

    public FerruleDynamicSourcePhase Phase { get; }

    public FerruleDynamicSourceCodecKind? CodecKind { get; }

    public FerruleDynamicSourceResourceLimit? ResourceLimit { get; }

    public FerruleDynamicSourceCategory Category => Phase switch
    {
        FerruleDynamicSourcePhase.Acquisition => FerruleDynamicSourceCategory.Host,
        FerruleDynamicSourcePhase.Codec => CodecKind switch
        {
            FerruleDynamicSourceCodecKind.Encoding => FerruleDynamicSourceCategory.Encoding,
            FerruleDynamicSourceCodecKind.UndeclaredSource => FerruleDynamicSourceCategory.UndeclaredSource,
            _ => FerruleDynamicSourceCategory.Document,
        },
        _ => ResourceLimit!.Resource switch
        {
            FerruleDynamicSourceResource.DocumentBytes => FerruleDynamicSourceCategory.DocumentBytes,
            FerruleDynamicSourceResource.TotalBytes => FerruleDynamicSourceCategory.TotalBytes,
            _ => FerruleDynamicSourceCategory.RequestCount,
        },
    };

    public static FerruleDynamicSourceFault ForAcquisition(Exception cause)
    {
        ArgumentNullException.ThrowIfNull(cause);
        return new(FerruleDynamicSourcePhase.Acquisition, null, null, cause.Message, cause);
    }

    public static FerruleDynamicSourceFault ForCodec(FerruleDynamicSourceCodecKind kind, Exception cause)
    {
        ArgumentNullException.ThrowIfNull(cause);
        var message = kind == FerruleDynamicSourceCodecKind.Encoding
            ? $"document is not UTF-8: {cause.Message}"
            : cause.Message;
        return new(FerruleDynamicSourcePhase.Codec, kind, null, message, cause);
    }

    public static FerruleDynamicSourceFault ForUndeclaredSource(string sourceName)
    {
        ArgumentNullException.ThrowIfNull(sourceName);
        return new(FerruleDynamicSourcePhase.Codec, FerruleDynamicSourceCodecKind.UndeclaredSource, null,
            $"undeclared dynamic source '{sourceName}'", null);
    }

    public static FerruleDynamicSourceFault ForResource(
        FerruleDynamicSourceResource resource, ulong observed, ulong limit)
    {
        var message = resource switch
        {
            FerruleDynamicSourceResource.DocumentBytes =>
                $"document exceeds the {limit}-byte dynamic-source limit",
            FerruleDynamicSourceResource.TotalBytes =>
                $"documents exceed the {limit}-byte combined dynamic-source limit",
            _ => $"requests exceed the {limit}-request dynamic-source limit",
        };
        return new(FerruleDynamicSourcePhase.Resource, null,
            new FerruleDynamicSourceResourceLimit(resource, observed, limit), message, null);
    }
}

/// <summary>A small, stable view of one dynamic-source failure.</summary>
/// <remarks>Phase and category tokens are shared with the Rust runtime's
/// <c>DynamicSourceDiagnostic</c>; messages and causes stay on the original exception.</remarks>
public sealed record FerruleDynamicSourceDiagnostic(
    FerruleDynamicSourcePhase Phase,
    string Endpoint,
    FerruleDynamicSourceCategory Category,
    string? Path,
    FerruleDynamicSourceResourceLimit? Resource)
{
    public string PhaseToken => Token(Phase);

    public string CategoryToken => Token(Category);

    public static string Token(FerruleDynamicSourcePhase phase) => phase switch
    {
        FerruleDynamicSourcePhase.Request => "request",
        FerruleDynamicSourcePhase.Acquisition => "acquisition",
        FerruleDynamicSourcePhase.Codec => "codec",
        FerruleDynamicSourcePhase.Resource => "resource",
        _ => throw new ArgumentOutOfRangeException(nameof(phase)),
    };

    public static string Token(FerruleDynamicSourceCategory category) => category switch
    {
        FerruleDynamicSourceCategory.MissingLoader => "missing_loader",
        FerruleDynamicSourceCategory.PathType => "path_type",
        FerruleDynamicSourceCategory.PathTooLong => "path_too_long",
        FerruleDynamicSourceCategory.TooManyRequests => "too_many_requests",
        FerruleDynamicSourceCategory.Host => "host",
        FerruleDynamicSourceCategory.Encoding => "encoding",
        FerruleDynamicSourceCategory.Document => "document",
        FerruleDynamicSourceCategory.UndeclaredSource => "undeclared_source",
        FerruleDynamicSourceCategory.DocumentBytes => "document_bytes",
        FerruleDynamicSourceCategory.TotalBytes => "total_bytes",
        FerruleDynamicSourceCategory.RequestCount => "request_count",
        _ => throw new ArgumentOutOfRangeException(nameof(category)),
    };

    /// <summary>Projects the runtime's own dynamic-source failures; others are null.</summary>
    public static FerruleDynamicSourceDiagnostic? FromRuntimeException(FerruleRuntimeException error)
    {
        ArgumentNullException.ThrowIfNull(error);
        if (error.SourceField is not { } endpoint)
        {
            return null;
        }
        FerruleDynamicSourceDiagnostic Request(FerruleDynamicSourceCategory category) =>
            new(FerruleDynamicSourcePhase.Request, endpoint, category, null, null);
        return error.Error switch
        {
            FerruleRuntimeError.MissingDynamicSourceLoader => Request(FerruleDynamicSourceCategory.MissingLoader),
            FerruleRuntimeError.DynamicSourcePath => Request(FerruleDynamicSourceCategory.PathType),
            FerruleRuntimeError.DynamicSourcePathTooLong => Request(FerruleDynamicSourceCategory.PathTooLong),
            FerruleRuntimeError.DynamicSourceTooMany => Request(FerruleDynamicSourceCategory.TooManyRequests),
            FerruleRuntimeError.DynamicSourceLoad => error.InnerException is FerruleDynamicSourceFault fault
                ? new(fault.Phase, endpoint, fault.Category, error.Detail, fault.ResourceLimit)
                : new(FerruleDynamicSourcePhase.Acquisition, endpoint, FerruleDynamicSourceCategory.Host,
                    error.Detail, null),
            _ => null,
        };
    }

    /// <summary>Projects a generated XML adapter's owned dynamic-input refusal.
    /// Unowned XML failures are null; <see cref="Find"/> continues to their cause.</summary>
    public static FerruleDynamicSourceDiagnostic? FromXmlExecutionException(FerruleXmlExecutionException error)
    {
        ArgumentNullException.ThrowIfNull(error);
        if (error.Request is not { } request)
        {
            return null;
        }
        var boundary = error.Boundary;
        if (boundary.Kind == FerruleXmlBoundaryErrorKind.DocumentLimit)
        {
            var limit = boundary.Bytes is { } bytes && boundary.Limit is { } maximum
                ? new FerruleDynamicSourceResourceLimit(
                    FerruleDynamicSourceResource.DocumentBytes, (ulong)bytes, (ulong)maximum)
                : null;
            return new(FerruleDynamicSourcePhase.Resource, request.Source,
                FerruleDynamicSourceCategory.DocumentBytes, request.Path, limit);
        }
        if (boundary.InnerException is FerruleXmlInputSetResourceException counter)
        {
            var (category, resource) = counter.Resource == "xml_input_artifact_count"
                ? (FerruleDynamicSourceCategory.RequestCount, FerruleDynamicSourceResource.RequestCount)
                : (FerruleDynamicSourceCategory.TotalBytes, FerruleDynamicSourceResource.TotalBytes);
            return new(FerruleDynamicSourcePhase.Resource, request.Source, category, request.Path,
                new FerruleDynamicSourceResourceLimit(resource, counter.ObservedCount, counter.Limit));
        }
        var codec = boundary.Kind == FerruleXmlBoundaryErrorKind.Utf8
            ? FerruleDynamicSourceCategory.Encoding
            : FerruleDynamicSourceCategory.Document;
        return new(FerruleDynamicSourcePhase.Codec, request.Source, codec, request.Path, null);
    }

    /// <summary>Projects the first dynamic-source failure in an exception and its inner chain.</summary>
    public static FerruleDynamicSourceDiagnostic? Find(Exception error)
    {
        ArgumentNullException.ThrowIfNull(error);
        for (Exception? current = error; current is not null; current = current.InnerException)
        {
            var projected = current switch
            {
                FerruleRuntimeException runtime => FromRuntimeException(runtime),
                FerruleXmlExecutionException xml => FromXmlExecutionException(xml),
                _ => null,
            };
            if (projected is not null)
            {
                return projected;
            }
        }
        return null;
    }
}

/// <summary>Admits one execution's host JSON documents for their declared endpoints.</summary>
/// <remarks>
/// Each request calls the host first, then checks the per-document and combined
/// byte limits, strict UTF-8 and the endpoint's embedded schema, in that order.
/// Host exceptions pass through unchanged; adapter refusals throw
/// <see cref="FerruleDynamicSourceFault"/> with the original cause inside.
/// </remarks>
public sealed class FerruleJsonDynamicSourceAdapter : IFerruleDynamicSourceLoader
{
    private static readonly System.Text.UTF8Encoding StrictUtf8 = new(false, true);
    private readonly IFerruleDynamicJsonSourceLoader _loader;
    private readonly Func<string, string?> _schema;
    private readonly long _documentLimit;
    private readonly long _totalLimit;
    private long _totalBytes;

    /// <param name="loader">The host loader.</param>
    /// <param name="schema">Resolves an exact declared endpoint name to its embedded schema.</param>
    public FerruleJsonDynamicSourceAdapter(IFerruleDynamicJsonSourceLoader loader, Func<string, string?> schema)
        : this(loader, schema, FerruleDynamicSourceLimits.MaximumDocumentBytes,
            FerruleDynamicSourceLimits.MaximumTotalBytes)
    {
    }

    internal FerruleJsonDynamicSourceAdapter(
        IFerruleDynamicJsonSourceLoader loader,
        Func<string, string?> schema,
        long documentLimit,
        long totalLimit)
    {
        ArgumentNullException.ThrowIfNull(loader);
        ArgumentNullException.ThrowIfNull(schema);
        _loader = loader;
        _schema = schema;
        _documentLimit = documentLimit;
        _totalLimit = totalLimit;
    }

    public FerruleInstance Load(string sourceName, string logicalPath)
    {
        var document = _loader.Load(sourceName, logicalPath) ??
            throw new InvalidOperationException("loader returned null");
        var bytes = document.LongLength;
        if (bytes > _documentLimit)
        {
            throw FerruleDynamicSourceFault.ForResource(
                FerruleDynamicSourceResource.DocumentBytes, (ulong)bytes, (ulong)_documentLimit);
        }
        var total = _totalBytes + bytes;
        if (total > _totalLimit)
        {
            throw FerruleDynamicSourceFault.ForResource(
                FerruleDynamicSourceResource.TotalBytes, (ulong)total, (ulong)_totalLimit);
        }
        _totalBytes = total;
        string text;
        try
        {
            text = StrictUtf8.GetString(document);
        }
        catch (System.Text.DecoderFallbackException error)
        {
            throw FerruleDynamicSourceFault.ForCodec(FerruleDynamicSourceCodecKind.Encoding, error);
        }
        var schema = _schema(sourceName) ?? throw FerruleDynamicSourceFault.ForUndeclaredSource(sourceName);
        try
        {
            return FerruleJson.ParseEmbedded(schema, text);
        }
        catch (FerruleRuntimeException error) when (error.Error == FerruleRuntimeError.JsonBoundary)
        {
            throw FerruleDynamicSourceFault.ForCodec(FerruleDynamicSourceCodecKind.Document, error);
        }
    }
}
