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
    MissingDynamicSourceLoader,
    DynamicSourcePath,
    DynamicSourcePathTooLong,
    DynamicSourceTooMany,
    DynamicSourceLoad,
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
    DynamicTargetPath,
    EmptyDynamicTargetPath,
    DynamicPropertyName,
    DuplicateDynamicProperty,
    InvalidDynamicPropertyFragment,
    AggregateIntegerOverflow,
    AggregateNonFinite,
    CopyCurrentSourceRequiresGroup,
    RecursiveFilterDepth,
    RecursiveFilterRequiresGroup,
    RecursiveFilterRequiresCollection,
    PathHierarchyValueType,
    PathHierarchyDepth,
    PathHierarchyTooLarge,
    PathHierarchyRootCount,
    MissingAdjacencyCollection,
    AdjacencyTreeTooLarge,
    DuplicateAdjacencyKey,
    InvalidAdjacencyRoot,
    AdjacencyRootCount,
    AdjacencyTreeDepth,
    AdjacencyCycle,
    InvalidAdjacencyField,
    MissingRuntimeValue,
    MissingRuntimeParameter,
    RuntimeParameterType,
    GeneratedSequenceTooLarge,
    RecursiveSequenceDepth,
    RecursiveSequenceTooLarge,
    TokenizeRegexPatternTooLarge,
    InvalidTokenizeRegexFlags,
    InvalidTokenizeRegex,
    ZeroWidthTokenizeRegex,
    TokenizeRegexTooLarge,
    UserFunctionType,
    XmlSerialization,
    JsonBoundary,
    PrimaryRoot,
    MappingException,
    UserFunctionDepth,
    UnknownTarget,
    OwnedSource,
    BasicSourceBridge,
    PrimaryFilenameType,
    AggregateUnsupportedScalar,
    DecimalConversion,
    AggregateDecimal,
    MissingInvocationCounters,
    InvocationCounterOperand,
    InvocationCounter,
    DocumentFilenameClock,
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
        FerruleScalarType? expectedScalarType = null,
        string? runtimeParameter = null,
        string? sourceField = null,
        string? foundInstance = null,
        FerrulePrimaryRootFailure? primaryRoot = null,
        string? mappingExceptionMessage = null)
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
        RuntimeParameter = runtimeParameter;
        SourceField = sourceField;
        FoundInstance = foundInstance;
        PrimaryRoot = primaryRoot;
        MappingExceptionMessage = mappingExceptionMessage;
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
        FerruleScalarType? expectedScalarType = null,
        string? runtimeParameter = null,
        string? sourceField = null,
        string? foundInstance = null,
        FerrulePrimaryRootFailure? primaryRoot = null,
        string? mappingExceptionMessage = null)
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
        RuntimeParameter = runtimeParameter;
        SourceField = sourceField;
        FoundInstance = foundInstance;
        PrimaryRoot = primaryRoot;
        MappingExceptionMessage = mappingExceptionMessage;
    }

    private FerruleRuntimeException(FerruleOwnedSourceException owned)
        : base("owned source access failed", owned)
    {
        Error = FerruleRuntimeError.OwnedSource;
        OwnedSourceFailure = owned;
    }

    private FerruleRuntimeException(FerruleBasicSourceBridgeException bridge)
        : base("basic source bridge failed", bridge)
    {
        Error = FerruleRuntimeError.BasicSourceBridge;
        BasicSourceBridgeFailure = bridge;
    }

    public static FerruleRuntimeException FromOwnedSource(FerruleOwnedSourceException owned)
    {
        ArgumentNullException.ThrowIfNull(owned);
        return new FerruleRuntimeException(owned);
    }

    public static FerruleRuntimeException FromBasicSourceBridge(FerruleBasicSourceBridgeException bridge)
    {
        ArgumentNullException.ThrowIfNull(bridge);
        return new FerruleRuntimeException(bridge);
    }

    /// <summary>Adds the first public Basic owned-leaf node without replacing the error or its causes.</summary>
    public FerruleRuntimeException WithOwnedSourceNode(uint node)
    {
        if (!Node.HasValue &&
            ((Error == FerruleRuntimeError.OwnedSource && OwnedSourceFailure is not null &&
                ReferenceEquals(OwnedSourceFailure, InnerException)) ||
             (Error == FerruleRuntimeError.BasicSourceBridge && BasicSourceBridgeFailure is not null &&
                ReferenceEquals(BasicSourceBridgeFailure, InnerException))))
        {
            Node = node;
        }
        return this;
    }

    public FerruleOwnedSourceException? OwnedSourceFailure { get; }

    public FerruleBasicSourceBridgeException? BasicSourceBridgeFailure { get; }

    public FerruleRuntimeError Error { get; }

    public uint? Node { get; private set; }

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

    public string? MappingExceptionMessage { get; }

    public ulong? Join { get; }

    public ulong? UserFunction { get; }

    public ulong? FunctionParameter { get; }

    public FerruleScalarType? ExpectedScalarType { get; }

    public string? RuntimeParameter { get; }

    public string? SourceField { get; }

    public string? FoundInstance { get; }

    public FerrulePrimaryRootFailure? PrimaryRoot { get; }
}
