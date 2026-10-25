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

/// <summary>How the selected documents form the one mapping source.</summary>
public enum FerruleDocumentShape
{
    /// <summary>Every document's rows, in selection then row order.</summary>
    Rows,
    /// <summary>The one directly named document.</summary>
    Document,
    /// <summary>A document set of every selected document, in selection order.</summary>
    DocumentSet,
}

public enum FerruleDocumentAcquisitionError
{
    InvalidLimit, Filename, Selection, DirectSelection, Request, Documents, Load, Input, Assembly,
}

/// <summary>
/// A stage-preserving failure of primary document acquisition or assembly. The cause of a
/// filename, selection, load or input failure is the inner exception.
/// </summary>
public sealed class FerrulePrimaryDocumentException : Exception
{
    public FerrulePrimaryDocumentException(FerruleDocumentAcquisitionError error, string? path = null,
        int? count = null, Exception? source = null)
        : base($"primary document {error}" + (path is null ? "" : $" for {path}") +
            (count is null ? "" : $" ({count})"), source)
    {
        Error = error;
        Path = path;
        Count = count;
    }

    public FerruleDocumentAcquisitionError Error { get; }
    public string? Path { get; }
    /// <summary>The document limit, or the number of documents a direct filename selected.</summary>
    public int? Count { get; }
}

public static class FerrulePrimaryDocuments
{
    private static T At<T>(FerruleDocumentAcquisitionError error, Func<T> call, string? path = null)
    {
        try { return call(); }
        catch (Exception source) when (source is not FerrulePrimaryDocumentException)
        { throw new FerrulePrimaryDocumentException(error, path, source: source); }
    }

    /// <summary>
    /// Evaluate the filename once, select once, then load and parse each request in order and
    /// form the one mapping source. The first failure ends acquisition before any later load;
    /// every document is parsed before the caller maps anything.
    /// </summary>
    public static FerruleInstance AcquireDocuments(Func<string> filename, FerrulePrimaryFileSelection selection,
        IFerrulePrimarySourceHost host, FerruleDocumentShape shape, int maximum,
        Func<FerruleCsvSourceData, FerruleInstance> parse)
    {
        ArgumentNullException.ThrowIfNull(filename);
        ArgumentNullException.ThrowIfNull(host);
        ArgumentNullException.ThrowIfNull(parse);
        return AcquireDocuments(filename, selection, host, shape, maximum, (_, data) => parse(data));
    }

    /// <summary>Like AcquireDocuments, with the exact logical request passed to the parser.</summary>
    public static FerruleInstance AcquireDocuments(Func<string> filename, FerrulePrimaryFileSelection selection,
        IFerrulePrimarySourceHost host, FerruleDocumentShape shape, int maximum,
        Func<string, FerruleCsvSourceData, FerruleInstance> parse)
    {
        ArgumentNullException.ThrowIfNull(filename);
        ArgumentNullException.ThrowIfNull(host);
        ArgumentNullException.ThrowIfNull(parse);
        if (maximum is < 0 or > FerruleCsvSourceRows.MaximumDocuments)
            throw new FerrulePrimaryDocumentException(FerruleDocumentAcquisitionError.InvalidLimit, count: maximum);
        string name = At(FerruleDocumentAcquisitionError.Filename, filename);
        var requests = At(FerruleDocumentAcquisitionError.Selection, () => host.Select(name, selection)
            ?? throw new InvalidOperationException("Primary document selector returned no requests."));
        if (!selection.IsFileNameWildcard && requests.Count != 1)
            throw new FerrulePrimaryDocumentException(FerruleDocumentAcquisitionError.DirectSelection,
                count: requests.Count);
        var documents = new List<(string Request, string? Resolved, FerruleInstance Value)>();
        foreach (string request in requests)
        {
            if (documents.Count == maximum)
                throw new FerrulePrimaryDocumentException(FerruleDocumentAcquisitionError.Documents, count: maximum);
            try { FerruleCsvSourceDocument.ValidatePath(request, nameof(request)); }
            catch (Exception)
            { throw new FerrulePrimaryDocumentException(FerruleDocumentAcquisitionError.Request, request); }
            var data = At(FerruleDocumentAcquisitionError.Load, () => host.Load(request)
                ?? throw new InvalidOperationException("Primary document loader returned no bytes."), request);
            var value = At(FerruleDocumentAcquisitionError.Input, () => parse(request, data), request);
            documents.Add((request, data.ResolvedPath, value));
        }
        return Assemble(shape, documents);
    }

    private static FerruleInstance Assemble(FerruleDocumentShape shape,
        List<(string Request, string? Resolved, FerruleInstance Value)> documents)
    {
        switch (shape)
        {
            case FerruleDocumentShape.Rows:
                var rows = new List<FerruleInstance>();
                foreach (var document in documents)
                {
                    if (document.Value is not FerruleRepeated repeated)
                        throw new FerrulePrimaryDocumentException(FerruleDocumentAcquisitionError.Assembly,
                            document.Request);
                    rows.AddRange(repeated.Items);
                }
                return new FerruleRepeated(rows);
            case FerruleDocumentShape.Document:
                if (documents.Count != 1)
                    throw new FerrulePrimaryDocumentException(FerruleDocumentAcquisitionError.Assembly,
                        count: documents.Count);
                return documents[0].Value;
            default:
                return new FerruleDocumentSet(documents.Select(document =>
                    At(FerruleDocumentAcquisitionError.Assembly,
                        () => new FerruleDocument(document.Request, document.Value, document.Resolved),
                        document.Request)));
        }
    }

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
