using System.Globalization;

namespace Ferrule.Runtime;

public static partial class FerruleX12
{
    private static bool HasOutputLexical(Node node) => node.Lexical is not null || node.Children.Any(HasOutputLexical);

    private static FerruleInstance FormatOutputView(Node node, FerruleInstance instance, string[] path, Budget budget, int depth)
    {
        budget.Visit(depth);
        if (node.Type is not null)
        {
            if (node.Lexical is null) return instance;
            if (instance is not FerruleScalar scalar) throw LexicalFailure(path);
            FerruleValue value = scalar.Value;
            if (value.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull) return instance;
            RequireUtf8(value.Kind == FerruleValueKind.String ? value.StringValue : "", MaximumDocumentBytes, "X12 field");
            FerruleValue formatted = FormatLexical(value, node.Lexical, path);
            return new FerruleScalar(formatted);
        }
        if (instance is not FerruleGroup group) return instance;
        var fields = new List<FerruleField>();
        foreach (FerruleField field in group.Fields)
        {
            Node? child = node.Children.FirstOrDefault(candidate => candidate.Name == field.Name);
            if (child is null) { budget.Visit(depth + 1); fields.Add(field); continue; }
            string[] childPath = [.. path, child.Name];
            FerruleInstance value = field.Value;
            if (child.Repeating && value is FerruleRepeated or FerruleMappedSequence)
            {
                IReadOnlyList<FerruleInstance> items = value is FerruleRepeated repeated ? repeated.Items : ((FerruleMappedSequence)value).Items;
                if (items.Count > MaximumLoopInstances) throw Limit("X12 output loop instance limit exceeded.");
                var formatted = new List<FerruleInstance>();
                foreach (FerruleInstance item in items)
                {
                    budget.Loop();
                    formatted.Add(FormatOutputView(child, item, childPath, budget, depth + 1));
                }
                value = value is FerruleRepeated ? new FerruleRepeated(formatted) : new FerruleMappedSequence(formatted);
            }
            else value = FormatOutputView(child, value, childPath, budget, depth + 1);
            fields.Add(new FerruleField(field.Name, value));
        }
        return group.CloneFields(fields);
    }

    private static FerruleValue FormatLexical(FerruleValue value, LexicalFormat format, string[] path)
    {
        if (format.Kind == "decimal")
        {
            if (value.Kind == FerruleValueKind.String)
            {
                string text = value.StringValue.Trim();
                if (!PlainDecimal(text) || !double.TryParse(text, NumberStyles.AllowLeadingSign | NumberStyles.AllowDecimalPoint,
                        CultureInfo.InvariantCulture, out double number) || !double.IsFinite(number)) throw LexicalFailure(path);
                if (text.Length <= format.MaximumChars) return FerruleValue.FromString(text);
                if (FerruleValueMaps.RustFloatText(number) != text) throw LexicalFailure(path);
                text = FerruleValueMaps.RustFloatText(CanonicalDecimal(number, format.MaximumChars, path));
                if (text.Length > format.MaximumChars) throw LexicalFailure(path);
                return FerruleValue.FromString(text);
            }
            if (value.Kind == FerruleValueKind.Int64)
            {
                if (value.Int64Value.ToString(CultureInfo.InvariantCulture).Length > format.MaximumChars) throw LexicalFailure(path);
                return value;
            }
            if (value.Kind == FerruleValueKind.Double)
                return FerruleValue.FromDouble(CanonicalDecimal(value.DoubleValue, format.MaximumChars, path));
            throw LexicalFailure(path);
        }
        if (value.Kind != FerruleValueKind.String) throw LexicalFailure(path);
        string? output = format.Kind switch
        {
            "compact_date6" => CompactDate(value.StringValue, shortDate: true),
            "compact_date8" => CompactDate(value.StringValue, shortDate: false),
            "compact_time" => CompactTime(value.StringValue, format.MinimumDigits, format.MaximumDigits),
            _ => null,
        };
        if (output is null) throw LexicalFailure(path);
        return FerruleValue.FromString(output);
    }

    private static FerruleX12Exception LexicalFailure(string[] path)
        => Failure(FerruleX12Error.Value, "X12 lexical value cannot be formatted.", path: string.Join('/', path));

    private static bool PlainDecimal(string value)
    {
        if (value.Length == 0) return false;
        int start = value[0] is '+' or '-' ? 1 : 0;
        int dot = value.IndexOf('.', start);
        if (dot < 0) return start < value.Length && value.AsSpan(start).IndexOfAnyExceptInRange('0', '9') < 0;
        return dot + 1 < value.Length && value.AsSpan(start, dot - start).IndexOfAnyExceptInRange('0', '9') < 0
            && value.AsSpan(dot + 1).IndexOfAnyExceptInRange('0', '9') < 0;
    }

