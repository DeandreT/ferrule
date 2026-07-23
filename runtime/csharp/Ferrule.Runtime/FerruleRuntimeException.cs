namespace Ferrule.Runtime;

/// <summary>Stable categories for errors produced by the Ferrule runtime model.</summary>
public enum FerruleRuntimeError
{
    ValueKindMismatch,
    DuplicateField,
    InvalidDocumentPath,
    NestedDocumentSet,
    MissingSourceField,
    MissingNamedSource,
    DuplicateNamedSource,
    UnexpectedNamedSource,
    MissingJoinContext,
    MappingFailure,
    UnknownFunction,
    FunctionArity,
    FunctionType,
    FunctionInvalidArgument,
    DivideByZero,
    IntegerOverflow,
    NotABool,
    NotAnItemCount,
    InvalidBlockSize,
    AggregateIntegerOverflow,
    AggregateNonFinite,
    CopyCurrentSourceRequiresGroup,
    MissingRuntimeValue,
    GeneratedSequenceTooLarge,
    RecursiveSequenceDepth,
    RecursiveSequenceTooLarge,
    TokenizeRegexPatternTooLarge,
    InvalidTokenizeRegexFlags,
    InvalidTokenizeRegex,
    ZeroWidthTokenizeRegex,
    TokenizeRegexTooLarge,
    UserFunctionType,
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
        FerruleValueKind? foundKind = null,
        FerruleAggregateOperation? aggregateOperation = null,
        string? detail = null,
        UInt128? requestedItems = null,
        UInt128? maximumItems = null,
        int? maximumDepth = null,
        FerruleRuntimeValue? runtimeValue = null,
        int? failureRule = null,
        string? mappingFailureMessage = null,
        ulong? join = null,
        ulong? userFunction = null,
        ulong? functionParameter = null,
        FerruleScalarType? expectedScalarType = null)
        : base(message)
    {
        Error = error;
        Node = node;
        Function = function;
        ExpectedArity = expectedArity;
        ActualArity = actualArity;
        FoundKind = foundKind;
        AggregateOperation = aggregateOperation;
        Detail = detail;
        RequestedItems = requestedItems;
        MaximumItems = maximumItems;
        MaximumDepth = maximumDepth;
        RuntimeValue = runtimeValue;
        FailureRule = failureRule;
        MappingFailureMessage = mappingFailureMessage;
        Join = join;
        UserFunction = userFunction;
        FunctionParameter = functionParameter;
        ExpectedScalarType = expectedScalarType;
    }

    public FerruleRuntimeException(
        FerruleRuntimeError error,
        string message,
        Exception innerException,
        uint? node = null,
        string? function = null,
        int? expectedArity = null,
        int? actualArity = null,
        FerruleValueKind? foundKind = null,
        FerruleAggregateOperation? aggregateOperation = null,
        string? detail = null,
        UInt128? requestedItems = null,
        UInt128? maximumItems = null,
        int? maximumDepth = null,
        FerruleRuntimeValue? runtimeValue = null,
        int? failureRule = null,
        string? mappingFailureMessage = null,
        ulong? join = null,
        ulong? userFunction = null,
        ulong? functionParameter = null,
        FerruleScalarType? expectedScalarType = null)
        : base(message, innerException)
    {
        Error = error;
        Node = node;
        Function = function;
        ExpectedArity = expectedArity;
        ActualArity = actualArity;
        FoundKind = foundKind;
        AggregateOperation = aggregateOperation;
        Detail = detail;
        RequestedItems = requestedItems;
        MaximumItems = maximumItems;
        MaximumDepth = maximumDepth;
        RuntimeValue = runtimeValue;
        FailureRule = failureRule;
        MappingFailureMessage = mappingFailureMessage;
        Join = join;
        UserFunction = userFunction;
        FunctionParameter = functionParameter;
        ExpectedScalarType = expectedScalarType;
    }

    public FerruleRuntimeError Error { get; }

    public uint? Node { get; }

    public string? Function { get; }

    public int? ExpectedArity { get; }

    public int? ActualArity { get; }

    public FerruleValueKind? FoundKind { get; }

    public FerruleAggregateOperation? AggregateOperation { get; }

    public string? Detail { get; }

    public UInt128? RequestedItems { get; }

    public UInt128? MaximumItems { get; }

    public int? MaximumDepth { get; }

    public FerruleRuntimeValue? RuntimeValue { get; }

    public int? FailureRule { get; }

    public string? MappingFailureMessage { get; }

    public ulong? Join { get; }

    public ulong? UserFunction { get; }

    public ulong? FunctionParameter { get; }

    public FerruleScalarType? ExpectedScalarType { get; }
}
