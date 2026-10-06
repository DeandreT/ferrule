using System.Globalization;
using System.Numerics;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>One ordered scalar column in a statically selected flat CSV boundary.</summary>
public sealed record FerruleCsvField(string Name, FerruleScalarType Type);

/// <summary>Static CSV output policy. Records use LF and quotes are doubled.</summary>
public sealed record FerruleCsvWriteOptions
{
    public char? Delimiter { get; init; }
    public char? Quote { get; init; }
    public bool QuoteDisabled { get; init; }
    public bool HasHeaders { get; init; } = true;
    public bool Utf8Bom { get; init; }
}

/// <summary>Typed failures for the bounded flat CSV output boundary.</summary>
public enum FerruleCsvError
{
    RootShape,
    UnsupportedSchema,
    BadDelimiter,
    BadQuote,
    DelimiterQuoteConflict,
    ConflictingQuoteSettings,
    RowShape,
    MissingField,
    UnexpectedField,
    DuplicateField,
    ValueType,
    UnquotedFieldBoundary,
    UnquotedHeaderBoundary,
    UnquotedSingleEmptyRow,
    OutputTooLarge,
}

/// <summary>A CSV failure with zero-based row and exact field identity when applicable.</summary>
public sealed class FerruleCsvException : Exception
{
    internal FerruleCsvException(
        FerruleCsvError error,
        string message,
        int? row = null,
        string? field = null,
        FerruleScalarType? expected = null,
        string? got = null,
        char? character = null,
        int? maximum = null)
        : base(message)
    {
        Error = error;
        Row = row;
        Field = field;
        Expected = expected;
        Got = got;
        Character = character;
        Maximum = maximum;
    }

    public FerruleCsvError Error { get; }
    public int? Row { get; }
    public string? Field { get; }
    public FerruleScalarType? Expected { get; }
    public string? Got { get; }
    public char? Character { get; }
    public int? Maximum { get; }
}

/// <summary>
/// Package-free flat CSV output. The 64 MiB limit covers returned UTF-8 bytes;
/// it does not bound the mapped instance or validated record storage.
/// </summary>
public static class FerruleCsv
{
    public const int MaximumOutputBytes = 64 * 1024 * 1024;

    /// <summary>
    /// Validate every row before writing output. The primary instance must be a
    /// repeated collection of groups, with one scalar value per declared field.
    /// No buffer is returned on any shape, type, dialect or resource failure.
    /// </summary>
    public static byte[] SerializeBytes(
        FerruleInstance primary,
        IReadOnlyList<FerruleCsvField> fields,
        FerruleCsvWriteOptions? options = null)
    {
        using var sink = EncodeValidatedDocument(primary, fields, options);
        return sink.ToArray();
    }

    /// <summary>Return the same bounded UTF-8 document as Unicode text.</summary>
    public static string Serialize(
        FerruleInstance primary,
        IReadOnlyList<FerruleCsvField> fields,
        FerruleCsvWriteOptions? options = null)
    {
        using var sink = EncodeValidatedDocument(primary, fields, options);
        return sink.ToText();
    }

