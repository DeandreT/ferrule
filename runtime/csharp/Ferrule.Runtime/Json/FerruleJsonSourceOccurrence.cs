namespace Ferrule.Runtime;

/// <summary>An exact value occurrence with its owning document and structural parent.</summary>
public sealed class FerruleJsonSourceOccurrence
{
    private readonly object owner;
    public FerruleJsonSourceDocument Document { get; }
    public FerruleJsonSourceValue Value { get; }
    public FerruleJsonSourceOccurrence? Parent { get; }

    private FerruleJsonSourceOccurrence(FerruleJsonSourceDocument document,
        FerruleJsonSourceValue value, object owner, FerruleJsonSourceOccurrence? parent)
    {
        Document = document;
        Value = value;
        this.owner = owner;
        Parent = parent;
    }

    /// <summary>Create a new document occurrence, even for the same document object.</summary>
    public static FerruleJsonSourceOccurrence Create(FerruleJsonSourceDocument document)
    {
        ArgumentNullException.ThrowIfNull(document);
        return new(document, document.Root, new object(), null);
    }

    public bool SameOccurrence(FerruleJsonSourceOccurrence? other) =>
        other is not null && ReferenceEquals(owner, other.owner) && ReferenceEquals(Value, other.Value);

    /// <summary>Zero or one occurrence without conversion or another parent frame.</summary>
    public FerruleJsonSourceOccurrence? OfKind(FerruleJsonSourceKind kind) => Value.Kind == kind ? this : null;

    public IEnumerable<FerruleJsonSourceOccurrence> MembersNamed(string name)
    {
        ArgumentNullException.ThrowIfNull(name);
        return Value.MembersNamed(name).Select(Child);
    }

    public IEnumerable<FerruleJsonSourceOccurrence> Items() => Value.Items.Select(Child);

    private FerruleJsonSourceOccurrence Child(FerruleJsonSourceValue value) =>
        new(Document, value, owner, this);
}

public sealed class FerruleJsonSourceLoadException : Exception
{
    public int Maximum { get; }
    internal FerruleJsonSourceLoadException(int maximum)
        : base($"JSON source exceeds the {maximum}-document limit") => Maximum = maximum;
}

public static class FerruleJsonSources
{
    public const int MaxDocuments = 1_000_000;

    /// <summary>
    /// Lazily enumerate and load in host order. Each enumeration is a fresh run;
    /// each enumerator ends on its first error. Host exceptions are not wrapped;
    /// normal request-enumerator disposal and exception precedence still apply.
    /// The host completes path policy, parsing, required validation and input
    /// cleanup before returning a document. This API performs no file access.
    /// The document-count bound is not a retained-memory ceiling.
    /// </summary>
    public static IEnumerable<FerruleJsonSourceOccurrence> Load(IEnumerable<string> requests,
        Func<string, FerruleJsonSourceDocument> load, int maximum = MaxDocuments)
    {
        ArgumentNullException.ThrowIfNull(requests);
        ArgumentNullException.ThrowIfNull(load);
        if (maximum is < 0 or > MaxDocuments) throw new ArgumentOutOfRangeException(nameof(maximum));
        return Enumerate();

        IEnumerable<FerruleJsonSourceOccurrence> Enumerate()
        {
            var count = 0;
            foreach (var request in requests)
            {
                if (count == maximum) throw new FerruleJsonSourceLoadException(maximum);
                var document = load(request);
                count++;
                yield return FerruleJsonSourceOccurrence.Create(document);
            }
        }
    }
}
