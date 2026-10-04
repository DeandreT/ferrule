namespace Ferrule.Runtime;

/// <summary>Zero-based final output member index and opaque host-owned logical path.</summary>
public sealed record FerruleXmlDocumentOutputOwner(FerruleXmlOutputTarget Target, int Index, string Path);

/// <summary>The original boundary and its typed inner cause remain unchanged.</summary>
public sealed class FerruleXmlDocumentExecutionException : Exception
{
    public FerruleXmlDocumentExecutionException(
        FerruleXmlDocumentOutputOwner? member, FerruleXmlBoundaryException boundary)
        : base(MessageFor(member, boundary), boundary)
    { Member = member; Boundary = boundary; }

    public FerruleXmlDocumentOutputOwner? Member { get; }
    public FerruleXmlBoundaryException Boundary { get; }

    private static string MessageFor(FerruleXmlDocumentOutputOwner? member, FerruleXmlBoundaryException boundary)
    {
        ArgumentNullException.ThrowIfNull(boundary);
        return member is null ? boundary.Message : $"XML output {member.Target} member {member.Index} `{member.Path}`: {boundary.Message}";
    }

    public static FerruleXmlDocumentExecutionException FromBoundary(FerruleXmlBoundaryException boundary) => new(null, boundary);

    /// <summary>Descriptor Schema setup is global; other serializer failures own the member.</summary>
    public static FerruleXmlDocumentExecutionException Serialization(int index, string path, FerruleXmlBoundaryException boundary)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(index);
        ArgumentNullException.ThrowIfNull(path);
        ArgumentNullException.ThrowIfNull(boundary);
        var member = boundary.Kind == FerruleXmlBoundaryErrorKind.Schema ? null :
            new FerruleXmlDocumentOutputOwner(FerruleXmlOutputTarget.Primary, index, path);
        return new(member, boundary);
    }

    public static FerruleXmlDocumentExecutionException Alignment(string detail) =>
        FromBoundary(FerruleXmlOutputSetBudget.Alignment(detail).Boundary);
}

/// <summary>Actual member count and serialized UTF-8 bytes; paths and peak memory are not charged.</summary>
public sealed class FerruleXmlDocumentSetBudget
{
    private readonly FerruleXmlOutputSetBudget _inner;

    public FerruleXmlDocumentSetBudget(int actualCount)
    {
        try { _inner = new FerruleXmlOutputSetBudget(actualCount); }
        catch (FerruleXmlOutputSetException error)
        { throw FerruleXmlDocumentExecutionException.FromBoundary(error.Boundary); }
    }

    public void Charge(int index, string path, long utf8Bytes)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(index);
        ArgumentNullException.ThrowIfNull(path);
        try { _inner.Charge(FerruleXmlOutputTarget.Primary, utf8Bytes); }
        catch (FerruleXmlOutputSetException error)
        { throw FerruleXmlDocumentExecutionException.Serialization(index, path, error.Boundary); }
    }
}
