// Exclusive static-input or mixed-output phase, preserving existing owner shapes.
namespace Ferrule.Runtime;

/// <summary>One original input or original mixed output owner; never both.</summary>
public abstract record FerruleXmlInputDocumentOutputsOwner
{
    private protected FerruleXmlInputDocumentOutputsOwner() { }
    public sealed record Input(FerruleXmlInputSource Source) : FerruleXmlInputDocumentOutputsOwner;
    public sealed record Output(FerruleXmlDocumentOutputsOwner Owner) : FerruleXmlInputDocumentOutputsOwner;
}

/// <summary>Retains the original boundary object and complete typed inner-cause chain.
/// Input Schema retains its source; output Schema and global refusals remain unowned.</summary>
public sealed class FerruleXmlInputDocumentOutputsExecutionException : Exception
{
    private FerruleXmlInputDocumentOutputsExecutionException(
        FerruleXmlInputDocumentOutputsOwner? owner, FerruleXmlBoundaryException boundary)
        : base(MessageFor(owner, boundary), boundary)
    { Owner = owner; Boundary = boundary; }
    public FerruleXmlInputDocumentOutputsOwner? Owner { get; }
    public FerruleXmlBoundaryException Boundary { get; }
    private static string MessageFor(FerruleXmlInputDocumentOutputsOwner? owner, FerruleXmlBoundaryException boundary)
    {
        ArgumentNullException.ThrowIfNull(boundary);
        return owner is null ? boundary.Message : $"XML phase {owner}: {boundary.Message}";
    }
    // Only trusted static admission calls may feed this channel; Count/indexer/
    // enumerator/null Document/name CLR guards remain outside its catch scope.
    public static FerruleXmlInputDocumentOutputsExecutionException FromInputBoundary(
        FerruleXmlInputSource? input, FerruleXmlBoundaryException boundary) =>
        new(input is null ? null : new FerruleXmlInputDocumentOutputsOwner.Input(input), boundary);
    public static FerruleXmlInputDocumentOutputsExecutionException FromBoundary(FerruleXmlBoundaryException boundary) => new(null, boundary);
    public static FerruleXmlInputDocumentOutputsExecutionException FromOutputs(FerruleXmlDocumentOutputsExecutionException error)
    {
        ArgumentNullException.ThrowIfNull(error);
        return new(error.Owner is null ? null : new FerruleXmlInputDocumentOutputsOwner.Output(error.Owner), error.Boundary);
    }
}
