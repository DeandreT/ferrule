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

/// <summary>One host-supplied XML document-set member: the portable path and optional
/// resolved location a local file-set reader records, and the member text.</summary>
public sealed record FerruleXmlDocumentSetMember(string Path, string? ResolvedSourcePath, string Document);

/// <summary>One host-supplied XML document-set member read from UTF-8 bytes.</summary>
public sealed record FerruleXmlBytesDocumentSetMember(string Path, string? ResolvedSourcePath, byte[] Document);

/// <summary>One supplied named document-set source and its ordered members.</summary>
public sealed record FerruleNamedXmlDocumentSet(string Name, IReadOnlyList<FerruleXmlDocumentSetMember> Members);

/// <summary>One supplied named document-set source read from UTF-8 bytes.</summary>
public sealed record FerruleNamedXmlBytesDocumentSet(string Name, IReadOnlyList<FerruleXmlBytesDocumentSetMember> Members);

/// <summary>The supplied member a failure belongs to: its source, zero-based index and path.</summary>
public sealed record FerruleXmlDocumentSetMemberOwner(FerruleXmlInputSource Source, int Index, string Path);

/// <summary>A refusal or failure of the owned XML document-set entry points. Name
/// admission, member-count and mapping failures belong to no member.</summary>
public sealed class FerruleXmlDocumentSetInputException : Exception
{
    private FerruleXmlDocumentSetInputException(FerruleXmlDocumentSetMemberOwner? member, FerruleXmlBoundaryException boundary)
        : base(member is null ? boundary.Message : $"XML input {member.Source} member {member.Index} `{member.Path}`: {boundary.Message}", boundary)
    { Member = member; Boundary = boundary; }

    public FerruleXmlDocumentSetMemberOwner? Member { get; }
    public FerruleXmlBoundaryException Boundary { get; }

    public static FerruleXmlDocumentSetInputException FromBoundary(FerruleXmlBoundaryException boundary)
    {
        ArgumentNullException.ThrowIfNull(boundary);
        return new(null, boundary);
    }

    public static FerruleXmlDocumentSetInputException FromMapping(FerruleRuntimeException error)
    {
        ArgumentNullException.ThrowIfNull(error);
        return new(null, new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Mapping, error.Message, error));
    }

    public static FerruleXmlDocumentSetInputException ForMember(
        FerruleXmlInputSource source, int index, string path, FerruleXmlBoundaryException boundary)
    {
        ArgumentNullException.ThrowIfNull(boundary);
        return new(new FerruleXmlDocumentSetMemberOwner(source, index, path), boundary);
    }
}

/// <summary>Member admission and parsing for the owned XML document-set entry points.</summary>
public static class FerruleXmlDocumentSets
{
    private const string MemberShape =
        "XML document-set members require a non-empty path and a non-empty resolved location";

    /// <summary>Admits every member of every supplied set before any member is parsed:
    /// the total member count, then each member's path and document size in supplied
    /// order, then the aggregate UTF-8 bytes of the whole input set.</summary>
    public static void Preflight(IReadOnlyList<(FerruleXmlInputSource Source, IReadOnlyList<FerruleXmlDocumentSetMember> Members)> sets) =>
        PreflightCore(sets, member => (member.Path, member.ResolvedSourcePath),
            (source, member) => FerruleXmlInputSetBudget.Measure(source, member.Document));

    public static void Preflight(IReadOnlyList<(FerruleXmlInputSource Source, IReadOnlyList<FerruleXmlBytesDocumentSetMember> Members)> sets) =>
        PreflightCore(sets, member => (member.Path, member.ResolvedSourcePath), (_, member) => member.Document.LongLength);

    /// <summary>Parses one supplied set, member by member in supplied order, into the
    /// document set a local file-set reader produces for the same files.</summary>
    public static FerruleInstance Parse(string descriptor, FerruleXmlInputSource source, IReadOnlyList<FerruleXmlDocumentSetMember> members) =>
        ParseCore(source, members, member => (member.Path, member.ResolvedSourcePath),
            member => FerruleXml.ParseStructuredEmbedded(descriptor, member.Document));

