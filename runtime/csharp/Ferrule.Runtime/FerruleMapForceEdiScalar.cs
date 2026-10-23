using System.Globalization;

namespace Ferrule.Runtime;

public enum FerruleMapForceEdiScalarKind { Text, Decimal, Date, Time }

public readonly record struct FerruleMapForceEdiScalar(
    FerruleMapForceEdiScalarKind Kind, int MinChars, int MaxChars);

/// <summary>Explicit mapped-X12 scalar formatting. No syntax escaping, byte encoding or I/O.</summary>
public static class FerruleMapForceEdiScalars
{
    public static string Format(FerruleMapForceEdiScalar field, FerruleValue value)
    {
        if (field.MinChars < 0 || field.MaxChars < field.MinChars || field.MaxChars > 65536)
            throw new ArgumentException("Invalid mapped EDI field bounds.", nameof(field));
        var text = value.Kind switch
        {
            FerruleValueKind.Null or FerruleValueKind.JsonNull => "",
            FerruleValueKind.String => value.StringValue,
            FerruleValueKind.Int64 => value.Int64Value.ToString(CultureInfo.InvariantCulture),
            FerruleValueKind.Double when double.IsFinite(value.DoubleValue) => FerruleValueMaps.RustFloatText(value.DoubleValue),
            FerruleValueKind.Decimal => value.DecimalValue.ToString(),
            _ => throw new ArgumentException("Expected a mapped String or finite numeric value.", nameof(value)),
        };
        if (System.Text.Encoding.UTF8.GetByteCount(text) > 64 * 1024 * 1024)
            throw new ArgumentException("Mapped EDI field exceeds its byte limit.", nameof(value));
        if (text.Length == 0) return text;
        return field.Kind switch
        {
            FerruleMapForceEdiScalarKind.Text => text.PadRight(field.MinChars),
            FerruleMapForceEdiScalarKind.Decimal => DecimalField(text, field.MinChars, field.MaxChars),
            FerruleMapForceEdiScalarKind.Date => DateField(text, field.MinChars, field.MaxChars),
            FerruleMapForceEdiScalarKind.Time => TimeField(text, field.MinChars, field.MaxChars),
            _ => throw new ArgumentException("Invalid mapped EDI field kind.", nameof(field)),
        };
    }

    private static string DecimalField(string text, int minimum, int maximum)
    {
        text = text.Trim();
        if (text.Length == 0) return text;
        bool negative = text[0] == '-';
        if (text[0] is '-' or '+') text = text[1..];
        var number = decimal.Parse(FerruleDecimal96.ParseRounded(text).ToString(), CultureInfo.InvariantCulture);
        var canonical = number.ToString(CultureInfo.InvariantCulture);
        int point = canonical.IndexOf('.');
        if (point >= 0 && canonical.Length - 1 > maximum)
            number = decimal.Round(number, Math.Clamp(maximum - point + 1, 0, 28), MidpointRounding.ToEven);
        canonical = number.ToString(CultureInfo.InvariantCulture);
        point = canonical.IndexOf('.');
        string whole = (point < 0 ? canonical : canonical[..point]).TrimStart('0');
        string fraction = point < 0 ? "" : canonical[(point + 1)..].TrimEnd('0');
        if (whole.Length == 0 && fraction.Length == 0) whole = "0";
        if (fraction.Length != 0) fraction = "." + fraction;
        return (negative ? "-" : "") + whole.PadLeft(Math.Max(0, minimum - fraction.Length), '0') + fraction;
    }

    private static string DateField(string text, int minimum, int maximum)
    {
        text = text.Trim();
        if (text.Any(c => c > 127)) throw new ArgumentException("Expected an ASCII date lexical.");
        if (text.Length >= 10) text = text[..4] + text.Substring(5, 2) + text.Substring(8, 2);
        if (text.Length is not (6 or 8)) throw new ArgumentException("Expected a compact or ISO date lexical.");
        if (maximum == 6 && text.Length == 8) text = text[2..];
        return text.PadRight(minimum);
    }

    private static string TimeField(string text, int minimum, int maximum)
    {
        if (maximum is < 4 or > 8) throw new ArgumentException("Unsupported mapped EDI time width.");
        text = text[(text.IndexOf('T') + 1)..].Replace(":", "", StringComparison.Ordinal).Replace(".", "", StringComparison.Ordinal);
        int zone = text.IndexOfAny(['Z', '-', '+']);
        if (zone >= 0) text = text[..zone];
        if (text.Any(c => c > 127)) throw new ArgumentException("Expected an ASCII time lexical.");
        if (text.Length > maximum)
        {
            if (maximum > 6)
            {
                double fraction = double.Parse("0." + text[6..], CultureInfo.InvariantCulture);
                text = text[..6];
                if (fraction > 0)
                {
                    int places = maximum - 6;
                    int power = places == 1 ? 10 : 100;
                    int digits = (int)Math.Round(fraction * power, MidpointRounding.ToEven) % power;
                    text += digits.ToString("D" + places, CultureInfo.InvariantCulture);
                }
            }
            else text = text[..maximum];
        }
        while (text.Length > minimum && text.Length > 6 && text[^1] == '0') text = text[..^1];
        return text;
    }
}
