using System.Globalization;
using System.Text;

namespace Ferrule.Runtime;

internal static class FerruleDelimitedText
{
    private const string Function = "flextext_parse_field";
    private const int MaximumInputBytes = 8 * 1024 * 1024;
    private const int MaximumValueBytes = 1024 * 1024;
    private const int MaximumRecords = 1_000_000;
    private const int MaximumNodes = 1_000_000;

    internal static FerruleValue ParseField(
        FerruleValue input,
        string fieldSeparator,
        string recordSeparator,
        string quote,
        string escape,
        IReadOnlyList<FerruleScalarType> fields,
        uint selected)
    {
        ArgumentNullException.ThrowIfNull(fieldSeparator);
        ArgumentNullException.ThrowIfNull(recordSeparator);
        ArgumentNullException.ThrowIfNull(quote);
        ArgumentNullException.ThrowIfNull(escape);
        ArgumentNullException.ThrowIfNull(fields);
        if (input.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
        {
            return FerruleValue.Null;
        }
        if (input.Kind != FerruleValueKind.String)
        {
            throw Type(input);
        }
        if (fieldSeparator.Length == 0 ||
            recordSeparator.Length == 0 ||
            !IsOneScalar(quote) ||
            !IsOneScalar(escape) ||
            selected >= fields.Count)
        {
            throw InvalidInput();
        }

        var text = input.StringValue;
        if (Encoding.UTF8.GetByteCount(text) > MaximumInputBytes)
        {
            throw InvalidInput();
        }
        if (text.StartsWith('\uFEFF'))
        {
            text = text[1..];
        }

        var record = new List<string>();
        var field = new StringBuilder();
        var index = 0;
        var quoted = false;
        var recordCount = 0;
        var nodeCount = 2;
        var first = FerruleValue.Null;
        var hasFirst = false;

        void CompleteRecord()
        {
            if (record.Count != fields.Count)
            {
                throw InvalidInput();
            }
            if (recordCount == MaximumRecords)
            {
                throw InvalidInput();
            }
            recordCount++;
            nodeCount = checked(nodeCount + fields.Count + 1);
            if (nodeCount > MaximumNodes)
            {
                throw InvalidInput();
            }

            for (var fieldIndex = 0; fieldIndex < fields.Count; fieldIndex++)
            {
                var value = ParseValue(record[fieldIndex], fields[fieldIndex]);
                if (!hasFirst && fieldIndex == selected)
                {
                    first = value;
                }
            }
            hasFirst = true;
            record.Clear();
        }

        try
        {
            while (index < text.Length)
            {
                var scalarLength = ScalarLength(text, index);
                if (quoted)
                {
                    if (StartsAt(text, index, escape))
                    {
                        var afterEscape = index + escape.Length;
                        if (escape == quote && StartsAt(text, afterEscape, quote))
                        {
                            field.Append(quote);
                            index = afterEscape + quote.Length;
                            continue;
                        }
                        if (escape != quote && afterEscape < text.Length)
                        {
                            var escapedLength = ScalarLength(text, afterEscape);
                            field.Append(text, afterEscape, escapedLength);
                            index = afterEscape + escapedLength;
                            continue;
                        }
                    }
                    if (StartsAt(text, index, quote))
                    {
                        quoted = false;
                    }
                    else
                    {
                        field.Append(text, index, scalarLength);
                    }
                    index += scalarLength;
                    continue;
                }

                if (StartsAt(text, index, quote) && field.Length == 0)
                {
                    quoted = true;
                    index += quote.Length;
                }
                else if (StartsAt(text, index, fieldSeparator))
                {
                    record.Add(field.ToString());
                    field.Clear();
                    index += fieldSeparator.Length;
                }
                else
                {
                    var separatorLength = RecordSeparatorLength(text, index, recordSeparator);
                    if (separatorLength != 0)
                    {
                        record.Add(field.ToString());
                        field.Clear();
                        CompleteRecord();
                        index += separatorLength;
                    }
                    else
                    {
                        field.Append(text, index, scalarLength);
                        index += scalarLength;
                    }
                }
            }
            if (quoted)
            {
                throw InvalidInput();
            }
            if (field.Length != 0 || record.Count != 0)
            {
                record.Add(field.ToString());
                CompleteRecord();
            }
            return hasFirst ? first : FerruleValue.Null;
        }
        catch (OverflowException)
        {
            throw InvalidInput();
        }
    }

    internal static FerruleValue ParseFixedWidthField(
        FerruleValue input,
        string fill,
        bool recordDelimiters,
        bool treatEmptyAsAbsent,
        IReadOnlyList<uint> widths,
        IReadOnlyList<FerruleScalarType> fields,
        uint selected)
    {
        ArgumentNullException.ThrowIfNull(fill);
        ArgumentNullException.ThrowIfNull(widths);
        ArgumentNullException.ThrowIfNull(fields);
        if (input.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
        {
            return FerruleValue.Null;
        }
        if (input.Kind != FerruleValueKind.String)
        {
            throw Type(input);
        }
        if (!IsOneScalar(fill) ||
            widths.Count == 0 ||
            widths.Count != fields.Count ||
            selected >= fields.Count)
        {
            throw InvalidInput();
        }

        var text = input.StringValue;
        if (Encoding.UTF8.GetByteCount(text) > MaximumInputBytes)
        {
            throw InvalidInput();
        }
        if (text.StartsWith('\uFEFF'))
        {
            text = text[1..];
        }

        try
        {
            var recordWidth = 0;
            foreach (var width in widths)
            {
                if (width == 0)
                {
                    throw InvalidInput();
                }
                recordWidth = checked(recordWidth + checked((int)width));
            }

            var minimumWidth =
                recordDelimiters && fields[^1] == FerruleScalarType.String
                    ? recordWidth - checked((int)widths[^1])
                    : recordWidth;
            var recordCount = 0;
            var nodeCount = 2;
            var first = FerruleValue.Null;
            var hasFirst = false;

            void CompleteRecord(string record)
            {
                var scalarCount = ScalarCount(record);
                if (scalarCount < minimumWidth || scalarCount > recordWidth)
                {
                    throw InvalidInput();
                }
                if (recordCount == MaximumRecords)
                {
                    throw InvalidInput();
                }
                recordCount++;
                nodeCount = checked(nodeCount + fields.Count + 1);
                if (nodeCount > MaximumNodes)
                {
                    throw InvalidInput();
                }

                var offset = 0;
                for (var fieldIndex = 0; fieldIndex < fields.Count; fieldIndex++)
                {
                    var raw = TakeScalars(record, ref offset, checked((int)widths[fieldIndex]));
                    raw = fields[fieldIndex] == FerruleScalarType.String
                        ? TrimFillEnd(raw, fill)
                        : TrimFill(raw, fill);
                    var value = ParseValue(raw, fields[fieldIndex], treatEmptyAsAbsent);
                    if (!hasFirst && fieldIndex == selected)
                    {
                        first = value;
                    }
                }
                hasFirst = true;
            }

            if (recordDelimiters)
            {
                var offset = 0;
                while (offset < text.Length)
                {
                    var newline = text.IndexOf('\n', offset);
                    var end = newline < 0 ? text.Length : newline;
                    if (end > offset && text[end - 1] == '\r')
                    {
                        end--;
                    }
                    CompleteRecord(text[offset..end]);
                    if (newline < 0)
                    {
                        break;
                    }
                    offset = newline + 1;
                }
            }
            else
            {
                var scalarCount = ScalarCount(text);
                if (scalarCount % recordWidth != 0 ||
                    scalarCount / recordWidth > MaximumRecords)
                {
                    throw InvalidInput();
                }
                var offset = 0;
                while (offset < text.Length)
                {
                    CompleteRecord(TakeScalars(text, ref offset, recordWidth));
                }
            }
            return hasFirst ? first : FerruleValue.Null;
        }
        catch (OverflowException)
        {
            throw InvalidInput();
        }
    }

    private static FerruleValue ParseValue(
        string text,
        FerruleScalarType type,
        bool emptyIsNull = true)
    {
        if (Encoding.UTF8.GetByteCount(text) > MaximumValueBytes)
        {
            throw InvalidInput();
        }
        if (text.Length == 0 && emptyIsNull)
        {
            return FerruleValue.Null;
        }
        return type switch
        {
            FerruleScalarType.String => FerruleValue.FromString(text),
            FerruleScalarType.Int64
                when long.TryParse(
                    text,
                    NumberStyles.AllowLeadingSign,
                    CultureInfo.InvariantCulture,
                    out var integer) =>
                FerruleValue.FromInt64(integer),
            FerruleScalarType.Double
                when text == text.Trim() &&
                     double.TryParse(
                         text,
                         NumberStyles.Float,
                         CultureInfo.InvariantCulture,
                         out var number) &&
                     double.IsFinite(number) =>
                FerruleValue.FromDouble(number),
            FerruleScalarType.Bool when text == "true" =>
                FerruleValue.FromBoolean(true),
            FerruleScalarType.Bool when text == "false" =>
                FerruleValue.FromBoolean(false),
            _ => throw InvalidInput(),
        };
    }

    private static int ScalarCount(string text)
    {
        var count = 0;
        var offset = 0;
        while (offset < text.Length)
        {
            offset += ScalarLength(text, offset);
            count = checked(count + 1);
        }
        return count;
    }

    private static string TakeScalars(string text, ref int offset, int count)
    {
        var start = offset;
        for (var taken = 0; taken < count && offset < text.Length; taken++)
        {
            offset += ScalarLength(text, offset);
        }
        return text[start..offset];
    }

    private static string TrimFillEnd(string text, string fill)
    {
        while (text.EndsWith(fill, StringComparison.Ordinal))
        {
            text = text[..^fill.Length];
        }
        return text;
    }

    private static string TrimFill(string text, string fill)
    {
        while (text.StartsWith(fill, StringComparison.Ordinal))
        {
            text = text[fill.Length..];
        }
        return TrimFillEnd(text, fill);
    }

    private static int RecordSeparatorLength(string text, int index, string configured)
    {
        if (configured is "\n" or "\r\n")
        {
            if (StartsAt(text, index, "\r\n"))
            {
                return 2;
            }
            return StartsAt(text, index, "\n") ? 1 : 0;
        }
        return StartsAt(text, index, configured) ? configured.Length : 0;
    }

    private static bool StartsAt(string text, int index, string value) =>
        index <= text.Length - value.Length &&
        text.AsSpan(index, value.Length).SequenceEqual(value);

    private static bool IsOneScalar(string value) =>
        value.Length == 1 && !char.IsSurrogate(value[0]) ||
        value.Length == 2 && char.IsSurrogatePair(value, 0);

    private static int ScalarLength(string text, int index)
    {
        if (char.IsHighSurrogate(text[index]) &&
            index + 1 < text.Length &&
            char.IsLowSurrogate(text[index + 1]))
        {
            return 2;
        }
        if (char.IsSurrogate(text[index]))
        {
            throw InvalidInput();
        }
        return 1;
    }

    private static FerruleRuntimeException Type(FerruleValue value) =>
        new(
            FerruleRuntimeError.FunctionType,
            $"`{Function}` cannot accept a {TypeName(value)} argument.",
            function: Function,
            foundKind: value.Kind);

    private static FerruleRuntimeException InvalidInput() =>
        new(
            FerruleRuntimeError.FunctionInvalidArgument,
            $"`{Function}` input does not match the FlexText layout.",
            function: Function,
            detail: "input does not match the FlexText layout");

    private static string TypeName(FerruleValue value) => value.Kind switch
    {
        FerruleValueKind.Null => "null",
        FerruleValueKind.JsonNull => "json null",
        FerruleValueKind.XmlNil => "xml nil",
        FerruleValueKind.Bool => "bool",
        FerruleValueKind.Int64 => "int",
        FerruleValueKind.Double => "float",
        FerruleValueKind.String => "string",
        _ => "unknown",
    };
}
