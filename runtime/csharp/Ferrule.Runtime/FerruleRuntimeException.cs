namespace Ferrule.Runtime;

/// <summary>Stable categories for errors produced by the Ferrule runtime model.</summary>
public enum FerruleRuntimeError
{
    NonFiniteDouble,
    ValueKindMismatch,
    DuplicateField,
    InvalidDocumentPath,
    NestedDocumentSet,
    MissingSourceField,
}

/// <summary>An error with a machine-readable Ferrule runtime category.</summary>
public sealed class FerruleRuntimeException : Exception
{
    public FerruleRuntimeException(FerruleRuntimeError error, string message)
        : base(message)
    {
        Error = error;
    }

    public FerruleRuntimeException(FerruleRuntimeError error, string message, Exception innerException)
        : base(message, innerException)
    {
        Error = error;
    }

    public FerruleRuntimeError Error { get; }
}