    private static BoundedUtf8Sink EncodeValidatedDocument(
        FerruleInstance primary,
        IReadOnlyList<FerruleCsvField> fields,
        FerruleCsvWriteOptions? options)
    {
        ArgumentNullException.ThrowIfNull(primary);
        ArgumentNullException.ThrowIfNull(fields);
        if (primary is not FerruleRepeated rows)
        {
            throw new FerruleCsvException(
                FerruleCsvError.RootShape,
                "CSV primary output must be a repeated collection.",
                got: InstanceTypeName(primary));
        }
        var orderedFields = ValidateFields(fields);
        options ??= new FerruleCsvWriteOptions();
        var (delimiter, quote) = ValidateDialect(options);
        var records = new List<string[]>(rows.Items.Count);
        for (var row = 0; row < rows.Items.Count; row++)
        {
            records.Add(FormatRow(row, rows.Items[row], orderedFields));
        }
        // Native CSV validation gives every row/type failure precedence over
        // no-quote header/data refusals and serialization/resource failures.
        if (options.QuoteDisabled)
        {
            foreach (var field in orderedFields)
            {
                if (options.HasHeaders && RequiresBoundaryQuoting(field.Name, delimiter))
                {
                    throw new FerruleCsvException(
                        FerruleCsvError.UnquotedHeaderBoundary,
                        $"Header '{field.Name}' cannot be written without quoting.",
                        field: field.Name);
                }
            }
            for (var row = 0; row < records.Count; row++)
            {
                var record = records[row];
                if (record.Length == 1 && record[0].Length == 0)
                {
                    throw new FerruleCsvException(
                        FerruleCsvError.UnquotedSingleEmptyRow,
                        $"Row {row}: a single empty field requires quoting.",
                        row: row);
                }
                for (var column = 0; column < record.Length; column++)
                {
                    if (RequiresBoundaryQuoting(record[column], delimiter))
                    {
                        throw new FerruleCsvException(
                            FerruleCsvError.UnquotedFieldBoundary,
                            $"Row {row}: column '{orderedFields[column].Name}' requires quoting.",
                            row: row,
                            field: orderedFields[column].Name);
                    }
                }
            }
        }
        var sink = new BoundedUtf8Sink();
        try
        {
            if (options.Utf8Bom)
            {
                sink.Write([0xef, 0xbb, 0xbf]);
            }
            if (options.HasHeaders)
            {
                WriteRecord(sink, orderedFields.Select(field => field.Name).ToArray(),
                    delimiter, quote, options.QuoteDisabled);
            }
            foreach (var record in records)
            {
                WriteRecord(sink, record, delimiter, quote, options.QuoteDisabled);
            }
            return sink;
        }
        catch
        {
            sink.Dispose();
            throw;
        }
    }

    private static FerruleCsvField[] ValidateFields(IReadOnlyList<FerruleCsvField> fields)
    {
        var ordered = new FerruleCsvField[fields.Count];
        var names = new HashSet<string>(StringComparer.Ordinal);
        for (var column = 0; column < fields.Count; column++)
        {
            var field = fields[column];
            if (field is null || field.Name is null ||
                !FerruleUnicode.IsWellFormed(field.Name) ||
                !Enum.IsDefined(field.Type) || !names.Add(field.Name))
            {
                throw new FerruleCsvException(
                    FerruleCsvError.UnsupportedSchema,
                    "CSV requires ordered unique scalar field descriptors.",
                    field: field?.Name);
            }
            ordered[column] = field;
        }
        return ordered;
    }

    private static (byte Delimiter, byte Quote) ValidateDialect(FerruleCsvWriteOptions options)
    {
        var delimiter = options.Delimiter ?? ',';
        if (delimiter > 0x7f || delimiter is '\0' or '\r' or '\n')
        {
            throw new FerruleCsvException(
                FerruleCsvError.BadDelimiter,
                "CSV delimiter must be an ASCII character other than NUL, CR or LF.",
                character: delimiter);
        }
        if (options.QuoteDisabled)
        {
            if (options.Quote.HasValue)
            {
                throw new FerruleCsvException(
                    FerruleCsvError.ConflictingQuoteSettings,
                    "CSV quote cannot be set while quoting is disabled.");
            }
            return ((byte)delimiter, (byte)'"');
        }
        var quote = options.Quote ?? '"';
        if (quote is < '!' or > '~')
        {
            throw new FerruleCsvException(
                FerruleCsvError.BadQuote,
                "CSV quote must be a printable ASCII character.",
                character: quote);
        }
        if (delimiter == quote)
        {
            throw new FerruleCsvException(
                FerruleCsvError.DelimiterQuoteConflict,
                "CSV delimiter and quote must be different characters.");
        }
        return ((byte)delimiter, (byte)quote);
    }

    private static string[] FormatRow(
        int row,
        FerruleInstance instance,
        IReadOnlyList<FerruleCsvField> fields)
    {
        if (instance is not FerruleGroup group)
        {
            throw new FerruleCsvException(
                FerruleCsvError.RowShape,
                $"Row {row}: expected a group.",
                row: row,
                got: InstanceTypeName(instance));
        }
        var names = new HashSet<string>(StringComparer.Ordinal);
        foreach (var field in group.Fields)
        {
            if (!fields.Any(expected => string.Equals(expected.Name, field.Name, StringComparison.Ordinal)))
            {
                throw new FerruleCsvException(
                    FerruleCsvError.UnexpectedField,
                    $"Row {row}: unexpected column '{field.Name}'.",
                    row: row,
                    field: field.Name);
            }
            if (!names.Add(field.Name))
            {
                throw new FerruleCsvException(
                    FerruleCsvError.DuplicateField,
                    $"Row {row}: duplicate column '{field.Name}'.",
                    row: row,
                    field: field.Name);
            }
        }
        var record = new string[fields.Count];
        for (var column = 0; column < fields.Count; column++)
        {
            var field = fields[column];
            if (!group.TryGetField(field.Name, out var value))
            {
                throw new FerruleCsvException(
                    FerruleCsvError.MissingField,
                    $"Row {row}: missing column '{field.Name}'.",
                    row: row,
                    field: field.Name);
            }
            if (value is not FerruleScalar scalar)
            {
                throw ValueType(row, field, InstanceTypeName(value));
            }
            record[column] = FormatValue(row, field, scalar.Value);
        }
        return record;
    }

