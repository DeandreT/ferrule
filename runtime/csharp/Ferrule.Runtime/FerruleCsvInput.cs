using System.Globalization;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>One ordered scalar field in a statically selected CSV input row.</summary>
public sealed record FerruleCsvReadField(string Name, FerruleScalarType Type);

/// <summary>Positional CSV input dialect and physically present empty-text policy.</summary>
public sealed record FerruleCsvReadOptions
{
    public char? Delimiter { get; init; }
    public char? Quote { get; init; }
    public bool QuoteDisabled { get; init; }
    public bool HasHeaders { get; init; } = true;
    public bool PreserveEmptyStrings { get; init; }
}

/// <summary>Typed failures for the bounded flat CSV input boundary.</summary>
public enum FerruleCsvInputError
{
    UnsupportedSchema,
    BadDelimiter,
    BadQuote,
    DelimiterQuoteConflict,
    ConflictingQuoteSettings,
    Encoding,
    ColumnCount,
    Parse,
    InputTooLarge,
    ValueTooLarge,
    TooManyRecords,
    TooManyColumns,
    NodeLimit,
}

/// <summary>A CSV input failure with zero-based data-row, logical-record and column locations.</summary>
public sealed class FerruleCsvInputException : Exception
{
    internal FerruleCsvInputException(
        FerruleCsvInputError error,
        string message,
        int? row = null,
        int? record = null,
        int? column = null,
        string? field = null,
        FerruleScalarType? expectedType = null,
        string? cell = null,
        char? character = null,
        long? maximum = null,
        long? observed = null,
        int? expectedColumns = null,
        int? actualColumns = null)
        : base(message)
    {
        Error = error;
        Row = row;
        Record = record;
        Column = column;
        Field = field;
        ExpectedType = expectedType;
        Cell = cell;
        Character = character;
        Maximum = maximum;
        Observed = observed;
        ExpectedColumns = expectedColumns;
        ActualColumns = actualColumns;
    }

    public FerruleCsvInputError Error { get; }
    public int? Row { get; }
    public int? Record { get; }
    public int? Column { get; }
    public string? Field { get; }
    public FerruleScalarType? ExpectedType { get; }
    public string? Cell { get; }
    public char? Character { get; }
    public long? Maximum { get; }
    public long? Observed { get; }
    public int? ExpectedColumns { get; }
    public int? ActualColumns { get; }
}

/// <summary>
/// Package-free positional CSV input. Parses the complete document to an ordered
/// repeated collection before returning; any failure returns no partial source.
/// The limits bound admitted input and logical values, not total process memory.
/// </summary>
public static class FerruleCsvInput
{
    public const int MaximumInputBytes = 8 * 1024 * 1024;
    public const int MaximumCellBytes = 1024 * 1024;
    public const int MaximumRecords = 100_000;
    public const int MaximumColumns = 256;
    public const int MaximumNodes = 1_000_000;

    private static readonly UTF8Encoding StrictUtf8 = new(false, true);

    /// <summary>Read well-formed Unicode text with positional fields and an optional skipped header.</summary>
    public static FerruleInstance Parse(
        string source,
        IReadOnlyList<FerruleCsvReadField> fields,
        FerruleCsvReadOptions? options = null)
    {
        ArgumentNullException.ThrowIfNull(source);
        ArgumentNullException.ThrowIfNull(fields);
        var ordered = ValidateFields(fields);
        options ??= new FerruleCsvReadOptions();
        var (delimiter, quote) = ValidateDialect(options);
        // Validate the entire UTF-16 string before enforcing the physical byte
        // bound, without allocating an encoded copy or truncating a scalar.
        var byteCount = CountUnicodeBytes(source);
        RequireInputSize(byteCount);
        return new Reader(source, ordered, options, delimiter, quote).Read();
    }

    /// <summary>Read strict UTF-8 bytes; the physical byte bound precedes decoding.</summary>
    public static FerruleInstance ParseBytes(
        byte[] source,
        IReadOnlyList<FerruleCsvReadField> fields,
        FerruleCsvReadOptions? options = null)
    {
        ArgumentNullException.ThrowIfNull(source);
        ArgumentNullException.ThrowIfNull(fields);
        var ordered = ValidateFields(fields);
        options ??= new FerruleCsvReadOptions();
        var (delimiter, quote) = ValidateDialect(options);
        RequireInputSize(source.LongLength);
        string text;
        try
        {
            text = StrictUtf8.GetString(source);
        }
        catch (DecoderFallbackException)
        {
            throw new FerruleCsvInputException(
                FerruleCsvInputError.Encoding,
                "CSV input bytes must be valid UTF-8.");
        }
        return new Reader(text, ordered, options, delimiter, quote).Read();
    }

