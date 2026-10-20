using System.Collections.ObjectModel;
using System.Diagnostics.CodeAnalysis;

namespace Ferrule.Runtime;

/// <summary>A typed Ferrule instance tree node.</summary>
public abstract class FerruleInstance
{
    private protected FerruleInstance()
    {
    }
}

public sealed class FerruleScalar : FerruleInstance
{
    public FerruleScalar(FerruleValue value)
    {
        Value = value;
    }

    public FerruleValue Value { get; }
}

public sealed class FerruleField
{
    public FerruleField(string name, FerruleInstance value)
    {
        ArgumentNullException.ThrowIfNull(name);
        FerruleUnicode.RequireWellFormed(name, nameof(name));
        Name = name;
        Value = value ?? throw new ArgumentNullException(nameof(value));
    }

    public string Name { get; }

    public FerruleInstance Value { get; }
}

/// <summary>An insertion-ordered group whose public construction requires unique field names.</summary>
public sealed class FerruleGroup : FerruleInstance
{
    private readonly IReadOnlyList<FerruleField> _fields;
    private readonly Dictionary<string, FerruleInstance>? _fieldsByName;
    private readonly bool _schemaDeclarations;
    private readonly FerruleXmlTypeOrigin? _origin;

    public FerruleGroup(IEnumerable<FerruleField> fields)
        : this(fields, schemaDeclarations: false)
    {
    }

    // Closed native schemas may declare the same field more than once. Their
    // ordered typed slots remain distinct; path lookup resolves the first slot.
    internal static FerruleGroup FromSchemaFields(IEnumerable<FerruleField> fields) =>
        new(fields, schemaDeclarations: true);

    internal FerruleGroup RebuildFields(IEnumerable<FerruleField> fields) =>
        new(fields, _schemaDeclarations);

    internal FerruleGroup CloneFields(IEnumerable<FerruleField> fields) =>
        new(fields, _schemaDeclarations, _origin);

    [System.Text.Json.Serialization.JsonIgnore]
    public FerruleXmlTypeOrigin XmlTypeOrigin => _origin ?? FerruleXmlTypeOrigin.Unknown;

    /// <summary>Attach an actual-reader fact at a trusted host boundary.</summary>
    public FerruleGroup WithXmlTypeOrigin(FerruleXmlTypeOrigin origin)
    {
        ArgumentNullException.ThrowIfNull(origin);
        return new(_fields, _schemaDeclarations, origin);
    }

    private FerruleGroup(IEnumerable<FerruleField> fields, bool schemaDeclarations,
        FerruleXmlTypeOrigin? origin = null)
    {
        ArgumentNullException.ThrowIfNull(fields);
        var ordered = new List<FerruleField>();
        _schemaDeclarations = schemaDeclarations;
        _origin = origin?.Kind == FerruleXmlTypeOriginKind.Unknown ? null : origin;
        _fieldsByName = new Dictionary<string, FerruleInstance>(StringComparer.Ordinal);
        foreach (var field in fields)
        {
            ArgumentNullException.ThrowIfNull(field);
            if (!_fieldsByName.TryAdd(field.Name, field.Value) && !schemaDeclarations)
            {
                throw new FerruleRuntimeException(
                    FerruleRuntimeError.DuplicateField,
                    $"Ferrule group field '{field.Name}' is duplicated.");
            }

            ordered.Add(field);
        }

        _fields = new ReadOnlyCollection<FerruleField>(ordered);
    }

    // The basic source bridge owns and admits every projection slot. This seam
    // makes one exact immutable snapshot without growing legacy collections.
    internal static FerruleGroup FromBasicSourceProjection(
        ReadOnlySpan<FerruleField> fields, nuint maximumFields, OwnedCounter slots)
    {
        ArgumentNullException.ThrowIfNull(slots);
        var count = (nuint)fields.Length;
        OwnedSourceAccounting.Limit(0, count, maximumFields, FerruleOwnedSourceLimitKind.Roots);
        if (fields.Length > Array.MaxLength)
            throw OwnedSourceAccounting.Overflow(FerruleOwnedSourceLimitKind.MetadataSlots);
        for (var i = 0; i < fields.Length; i++)
        {
            ArgumentNullException.ThrowIfNull(fields[i]);
            for (var j = 0; j < i; j++)
            {
                if (StringComparer.Ordinal.Equals(fields[j].Name, fields[i].Name))
                    throw new FerruleRuntimeException(FerruleRuntimeError.DuplicateField,
                        "Ferrule basic source projection has a duplicate field.");
            }
        }

        // Snapshot slots coexist with the caller's admitted field backing.
        // Both wrappers are admitted before the first allocation as well.
        slots.Charge(OwnedSourceAccounting.Add(count, 2, FerruleOwnedSourceLimitKind.MetadataSlots));
        var snapshot = new FerruleField[fields.Length];
        fields.CopyTo(snapshot);
        var view = new ReadOnlyCollection<FerruleField>(snapshot);
        return new FerruleGroup(view);
    }

