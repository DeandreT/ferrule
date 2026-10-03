using System.Collections.ObjectModel;
using System.Text;

namespace Ferrule.Runtime;

public enum FerrulePrimaryRootError
{
    MissingOwner, ExpectedGroup, UnknownXmlTypeOrigin, InvalidTypeIdentity,
    InvalidScalarPath, FieldLimit, DuplicateField, ExpectedGroupAt, ExpectedScalar,
    MissingRequiredField,
}

/// <summary>Structured failure reading the exact immutable primary source owner.</summary>
public sealed class FerrulePrimaryRootFailure
{
    internal FerrulePrimaryRootFailure(FerrulePrimaryRootError error,
        IEnumerable<string>? path = null, string? found = null)
    {
        Error = error;
        Path = new ReadOnlyCollection<string>((path ?? []).ToArray());
        Found = found;
    }
    public FerrulePrimaryRootError Error { get; }
    public IReadOnlyList<string> Path { get; }
    public string? Found { get; }
}

/// <summary>Exact source-root readers without frame, collection or named-input fallback.</summary>
public static class FerrulePrimaryRoot
{
    public const int MaximumIdentityBytes = 4096;
    public const int MaximumPathSegments = 16;
    public const int MaximumPathBytes = 4096;
    public const int MaximumFields = 4096;

    public static bool XmlTypeEquals(FerruleInstance? root, string identity, uint? node = null)
    {
        if (!TypeIdentityIsValid(identity))
            throw Failure(FerrulePrimaryRootError.InvalidTypeIdentity, node);
        var group = RootGroup(root, node);
        var origin = group.XmlTypeOrigin;
        if (origin.Kind == FerruleXmlTypeOriginKind.Unknown)
            throw Failure(FerrulePrimaryRootError.UnknownXmlTypeOrigin, node);
        if (origin.Kind == FerruleXmlTypeOriginKind.Absent) return false;
        if (origin.Kind == FerruleXmlTypeOriginKind.ExplicitPadded)
        {
            if (!PaddedTypeOriginIsValid(origin.Literal, origin.Identity))
                throw Failure(FerrulePrimaryRootError.InvalidTypeIdentity, node);
            return false;
        }
        if (!TypeIdentityIsValid(origin.Identity))
            throw Failure(FerrulePrimaryRootError.InvalidTypeIdentity, node);
        return string.Equals(origin.Identity, identity, StringComparison.Ordinal);
    }

    public static FerruleValue Scalar(FerruleInstance? root,
        IReadOnlyList<string> path, uint? node = null)
    {
        if (!ScalarPathIsValid(path))
            throw Failure(FerrulePrimaryRootError.InvalidScalarPath, node);
        FerruleInstance current = RootGroup(root, node);
        for (var index = 0; index < path.Count; index++)
        {
            if (current is not FerruleGroup group)
                throw Failure(FerrulePrimaryRootError.ExpectedGroupAt, node,
                    path.Take(index), InstanceKind(current));
            if (group.Fields.Count > MaximumFields)
                throw Failure(FerrulePrimaryRootError.FieldLimit, node);
            FerruleInstance? matching = null;
            foreach (var field in group.Fields)
            {
                if (!string.Equals(field.Name, path[index], StringComparison.Ordinal)) continue;
                if (matching is not null)
                    throw Failure(FerrulePrimaryRootError.DuplicateField, node, path.Take(index + 1));
                matching = field.Value;
            }
            if (matching is null) return FerruleValue.Null;
            current = matching;
        }
        if (current is FerruleScalar scalar) return scalar.Value;
        throw Failure(FerrulePrimaryRootError.ExpectedScalar, node, path, InstanceKind(current));
    }

    public static FerruleValue RequiredScalar(FerruleInstance? root,
        IReadOnlyList<string> path, uint? node = null)
    {
        var value = Scalar(root, path, node);
        if (value.Kind == FerruleValueKind.Null)
            throw Failure(FerrulePrimaryRootError.MissingRequiredField, node, path);
        return value;
    }

    public static bool TypeIdentityIsValid(string? identity)
    {
        if (!BoundedUnicode(identity, MaximumIdentityBytes) || identity!.Length == 0) return false;
        var local = identity;
        if (identity[0] == '{')
        {
            var end = identity.IndexOf('}');
            if (end <= 1) return false;
            var ns = identity[1..end];
            foreach (var rune in ns.EnumerateRunes())
                if (Rune.IsControl(rune) || Rune.IsWhiteSpace(rune) || rune.Value is '{' or '}')
                    return false;
            local = identity[(end + 1)..];
        }
        return NcNameIsValid(local);
    }

    public static bool PaddedTypeOriginIsValid(string? literal, string? resolvedIdentity)
    {
        if (!BoundedUnicode(literal, MaximumIdentityBytes) || !TypeIdentityIsValid(resolvedIdentity)) return false;
        var core = literal!.Trim(' ', '\t', '\r', '\n');
        if (core.Length == literal.Length || core.Length == 0) return false;
        foreach (var rune in core.EnumerateRunes()) if (Rune.IsWhiteSpace(rune)) return false;
        var colon = core.IndexOf(':');
        var local = core;
        if (colon >= 0)
        {
            if (!NcNameIsValid(core[..colon]) || !NcNameIsValid(core[(colon + 1)..])) return false;
            local = core[(colon + 1)..];
            if (resolvedIdentity![0] != '{') return false;
        }
        else if (!NcNameIsValid(core)) return false;
        var resolvedLocal = resolvedIdentity![0] == '{'
            ? resolvedIdentity[(resolvedIdentity.IndexOf('}') + 1)..] : resolvedIdentity;
        return string.Equals(local, resolvedLocal, StringComparison.Ordinal);
    }

