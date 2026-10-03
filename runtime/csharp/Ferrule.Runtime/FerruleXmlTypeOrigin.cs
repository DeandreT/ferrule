namespace Ferrule.Runtime;

public enum FerruleXmlTypeOriginKind { Unknown, Absent, Explicit, ExplicitPadded }

/// <summary>An immutable reader fact owned by one exact group occurrence.</summary>
public sealed class FerruleXmlTypeOrigin
{
    private FerruleXmlTypeOrigin(FerruleXmlTypeOriginKind kind, string? identity, string? literal = null)
    {
        Kind = kind;
        Identity = identity;
        Literal = literal;
    }
    public FerruleXmlTypeOriginKind Kind { get; }
    public string? Identity { get; }
    public string? Literal { get; }
    public static FerruleXmlTypeOrigin Unknown { get; } = new(FerruleXmlTypeOriginKind.Unknown, null);
    public static FerruleXmlTypeOrigin Absent { get; } = new(FerruleXmlTypeOriginKind.Absent, null);
    public static FerruleXmlTypeOrigin Explicit(string identity)
    {
        ArgumentException.ThrowIfNullOrEmpty(identity);
        FerruleUnicode.RequireWellFormed(identity, nameof(identity));
        return new(FerruleXmlTypeOriginKind.Explicit, identity);
    }
    public static FerruleXmlTypeOrigin ExplicitPadded(string literal, string resolvedIdentity)
    {
        if (!FerrulePrimaryRoot.PaddedTypeOriginIsValid(literal, resolvedIdentity))
            throw new ArgumentException("Padded XML annotation requires a bounded QName and canonical resolution.");
        return new(FerruleXmlTypeOriginKind.ExplicitPadded, resolvedIdentity, literal);
    }
}
