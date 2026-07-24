using System.Collections.ObjectModel;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>Host-supplied values available to generated mapping expressions.</summary>
public enum FerruleRuntimeValue
{
    MappingFilePath,
    MainMappingFilePath,
    CurrentDateTime,
}

/// <summary>Immutable, bounded named scalar inputs supplied by an execution host.</summary>
public sealed class FerruleRuntimeParameters
{
    public const int MaximumCount = 1_024;
    public const int MaximumNameUtf8Bytes = 256;
    public const int MaximumStringUtf8Bytes = 8 * 1024 * 1024;

    private static readonly UTF8Encoding StrictUtf8 = new(false, true);
    private readonly IReadOnlyDictionary<string, FerruleValue> _values;

    public FerruleRuntimeParameters(
        IEnumerable<KeyValuePair<string, FerruleValue>> parameters)
    {
        ArgumentNullException.ThrowIfNull(parameters);
        var values = new Dictionary<string, FerruleValue>(StringComparer.Ordinal);
        foreach (var (name, value) in parameters)
        {
            if (name is null)
            {
                throw new ArgumentException(
                    "Runtime parameter name cannot be null.",
                    nameof(parameters));
            }
            if (name.Length == 0)
            {
                throw new ArgumentException(
                    "Runtime parameter name cannot be empty.",
                    nameof(parameters));
            }
            if (name.Contains('\0', StringComparison.Ordinal))
            {
                throw new ArgumentException(
                    "Runtime parameter name cannot contain NUL.",
                    nameof(parameters));
            }
            if (Utf8ByteCount(name, nameof(parameters)) > MaximumNameUtf8Bytes)
            {
                throw new ArgumentException(
                    $"Runtime parameter name exceeds {MaximumNameUtf8Bytes} UTF-8 bytes.",
                    nameof(parameters));
            }
            if (values.ContainsKey(name))
            {
                throw new ArgumentException(
                    $"Runtime parameter '{name}' is duplicated.",
                    nameof(parameters));
            }
            if (values.Count >= MaximumCount)
            {
                throw new ArgumentException(
                    $"Runtime parameter count exceeds {MaximumCount}.",
                    nameof(parameters));
            }
            if (value.Kind == FerruleValueKind.String &&
                Utf8ByteCount(value.StringValue, nameof(parameters)) > MaximumStringUtf8Bytes)
            {
                throw new ArgumentException(
                    $"Runtime parameter '{name}' string value exceeds " +
                    $"{MaximumStringUtf8Bytes} UTF-8 bytes.",
                    nameof(parameters));
            }
            values.Add(name, value);
        }
        _values = new ReadOnlyDictionary<string, FerruleValue>(values);
    }

    public static FerruleRuntimeParameters Empty { get; } =
        new(Array.Empty<KeyValuePair<string, FerruleValue>>());

    public int Count => _values.Count;

    internal bool TryGetValue(string name, out FerruleValue value) =>
        _values.TryGetValue(name, out value);

    private static int Utf8ByteCount(string value, string parameterName)
    {
        try
        {
            return StrictUtf8.GetByteCount(value);
        }
        catch (EncoderFallbackException error)
        {
            throw new ArgumentException(
                "Runtime parameter names and strings must contain valid Unicode.",
                parameterName,
                error);
        }
    }
}

/// <summary>Immutable host values shared by every scope context in one run.</summary>
public sealed class FerruleExecutionContext
{
    public FerruleExecutionContext(string mappingFilePath)
        : this(mappingFilePath, mappingFilePath, null, null)
    {
    }

    public FerruleExecutionContext(
        string mappingFilePath,
        string mainMappingFilePath,
        string? currentDateTime = null,
        FerruleRuntimeParameters? runtimeParameters = null)
    {
        MappingFilePath = mappingFilePath ??
            throw new ArgumentNullException(nameof(mappingFilePath));
        MainMappingFilePath = mainMappingFilePath ??
            throw new ArgumentNullException(nameof(mainMappingFilePath));
        CurrentDateTime = currentDateTime;
        RuntimeParameters = runtimeParameters ?? FerruleRuntimeParameters.Empty;
    }

    public string MappingFilePath { get; }

    public string MainMappingFilePath { get; }

    public string? CurrentDateTime { get; }

    public FerruleRuntimeParameters RuntimeParameters { get; }

    public static FerruleExecutionContext WithParameters(
        string mappingFilePath,
        FerruleRuntimeParameters runtimeParameters)
    {
        ArgumentNullException.ThrowIfNull(runtimeParameters);
        return new FerruleExecutionContext(
            mappingFilePath,
            mappingFilePath,
            null,
            runtimeParameters);
    }

    internal string? GetValue(FerruleRuntimeValue value) => value switch
    {
        FerruleRuntimeValue.MappingFilePath => MappingFilePath,
        FerruleRuntimeValue.MainMappingFilePath => MainMappingFilePath,
        FerruleRuntimeValue.CurrentDateTime => CurrentDateTime,
        _ => throw new ArgumentOutOfRangeException(nameof(value), value, null),
    };

    internal bool TryGetParameter(string name, out FerruleValue value) =>
        RuntimeParameters.TryGetValue(name, out value);
}
