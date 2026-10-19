namespace Ferrule.Runtime;

/// <summary>One original input or final primary member, with no fabricated phase owner.</summary>
public abstract record FerruleXmlDynamicInputDocumentOwner
{
    private protected FerruleXmlDynamicInputDocumentOwner() { }
    public sealed record Input(FerruleXmlInputSource Source) : FerruleXmlDynamicInputDocumentOwner;
    public sealed record Member(FerruleXmlDocumentOutputOwner Output) : FerruleXmlDynamicInputDocumentOwner;
}

/// <summary>Retains original boundary, typed causes and adapter-origin request references.</summary>
public sealed class FerruleXmlDynamicInputDocumentExecutionException : Exception
{
    private FerruleXmlDynamicInputDocumentExecutionException(
        FerruleXmlDynamicInputDocumentOwner? owner, FerruleXmlBoundaryException boundary,
        FerruleXmlDynamicInputRequest? request = null)
        : base(MessageFor(owner, boundary), boundary)
    { Owner = owner; Boundary = boundary; Request = request; }
    public FerruleXmlDynamicInputDocumentOwner? Owner { get; }
    public FerruleXmlBoundaryException Boundary { get; }
    public FerruleXmlDynamicInputRequest? Request { get; }
    private static string MessageFor(FerruleXmlDynamicInputDocumentOwner? owner, FerruleXmlBoundaryException boundary)
    {
        ArgumentNullException.ThrowIfNull(boundary);
        return owner is null ? boundary.Message : $"XML phase {owner}: {boundary.Message}";
    }
    public static FerruleXmlDynamicInputDocumentExecutionException FromInputBoundary(
        FerruleXmlInputSource? input, FerruleXmlBoundaryException boundary) =>
        new(input is null ? null : new FerruleXmlDynamicInputDocumentOwner.Input(input), boundary);
    /// <summary>Use the exact request and boundary from a fresh adapter's first
    /// product refusal. This conversion does not authenticate arbitrary requests
    /// or permit adapter reuse after a failed load.</summary>
    public static FerruleXmlDynamicInputDocumentExecutionException FromDynamicInputBoundary(
        FerruleXmlDynamicInputRequest request, FerruleXmlBoundaryException boundary)
    {
        ArgumentNullException.ThrowIfNull(request);
        return new(new FerruleXmlDynamicInputDocumentOwner.Input(
            FerruleXmlInputSource.Named(request.DeclarationIndex, request.Source)), boundary, request);
    }
    public static FerruleXmlDynamicInputDocumentExecutionException FromBoundary(FerruleXmlBoundaryException boundary) => new(null, boundary);
    public static FerruleXmlDynamicInputDocumentExecutionException FromDocuments(FerruleXmlDocumentExecutionException error)
    {
        ArgumentNullException.ThrowIfNull(error);
        return new(error.Member is null ? null : new FerruleXmlDynamicInputDocumentOwner.Member(error.Member), error.Boundary);
    }
}

