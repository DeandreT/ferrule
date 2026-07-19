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
        Name = name ?? throw new ArgumentNullException(nameof(name));
        Value = value ?? throw new ArgumentNullException(nameof(value));
    }

    public string Name { get; }

    public FerruleInstance Value { get; }
}

/// <summary>An insertion-ordered collection of uniquely named fields.</summary>
public sealed class FerruleGroup : FerruleInstance
{
    private readonly IReadOnlyList<FerruleField> _fields;
    private readonly Dictionary<string, FerruleInstance> _fieldsByName;

    public FerruleGroup(IEnumerable<FerruleField> fields)
    {
        ArgumentNullException.ThrowIfNull(fields);
        var ordered = new List<FerruleField>();
        _fieldsByName = new Dictionary<string, FerruleInstance>(StringComparer.Ordinal);
        foreach (var field in fields)
        {
            ArgumentNullException.ThrowIfNull(field);
            if (!_fieldsByName.TryAdd(field.Name, field.Value))
            {
                throw new FerruleRuntimeException(
                    FerruleRuntimeError.DuplicateField,
                    $"Ferrule group field '{field.Name}' is duplicated.");
            }

            ordered.Add(field);
        }

        _fields = new ReadOnlyCollection<FerruleField>(ordered);
    }

    public IReadOnlyList<FerruleField> Fields => _fields;

    public bool TryGetField(
        string name,
        [NotNullWhen(true)] out FerruleInstance? value) =>
        _fieldsByName.TryGetValue(name, out value);
}

public sealed class FerruleRepeated : FerruleInstance
{
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
