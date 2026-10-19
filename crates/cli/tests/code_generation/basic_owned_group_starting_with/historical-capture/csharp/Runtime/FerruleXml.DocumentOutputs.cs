namespace Ferrule.Runtime;

/// <summary>A static primary has no member index/path. Named indices select the final ordered list.</summary>
public abstract record FerruleXmlDocumentOutputsOwner
{
    private FerruleXmlDocumentOutputsOwner() { }

    public sealed record Primary() : FerruleXmlDocumentOutputsOwner;
    public sealed record NamedMember(int DeclarationIndex, string Name, int Index, string Path)
        : FerruleXmlDocumentOutputsOwner;
}

/// <summary>The original boundary and its typed inner cause remain unchanged.
/// Input, mapping, Schema setup, alignment and total artifact count are unowned.</summary>
public sealed class FerruleXmlDocumentOutputsExecutionException : Exception
{
    public FerruleXmlDocumentOutputsExecutionException(
        FerruleXmlDocumentOutputsOwner? owner, FerruleXmlBoundaryException boundary)
        : base(MessageFor(owner, boundary), boundary)
    { Owner = owner; Boundary = boundary; }

    public FerruleXmlDocumentOutputsOwner? Owner { get; }
    public FerruleXmlBoundaryException Boundary { get; }

    private static string MessageFor(FerruleXmlDocumentOutputsOwner? owner, FerruleXmlBoundaryException boundary)
    {
        ArgumentNullException.ThrowIfNull(boundary);
        return owner switch
        {
            FerruleXmlDocumentOutputsOwner.Primary => $"primary XML output: {boundary.Message}",
            FerruleXmlDocumentOutputsOwner.NamedMember member =>
                $"named XML output {member.DeclarationIndex} `{member.Name}` member {member.Index} `{member.Path}`: {boundary.Message}",
            null => boundary.Message,
            _ => throw new ArgumentException("Unknown XML document output owner.", nameof(owner)),
        };
    }

    public static FerruleXmlDocumentOutputsExecutionException FromBoundary(FerruleXmlBoundaryException boundary) => new(null, boundary);

    /// <summary>Descriptor Schema setup is global; other primary serializer failures own Primary.</summary>
    public static FerruleXmlDocumentOutputsExecutionException PrimarySerialization(FerruleXmlBoundaryException boundary)
    {
        ArgumentNullException.ThrowIfNull(boundary);
        FerruleXmlDocumentOutputsOwner? owner = boundary.Kind == FerruleXmlBoundaryErrorKind.Schema ? null :
            new FerruleXmlDocumentOutputsOwner.Primary();
        return new(owner, boundary);
    }

    /// <summary>Names and opaque paths are preserved. Descriptor Schema setup remains global.</summary>
    public static FerruleXmlDocumentOutputsExecutionException NamedSerialization(
        int declarationIndex, string name, int index, string path, FerruleXmlBoundaryException boundary)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(declarationIndex);
        ArgumentOutOfRangeException.ThrowIfNegative(index);
        ArgumentNullException.ThrowIfNull(name);
        ArgumentNullException.ThrowIfNull(path);
        ArgumentNullException.ThrowIfNull(boundary);
        FerruleXmlDocumentOutputsOwner? owner = boundary.Kind == FerruleXmlBoundaryErrorKind.Schema ? null :
            new FerruleXmlDocumentOutputsOwner.NamedMember(declarationIndex, name, index, path);
        return new(owner, boundary);
    }

    public static FerruleXmlDocumentOutputsExecutionException Alignment(string detail) =>
        FromBoundary(FerruleXmlOutputSetBudget.Alignment(detail).Boundary);
}

/// <summary>One mapped execution. Actual count includes Primary; the caller checks 1+members.Count
/// before capacity/serialization, serializes and charges Primary first then members in final order,
/// and aborts on its first failure. Per-document limits are checked by the serializer. Logical paths
/// and peak memory are not charged; this is not a streaming/RSS bound.</summary>
public sealed class FerruleXmlDocumentOutputsBudget
{
    private readonly FerruleXmlOutputSetBudget _inner;

    public FerruleXmlDocumentOutputsBudget(int actualArtifactCount)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(actualArtifactCount);
        if (actualArtifactCount == 0)
            throw FerruleXmlDocumentOutputsExecutionException.Alignment("mixed XML outputs require the static primary artifact");
        try { _inner = new FerruleXmlOutputSetBudget(actualArtifactCount); }
        catch (FerruleXmlOutputSetException error)
        { throw FerruleXmlDocumentOutputsExecutionException.FromBoundary(error.Boundary); }
    }

    public void ChargePrimary(long utf8Bytes)
    {
        try { _inner.Charge(FerruleXmlOutputTarget.Primary, utf8Bytes); }
        catch (FerruleXmlOutputSetException error)
        { throw FerruleXmlDocumentOutputsExecutionException.PrimarySerialization(error.Boundary); }
    }

    public void ChargeNamed(int declarationIndex, string name, int index, string path, long utf8Bytes)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(declarationIndex);
        ArgumentOutOfRangeException.ThrowIfNegative(index);
        ArgumentNullException.ThrowIfNull(name);
        ArgumentNullException.ThrowIfNull(path);
        try { _inner.Charge(FerruleXmlOutputTarget.Named(declarationIndex, name), utf8Bytes); }
        catch (FerruleXmlOutputSetException error)
        { throw FerruleXmlDocumentOutputsExecutionException.NamedSerialization(declarationIndex, name, index, path, error.Boundary); }
    }
}
