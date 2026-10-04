namespace Ferrule.Runtime;

/// <summary>The phase that refused a generated XML document boundary.</summary>
public enum FerruleXmlBoundaryErrorKind
{
    Schema, DocumentLimit, Utf8, Input, Mapping, Output,
}

/// <summary>Retains the original typed mapping failure as its inner exception.</summary>
public sealed class FerruleXmlBoundaryException : Exception
{
    public FerruleXmlBoundaryException(
        FerruleXmlBoundaryErrorKind kind,
        string detail,
        Exception? inner = null,
        long? bytes = null,
        long? limit = null)
        : base($"{kind}: XML boundary failed: {detail}", inner)
    {
        Kind = kind;
        Detail = detail;
        Bytes = bytes;
        Limit = limit;
    }

    public FerruleXmlBoundaryErrorKind Kind { get; }
    public string Detail { get; }
    public long? Bytes { get; }
    public long? Limit { get; }
}