    public static bool NcNameIsValid(string? name)
    {
        if (!BoundedUnicode(name, MaximumIdentityBytes) || name!.Length == 0) return false;
        var first = true;
        foreach (var rune in name.EnumerateRunes())
        {
            if (!NameStart(rune.Value) && (first || !NameContinuation(rune.Value))) return false;
            first = false;
        }
        return true;
    }

    public static bool ScalarPathIsValid(IReadOnlyList<string>? path)
    {
        if (path is null || path.Count == 0 || path.Count > MaximumPathSegments) return false;
        var bytes = 0;
        foreach (var name in path)
        {
            if (!BoundedUnicode(name, MaximumPathBytes) || name.Length == 0 || name is
                "\u001fferrule-xml-type" or "\u001fferrule-xml-substitution" or
                "\u001fferrule-xml-mixed-content" or "\u001fferrule-xml-mixed-value" or
                "element()" or "attribute()") return false;
            bytes += Encoding.UTF8.GetByteCount(name);
            if (bytes > MaximumPathBytes) return false;
        }
        return true;
    }

    private static bool BoundedUnicode(string? value, int maximum) =>
        value is not null && value.Length <= maximum && FerruleUnicode.IsWellFormed(value)
        && Encoding.UTF8.GetByteCount(value) <= maximum;

    // XML 1.0 fifth-edition NameStartChar, excluding namespace colon.
    private static bool NameStart(int ch) => ch is >= 'A' and <= 'Z' or '_' or >= 'a' and <= 'z'
        or >= 0xC0 and <= 0xD6 or >= 0xD8 and <= 0xF6 or >= 0xF8 and <= 0x2FF
        or >= 0x370 and <= 0x37D or >= 0x37F and <= 0x1FFF or >= 0x200C and <= 0x200D
        or >= 0x2070 and <= 0x218F or >= 0x2C00 and <= 0x2FEF or >= 0x3001 and <= 0xD7FF
        or >= 0xF900 and <= 0xFDCF or >= 0xFDF0 and <= 0xFFFD or >= 0x10000 and <= 0xEFFFF;
    private static bool NameContinuation(int ch) => ch is '-' or '.' or >= '0' and <= '9'
        or 0xB7 or >= 0x300 and <= 0x36F or >= 0x203F and <= 0x2040;

    private static FerruleGroup RootGroup(FerruleInstance? root, uint? node)
    {
        if (root is null) throw Failure(FerrulePrimaryRootError.MissingOwner, node);
        if (root is not FerruleGroup group)
            throw Failure(FerrulePrimaryRootError.ExpectedGroup, node, found: InstanceKind(root));
        if (group.Fields.Count > MaximumFields)
            throw Failure(FerrulePrimaryRootError.FieldLimit, node);
        return group;
    }

    private static string InstanceKind(FerruleInstance value) => value switch
    {
        FerruleScalar => "scalar", FerruleGroup => "group", FerruleRepeated => "repeated",
        FerruleDocumentSet => "document set", FerruleMappedSequence => "mapped sequence",
        _ => throw new InvalidOperationException("Unknown Ferrule instance subclass."),
    };

    private static FerruleRuntimeException Failure(FerrulePrimaryRootError error, uint? node,
        IEnumerable<string>? path = null, string? found = null)
    {
        var failure = new FerrulePrimaryRootFailure(error, path, found);
        var at = string.Join("/", failure.Path);
        var message = error switch
        {
            FerrulePrimaryRootError.MissingOwner => "immutable primary source root is unavailable",
            FerrulePrimaryRootError.ExpectedGroup => $"primary source root must be a group, found {found}",
            FerrulePrimaryRootError.UnknownXmlTypeOrigin => "primary source root has no retained XML annotation provenance",
            FerrulePrimaryRootError.InvalidTypeIdentity => "primary root XML type identity is malformed or exceeds its byte limit",
            FerrulePrimaryRootError.InvalidScalarPath => "primary root scalar path is empty, virtual, or exceeds its limits",
            FerrulePrimaryRootError.FieldLimit => "primary root path group exceeds its field limit",
            FerrulePrimaryRootError.DuplicateField => $"primary root path `{at}` has duplicate fields",
            FerrulePrimaryRootError.ExpectedGroupAt => $"primary root path `{at}` must traverse a group, found {found}",
            FerrulePrimaryRootError.MissingRequiredField => $"primary root path `{at}` is required but has no value",
            FerrulePrimaryRootError.ExpectedScalar => $"primary root path `{at}` must end at a scalar, found {found}",
            _ => throw new InvalidOperationException("Unknown primary-root failure."),
        };
        return new FerruleRuntimeException(FerruleRuntimeError.PrimaryRoot,
            node is { } id ? $"node {id}: {message}" : message,
            node: node, foundInstance: found, primaryRoot: failure);
    }
}
