namespace Ferrule.Runtime;

public enum FerruleXmlOutputTargetKind { Primary, Named }
public sealed record FerruleXmlOutputTarget(FerruleXmlOutputTargetKind Kind, int? Index, string? Name)
{
    public static FerruleXmlOutputTarget Primary { get; } = new(FerruleXmlOutputTargetKind.Primary, null, null);
    public static FerruleXmlOutputTarget Named(int index, string name) => new(FerruleXmlOutputTargetKind.Named, index, name);
}

/// <summary>The original typed XML boundary remains the inner exception.</summary>
public sealed class FerruleXmlOutputSetException : Exception
{
    public FerruleXmlOutputSetException(FerruleXmlOutputTarget? target, FerruleXmlBoundaryException boundary)
        : base(target is null ? boundary.Message : $"XML output {target}: {boundary.Message}", boundary)
    { Target = target; Boundary = boundary; }
    public FerruleXmlOutputTarget? Target { get; }
    public FerruleXmlBoundaryException Boundary { get; }
}

/// <summary>Set counters, distinct from original-document Bytes/Limit.</summary>
public sealed class FerruleXmlOutputSetResourceException : Exception
{
    public FerruleXmlOutputSetResourceException(string resource, ulong observedCount, ulong limit)
        : base($"{resource} is {observedCount}; maximum is {limit}")
    { Resource = resource; ObservedCount = observedCount; Limit = limit; }
    public string Resource { get; }
    public ulong ObservedCount { get; }
    public ulong Limit { get; }
}

/// <summary>Admission counters; all typed targets and one temporary XML can be live.</summary>
public sealed class FerruleXmlOutputSetBudget
{
    public const ulong MaximumArtifacts = 4096;
    public const ulong MaximumUtf8Bytes = 256 * 1024 * 1024;
    private ulong _bytes;
    public FerruleXmlOutputSetBudget(int artifactCount)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(artifactCount);
        if ((ulong)artifactCount > MaximumArtifacts)
            throw Refusal(null, "xml_output_artifact_count", (ulong)artifactCount, MaximumArtifacts);
    }
    public void Charge(FerruleXmlOutputTarget target, long utf8Bytes)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(utf8Bytes);
        var count = (ulong)utf8Bytes;
        var observed = ulong.MaxValue - _bytes < count ? ulong.MaxValue : _bytes + count;
        if (observed > MaximumUtf8Bytes)
            throw Refusal(target, "xml_output_set_utf8_bytes", observed, MaximumUtf8Bytes);
        _bytes = observed;
    }
    public static FerruleXmlOutputSetException Alignment(string detail) => new(null,
        new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Output, detail,
            new InvalidOperationException(detail)));
    private static FerruleXmlOutputSetException Refusal(FerruleXmlOutputTarget? target, string resource, ulong count, ulong limit)
    {
        var cause = new FerruleXmlOutputSetResourceException(resource, count, limit);
        return new(target, new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Output, cause.Message, cause));
    }
}
