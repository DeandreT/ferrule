namespace Ferrule.Runtime;

/// <summary>Stable categories for errors produced by the Ferrule runtime model.</summary>
public enum FerruleRuntimeError
{
    ValueKindMismatch,
    DuplicateField,
    InvalidDocumentPath,
    NestedDocumentSet,
    MissingSourceField,
    UnknownFunction,
    FunctionArity,
    FunctionType,
    DivideByZero,
    IntegerOverflow,
    NotABool,
}

/// <summary>An error with a machine-readable Ferrule runtime category.</summary>
public sealed class FerruleRuntimeException : Exception
{
    public FerruleRuntimeException(
        FerruleRuntimeError error,
        string message,
        uint? node = null,
        string? function = null,
        int? expectedArity = null,
        int? actualArity = null,
        FerruleValueKind? foundKind = null)
        : base(message)
    {
        Error = error;
        Node = node;
        Function = function;
        ExpectedArity = expectedArity;
        ActualArity = actualArity;
        FoundKind = foundKind;
    }

    public FerruleRuntimeException(
        FerruleRuntimeError error,
        string message,
        Exception innerException,
        uint? node = null,
        string? function = null,
        int? expectedArity = null,
        int? actualArity = null,
        FerruleValueKind? foundKind = null)
        : base(message, innerException)
    {
        Error = error;
        Node = node;
        Function = function;
        ExpectedArity = expectedArity;
        ActualArity = actualArity;
        FoundKind = foundKind;
    }

    public FerruleRuntimeError Error { get; }

    public uint? Node { get; }

    public string? Function { get; }

    public int? ExpectedArity { get; }

    public int? ActualArity { get; }

    public FerruleValueKind? FoundKind { get; }
}
