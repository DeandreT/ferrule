namespace Ferrule.Runtime;

/// <summary>One original input or final primary-document member; never both.</summary>
public abstract record FerruleXmlInputDocumentOwner
{
    private protected FerruleXmlInputDocumentOwner() { }
    public sealed record Input(FerruleXmlInputSource Source) : FerruleXmlInputDocumentOwner;
    public sealed record Member(FerruleXmlDocumentOutputOwner Output) : FerruleXmlInputDocumentOwner;
}

/// <summary>Retains the original boundary object and complete typed inner-cause chain.</summary>
public sealed class FerruleXmlInputDocumentExecutionException : Exception
{
    private FerruleXmlInputDocumentExecutionException(
        FerruleXmlInputDocumentOwner? owner, FerruleXmlBoundaryException boundary)
        : base(MessageFor(owner, boundary), boundary)
    { Owner = owner; Boundary = boundary; }

    public FerruleXmlInputDocumentOwner? Owner { get; }
    public FerruleXmlBoundaryException Boundary { get; }

    private static string MessageFor(FerruleXmlInputDocumentOwner? owner, FerruleXmlBoundaryException boundary)
    {
        ArgumentNullException.ThrowIfNull(boundary);
        return owner is null ? boundary.Message : $"XML phase {owner}: {boundary.Message}";
    }

    // The static collector callsite forwards original Input and Boundary only.
    // No general conversion consumes arbitrary old Output/Request metadata.
    public static FerruleXmlInputDocumentExecutionException FromInputBoundary(
        FerruleXmlInputSource? input, FerruleXmlBoundaryException boundary) =>
        new(input is null ? null : new FerruleXmlInputDocumentOwner.Input(input), boundary);

    public static FerruleXmlInputDocumentExecutionException FromBoundary(FerruleXmlBoundaryException boundary) => new(null, boundary);

    public static FerruleXmlInputDocumentExecutionException FromDocuments(FerruleXmlDocumentExecutionException error)
    {
        ArgumentNullException.ThrowIfNull(error);
        return new(error.Member is null ? null : new FerruleXmlInputDocumentOwner.Member(error.Member), error.Boundary);
    }
}