    private FerruleGroup(ReadOnlyCollection<FerruleField> projection)
    {
        _fields = projection;
        _fieldsByName = null;
        _schemaDeclarations = false;
        _origin = null;
    }

    internal static FerruleGroup FromBasicGroupWrapper(string name, FerruleRepeated members,
        BasicGroupPrefixTemplate admission)
    {
        admission.ConsumeWrapper();
        var fields = new FerruleField[1];
        fields[0] = new FerruleField(name, members);
        return new FerruleGroup(new ReadOnlyCollection<FerruleField>(fields));
    }

    public IReadOnlyList<FerruleField> Fields => _fields;

    public bool TryGetField(
        string name,
        [NotNullWhen(true)] out FerruleInstance? value)
    {
        if (_fieldsByName is not null)
            return _fieldsByName.TryGetValue(name, out value);
        ArgumentNullException.ThrowIfNull(name, "key");
        for (var i = 0; i < _fields.Count; i++)
        {
            if (StringComparer.Ordinal.Equals(_fields[i].Name, name))
            {
                value = _fields[i].Value;
                return true;
            }
        }

        value = null;
        return false;
    }
}

public sealed class FerruleRepeated : FerruleInstance
{
    private FerruleRepeated(ReadOnlyCollection<FerruleInstance> admitted) { Items = admitted; }
    internal static FerruleRepeated FromBasicGroupMembers(FerruleInstance[] admitted,
        BasicGroupPrefixTemplate admission)
    {
        admission.ConsumeMembers(admitted.Length);
        return new FerruleRepeated(new ReadOnlyCollection<FerruleInstance>(admitted));
    }
    public FerruleRepeated(IEnumerable<FerruleInstance> items)
    {
        Items = InstanceCollection.Copy(items, nameof(items));
    }

    public IReadOnlyList<FerruleInstance> Items { get; }
}

public sealed class FerruleMappedSequence : FerruleInstance
{
    public FerruleMappedSequence(IEnumerable<FerruleInstance> items)
    {
        Items = InstanceCollection.Copy(items, nameof(items));
    }

    public IReadOnlyList<FerruleInstance> Items { get; }
}

public sealed class FerruleDocument
{
    public FerruleDocument(
        string path,
        FerruleInstance value,
        string? resolvedSourcePath = null)
    {
        if (string.IsNullOrEmpty(path) || resolvedSourcePath is not null && resolvedSourcePath.Length == 0)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.InvalidDocumentPath,
                "Ferrule document paths must not be empty.");
        }

        if (!FerruleUnicode.IsWellFormed(path) ||
            resolvedSourcePath is not null && !FerruleUnicode.IsWellFormed(resolvedSourcePath))
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.InvalidDocumentPath,
                "Ferrule document paths must contain valid Unicode scalar values.");
        }

        ArgumentNullException.ThrowIfNull(value);
        if (value is FerruleDocumentSet)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.NestedDocumentSet,
                "A Ferrule document cannot contain another document set.");
        }

        Path = path;
        ResolvedSourcePath = resolvedSourcePath;
        Value = value;
    }

    public string Path { get; }

    public string? ResolvedSourcePath { get; }

    public FerruleInstance Value { get; }
}

public sealed class FerruleDocumentSet : FerruleInstance
{
    private readonly IReadOnlyList<FerruleDocument> _documents;

    public FerruleDocumentSet(IEnumerable<FerruleDocument> documents)
    {
        ArgumentNullException.ThrowIfNull(documents);
        var copy = new List<FerruleDocument>();
        foreach (var document in documents)
        {
            ArgumentNullException.ThrowIfNull(document);
            copy.Add(document);
        }

        _documents = new ReadOnlyCollection<FerruleDocument>(copy);
    }

    public IReadOnlyList<FerruleDocument> Documents => _documents;
}

internal static class InstanceCollection
{
    public static IReadOnlyList<FerruleInstance> Copy(
        IEnumerable<FerruleInstance> items,
        string parameterName)
    {
        ArgumentNullException.ThrowIfNull(items, parameterName);
        var copy = new List<FerruleInstance>();
        foreach (var item in items)
        {
            ArgumentNullException.ThrowIfNull(item);
            copy.Add(item);
        }

        return new ReadOnlyCollection<FerruleInstance>(copy);
    }
}