    private static string FormatValue(int row, FerruleCsvField field, FerruleValue value)
    {
        if (value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
        {
            return string.Empty;
        }
        if (value.Kind == FerruleValueKind.Double && !double.IsFinite(value.DoubleValue) &&
            field.Type is FerruleScalarType.String or FerruleScalarType.Double)
        {
            throw ValueType(row, field, "non-finite float");
        }
        if (field.Type == FerruleScalarType.Double && value.Kind == FerruleValueKind.Int64)
        {
            var integer = value.Int64Value;
            var magnitude = integer < 0 ? unchecked(0UL - (ulong)integer) : (ulong)integer;
            var significantBits = magnitude == 0 ? 0 :
                64 - BitOperations.LeadingZeroCount(magnitude) - BitOperations.TrailingZeroCount(magnitude);
            if (significantBits > 53)
            {
                throw ValueType(row, field, "int outside the exact f64 range");
            }
            // Native CSV retains integer lexical output after the exactness check.
            return integer.ToString(CultureInfo.InvariantCulture);
        }
        if (field.Type == FerruleScalarType.Int64 && value.Kind == FerruleValueKind.Double)
        {
            throw ValueType(row, field, "float");
        }
        if (value.Kind == FerruleValueKind.String)
        {
            var lexical = value.StringValue.AsSpan().Trim();
            if (field.Type == FerruleScalarType.Int64 && !IsNumericLexical(lexical, integerOnly: true) ||
                field.Type == FerruleScalarType.Double && !IsNumericLexical(lexical, integerOnly: false))
            {
                throw ValueType(row, field, "string");
            }
        }
        if (!FerruleValueMaps.TryCoerce(value, field.Type, out var coerced))
        {
            throw ValueType(row, field, ValueTypeName(value.Kind));
        }
        return coerced.Kind switch
        {
            FerruleValueKind.String => coerced.StringValue,
            FerruleValueKind.Int64 => coerced.Int64Value.ToString(CultureInfo.InvariantCulture),
            FerruleValueKind.Double when double.IsFinite(coerced.DoubleValue) =>
                FerruleValueMaps.RustFloatText(coerced.DoubleValue),
            FerruleValueKind.Bool => coerced.BooleanValue ? "true" : "false",
            _ => throw ValueType(row, field, ValueTypeName(value.Kind)),
        };
    }

    // Guard the shared .NET coercer against host-only accepted numeric suffixes.
    // CSV integer coercion accepts signed ASCII digits; finite decimal float
    // coercion additionally accepts a decimal point and signed decimal exponent.
    private static bool IsNumericLexical(ReadOnlySpan<char> text, bool integerOnly)
    {
        var index = 0;
        if (index < text.Length && text[index] is '+' or '-')
        {
            index++;
        }
        var digits = 0;
        while (index < text.Length && text[index] is >= '0' and <= '9')
        {
            index++;
            digits++;
        }
        if (!integerOnly && index < text.Length && text[index] == '.')
        {
            index++;
            while (index < text.Length && text[index] is >= '0' and <= '9')
            {
                index++;
                digits++;
            }
        }
        if (digits == 0)
        {
            return false;
        }
        if (!integerOnly && index < text.Length && text[index] is 'e' or 'E')
        {
            index++;
            if (index < text.Length && text[index] is '+' or '-')
            {
                index++;
            }
            var exponentStart = index;
            while (index < text.Length && text[index] is >= '0' and <= '9')
            {
                index++;
            }
            if (index == exponentStart)
            {
                return false;
            }
        }
        return index == text.Length;
    }

    private static FerruleCsvException ValueType(int row, FerruleCsvField field, string got) =>
        new(FerruleCsvError.ValueType,
            $"Row {row}: column '{field.Name}' expected {field.Type}, got {got}.",
            row: row, field: field.Name, expected: field.Type, got: got);

    private static string ValueTypeName(FerruleValueKind kind) => kind switch
    {
        FerruleValueKind.Null => "null",
        FerruleValueKind.JsonNull => "json null",
        FerruleValueKind.XmlNil => "xml nil",
        FerruleValueKind.Bool => "bool",
        FerruleValueKind.Int64 => "int",
        FerruleValueKind.Double => "float",
        FerruleValueKind.String => "string",
        _ => throw new ArgumentOutOfRangeException(nameof(kind)),
    };

    private static string InstanceTypeName(FerruleInstance instance) => instance switch
    {
        FerruleScalar scalar => ValueTypeName(scalar.Value.Kind),
        FerruleGroup => "group",
        FerruleRepeated => "repeated",
        FerruleMappedSequence => "mapped sequence",
        FerruleDocumentSet => "document set",
        _ => throw new ArgumentOutOfRangeException(nameof(instance)),
    };

    private static bool RequiresBoundaryQuoting(string value, byte delimiter) =>
        value.Contains((char)delimiter) || value.IndexOfAny(['\r', '\n']) >= 0;

    private static void WriteRecord(
        BoundedUtf8Sink sink,
        IReadOnlyList<string> record,
        byte delimiter,
        byte quote,
        bool quoteDisabled)
    {
        // The native writer emits an explicit empty record even with zero
        // columns or one empty header, including when quoting is disabled.
        if (record.Count == 0 || record.Count == 1 && record[0].Length == 0)
        {
            sink.WriteByte(quote);
            sink.WriteByte(quote);
        }
        else
        {
            for (var column = 0; column < record.Count; column++)
            {
                if (column > 0)
                {
                    sink.WriteByte(delimiter);
                }
                var value = record[column];
                var quoted = !quoteDisabled &&
                    (RequiresBoundaryQuoting(value, delimiter) || value.Contains((char)quote));
                if (quoted)
                {
                    sink.WriteByte(quote);
                    sink.WriteText(value, quote);
                    sink.WriteByte(quote);
                }
                else
                {
                    sink.WriteText(value);
                }
            }
        }
        sink.WriteByte((byte)'\n');
    }

    private sealed class BoundedUtf8Sink : IDisposable
    {
        private static readonly UTF8Encoding Utf8 = new(false, true);
        private readonly MemoryStream _bytes = new();

        internal void Write(ReadOnlySpan<byte> bytes)
        {
            if (bytes.Length > MaximumOutputBytes - _bytes.Length)
            {
                throw new FerruleCsvException(
                    FerruleCsvError.OutputTooLarge,
                    "CSV output exceeds the 64 MiB UTF-8 byte limit.",
                    maximum: MaximumOutputBytes);
            }
            _bytes.Write(bytes);
        }

        internal void WriteByte(byte value)
        {
            Span<byte> bytes = stackalloc byte[1];
            bytes[0] = value;
            Write(bytes);
        }

        internal void WriteText(string value, byte? escapedQuote = null)
        {
            var remaining = value.AsSpan();
            var encoder = Utf8.GetEncoder();
            Span<byte> encoded = stackalloc byte[4096];
            Span<byte> escaped = stackalloc byte[8192];
            while (!remaining.IsEmpty)
            {
                encoder.Convert(remaining, encoded, flush: true,
                    out var charactersUsed, out var bytesUsed, out _);
                if (!escapedQuote.HasValue)
                {
                    Write(encoded[..bytesUsed]);
                }
                else
                {
                    var length = 0;
                    foreach (var valueByte in encoded[..bytesUsed])
                    {
                        escaped[length++] = valueByte;
                        if (valueByte == escapedQuote.Value)
                        {
                            escaped[length++] = valueByte;
                        }
                    }
                    Write(escaped[..length]);
                }
                remaining = remaining[charactersUsed..];
            }
        }

        internal byte[] ToArray() => _bytes.ToArray();

        internal string ToText() => Encoding.UTF8.GetString(
            _bytes.GetBuffer().AsSpan(0, checked((int)_bytes.Length)));

        public void Dispose() => _bytes.Dispose();
    }
}
