namespace Ferrule.Runtime;

/// <summary>What a filename wildcard that matches no file means.</summary>
public enum FerruleEmptyFileSelection
{
    /// <summary>A typed selection failure; the mapping does not run.</summary>
    Refuse,
    /// <summary>The mapping runs once over zero documents.</summary>
    MapNoDocuments,
}

/// <summary>How an evaluated primary filename selects documents. The default is Direct.</summary>
public readonly record struct FerrulePrimaryFileSelection
{
    private FerrulePrimaryFileSelection(bool wildcard, FerruleEmptyFileSelection empty)
    {
        IsFileNameWildcard = wildcard;
        Empty = empty;
    }

    /// <summary>The filename names one document; '*' and '?' are ordinary characters.</summary>
    public static FerrulePrimaryFileSelection Direct => default;

    /// <summary>'*' and '?' in the final component select every matching file in one directory.</summary>
    public static FerrulePrimaryFileSelection FileNameWildcard(FerruleEmptyFileSelection empty) => new(true, empty);

    public bool IsFileNameWildcard { get; }
    public FerruleEmptyFileSelection Empty { get; }
}

/// <summary>
/// The host side of primary acquisition: Select interprets the evaluated filename under
/// the plan's selection and returns logical requests in load order; Load returns one
/// request's bytes. Neither parses nor maps.
/// </summary>
public interface IFerrulePrimarySourceHost
{
    IReadOnlyList<string> Select(string filename, FerrulePrimaryFileSelection selection);
    FerruleCsvSourceData Load(string request);
}

public static class FerrulePrimaryDocuments
{
    /// <summary>
    /// Evaluate the filename, select, then load and parse every selected document in
    /// order. The first failure ends acquisition before any later load. Success is one
    /// repeated row instance across all documents for a single mapping execution.
    /// </summary>
    public static FerruleRepeated AcquireCsvRows(Func<string> filename, FerrulePrimaryFileSelection selection,
        IFerrulePrimarySourceHost host, IReadOnlyList<FerruleCsvReadField> fields, FerruleCsvReadOptions? options,
        FerruleCsvByteDecodingPolicy decoding, int maximum)
    {
        ArgumentNullException.ThrowIfNull(host);
        using var rows = new FerruleCsvAcquisition(filename, name => host.Select(name, selection), host.Load,
            fields, options, decoding, maximum);
        var values = new List<FerruleInstance>();
        while (rows.MoveNext()) values.Add(rows.Current.Value);
        return new FerruleRepeated(values);
    }
}