    private static FerruleCsvReadField[] ValidateFields(IReadOnlyList<FerruleCsvReadField> fields)
    {
        if (fields.Count > MaximumColumns)
        {
            throw new FerruleCsvInputException(
                FerruleCsvInputError.UnsupportedSchema,
                $"CSV input schema exceeds {MaximumColumns} fields.",
                maximum: MaximumColumns, observed: fields.Count);
        }
        var ordered = new FerruleCsvReadField[fields.Count];
        var names = new HashSet<string>(StringComparer.Ordinal);
        for (var column = 0; column < ordered.Length; column++)
        {
            var field = fields[column];
            if (field is null)
            {
                throw new FerruleCsvInputException(
                    FerruleCsvInputError.UnsupportedSchema,
                    $"CSV read field at index {column} cannot be null.", column: column);
            }
            if (string.IsNullOrEmpty(field.Name) || !FerruleUnicode.IsWellFormed(field.Name))
            {
                throw new FerruleCsvInputException(
                    FerruleCsvInputError.UnsupportedSchema,
                    $"CSV read field at index {column} must have a nonempty well-formed Unicode name.",
                    column: column);
            }
            if (!names.Add(field.Name))
            {
                throw new FerruleCsvInputException(
                    FerruleCsvInputError.UnsupportedSchema,
                    $"CSV read field name '{field.Name}' is duplicated.",
                    column: column, field: field.Name);
            }
            if (field.Type is not (FerruleScalarType.String or FerruleScalarType.Int64 or
                FerruleScalarType.Double or FerruleScalarType.Bool))
            {
                throw new FerruleCsvInputException(
                    FerruleCsvInputError.UnsupportedSchema,
                    $"CSV read field '{field.Name}' has an unsupported scalar type.",
                    column: column, field: field.Name, expectedType: field.Type);
            }
            ordered[column] = field;
        }
        return ordered;
    }

    private static (char Delimiter, char? Quote) ValidateDialect(FerruleCsvReadOptions options)
    {
        var delimiter = options.Delimiter ?? ',';
        if (delimiter > '\x7f' || delimiter is '\0' or '\r' or '\n')
        {
            throw new FerruleCsvInputException(
                FerruleCsvInputError.BadDelimiter,
                $"`{delimiter}` is not a valid CSV delimiter (must be a single-byte character)",
                character: delimiter);
        }
        if (options.QuoteDisabled)
        {
            if (options.Quote.HasValue)
            {
                throw new FerruleCsvInputException(
                    FerruleCsvInputError.ConflictingQuoteSettings,
                    "CSV quote character cannot be set while quoting is disabled");
            }
            return (delimiter, null);
        }
        var quote = options.Quote ?? '"';
        if (quote is < '\x21' or > '\x7e')
        {
            throw new FerruleCsvInputException(
                FerruleCsvInputError.BadQuote,
                $"`{quote}` is not a valid CSV quote (must be a printable ASCII character)",
                character: quote);
        }
        if (delimiter == quote)
        {
            throw new FerruleCsvInputException(
                FerruleCsvInputError.DelimiterQuoteConflict,
                "CSV delimiter and quote must be different characters");
        }
        return (delimiter, quote);
    }

    private static long CountUnicodeBytes(string source)
    {
        long bytes = 0;
        for (var index = 0; index < source.Length; index++)
        {
            var character = source[index];
            if (char.IsHighSurrogate(character))
            {
                if (index + 1 >= source.Length || !char.IsLowSurrogate(source[index + 1]))
                {
                    throw InvalidUnicode();
                }
                index++;
                bytes += 4;
            }
            else if (char.IsLowSurrogate(character))
            {
                throw InvalidUnicode();
            }
            else
            {
                bytes += character < '\x80' ? 1 : character < '\x800' ? 2 : 3;
            }
        }
        return bytes;
    }

    private static FerruleCsvInputException InvalidUnicode() => new(
        FerruleCsvInputError.Encoding, "CSV input must contain valid Unicode.");

    private static void RequireInputSize(long observed)
    {
        if (observed > MaximumInputBytes)
        {
            throw new FerruleCsvInputException(
                FerruleCsvInputError.InputTooLarge,
                $"CSV input exceeds {MaximumInputBytes} UTF-8 bytes (observed {observed}).",
                maximum: MaximumInputBytes, observed: observed);
        }
    }

    private enum FieldState { Start, Unquoted, Quoted, AfterQuote }

    private sealed class Reader
    {
        private readonly string _source;
        private readonly FerruleCsvReadField[] _fields;
        private readonly FerruleCsvReadOptions _options;
        private readonly char _delimiter;
        private readonly char? _quote;
        private readonly List<FerruleInstance> _rows = new();
        private readonly List<string> _cells = new();
        private readonly StringBuilder _cell = new();
        private FieldState _state;
        private bool _recordPresent;
        private int _cellBytes;
        private int _record;
        private long _nodes = 1;

