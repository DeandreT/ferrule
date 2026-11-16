using System.Text.Json.Serialization;

namespace Ferrule.Host;

public readonly record struct InspectionLimits(uint MaximumItems, uint MaximumTextBytes)
{
    public static InspectionLimits Default => new(8_192, 2_048);
}

public readonly record struct ValidationLimits(
    InspectionLimits Inspection, uint MaximumIssues, uint MaximumDiagnostics)
{
    public static ValidationLimits Default => new(InspectionLimits.Default, 32_768, 128);
}

public enum BoundaryKind { Source, NamedSource, Target, NamedTarget }
public enum BoundaryShape { Scalar, ScalarUnion, Object }

public enum ResourceKind
{
    DesignTimeInput, DesignTimeOutput, DynamicInput, DynamicOutput,
    EdiConfiguration, ExternalSource, XmlSchemaHint, SchemaReference,
}

public sealed record BoundaryId(BoundaryKind Kind, ulong? Index = null);
public sealed record DisplayText(string Text, bool Truncated);

[JsonPolymorphic(TypeDiscriminatorPropertyName = "kind")]
[JsonDerivedType(typeof(NativeLocation.Document), "document")]
[JsonDerivedType(typeof(NativeLocation.JsonPointer), "jsonPointer")]
[JsonDerivedType(typeof(NativeLocation.GraphNode), "graphNode")]
public abstract record NativeLocation
{
    private NativeLocation() { }
    public sealed record Document : NativeLocation;
    public sealed record JsonPointer(string Pointer) : NativeLocation;
    public sealed record GraphNode(uint NodeId, ulong? FunctionId) : NativeLocation;
}

public sealed record Boundary(
    BoundaryId Id, DisplayText Name, DisplayText RootName, BoundaryShape Shape,
    bool Repeating, NativeLocation NativeLocation);

public sealed record ResourceReference(
    BoundaryId BoundaryId, ResourceKind Kind, NativeLocation NativeLocation);

public sealed record StructuralCounts(
    uint SchemaNodes, uint GraphNodes, uint ScopeNodes, uint UserFunctions,
    uint FailureRules, uint TotalItems);

public sealed record InspectionReport(
    IReadOnlyList<Boundary> Boundaries, StructuralCounts Counts,
    IReadOnlyList<ResourceReference> UnresolvedResources, bool Truncated);

public sealed record ValidationEndpoint(
    BoundaryKind Kind, ulong? Index = null, string? Name = null);

public enum SchemaStepKind { Child, DynamicField, Contains, DependentSchema }
public enum ScopeStepKind { Child, DynamicChild, Segment }
public sealed record SchemaStep(SchemaStepKind Kind, ulong? Index = null);
public sealed record ScopeStep(ScopeStepKind Kind, ulong Index);

[JsonPolymorphic(TypeDiscriminatorPropertyName = "kind")]
[JsonDerivedType(typeof(ValidationOwner.GraphNode), "graphNode")]
[JsonDerivedType(typeof(ValidationOwner.UserFunction), "userFunction")]
[JsonDerivedType(typeof(ValidationOwner.EndpointOwner), "endpoint")]
[JsonDerivedType(typeof(ValidationOwner.SchemaNode), "schemaNode")]
[JsonDerivedType(typeof(ValidationOwner.Scope), "scope")]
[JsonDerivedType(typeof(ValidationOwner.FailureRule), "failureRule")]
public abstract record ValidationOwner
{
    private ValidationOwner() { }
    public sealed record GraphNode(uint NodeId, ulong? FunctionId) : ValidationOwner;
    public sealed record UserFunction(ulong FunctionId) : ValidationOwner;
    public sealed record EndpointOwner(ValidationEndpoint Endpoint) : ValidationOwner;
    public sealed record SchemaNode(ValidationEndpoint Endpoint, IReadOnlyList<SchemaStep> Path) : ValidationOwner;
    public sealed record Scope(ValidationEndpoint Target, IReadOnlyList<ScopeStep> Path) : ValidationOwner;
    public sealed record FailureRule(ulong Index) : ValidationOwner;
}

public sealed record Diagnostic(
    DisplayText Message, DisplayText Location, ValidationOwner? Owner, NativeLocation NativeLocation);

public sealed record ValidationReport(
    bool IsValid, uint TotalIssueCount, IReadOnlyList<Diagnostic> Diagnostics, bool Truncated)
{
    public int EmittedIssueCount => Diagnostics.Count;
}

internal sealed record NativeError(string Code, string Message);

[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    NumberHandling = JsonNumberHandling.AllowReadingFromString,
    UseStringEnumConverter = true,
    RespectRequiredConstructorParameters = true)]
[JsonSerializable(typeof(InspectionReport))]
[JsonSerializable(typeof(ValidationReport))]
[JsonSerializable(typeof(NativeError))]
[JsonSerializable(typeof(NativeLocation.GraphNode), TypeInfoPropertyName = "GraphNodeLocation")]
[JsonSerializable(typeof(ValidationOwner.GraphNode), TypeInfoPropertyName = "GraphNodeOwner")]
internal sealed partial class NativeJsonContext : JsonSerializerContext;
