using System.Text;

namespace Ferrule.Runtime;

public enum FerruleFixedWidthError { Layout, Encoding, PartialRecord, RecordOverflow, FieldOverflow, InputTooLarge, OutputTooLarge, RootShape }

public sealed class FerruleFixedWidthException(FerruleFixedWidthError error, long expected = 0,
    long actual = 0, int? record = null, string? field = null, Exception? source = null)
    : Exception($"fixed-width {error}: expected {expected}, got {actual}", source)
{
    public FerruleFixedWidthError Error { get; } = error;
    public long Expected { get; } = expected;
    public long Actual { get; } = actual;
    public int? Record { get; } = record;
    public string? Field { get; } = field;
}

/// <summary>Validated Unicode-scalar fixed-width layout. Output delimiters are LF.</summary>
public sealed class FerruleFixedWidthLayout
{
    public IReadOnlyList<long> Widths { get; }
    public Rune Fill { get; }
    public bool RecordDelimiters { get; }
    public bool EmptyAsAbsent { get; }
    public long RecordWidth { get; }
    public FerruleFixedWidthLayout(long[] widths, int fill, bool recordDelimiters, bool emptyAsAbsent)
    {
        if (widths.Length == 0 || widths.Any(w => w <= 0 || w > uint.MaxValue) || !Rune.IsValid(fill) || fill is 10 or 13)
            throw new FerruleFixedWidthException(FerruleFixedWidthError.Layout);
        long total = widths.Sum(w => (long)w);
        Widths = Array.AsReadOnly((long[])widths.Clone());
        Fill = new Rune(fill); RecordWidth = total;
        RecordDelimiters = recordDelimiters; EmptyAsAbsent = emptyAsAbsent;
    }
}

public sealed class FerruleFixedWidthInputBudget
{
    public const long MaximumDocumentBytes = 64L * 1024 * 1024;
    public const long MaximumTotalBytes = 256L * 1024 * 1024;
    private long total;
    public void Charge(int bytes)
    {
        ArgumentOutOfRangeException.ThrowIfNegative(bytes);
        if (bytes > MaximumDocumentBytes)
            throw new FerruleFixedWidthException(FerruleFixedWidthError.InputTooLarge, MaximumDocumentBytes, bytes);
        long next = total + bytes;
        if (next > MaximumTotalBytes)
            throw new FerruleFixedWidthException(FerruleFixedWidthError.InputTooLarge, MaximumTotalBytes, next);
        total = next;
    }
}

public static class FerruleFixedWidth
{
    public const int MaximumOutputBytes = 64 * 1024 * 1024;
    private static readonly UTF8Encoding Utf8 = new(false, true);

    public static FerruleRepeated ParseBytes(byte[] bytes, IReadOnlyList<FerruleCsvReadField> fields,
        FerruleFixedWidthLayout layout, string path, string? resolvedPath = null)
    {
        if (fields.Count != layout.Widths.Count)
            throw new FerruleFixedWidthException(FerruleFixedWidthError.Layout, fields.Count, layout.Widths.Count);
        if (bytes.Length > FerruleFixedWidthInputBudget.MaximumDocumentBytes)
            throw new FerruleFixedWidthException(FerruleFixedWidthError.InputTooLarge, FerruleFixedWidthInputBudget.MaximumDocumentBytes, bytes.Length);
        FerruleCsvSourceDocument.ValidatePath(path, nameof(path));
        if (resolvedPath is not null) FerruleCsvSourceDocument.ValidatePath(resolvedPath, nameof(resolvedPath));
        string text;
        try { text = Utf8.GetString(bytes); }
        catch (DecoderFallbackException error) { throw new FerruleFixedWidthException(FerruleFixedWidthError.Encoding, source: error); }
        if (text.StartsWith('\uFEFF')) text = text[1..];
        var rows = new List<FerruleInstance>();
        var origin = new FerruleSourceDocumentOrigin(path, resolvedPath);
        void Record(Rune[] runes)
        {
            int record = rows.Count;
            if (runes.Length != layout.RecordWidth)
                throw new FerruleFixedWidthException(runes.Length < layout.RecordWidth ? FerruleFixedWidthError.PartialRecord : FerruleFixedWidthError.RecordOverflow,
                    layout.RecordWidth, runes.Length, record);
            var values = new FerruleField[fields.Count];
            int offset = 0;
            for (int column = 0; column < fields.Count; column++)
            {
                int end = offset + checked((int)layout.Widths[column]), trimmed = end;
                while (trimmed > offset && runes[trimmed - 1] == layout.Fill) trimmed--;
                var cell = new StringBuilder();
                for (int index = offset; index < trimmed; index++) cell.Append(runes[index].ToString());
                string value = cell.ToString();
                var scalar = value.Length == 0 && layout.EmptyAsAbsent ? FerruleValue.Null
                    : FerruleCsvInput.ParseCell(fields[column], value, record, record, column);
                values[column] = new FerruleField(fields[column].Name, new FerruleScalar(scalar));
                offset = end;
            }
            rows.Add(new FerruleGroup(values).WithSourceOrigin(origin.Row((ulong)record + 1)));
        }
        if (layout.RecordDelimiters)
        {
            int start = 0;
            while (start < text.Length)
            {
                int lf = text.IndexOf('\n', start);
                int end = lf < 0 ? text.Length : lf;
                if (lf >= 0 && end > start && text[end - 1] == '\r') end--;
                Record(text[start..end].EnumerateRunes().ToArray());
                start = lf < 0 ? text.Length : lf + 1;
            }
        }
        else
        {
            var runes = text.EnumerateRunes().ToArray();
            for (int start = 0; start < runes.Length;)
            {
                int length = (int)Math.Min(runes.Length - start, layout.RecordWidth);
                Record(runes[start..(start + length)]);
                start += length;
            }
        }
        return new FerruleRepeated(rows);
    }