        internal Reader(string source, FerruleCsvReadField[] fields,
            FerruleCsvReadOptions options, char delimiter, char? quote)
        {
            _source = source;
            _fields = fields;
            _options = options;
            _delimiter = delimiter;
            _quote = quote;
        }

        private int? Row => _options.HasHeaders && _record == 0 ? null :
            _record - (_options.HasHeaders ? 1 : 0);

        internal FerruleInstance Read()
        {
            // The physical BOM was counted during admission; only the first
            // leading UFEFF is stripped. Subsequent occurrences remain data.
            var index = _source.Length > 0 && _source[0] == '\uFEFF' ? 1 : 0;
            for (; index < _source.Length; index++)
            {
                var character = _source[index];
                if (_state == FieldState.Quoted)
                {
                    if (character == _quote)
                    {
                        if (index + 1 < _source.Length && _source[index + 1] == _quote)
                        {
                            AppendScalar(ref index);
                            index++;
                        }
                        else
                        {
                            _state = FieldState.AfterQuote;
                        }
                    }
                    else
                    {
                        AppendScalar(ref index);
                    }
                    continue;
                }
                if (character == _delimiter)
                {
                    _recordPresent = true;
                    FinishCell();
                    if (_cells.Count >= MaximumColumns)
                    {
                        throw new FerruleCsvInputException(
                            FerruleCsvInputError.TooManyColumns,
                            $"record {_record}: CSV column count exceeds {MaximumColumns} (observed {MaximumColumns + 1}).",
                            row: Row, record: _record, column: MaximumColumns,
                            maximum: MaximumColumns, observed: MaximumColumns + 1);
                    }
                    continue;
                }
                if (character is '\r' or '\n')
                {
                    if (_recordPresent)
                    {
                        FinishRecord();
                    }
                    if (character == '\r' && index + 1 < _source.Length && _source[index + 1] == '\n')
                    {
                        index++;
                    }
                    continue;
                }
                _recordPresent = true;
                if (_state == FieldState.Start && character == _quote)
                {
                    _state = FieldState.Quoted;
                }
                else
                {
                    // A quote in an unquoted field is literal. Native parsing
                    // also keeps an unquoted tail after a closing quote.
                    _state = FieldState.Unquoted;
                    AppendScalar(ref index);
                }
            }
            if (_recordPresent)
            {
                // EOF within quotes is a native permissive record ending.
                FinishRecord();
            }
            return new FerruleRepeated(_rows);
        }

        private void AppendScalar(ref int index)
        {
            var character = _source[index];
            var units = char.IsHighSurrogate(character) ? 2 : 1;
            var bytes = units == 2 ? 4 : character < '\x80' ? 1 : character < '\x800' ? 2 : 3;
            var observed = _cellBytes + bytes;
            if (observed > MaximumCellBytes)
            {
                throw new FerruleCsvInputException(
                    FerruleCsvInputError.ValueTooLarge,
                    $"record {_record}, column {_cells.Count}: decoded CSV cell exceeds {MaximumCellBytes} UTF-8 bytes (observed {observed}).",
                    row: Row, record: _record, column: _cells.Count,
                    maximum: MaximumCellBytes, observed: observed);
            }
            _cellBytes = observed;
            _cell.Append(character);
            if (units == 2)
            {
                _cell.Append(_source[++index]);
            }
        }

        private void FinishCell()
        {
            _cells.Add(_cell.ToString());
            _cell.Clear();
            _cellBytes = 0;
            _state = FieldState.Start;
        }

        private void FinishRecord()
        {
            FinishCell();
            var observedRecords = _record + 1;
            if (observedRecords > MaximumRecords)
            {
                throw new FerruleCsvInputException(
                    FerruleCsvInputError.TooManyRecords,
                    $"CSV logical record count exceeds {MaximumRecords} (observed {observedRecords}).",
                    row: Row, record: _record, maximum: MaximumRecords, observed: observedRecords);
            }
            if (!_options.HasHeaders || _record != 0)
            {
                MaterializeRow();
            }
            _record = observedRecords;
            _cells.Clear();
            _recordPresent = false;
        }

