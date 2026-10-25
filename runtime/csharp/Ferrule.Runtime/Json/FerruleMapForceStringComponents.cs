using System.Text;

namespace Ferrule.Runtime;

public enum FerruleMapForceStringComponentError { NullInput, InputType, UnsupportedText, TableLimit, DuplicateKey }
public sealed class FerruleMapForceStringComponentException(FerruleMapForceStringComponentError kind)
    : Exception($"MapForce string component refused: {kind}")
{
    public FerruleMapForceStringComponentError Kind { get; } = kind;
}

/// <summary>Explicit strict-JSON/well-formed-Unicode subset. Eager syntax, lexical
/// numbers, no schema/default validation, and no transport. This does not admit
/// native raw controls or lone UTF-16 surrogates.</summary>
public sealed class FerruleMapForceJsonString
{
    private readonly FerruleJsonSourceDocument document;
    private FerruleMapForceJsonString(FerruleJsonSourceDocument document) => this.document = document;
    public FerruleJsonSourceValue Root => document.Root;
    public FerruleJsonSourceValue? RootObject => Root.Kind == FerruleJsonSourceKind.Object ? Root : null;

    public static FerruleMapForceJsonString Parse(FerruleValue input, FerruleJsonSourceLimits? limits = null)
    {
        if (input.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
            throw new FerruleMapForceStringComponentException(FerruleMapForceStringComponentError.NullInput);
        if (input.Kind != FerruleValueKind.String)
            throw new FerruleMapForceStringComponentException(FerruleMapForceStringComponentError.InputType);
        var text = input.StringValue;
        if (text.StartsWith('\uFEFF'))
            throw new FerruleMapForceStringComponentException(FerruleMapForceStringComponentError.UnsupportedText);
        limits ??= new();
        if (limits.DocumentBytes is < 0 or > FerruleJsonSourceDocument.MaxDocumentBytes ||
            limits.ContainerDepth is < 0 or > FerruleJsonSourceDocument.MaxContainerDepth ||
            limits.Values is < 0 or > FerruleJsonSourceDocument.MaxValues)
            throw new FerruleJsonSourceException(FerruleJsonSourceErrorKind.InvalidLimits);
        var utf8 = new UTF8Encoding(false, true);
        if (utf8.GetByteCount(text) > limits.DocumentBytes)
            throw new FerruleJsonSourceException(FerruleJsonSourceErrorKind.DocumentBytes);
        return new(FerruleJsonSourceDocument.Parse("mapforce-string-component", utf8.GetBytes(text), limits));
    }

    public IEnumerable<string> Strings(string name) => Root.MembersNamed(name)
        .Where(value => value.Kind == FerruleJsonSourceKind.String).Select(value => value.StringValue!);
    public string? FirstString(string name) => Strings(name).FirstOrDefault();
}

/// <summary>Bounded exact String table. Misses preserve the input. Absence and
/// reached null stay distinct. Duplicate keys are not admitted.</summary>
public sealed class FerruleMapForceStringInputFallback
{
    private readonly KeyValuePair<string, string>[] entries;
    public FerruleMapForceStringInputFallback(IReadOnlyList<KeyValuePair<string, string>> entries)
    {
        ArgumentNullException.ThrowIfNull(entries);
        if (entries.Count > 256)
            throw new FerruleMapForceStringComponentException(FerruleMapForceStringComponentError.TableLimit);
        var keys = new HashSet<string>(StringComparer.Ordinal);
        long bytes = 0;
        var utf8 = new UTF8Encoding(false, true);
        this.entries = new KeyValuePair<string, string>[entries.Count];
        for (int i = 0; i < entries.Count; i++)
        {
            var entry = entries[i];
            ArgumentNullException.ThrowIfNull(entry.Key);
            ArgumentNullException.ThrowIfNull(entry.Value);
            bytes += utf8.GetByteCount(entry.Key) + (long)utf8.GetByteCount(entry.Value);
            if (bytes > 65_536)
                throw new FerruleMapForceStringComponentException(FerruleMapForceStringComponentError.TableLimit);
            if (!keys.Add(entry.Key))
                throw new FerruleMapForceStringComponentException(FerruleMapForceStringComponentError.DuplicateKey);
            this.entries[i] = entry;
        }
    }
    public string Apply(FerruleValue input)
    {
        if (input.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
            throw new FerruleMapForceStringComponentException(FerruleMapForceStringComponentError.NullInput);
        if (input.Kind != FerruleValueKind.String)
            throw new FerruleMapForceStringComponentException(FerruleMapForceStringComponentError.InputType);
        var text = input.StringValue;
        foreach (var entry in entries)
            if (string.Equals(entry.Key, text, StringComparison.Ordinal)) return entry.Value;
        return text;
    }
    public string? ApplyPresent(FerruleValue? input) => input.HasValue ? Apply(input.Value) : null;
}