    public static byte[] SerializeBytes(FerruleInstance source, IReadOnlyList<FerruleCsvField> fields,
        FerruleFixedWidthLayout layout, int maximum = MaximumOutputBytes)
    {
        if (maximum < 0 || maximum > MaximumOutputBytes) throw new ArgumentOutOfRangeException(nameof(maximum));
        var ordered = FerruleCsv.ValidateFields(fields);
        if (ordered.Length != layout.Widths.Count)
            throw new FerruleFixedWidthException(FerruleFixedWidthError.Layout, ordered.Length, layout.Widths.Count);
        if (source is not FerruleRepeated rows) throw new FerruleFixedWidthException(FerruleFixedWidthError.RootShape);
        long measured = 0;
        for (int row = 0; row < rows.Items.Count; row++)
        {
            var values = FerruleCsv.FormatRow(row, rows.Items[row], ordered);
            for (int column = 0; column < values.Length; column++)
            {
                int length = values[column].EnumerateRunes().Count();
                long width = layout.Widths[column];
                if (length > width) throw new FerruleFixedWidthException(FerruleFixedWidthError.FieldOverflow, width, length, row, ordered[column].Name);
                measured += Utf8.GetByteCount(values[column]) + (width - length) * layout.Fill.Utf8SequenceLength;
                if (measured > maximum) throw new FerruleFixedWidthException(FerruleFixedWidthError.OutputTooLarge, maximum, measured);
            }
            if (layout.RecordDelimiters) measured++;
            if (measured > maximum) throw new FerruleFixedWidthException(FerruleFixedWidthError.OutputTooLarge, maximum, measured);
        }
        using var output = new MemoryStream();
        void Write(string text)
        {
            int count = Utf8.GetByteCount(text);
            if (count > maximum - output.Length)
                throw new FerruleFixedWidthException(FerruleFixedWidthError.OutputTooLarge, maximum, output.Length + count);
            output.Write(Utf8.GetBytes(text));
        }
        for (int row = 0; row < rows.Items.Count; row++)
        {
            var values = FerruleCsv.FormatRow(row, rows.Items[row], ordered);
            for (int column = 0; column < values.Length; column++)
            {
                int length = values[column].EnumerateRunes().Count();
                long width = layout.Widths[column];
                if (length > width)
                    throw new FerruleFixedWidthException(FerruleFixedWidthError.FieldOverflow, width, length, row, ordered[column].Name);
                Write(values[column]);
                long padding = (long)(width - length) * layout.Fill.Utf8SequenceLength;
                if (padding > maximum - output.Length)
                    throw new FerruleFixedWidthException(FerruleFixedWidthError.OutputTooLarge, maximum, output.Length + padding);
                for (long i = length; i < width; i++) Write(layout.Fill.ToString());
            }
            if (layout.RecordDelimiters) Write("\n");
        }
        return output.ToArray();
    }
}