        private void MaterializeRow()
        {
            var row = Row!.Value;
            if (_cells.Count > _fields.Length)
            {
                throw new FerruleCsvInputException(
                    FerruleCsvInputError.ColumnCount,
                    $"row {row}: expected {_fields.Length} column(s), got {_cells.Count}",
                    row: row, record: _record,
                    expectedColumns: _fields.Length, actualColumns: _cells.Count);
            }
            var observedNodes = _nodes + 1 + _fields.Length;
            if (observedNodes > MaximumNodes)
            {
                throw new FerruleCsvInputException(
                    FerruleCsvInputError.NodeLimit,
                    $"CSV instance node count exceeds {MaximumNodes} (observed {observedNodes}).",
                    row: row, record: _record, maximum: MaximumNodes, observed: observedNodes);
            }
            _nodes = observedNodes;
            var values = new FerruleField[_fields.Length];
            for (var column = 0; column < _fields.Length; column++)
            {
                var field = _fields[column];
                FerruleValue value;
                if (column >= _cells.Count)
                {
                    value = FerruleValue.Null;
                }
                else if (_cells[column].Length == 0)
                {
                    value = field.Type == FerruleScalarType.String && _options.PreserveEmptyStrings
                        ? FerruleValue.FromString(string.Empty) : FerruleValue.Null;
                }
                else
                {
                    value = ParseCell(field, _cells[column], row, _record, column);
                }
                values[column] = new FerruleField(field.Name, new FerruleScalar(value));
            }
            _rows.Add(new FerruleGroup(values));
        }
    }

    private static FerruleValue ParseCell(
        FerruleCsvReadField field, string cell, int row, int record, int column)
    {
        switch (field.Type)
        {
            case FerruleScalarType.String:
                return FerruleValue.FromString(cell);
            case FerruleScalarType.Int64:
                if (IsIntegerLexical(cell) && long.TryParse(cell,
                    NumberStyles.AllowLeadingSign, CultureInfo.InvariantCulture, out var integer))
                {
                    return FerruleValue.FromInt64(integer);
                }
                break;
            case FerruleScalarType.Double:
                if (IsFloatLexical(cell) && double.TryParse(cell,
                    NumberStyles.AllowLeadingSign | NumberStyles.AllowDecimalPoint | NumberStyles.AllowExponent,
                    CultureInfo.InvariantCulture, out var number) && double.IsFinite(number))
                {
                    if (number == 0 && cell[0] == '-')
                    {
                        number = BitConverter.Int64BitsToDouble(long.MinValue);
                    }
                    return FerruleValue.FromDouble(number);
                }
                break;
            case FerruleScalarType.Bool:
                var trimmed = TrimBooleanWhitespace(cell);
                if (trimmed.SequenceEqual("true".AsSpan()) || trimmed.SequenceEqual("1".AsSpan()))
                {
                    return FerruleValue.FromBoolean(true);
                }
                if (trimmed.SequenceEqual("false".AsSpan()) || trimmed.SequenceEqual("0".AsSpan()))
                {
                    return FerruleValue.FromBoolean(false);
                }
                break;
        }
        var typeName = field.Type switch
        {
            FerruleScalarType.Int64 => "Int",
            FerruleScalarType.Double => "Float",
            FerruleScalarType.Bool => "Bool",
            _ => "String",
        };
        throw new FerruleCsvInputException(
            FerruleCsvInputError.Parse,
            $"row {row}: column `{field.Name}` expected {typeName}, got `{cell}`",
            row: row, record: record, column: column, field: field.Name,
            expectedType: field.Type, cell: cell);
    }

    private static bool IsIntegerLexical(string value)
    {
        var index = value[0] is '+' or '-' ? 1 : 0;
        var start = index;
        for (; index < value.Length; index++)
        {
            if (value[index] is < '0' or > '9')
            {
                return false;
            }
        }
        return index > start;
    }

    private static bool IsFloatLexical(string value)
    {
        var index = value[0] is '+' or '-' ? 1 : 0;
        var digits = 0;
        while (index < value.Length && value[index] is >= '0' and <= '9')
        {
            index++;
            digits++;
        }
        if (index < value.Length && value[index] == '.')
        {
            index++;
            while (index < value.Length && value[index] is >= '0' and <= '9')
            {
                index++;
                digits++;
            }
        }
        if (digits == 0)
        {
            return false;
        }
        if (index < value.Length && value[index] is 'e' or 'E')
        {
            index++;
            if (index < value.Length && value[index] is '+' or '-')
            {
                index++;
            }
            var exponentStart = index;
            while (index < value.Length && value[index] is >= '0' and <= '9')
            {
                index++;
            }
            if (index == exponentStart)
            {
                return false;
            }
        }
        return index == value.Length;
    }

    private static ReadOnlySpan<char> TrimBooleanWhitespace(string value)
    {
        var start = 0;
        var end = value.Length;
        while (start < end && IsBooleanWhitespace(value[start]))
        {
            start++;
        }
        while (end > start && IsBooleanWhitespace(value[end - 1]))
        {
            end--;
        }
        return value.AsSpan(start, end - start);
    }

    private static bool IsBooleanWhitespace(char character) => character is
        >= '\u0009' and <= '\u000D' or '\u0020' or '\u0085' or '\u00A0' or
        '\u1680' or >= '\u2000' and <= '\u200A' or '\u2028' or '\u2029' or
        '\u202F' or '\u205F' or '\u3000';
}
