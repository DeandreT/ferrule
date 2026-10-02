namespace Ferrule.Runtime;

public enum FerruleXmlTypeOriginKind { Unknown, Absent, Explicit }

/// <summary>An immutable reader fact owned by one exact group occurrence.</summary>
public sealed class FerruleXmlTypeOrigin
{
    private FerruleXmlTypeOrigin(FerruleXmlTypeOriginKind kind, string? identity)
    {
        Kind = kind;
        Identity = identity;
    }
    public FerruleXmlTypeOriginKind Kind { get; }
    public string? Identity { get; }
    public static FerruleXmlTypeOrigin Unknown { get; } = new(FerruleXmlTypeOriginKind.Unknown, null);
    public static FerruleXmlTypeOrigin Absent { get; } = new(FerruleXmlTypeOriginKind.Absent, null);
    public static FerruleXmlTypeOrigin Explicit(string identity)
    {
        ArgumentException.ThrowIfNullOrEmpty(identity);
        FerruleUnicode.RequireWellFormed(identity, nameof(identity));
        return new(FerruleXmlTypeOriginKind.Explicit, identity);
    }
}
