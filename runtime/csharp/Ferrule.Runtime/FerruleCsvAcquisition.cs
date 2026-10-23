using System.Collections;

namespace Ferrule.Runtime;

/// <summary>Host-loaded bytes with a separate optional resolved source location.</summary>
public sealed record FerruleCsvSourceData(byte[] Bytes, string? ResolvedPath = null);

public enum FerruleCsvAcquisitionError
{
    InvalidLimit, Filename, Selection, Request, Documents, Load, Input, InvalidFileSetSchema, FileSetBytes, FileSetRows,
}

public sealed class FerruleCsvAcquisitionException : Exception
{
    public FerruleCsvAcquisitionException(FerruleCsvAcquisitionError error, string? path = null,
        int? maximum = null, Exception? source = null)
        : base($"CSV acquisition {error}" + (path is null ? "" : $" for {path}") +
            (maximum is null ? "" : $" ({maximum})"), source)
    {
        Error = error;
        Path = path;
        Maximum = maximum;
    }
    public FerruleCsvAcquisitionError Error { get; }
    public string? Path { get; }
    public int? Maximum { get; }
}

/// <summary>
/// Single-pass filename evaluation, host selection, byte loading and CSV parsing.
/// No host callback runs before MoveNext. This does not map or publish documents.
/// </summary>
public sealed class FerruleCsvAcquisition : IEnumerator<FerruleCsvRowOccurrence>
{
    private readonly Func<string> _filename;
    private readonly Func<string, IEnumerable<string>> _select;
    private readonly Func<string, FerruleCsvSourceData> _load;
    private readonly FerruleCsvReadField[] _fields;
    private readonly FerruleCsvReadOptions? _options;
    private readonly FerruleCsvByteDecodingPolicy _decoding;
    private readonly int _maximum;
    private IEnumerator<string>? _requests;
    private FerruleCsvSourceDocument? _document;
    private FerruleCsvRowOccurrence? _current;
    private bool _started;
    private bool _finished;
    private int _count;
    private int _row;

    public FerruleCsvAcquisition(Func<string> filename, Func<string, IEnumerable<string>> select,
        Func<string, FerruleCsvSourceData> load, IReadOnlyList<FerruleCsvReadField> fields,
        FerruleCsvReadOptions? options, FerruleCsvByteDecodingPolicy decoding, int maximum)
    {
        ArgumentNullException.ThrowIfNull(filename);
        ArgumentNullException.ThrowIfNull(select);
        ArgumentNullException.ThrowIfNull(load);
        ArgumentNullException.ThrowIfNull(fields);
        if (maximum is < 0 or > FerruleCsvSourceRows.MaximumDocuments)
            throw new FerruleCsvAcquisitionException(FerruleCsvAcquisitionError.InvalidLimit, maximum: maximum);
        _filename = filename;
        _select = select;
        _load = load;
        _fields = fields.ToArray();
        _options = options;
        _decoding = decoding;
        _maximum = maximum;
    }

    public FerruleCsvRowOccurrence Current => _current ?? throw new InvalidOperationException("No current CSV row.");
    object IEnumerator.Current => Current;

    private static T At<T>(FerruleCsvAcquisitionError error, Func<T> call, string? path = null)
    {
        try { return call(); }
        catch (Exception source) { throw new FerruleCsvAcquisitionException(error, path, source: source); }
    }

    public bool MoveNext()
    {
        _current = null;
        if (_finished) return false;
        _finished = true;
        if (!_started)
        {
            _started = true;
            string filename = At(FerruleCsvAcquisitionError.Filename, _filename);
            var selected = At(FerruleCsvAcquisitionError.Selection, () => _select(filename)
                ?? throw new InvalidOperationException("CSV selector returned no sequence."));
            _requests = At(FerruleCsvAcquisitionError.Request, selected.GetEnumerator);
        }
        while (true)
        {
            if (_document is not null && _row < _document.Count)
            {
                _current = _document.RowAt(_row++);
                _finished = false;
                return true;
            }
            _document = null;
            _row = 0;
            if (!At(FerruleCsvAcquisitionError.Request, () => _requests!.MoveNext())) return false;
            string path = At(FerruleCsvAcquisitionError.Request, () => _requests!.Current);
            if (_count == _maximum)
                throw new FerruleCsvAcquisitionException(FerruleCsvAcquisitionError.Documents, maximum: _maximum);
            At(FerruleCsvAcquisitionError.Input, () => { FerruleCsvSourceDocument.ValidatePath(path, nameof(path)); return true; }, path);
            var data = At(FerruleCsvAcquisitionError.Load, () => _load(path)
                ?? throw new InvalidOperationException("CSV loader returned no bytes."), path);
            _document = At(FerruleCsvAcquisitionError.Input, () => FerruleCsvSourceDocument.ParseBytes(
                path, data.ResolvedPath, data.Bytes, _fields, _decoding, _options), path);
            _count++;
        }
    }

    public void Reset() => throw new NotSupportedException("CSV acquisition is single-pass.");

    public void Dispose()
    {
        _finished = true;
        _current = null;
        _document = null;
        var requests = _requests;
        _requests = null;
        requests?.Dispose();
    }
}
