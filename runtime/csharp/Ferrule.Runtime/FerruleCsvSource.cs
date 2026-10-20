using System.Collections;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>A complete physical CSV document with stable occurrence identity.</summary>
public sealed class FerruleCsvSourceDocument
{
    public const int MaximumPathBytes = 4096;
    private readonly IReadOnlyList<FerruleInstance> _rows;

    private FerruleCsvSourceDocument(string path, string? resolvedPath, FerruleRepeated rows)
    {
        Path = path;
        ResolvedPath = resolvedPath;
        _rows = rows.Items;
    }

    public string Path { get; }
    public string? ResolvedPath { get; }
    public string SourcePath => ResolvedPath ?? Path;

    /// <summary>
    /// No I/O or normalization. Failed parsing publishes no partial document.
    /// A fresh parse creates a distinct identity, even for identical input.
    /// </summary>
    public static FerruleCsvSourceDocument ParseBytes(
        string path, string? resolvedPath, byte[] bytes,
        IReadOnlyList<FerruleCsvReadField> fields,
        FerruleCsvByteDecodingPolicy decoding, FerruleCsvReadOptions? options = null)
    {
        ValidatePath(path, nameof(path));
        if (resolvedPath is not null) ValidatePath(resolvedPath, nameof(resolvedPath));
        var rows = (FerruleRepeated)FerruleCsvInput.ParseBytesWithDecodingPolicy(bytes, fields, decoding, options);
        return new FerruleCsvSourceDocument(path, resolvedPath, rows);
    }

    private static void ValidatePath(string path, string parameter)
    {
        if (string.IsNullOrEmpty(path) || path.Contains('\0') ||
            !FerruleUnicode.IsWellFormed(path) || Encoding.UTF8.GetByteCount(path) > MaximumPathBytes)
            throw new ArgumentException("CSV source paths must be nonempty, NUL-free, well-formed Unicode and at most 4096 UTF-8 bytes.", parameter);
    }

    public bool SameDocument(FerruleCsvSourceDocument other) => ReferenceEquals(this, other);

    public IEnumerable<FerruleCsvRowOccurrence> Rows()
    {
        for (var index = 0; index < _rows.Count; index++)
            yield return RowAt(index);
    }

    internal int Count => _rows.Count;
    internal FerruleCsvRowOccurrence RowAt(int index) => new(this, index, _rows[index]);
}

/// <summary>An exact data row retaining its physical document and local position.</summary>
public sealed class FerruleCsvRowOccurrence
{
    internal FerruleCsvRowOccurrence(FerruleCsvSourceDocument document, int index, FerruleInstance value)
    {
        Document = document;
        Position = index + 1;
        Value = value;
    }

    public FerruleCsvSourceDocument Document { get; }
    public FerruleInstance Value { get; }
    /// <summary>One-based data row, excluding a skipped header.</summary>
    public int Position { get; }
    public bool SameOccurrence(FerruleCsvRowOccurrence other) =>
        other is not null && Position == other.Position && Document.SameDocument(other.Document);
}

public sealed class FerruleCsvSourceLimitException : Exception
{
    public FerruleCsvSourceLimitException(int maximum)
        : base($"CSV source exceeds the {maximum}-document limit.") => Maximum = maximum;

    public int Maximum { get; }
}

/// <summary>
/// Single-pass lazy row enumeration over physical documents, in host order.
/// Empty documents consume a slot. The first reached exception ends this run;
/// it never asks for the next document. No implicit mapping or filesystem I/O.
/// Per-document and count limits do not impose a retained-memory ceiling.
/// </summary>
public sealed class FerruleCsvSourceRows : IEnumerator<FerruleCsvRowOccurrence>
{
    public const int MaximumDocuments = 1_000_000;
    private readonly IEnumerable<string> _requests;
    private readonly Func<string, FerruleCsvSourceDocument> _load;
    private readonly int _maximum;
    private IEnumerator<string>? _iterator;
    private FerruleCsvSourceDocument? _document;
    private FerruleCsvRowOccurrence? _current;
    private int _count;
    private int _row;
    private bool _finished;

    public FerruleCsvSourceRows(IEnumerable<string> requests, Func<string, FerruleCsvSourceDocument> load,
        int maximum)
    {
        ArgumentNullException.ThrowIfNull(requests);
        ArgumentNullException.ThrowIfNull(load);
        if (maximum is < 0 or > MaximumDocuments) throw new ArgumentOutOfRangeException(nameof(maximum));
        _requests = requests;
        _load = load;
        _maximum = maximum;
    }

    public FerruleCsvRowOccurrence Current => _current ?? throw new InvalidOperationException("No current CSV row.");
    object IEnumerator.Current => Current;

    public bool MoveNext()
    {
        _current = null;
        if (_finished) return false;
        // Stay terminal after any exception, including GetEnumerator/MoveNext/load.
        _finished = true;
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
            _iterator ??= _requests.GetEnumerator();
            if (!_iterator.MoveNext()) return false;
            if (_count == _maximum) throw new FerruleCsvSourceLimitException(_maximum);
            _document = _load(_iterator.Current) ?? throw new InvalidOperationException("CSV source loader returned no document.");
            _count++;
        }
    }

    public void Reset() => throw new NotSupportedException("CSV source rows are single-pass.");

    public void Dispose()
    {
        _finished = true;
        _current = null;
        _document = null;
        var iterator = _iterator;
        _iterator = null;
        iterator?.Dispose();
    }
}
