namespace Ferrule.Runtime;

public enum FerruleJson5SyntaxKind
{
    UnexpectedToken,
    TrailingRootValue,
    UnterminatedComment,
    UnterminatedString,
    LineTerminatorInString,
    InvalidEscape,
    UnsupportedIdentifier,
    InvalidNumber,
    IntegerOutOfRange,
    NonFiniteNumber,
}

public enum FerruleJson5SyntaxResource
{
    OriginalDocumentBytes,
    NormalizedDocumentBytes,
    ContainerDepth,
    SyntaxWork,
}

public enum FerruleJson5SyntaxFailure
{
    Syntax,
    Limit,
    Encoding,
}

/// <summary>An additive pure syntax failure, independent of schema/mapping errors.</summary>
public sealed class FerruleJson5SyntaxException : Exception
{
    public FerruleJson5SyntaxFailure Failure { get; }
    public FerruleJson5SyntaxKind? Kind { get; }
    public FerruleJson5SyntaxResource? Resource { get; }
    public long Offset { get; }
    public long? Requested { get; }
    public long? Maximum { get; }
    public int? Utf16Index { get; }

    private FerruleJson5SyntaxException(
        FerruleJson5SyntaxFailure failure, string message, long offset,
        FerruleJson5SyntaxKind? kind = null, FerruleJson5SyntaxResource? resource = null,
        long? requested = null, long? maximum = null, int? utf16Index = null,
        Exception? inner = null) : base(message, inner)
    {
        Failure = failure;
        Kind = kind;
        Resource = resource;
        Offset = offset;
        Requested = requested;
        Maximum = maximum;
        Utf16Index = utf16Index;
    }

    internal static FerruleJson5SyntaxException Syntax(FerruleJson5SyntaxKind kind, long offset) =>
        new(FerruleJson5SyntaxFailure.Syntax,
            $"JSON5 syntax {kind} at original UTF-8 byte {offset}", offset, kind: kind);

    internal static FerruleJson5SyntaxException Limit(
        FerruleJson5SyntaxResource resource, long offset, long requested, long maximum) =>
        new(FerruleJson5SyntaxFailure.Limit,
            $"JSON5 {resource} limit at original UTF-8 byte {offset}: requested {requested}, max {maximum}",
            offset, resource: resource, requested: requested, maximum: maximum);

    internal static FerruleJson5SyntaxException Encoding(long offset, int utf16Index, Exception inner) =>
        new(FerruleJson5SyntaxFailure.Encoding,
            $"JSON5 text has an unpaired UTF-16 surrogate at index {utf16Index}; valid-prefix UTF-8 byte {offset}",
            offset, utf16Index: utf16Index, inner: inner);
}
