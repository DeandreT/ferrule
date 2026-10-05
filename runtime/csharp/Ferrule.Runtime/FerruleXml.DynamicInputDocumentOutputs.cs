namespace Ferrule.Runtime;

/// <summary>One original input declaration or original mixed output owner; never both.</summary>
public abstract record FerruleXmlDynamicInputDocumentOutputsOwner
{
    private protected FerruleXmlDynamicInputDocumentOutputsOwner() { }
    public sealed record Input(FerruleXmlInputSource Source) : FerruleXmlDynamicInputDocumentOutputsOwner;
    public sealed record Output(FerruleXmlDocumentOutputsOwner Owner) : FerruleXmlDynamicInputDocumentOutputsOwner;
}

/// <summary>Preserves the original boundary, typed inner causes and adapter-origin request.
/// Input Schema retains its source; output Schema and global refusals remain unowned.</summary>
public sealed class FerruleXmlDynamicInputDocumentOutputsExecutionException : Exception
{
    private FerruleXmlDynamicInputDocumentOutputsExecutionException(
        FerruleXmlDynamicInputDocumentOutputsOwner? owner, FerruleXmlBoundaryException boundary,
        FerruleXmlDynamicInputRequest? request = null)
        : base(MessageFor(owner, boundary), boundary)
    { Owner = owner; Boundary = boundary; Request = request; }
    public FerruleXmlDynamicInputDocumentOutputsOwner? Owner { get; }
    public FerruleXmlBoundaryException Boundary { get; }
    public FerruleXmlDynamicInputRequest? Request { get; }
    private static string MessageFor(FerruleXmlDynamicInputDocumentOutputsOwner? owner, FerruleXmlBoundaryException boundary)
    {
        ArgumentNullException.ThrowIfNull(boundary);
        return owner is null ? boundary.Message : $"XML phase {owner}: {boundary.Message}";
    }
    /// <summary>Only trusted static admission calls feed this channel. Host list
    /// Count/indexer/enumerator/name/Document guards remain outside conversion catches.</summary>
    public static FerruleXmlDynamicInputDocumentOutputsExecutionException FromInputBoundary(
        FerruleXmlInputSource? input, FerruleXmlBoundaryException boundary) =>
        new(input is null ? null : new FerruleXmlDynamicInputDocumentOutputsOwner.Input(input), boundary);
    /// <summary>Supply only the exact request and boundary synchronously recovered
    /// from a fresh adapter's first product refusal, with the original marker reference
    /// preserved by the typed loader. This factory does not authenticate arbitrary
    /// requests and does not permit retry or reuse after any failed load.</summary>
    public static FerruleXmlDynamicInputDocumentOutputsExecutionException FromDynamicInputBoundary(
        FerruleXmlDynamicInputRequest request, FerruleXmlBoundaryException boundary)
    {
        ArgumentNullException.ThrowIfNull(request);
        return new(new FerruleXmlDynamicInputDocumentOutputsOwner.Input(
            FerruleXmlInputSource.Named(request.DeclarationIndex, request.Source)), boundary, request);
    }
    public static FerruleXmlDynamicInputDocumentOutputsExecutionException FromBoundary(FerruleXmlBoundaryException boundary) => new(null, boundary);
    public static FerruleXmlDynamicInputDocumentOutputsExecutionException FromOutputs(FerruleXmlDocumentOutputsExecutionException error)
    {
        ArgumentNullException.ThrowIfNull(error);
        return new(error.Owner is null ? null : new FerruleXmlDynamicInputDocumentOutputsOwner.Output(error.Owner), error.Boundary);
    }
}
