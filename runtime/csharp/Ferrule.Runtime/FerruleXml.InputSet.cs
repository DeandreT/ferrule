using System.Text;

namespace Ferrule.Runtime;

public enum FerruleXmlInputSourceKind { Primary, Named }
public sealed record FerruleXmlInputSource(FerruleXmlInputSourceKind Kind, int? Index, string? Name)
{
    public static FerruleXmlInputSource Primary { get; } = new(FerruleXmlInputSourceKind.Primary, null, null);
    public static FerruleXmlInputSource Named(int index, string name) => new(FerruleXmlInputSourceKind.Named, index, name);
}

/// <summary>One exact phase owner and the unchanged original typed boundary.</summary>
public sealed class FerruleXmlExecutionException : Exception
{
    private FerruleXmlExecutionException(FerruleXmlInputSource? input, FerruleXmlOutputTarget? output,
        FerruleXmlBoundaryException boundary)
        : base(input is not null ? $"XML input {input}: {boundary.Message}" :
            output is not null ? $"XML output {output}: {boundary.Message}" : boundary.Message, boundary)
    { Input = input; Output = output; Boundary = boundary; }
    public FerruleXmlInputSource? Input { get; }
    public FerruleXmlOutputTarget? Output { get; }
    public FerruleXmlBoundaryException Boundary { get; }
    public static FerruleXmlExecutionException ForInput(FerruleXmlInputSource input, FerruleXmlBoundaryException boundary) =>
        new(input, null, boundary);
    public static FerruleXmlExecutionException Unowned(FerruleXmlBoundaryException boundary) => new(null, null, boundary);
    public static FerruleXmlExecutionException FromOutput(FerruleXmlOutputSetException error) => new(null, error.Target, error.Boundary);
    public FerruleXmlOutputSetException ToOutputSet() => new(Output, Boundary);
}

/// <summary>Original input counters, separate from per-document Bytes/Limit.</summary>
public sealed class FerruleXmlInputSetResourceException : Exception
{
    public FerruleXmlInputSetResourceException(string resource, ulong observedCount, ulong limit)
        : base($"{resource} is {observedCount}; maximum is {limit}")
    { Resource = resource; ObservedCount = observedCount; Limit = limit; }
    public string Resource { get; }
    public ulong ObservedCount { get; }
    public ulong Limit { get; }
}

/// <summary>Original UTF-8 admission; retained instance and work ledgers remain per document.</summary>
public sealed class FerruleXmlInputSetBudget
{
    public const ulong MaximumArtifacts = 4096;
    public const ulong MaximumUtf8Bytes = 256UL * 1024 * 1024;
    public const long MaximumDocumentBytes = 64L * 1024 * 1024;
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);
    private ulong _bytes;

    public FerruleXmlInputSetBudget(long artifactCount)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(artifactCount);
        if ((ulong)artifactCount > MaximumArtifacts)
            throw Refusal(null, "xml_input_artifact_count", (ulong)artifactCount, MaximumArtifacts);
    }

    public void Charge(FerruleXmlInputSource source, long utf8Bytes)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(utf8Bytes);
        var count = (ulong)utf8Bytes;
        var observed = ulong.MaxValue - _bytes < count ? ulong.MaxValue : _bytes + count;
        if (observed > MaximumUtf8Bytes)
            throw Refusal(source, "xml_input_set_utf8_bytes", observed, MaximumUtf8Bytes);
        _bytes = observed;
    }

    public static void RequireDocumentSize(FerruleXmlInputSource source, long bytes)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(bytes);
        if (bytes > MaximumDocumentBytes)
            throw FerruleXmlExecutionException.ForInput(source, new FerruleXmlBoundaryException(
                FerruleXmlBoundaryErrorKind.DocumentLimit,
                $"XML document is {bytes} bytes; maximum is {MaximumDocumentBytes}",
                bytes: bytes, limit: MaximumDocumentBytes));
    }

    /// <summary>No byte array is allocated; invalid CLR Unicode can refuse during this size pass.</summary>
    public static long Measure(FerruleXmlInputSource source, string xml)
    {
        ArgumentNullException.ThrowIfNull(xml);
        try { return StrictUtf8.GetByteCount(xml); }
        catch (EncoderFallbackException error)
        {
            throw FerruleXmlExecutionException.ForInput(source, new FerruleXmlBoundaryException(
                FerruleXmlBoundaryErrorKind.Utf8, "XML input is not well-formed Unicode", error));
        }
        catch (Exception error) when (error is ArgumentException or OverflowException)
        {
            // Exceptional strings exceeding Int32 bytes retain a long size result.
            var remaining = xml.AsSpan();
            long bytes = 0;
            while (!remaining.IsEmpty)
            {
                if (Rune.DecodeFromUtf16(remaining, out var rune, out var consumed) !=
                    System.Buffers.OperationStatus.Done)
                    throw FerruleXmlExecutionException.ForInput(source, new FerruleXmlBoundaryException(
                        FerruleXmlBoundaryErrorKind.Utf8, "XML input is not well-formed Unicode"));
                bytes += rune.Utf8SequenceLength;
                remaining = remaining[consumed..];
            }
            return bytes;
        }
    }

    /// <summary>Exact supplied-order unknown/duplicate checks, then declaration-order missing checks.</summary>
    public static int[] Indices(IReadOnlyList<string> expected, IReadOnlyList<string> supplied)
    {
        _ = new FerruleXmlInputSetBudget((long)supplied.Count + 1);
        _ = new FerruleXmlInputSetBudget((long)expected.Count + 1);
        var matched = new int[expected.Count];
        Array.Fill(matched, -1);
        for (var suppliedIndex = 0; suppliedIndex < supplied.Count; suppliedIndex++)
        {
            var name = supplied[suppliedIndex];
            var index = -1;
            for (var i = 0; i < expected.Count; i++)
                if (string.Equals(expected[i], name, StringComparison.Ordinal)) { index = i; break; }
            if (index < 0) throw NameError(FerruleRuntimeError.UnexpectedNamedSource,
                $"named source '{name}' is not declared by this mapping", name);
            if (matched[index] >= 0) throw NameError(FerruleRuntimeError.DuplicateNamedSource,
                $"named source '{expected[index]}' was supplied more than once", expected[index]);
            matched[index] = suppliedIndex;
        }
        for (var i = 0; i < expected.Count; i++)
            if (matched[i] < 0) throw NameError(FerruleRuntimeError.MissingNamedSource,
                $"named source {expected[i]} is required by this mapping", expected[i]);
        return matched;
    }

    private static FerruleXmlExecutionException NameError(FerruleRuntimeError kind, string message, string name)
    {
        var cause = new FerruleRuntimeException(kind, message, detail: name);
        return FerruleXmlExecutionException.Unowned(new FerruleXmlBoundaryException(
            FerruleXmlBoundaryErrorKind.Mapping, cause.Message, cause));
    }

    private static FerruleXmlExecutionException Refusal(FerruleXmlInputSource? source, string resource, ulong count, ulong limit)
    {
        var cause = new FerruleXmlInputSetResourceException(resource, count, limit);
        var boundary = new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Input, cause.Message, cause);
        return source is null ? FerruleXmlExecutionException.Unowned(boundary) : FerruleXmlExecutionException.ForInput(source, boundary);
    }
}