    public static FerruleInstance Parse(string descriptor, FerruleXmlInputSource source, IReadOnlyList<FerruleXmlBytesDocumentSetMember> members) =>
        ParseCore(source, members, member => (member.Path, member.ResolvedSourcePath),
            member => FerruleXml.ParseStructuredEmbeddedBytes(descriptor, member.Document));

    private static void PreflightCore<T>(
        IReadOnlyList<(FerruleXmlInputSource Source, IReadOnlyList<T> Members)> sets,
        Func<T, (string Path, string? Resolved)> location, Func<FerruleXmlInputSource, T, long> measure)
        where T : class
    {
        ArgumentNullException.ThrowIfNull(sets);
        long count = 0;
        foreach (var (_, members) in sets)
        {
            ArgumentNullException.ThrowIfNull(members);
            count = long.MaxValue - count < members.Count ? long.MaxValue : count + members.Count;
        }
        FerruleXmlInputSetBudget budget;
        try { budget = new FerruleXmlInputSetBudget(count); }
        catch (FerruleXmlExecutionException error) { throw FerruleXmlDocumentSetInputException.FromBoundary(error.Boundary); }
        var sizes = new List<long>();
        foreach (var (source, members) in sets)
        {
            for (var index = 0; index < members.Count; index++)
            {
                var member = members[index];
                ArgumentNullException.ThrowIfNull(member);
                var (path, resolved) = location(member);
                ArgumentNullException.ThrowIfNull(path);
                if (path.Length == 0 || resolved is { Length: 0 } || !FerruleUnicode.IsWellFormed(path) ||
                    resolved is not null && !FerruleUnicode.IsWellFormed(resolved))
                    throw FerruleXmlDocumentSetInputException.ForMember(source, index, path,
                        new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Input, MemberShape));
                try
                {
                    var bytes = measure(source, member);
                    FerruleXmlInputSetBudget.RequireDocumentSize(source, bytes);
                    sizes.Add(bytes);
                }
                catch (FerruleXmlExecutionException error)
                { throw FerruleXmlDocumentSetInputException.ForMember(source, index, path, error.Boundary); }
            }
        }
        var next = 0;
        foreach (var (source, members) in sets)
        {
            for (var index = 0; index < members.Count; index++, next++)
            {
                try { budget.Charge(source, sizes[next]); }
                catch (FerruleXmlExecutionException error)
                { throw FerruleXmlDocumentSetInputException.ForMember(source, index, location(members[index]).Path, error.Boundary); }
            }
        }
    }

    private static FerruleInstance ParseCore<T>(FerruleXmlInputSource source, IReadOnlyList<T> members,
        Func<T, (string Path, string? Resolved)> location, Func<T, FerruleInstance> parse)
        where T : class
    {
        ArgumentNullException.ThrowIfNull(source);
        ArgumentNullException.ThrowIfNull(members);
        var documents = new List<FerruleDocument>(members.Count);
        for (var index = 0; index < members.Count; index++)
        {
            var member = members[index];
            ArgumentNullException.ThrowIfNull(member);
            var (path, resolved) = location(member);
            FerruleInstance value;
            try { value = parse(member); }
            catch (FerruleXmlBoundaryException error)
            { throw FerruleXmlDocumentSetInputException.ForMember(source, index, path, error); }
            try { documents.Add(new FerruleDocument(path, value, resolved)); }
            catch (FerruleRuntimeException error)
            {
                throw FerruleXmlDocumentSetInputException.ForMember(source, index, path,
                    new FerruleXmlBoundaryException(FerruleXmlBoundaryErrorKind.Input, MemberShape, error));
            }
        }
        return new FerruleDocumentSet(documents);
    }
}
