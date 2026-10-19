using System.Globalization;
using System.Numerics;
using System.Text;

namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
    private const string FormatNumberName = "format_number";

    private static FerruleValue FormatNumber(IReadOnlyList<FerruleValue> arguments)
    {
        if (arguments.Count >= 2 && arguments[1].Kind != FerruleValueKind.String)
        {
            throw Type(FormatNumberName, arguments[1]);
        }
        if (arguments.Count is < 2 or > 4)
        {
            throw Arity(FormatNumberName, 2, arguments.Count);
        }

        var decimalPoint = arguments.Count >= 3
            ? SingleFormatRune(arguments[2])
            : new Rune('.');
        var groupingSeparator = arguments.Count == 4
            ? SingleFormatRune(arguments[3])
            : new Rune(',');
        if (decimalPoint == groupingSeparator)
        {
            throw InvalidArgument(
                FormatNumberName,
                "requires distinct decimal and grouping separators");
        }
        if (CollidesWithPicture(decimalPoint) || CollidesWithPicture(groupingSeparator))
        {
            throw InvalidArgument(
                FormatNumberName,
                "separator collides with a picture character");
        }

        var subformats = arguments[1].StringValue.Split(';');
        if (subformats.Length is < 1 or > 2 || subformats.Any(string.IsNullOrEmpty))
        {
            throw InvalidArgument(
                FormatNumberName,
                "format requires one or two non-empty subformats");
        }
        var pictures = subformats
            .Select(part => NumberPicture.Parse(part, decimalPoint, groupingSeparator))
            .ToArray();

        var number = FormatDecimal.From(arguments[0]);
        var hasNegativeSubformat = pictures.Length == 2;
        var picture = number.IsNegative && hasNegativeSubformat ? pictures[1] : pictures[0];
        var (integerPart, fractionPart) = RenderFormatDecimal(
            number,
            picture.MultiplierDigits,
            picture.MaximumFractionDigits);
        while (fractionPart.Length > picture.MinimumFractionDigits &&
               fractionPart.EndsWith('0'))
        {
            fractionPart = fractionPart[..^1];
        }

        if (integerPart == "0" && picture.MinimumIntegerDigits == 0 && fractionPart.Length > 0)
        {
            integerPart = string.Empty;
        }
        else
        {
            integerPart = integerPart.PadLeft(picture.MinimumIntegerDigits, '0');
        }
        if (integerPart.Length == 0 && fractionPart.Length == 0)
        {
            integerPart = "0";
        }
        if (picture.GroupingSize is int groupingSize)
        {
            integerPart = GroupFormatDigits(integerPart, groupingSize, groupingSeparator);
        }

        var output = new StringBuilder();
        if (number.IsNegative && !hasNegativeSubformat)
        {
            output.Append('-');
        }
        output.Append(picture.Prefix);
        output.Append(integerPart);
        if (fractionPart.Length > 0)
        {
            output.Append(decimalPoint.ToString());
            output.Append(fractionPart);
        }
        output.Append(picture.Suffix);
        return FerruleValue.FromString(output.ToString());
    }

    private static Rune SingleFormatRune(FerruleValue value)
    {
        if (value.Kind != FerruleValueKind.String)
        {
            throw Type(FormatNumberName, value);
        }

        var runes = value.StringValue.EnumerateRunes().GetEnumerator();
        if (!runes.MoveNext())
        {
            throw InvalidArgument(FormatNumberName, "separator must be one character");
        }
        var first = runes.Current;
        if (runes.MoveNext())
        {
            throw InvalidArgument(FormatNumberName, "separator must be one character");
        }
        return first;
    }

    private static bool CollidesWithPicture(Rune separator) => separator.Value is
        >= '0' and <= '9' or '#' or ';' or '%' or 0x2030;

    private static (string Integer, string Fraction) RenderFormatDecimal(
        FormatDecimal value,
        int multiplierDigits,
        int fractionDigits)
    {
        var magnitude = value.Magnitude();
        var exponentMarker = magnitude.IndexOfAny(['e', 'E']);
        var mantissa = exponentMarker < 0 ? magnitude : magnitude[..exponentMarker];
        var exponent = exponentMarker < 0
            ? 0
            : int.Parse(magnitude[(exponentMarker + 1)..], CultureInfo.InvariantCulture);
        var decimalMarker = mantissa.IndexOf('.');
        var whole = decimalMarker < 0 ? mantissa : mantissa[..decimalMarker];
        var fractional = decimalMarker < 0 ? string.Empty : mantissa[(decimalMarker + 1)..];
        var digits = whole + fractional;
        var decimalPosition = whole.Length + exponent + multiplierDigits;

        string integerPart;
        string fractionPart;
        if (decimalPosition <= 0)
        {
            integerPart = "0";
            fractionPart = new string('0', -decimalPosition) + digits;
        }
        else if (decimalPosition >= digits.Length)
        {
            integerPart = digits.PadRight(decimalPosition, '0');
            fractionPart = string.Empty;
        }
        else
        {
            integerPart = digits[..decimalPosition];
            fractionPart = digits[decimalPosition..];
        }

        var firstRetainedDigit = 0;
        while (firstRetainedDigit + 1 < integerPart.Length &&
               integerPart[firstRetainedDigit] == '0')
        {
            firstRetainedDigit++;
        }
        integerPart = integerPart[firstRetainedDigit..];

        var combined = integerPart + fractionPart;
        var integerLength = integerPart.Length;
        var keep = integerLength + fractionDigits;
        var roundUp = keep < combined.Length && combined[keep] >= '5';
        combined = combined[..Math.Min(keep, combined.Length)].PadRight(keep, '0');
        if (roundUp)
        {
            combined = IncrementFormatDecimal(combined);
        }
        if (combined.Length > keep)
        {
            integerLength++;
        }
        combined = combined.PadRight(integerLength, '0');
        return (combined[..integerLength], combined[integerLength..]);
    }

    private static string IncrementFormatDecimal(string digits)
    {
        var result = digits.ToCharArray();
        for (var index = result.Length - 1; index >= 0; index--)
        {
            if (result[index] < '9')
            {
                result[index]++;
                return new string(result);
            }
            result[index] = '0';
        }
        return "1" + new string(result);
    }

    private static string GroupFormatDigits(string digits, int size, Rune separator)
    {
        var output = new StringBuilder(digits.Length + (digits.Length / size));
        var firstGroupSize = digits.Length % size;
        if (firstGroupSize == 0)
        {
            firstGroupSize = size;
        }
        output.Append(digits.AsSpan(0, Math.Min(firstGroupSize, digits.Length)));
        for (var index = firstGroupSize; index < digits.Length; index += size)
        {
            output.Append(separator.ToString());
            output.Append(digits.AsSpan(index, Math.Min(size, digits.Length - index)));
        }
        return output.ToString();
    }

    private static List<string> SplitFormatRunes(string value, Rune separator)
    {
        var parts = new List<string>();
        var start = 0;
        var index = 0;
        foreach (var rune in value.EnumerateRunes())
        {
            if (rune == separator)
            {
                parts.Add(value[start..index]);
                start = index + rune.Utf16SequenceLength;
            }
            index += rune.Utf16SequenceLength;
        }
        parts.Add(value[start..]);
        return parts;
    }

    private static bool ContainsFormatRune(string value, Rune expected)
    {
        foreach (var rune in value.EnumerateRunes())
        {
            if (rune == expected)
            {
                return true;
            }
        }
        return false;
    }

    private static string RemoveFormatRune(string value, Rune removed)
    {
        var output = new StringBuilder(value.Length);
        foreach (var rune in value.EnumerateRunes())
        {
            if (rune != removed)
            {
                output.Append(rune.ToString());
            }
        }
        return output.ToString();
    }

    private readonly record struct FormatDecimal(bool IsInteger, long Integer, double Double)
    {
        internal bool IsNegative => IsInteger
            ? Integer < 0
            : BitConverter.DoubleToInt64Bits(Double) < 0;

        internal static FormatDecimal From(FerruleValue value) => value.Kind switch
        {
            FerruleValueKind.Int64 => new(true, value.Int64Value, default),
            FerruleValueKind.Double when double.IsFinite(value.DoubleValue) =>
                new(false, default, value.DoubleValue),
            FerruleValueKind.Double => throw InvalidArgument(
                FormatNumberName,
                "requires a finite number"),
            _ => throw Type(FormatNumberName, value),
        };

        internal string Magnitude() => IsInteger
            ? BigInteger.Abs(new BigInteger(Integer)).ToString(CultureInfo.InvariantCulture)
            : DoubleText(Math.Abs(Double));
    }

    private sealed record NumberPicture(
        string Prefix,
        string Suffix,
        int MinimumIntegerDigits,
        int MinimumFractionDigits,
        int MaximumFractionDigits,
        int? GroupingSize,
        int MultiplierDigits)
    {
        internal static NumberPicture Parse(
            string subformat,
            Rune decimalPoint,
            Rune groupingSeparator)
        {
            var first = -1;
            var last = -1;
            var index = 0;
            foreach (var rune in subformat.EnumerateRunes())
            {
                if (IsPictureRune(rune, decimalPoint, groupingSeparator))
                {
                    first = first < 0 ? index : first;
                    last = index + rune.Utf16SequenceLength;
                }
                index += rune.Utf16SequenceLength;
            }
            if (first < 0)
            {
                throw InvalidArgument(
                    FormatNumberName,
                    "format must contain a digit placeholder");
            }

            var prefix = subformat[..first];
            var suffix = subformat[last..];
            var body = subformat[first..last];
            var hasDigit = false;
            foreach (var rune in body.EnumerateRunes())
            {
                hasDigit |= rune.Value is '0' or '#';
                if (!IsPictureRune(rune, decimalPoint, groupingSeparator))
                {
                    throw InvalidArgument(
                        FormatNumberName,
                        "format contains an invalid numeric picture");
                }
            }
            if (!hasDigit)
            {
                throw InvalidArgument(
                    FormatNumberName,
                    "format contains an invalid numeric picture");
            }

            var parts = SplitFormatRunes(body, decimalPoint);
            var integerPicture = parts[0];
            var fractionPicture = parts.Count == 2 ? parts[1] : string.Empty;
            if (parts.Count > 2 || ContainsFormatRune(fractionPicture, groupingSeparator))
            {
                throw InvalidArgument(
                    FormatNumberName,
                    "format contains invalid decimal or grouping separators");
            }

            var integerDigits = RemoveFormatRune(integerPicture, groupingSeparator);
            if (!ValidIntegerPicture(integerDigits) || !ValidFractionPicture(fractionPicture))
            {
                throw InvalidArgument(
                    FormatNumberName,
                    "format contains placeholders in an invalid order");
            }
            var groupingSize = ParseGrouping(integerPicture, groupingSeparator);

            var percentCount = 0;
            var perMilleCount = 0;
            foreach (var rune in subformat.EnumerateRunes())
            {
                percentCount += rune.Value == '%' ? 1 : 0;
                perMilleCount += rune.Value == 0x2030 ? 1 : 0;
            }
            if (percentCount > 1 || perMilleCount > 1 || percentCount + perMilleCount > 1)
            {
                throw InvalidArgument(
                    FormatNumberName,
                    "format allows one percent or per-mille character");
            }

            return new NumberPicture(
                prefix,
                suffix,
                integerDigits.Count(character => character == '0'),
                fractionPicture.Count(character => character == '0'),
                RuneCount(fractionPicture),
                groupingSize,
                percentCount == 1 ? 2 : perMilleCount == 1 ? 3 : 0);
        }

        private static bool IsPictureRune(
            Rune rune,
            Rune decimalPoint,
            Rune groupingSeparator) =>
            rune.Value is '0' or '#' || rune == decimalPoint || rune == groupingSeparator;

        private static bool ValidIntegerPicture(string picture)
        {
            var mandatory = false;
            foreach (var rune in picture.EnumerateRunes())
            {
                if (rune.Value == '#' && !mandatory)
                {
                    continue;
                }
                if (rune.Value == '0')
                {
                    mandatory = true;
                    continue;
                }
                return false;
            }
            return true;
        }

        private static bool ValidFractionPicture(string picture)
        {
            var optional = false;
            foreach (var rune in picture.EnumerateRunes())
            {
                if (rune.Value == '0' && !optional)
                {
                    continue;
                }
                if (rune.Value == '#')
                {
                    optional = true;
                    continue;
                }
                return false;
            }
            return true;
        }

        private static int? ParseGrouping(string picture, Rune separator)
        {
            if (!ContainsFormatRune(picture, separator))
            {
                return null;
            }
            var groups = SplitFormatRunes(picture, separator);
            if (groups.Any(string.IsNullOrEmpty))
            {
                throw InvalidArgument(
                    FormatNumberName,
                    "format contains misplaced grouping separators");
            }
            return RuneCount(groups[^1]);
        }
    }
}