    private static double CanonicalDecimal(double number, int maximumChars, string[] path)
    {
        if (!double.IsFinite(number)) throw LexicalFailure(path);
        if (FerruleValueMaps.RustFloatText(number).Length <= maximumChars) return number;
        const double MachineEpsilon = 2.220446049250313e-16;
        double tolerance = MachineEpsilon * Math.Max(Math.Abs(number), 1.0) * 8.0;
        for (int places = 0; places <= 15; places++)
        {
            string text = number.ToString("F" + places, CultureInfo.InvariantCulture);
            if (text.Contains('.', StringComparison.Ordinal)) text = text.TrimEnd('0').TrimEnd('.');
            if (text.Length <= maximumChars
                && double.TryParse(text, NumberStyles.AllowLeadingSign | NumberStyles.AllowDecimalPoint, CultureInfo.InvariantCulture, out double parsed)
                && Math.Abs(parsed - number) <= tolerance) return parsed;
        }
        throw LexicalFailure(path);
    }

    private static bool Digits(string value) => value.Length > 0 && value.All(char.IsAsciiDigit);

    private static string? CompactDate(string value, bool shortDate)
    {
        int width = shortDate ? 6 : 8;
        if (value.Length == width && Digits(value)) return value;
        if (value.Length < 10 || value[4] != '-' || value[7] != '-'
            || !Digits(value[..4]) || !Digits(value[5..7]) || !Digits(value[8..10]) || !ValidTimezone(value[10..])) return null;
        int year = int.Parse(value[..4], CultureInfo.InvariantCulture);
        int month = int.Parse(value[5..7], CultureInfo.InvariantCulture);
        int day = int.Parse(value[8..10], CultureInfo.InvariantCulture);
        bool leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
        int maximumDay = month switch { 1 or 3 or 5 or 7 or 8 or 10 or 12 => 31, 4 or 6 or 9 or 11 => 30, 2 => leap ? 29 : 28, _ => 0 };
        if (day < 1 || day > maximumDay) return null;
        return value[(shortDate ? 2 : 0)..4] + value[5..7] + value[8..10];
    }

    private static string? CompactTime(string value, int minimum, int maximum)
    {
        if (value.Length >= minimum && value.Length <= maximum && Digits(value))
            return ValidCompactClock(value) ? value : null;
        if (value.Length < 5 || value[2] != ':' || !Digits(value[..2]) || !Digits(value[3..5])
            || int.Parse(value[..2], CultureInfo.InvariantCulture) > 23 || int.Parse(value[3..5], CultureInfo.InvariantCulture) > 59) return null;
        int cursor = 5;
        string second = "00";
        if (cursor < value.Length && value[cursor] == ':')
        {
            if (value.Length < cursor + 3 || !Digits(value[(cursor + 1)..(cursor + 3)])) return null;
            second = value[(cursor + 1)..(cursor + 3)];
            cursor += 3;
        }
        if (int.Parse(second, CultureInfo.InvariantCulture) > 59) return null;
        int fractionStart = cursor;
        bool fractional = cursor < value.Length && value[cursor] == '.';
        if (fractional)
        {
            fractionStart = ++cursor;
            while (cursor < value.Length && char.IsAsciiDigit(value[cursor])) cursor++;
        }
        if (!ValidTimezone(value[cursor..])) return null;
        string fraction = fractional ? value[fractionStart..cursor] : "";
        string compact = value[..2] + value[3..5] + second;
        if (maximum == 4)
        {
            if (second != "00" || fraction.Any(character => character != '0')) return null;
            compact = compact[..4];
        }
        else
        {
            int retained = Math.Min(fraction.Length, Math.Max(0, maximum - 6));
            if (fraction[retained..].Any(character => character != '0')) return null;
            compact += fraction[..retained];
        }
        compact = compact.PadRight(minimum, '0');
        return compact.Length <= maximum ? compact : null;
    }

    private static bool ValidCompactClock(string value)
        => value.Length >= 4 && Digits(value) && int.Parse(value[..2], CultureInfo.InvariantCulture) <= 23
            && int.Parse(value[2..4], CultureInfo.InvariantCulture) <= 59
            && (value.Length < 6 || int.Parse(value[4..6], CultureInfo.InvariantCulture) <= 59);

    private static bool ValidTimezone(string value)
    {
        if (value.Length == 0 || value == "Z") return true;
        if (value.Length != 6 || value[0] is not ('+' or '-') || value[3] != ':'
            || !Digits(value[1..3]) || !Digits(value[4..6])) return false;
        int hour = int.Parse(value[1..3], CultureInfo.InvariantCulture);
        int minute = int.Parse(value[4..6], CultureInfo.InvariantCulture);
        return hour <= 14 && minute <= 59 && (hour != 14 || minute == 0);
    }
}
