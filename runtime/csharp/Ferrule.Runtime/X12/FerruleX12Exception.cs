namespace Ferrule.Runtime;

/// <summary>Stable failures at the optional, strict 004010 X12 boundary.</summary>
public enum FerruleX12Error
{
    UnsupportedProfile,
    Schema,
    Encoding,
    Syntax,
    Envelope,
    Value,
    ResourceLimit,
}

/// <summary>An X12 boundary failure without including document field values.</summary>
public sealed class FerruleX12Exception : Exception
{
    internal FerruleX12Exception(FerruleX12Error error, string message,
        int? segmentIndex = null, string? fieldPath = null) : base(message)
    {
        Error = error;
        SegmentIndex = segmentIndex;
        FieldPath = fieldPath;
    }

    public FerruleX12Error Error { get; }
    public int? SegmentIndex { get; }
    public string? FieldPath { get; }
}
