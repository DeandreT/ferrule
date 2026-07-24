using System.Globalization;
using System.Text;

namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
    private enum DateTimePictureField
    {
        Year,
        Month,
        MonthName,
        Day,
        DayOfYear,
        Hour24,
        Hour12,
        Period,
        Minute,
        Second,
        Fraction,
        Timezone,
        GmtTimezone,
    }

    private enum DateTimePictureLetterCase
    {
        Upper,
        Lower,
        Title,
    }

    private abstract record DateTimePicturePart;

    private sealed record DateTimePictureLiteral(string Value) : DateTimePicturePart;

    private sealed record DateTimePictureComponent(
        DateTimePictureField Field,
        int MinimumWidth,
        int MaximumWidth,
        int? FixedWidth,
        DateTimePictureLetterCase? LetterCase) : DateTimePicturePart;

    private static FerruleValue ParseDate(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "parse_date";
        var (value, picture) = DateTimePictureStringPair(arguments, function);
        var parsed = ParseDateTimePicture(value, picture, function);
        var (year, month, day) = parsed.Date(function);
        var output = FormatPictureDate(year, month, day);
        if (parsed.Timezone is not null)
        {
            output += parsed.Timezone;
        }
        return FerruleValue.FromString(output);
    }

    private static FerruleValue ParseDateTime(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "parse_datetime";
        var (value, picture) = DateTimePictureStringPair(arguments, function);
        var parsed = ParseDateTimePicture(value, picture, function);
        var (year, month, day) = parsed.Date(function);
        var (hour, minute, second) = parsed.Time(function, allowDefault: true);
        var output = new StringBuilder();
        output.Append(FormatPictureDate(year, month, day));
        output.Append('T');
        output.Append(FormatPictureTime(hour, minute, second));
        AppendDateTimePictureSuffix(output, parsed);
        return FerruleValue.FromString(output.ToString());
    }

    private static FerruleValue ParseTime(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "parse_time";
        if (arguments.Count != 2)
        {
            throw Arity(function, 2, arguments.Count);
        }
        if (arguments[1].Kind != FerruleValueKind.String)
        {
            throw Type(function, arguments[1]);
        }

        var value = arguments[0].Kind == FerruleValueKind.String
            ? arguments[0].StringValue
            : ScalarText(arguments[0]);
        var parsed = ParseDateTimePicture(value, arguments[1].StringValue, function);
        var (hour, minute, second) = parsed.Time(function, allowDefault: false);
        var output = new StringBuilder(FormatPictureTime(hour, minute, second));
        AppendDateTimePictureSuffix(output, parsed);
        return FerruleValue.FromString(output.ToString());
    }

    private static (string Value, string Picture) DateTimePictureStringPair(
        IReadOnlyList<FerruleValue> arguments,
        string function)
    {
        if (arguments.Count != 2)
        {
            throw Arity(function, 2, arguments.Count);
        }
        if (arguments[0].Kind != FerruleValueKind.String)
        {
            throw Type(function, arguments[0]);
        }
        if (arguments[1].Kind != FerruleValueKind.String)
        {
            throw Type(function, arguments[1]);
        }
        return (arguments[0].StringValue, arguments[1].StringValue);
    }

    private static ParsedDateTimePicture ParseDateTimePicture(
        string value,
        string picture,
        string function)
    {
        var parts = DateTimePictureParts(picture);
        if (parts is null)
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }

        var cursor = 0;
        var parsed = new ParsedDateTimePicture();
        for (var index = 0; index < parts.Count; index++)
        {
            var remaining = value[cursor..];
            if (parts[index] is DateTimePictureLiteral literal)
            {
                if (!remaining.StartsWith(literal.Value, StringComparison.Ordinal))
                {
                    throw InvalidArgument(function, SupportedPictureDetail);
                }
                cursor += literal.Value.Length;
                continue;
            }

            var component = (DateTimePictureComponent)parts[index];
            var width = DateTimePictureComponentWidth(
                component,
                parts.GetRange(index + 1, parts.Count - index - 1),
                remaining);
            if (width is null)
            {
                throw InvalidArgument(function, SupportedPictureDetail);
            }
            var field = TakeDateTimePictureRunes(remaining, width.Value);
            if (field is null)
            {
                throw InvalidArgument(function, SupportedPictureDetail);
            }
            cursor += field.Length;
            parsed.Set(component.Field, field, function);
        }
        if (cursor != value.Length)
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }
        return parsed;
    }

    private static int? DateTimePictureComponentWidth(
        DateTimePictureComponent component,
        IReadOnlyList<DateTimePicturePart> following,
        string value)
    {
        int width;
        if (component.FixedWidth is int fixedWidth)
        {
            width = fixedWidth;
        }
        else if (following.Count > 0 && following[0] is DateTimePictureLiteral literal)
        {
            if (literal.Value.Length == 0)
            {
                return null;
            }
            var end = value.IndexOf(literal.Value, StringComparison.Ordinal);
            if (end < 0)
            {
                return null;
            }
            width = DateTimePictureRuneCount(value[..end]);
        }
        else if (following.Count == 0)
        {
            width = DateTimePictureRuneCount(value);
        }
        else
        {
            return null;
        }
        return width >= component.MinimumWidth && width <= component.MaximumWidth
            ? width
            : null;
    }

    private static string? TakeDateTimePictureRunes(string value, int count)
    {
        var index = 0;
        var taken = 0;
        foreach (var rune in value.EnumerateRunes())
        {
            if (taken == count)
            {
                break;
            }
            index += rune.Utf16SequenceLength;
            taken++;
        }
        return taken == count ? value[..index] : null;
    }

    private static int DateTimePictureRuneCount(string value)
    {
        var count = 0;
        foreach (var _ in value.EnumerateRunes())
        {
            count++;
        }
        return count;
    }

    private static List<DateTimePicturePart>? DateTimePictureParts(string picture)
    {
        var parts = new List<DateTimePicturePart>();
        var cursor = 0;
        while (true)
        {
            var start = picture.IndexOf('[', cursor);
            if (start < 0)
            {
                break;
            }
            if (start > cursor)
            {
                parts.Add(new DateTimePictureLiteral(picture[cursor..start]));
            }
            var contentStart = start + 1;
            var end = picture.IndexOf(']', contentStart);
            if (end < 0)
            {
                return null;
            }
            var component = ParseDateTimePictureComponent(picture[contentStart..end]);
            if (component is null)
            {
                return null;
            }
            parts.Add(component);
            cursor = end + 1;
        }
        if (cursor < picture.Length)
        {
            parts.Add(new DateTimePictureLiteral(picture[cursor..]));
        }
        return parts.Count == 0 ? null : parts;
    }

    private static DateTimePictureComponent? ParseDateTimePictureComponent(string specification)
    {
        var comma = specification.IndexOf(',');
        var presentation = comma < 0 ? specification : specification[..comma];
        var width = comma < 0 ? null : specification[(comma + 1)..];

        DateTimePictureField field;
        string modifier;
        if (presentation.StartsWith('M'))
        {
            modifier = presentation[1..];
            field = modifier is "N" or "Nn" or "n"
                ? DateTimePictureField.MonthName
                : DateTimePictureField.Month;
        }
        else
        {
            var runes = presentation.EnumerateRunes().GetEnumerator();
            if (!runes.MoveNext())
            {
                return null;
            }
            var head = runes.Current;
            modifier = presentation[head.Utf16SequenceLength..];
            field = head.Value switch
            {
                'Y' => DateTimePictureField.Year,
                'D' => DateTimePictureField.Day,
                'd' => DateTimePictureField.DayOfYear,
                'H' => DateTimePictureField.Hour24,
                'h' => DateTimePictureField.Hour12,
                'P' => DateTimePictureField.Period,
                'm' => DateTimePictureField.Minute,
                's' => DateTimePictureField.Second,
                'f' => DateTimePictureField.Fraction,
                'Z' => DateTimePictureField.Timezone,
                'z' => DateTimePictureField.GmtTimezone,
                _ => (DateTimePictureField)(-1),
            };
            if ((int)field < 0)
            {
                return null;
            }
        }

        var (defaultMinimum, defaultMaximum) = field switch
        {
            DateTimePictureField.Year => (1, 9),
            DateTimePictureField.Month or
                DateTimePictureField.Day or
                DateTimePictureField.Hour24 or
                DateTimePictureField.Hour12 or
                DateTimePictureField.Minute or
                DateTimePictureField.Second => (1, 2),
            DateTimePictureField.DayOfYear => (1, 3),
            DateTimePictureField.Fraction => (1, 9),
            DateTimePictureField.MonthName => (3, 9),
            DateTimePictureField.Period => (2, 4),
            DateTimePictureField.Timezone => (1, 6),
            DateTimePictureField.GmtTimezone => (4, 9),
            _ => (0, 0),
        };
        int? presentationWidth;
        if (modifier is "" or "1" or "N" or "Nn" or "n")
        {
            presentationWidth = null;
        }
        else if (modifier.All(IsAsciiDigit))
        {
            presentationWidth = modifier.Length;
        }
        else
        {
            return null;
        }

        var parsedWidth = width is null
            ? (Minimum: defaultMinimum, Maximum: defaultMaximum)
            : ParseDateTimePictureWidth(width, defaultMaximum);
        if (parsedWidth is null)
        {
            return null;
        }
        var fixedWidth = parsedWidth.Value.Minimum == parsedWidth.Value.Maximum
            ? parsedWidth.Value.Minimum
            : presentationWidth;
        var letterCase = (field, modifier) switch
        {
            (DateTimePictureField.MonthName, "N") or
                (DateTimePictureField.Period, "N") => DateTimePictureLetterCase.Upper,
            (DateTimePictureField.MonthName, "n") or
                (DateTimePictureField.Period, "n") => DateTimePictureLetterCase.Lower,
            (DateTimePictureField.MonthName, "Nn") or
                (DateTimePictureField.Period, "Nn") => DateTimePictureLetterCase.Title,
            _ => (DateTimePictureLetterCase?)null,
        };
        return new DateTimePictureComponent(
            field,
            parsedWidth.Value.Minimum,
            parsedWidth.Value.Maximum,
            fixedWidth,
            letterCase);
    }

    private static (int Minimum, int Maximum)? ParseDateTimePictureWidth(
        string width,
        int naturalMaximum)
    {
        var dash = width.IndexOf('-');
        var minimumText = dash < 0 ? width : width[..dash];
        var maximumText = dash < 0
            ? naturalMaximum.ToString(CultureInfo.InvariantCulture)
            : width[(dash + 1)..];
        if (!int.TryParse(
                minimumText,
                NumberStyles.None,
                CultureInfo.InvariantCulture,
                out var minimum) ||
            !int.TryParse(
                maximumText,
                NumberStyles.None,
                CultureInfo.InvariantCulture,
                out var maximum) ||
            minimum <= 0 ||
            minimum > maximum)
        {
            return null;
        }
        return (minimum, maximum);
    }

    private static void AppendDateTimePictureSuffix(
        StringBuilder output,
        ParsedDateTimePicture parsed)
    {
        if (parsed.Fraction is not null)
        {
            output.Append('.');
            output.Append(parsed.Fraction);
        }
        if (parsed.Timezone is not null)
        {
            output.Append(parsed.Timezone);
        }
    }

    private static string FormatPictureDate(uint year, uint month, uint day) =>
        year.ToString("D4", CultureInfo.InvariantCulture) + "-" +
        month.ToString("D2", CultureInfo.InvariantCulture) + "-" +
        day.ToString("D2", CultureInfo.InvariantCulture);

    private static string FormatPictureTime(uint hour, uint minute, uint second) =>
        hour.ToString("D2", CultureInfo.InvariantCulture) + ":" +
        minute.ToString("D2", CultureInfo.InvariantCulture) + ":" +
        second.ToString("D2", CultureInfo.InvariantCulture);

    private static uint DateTimePictureNumber(string value, string function)
    {
        if (value.Length == 0 || !value.All(IsAsciiDigit) ||
            !uint.TryParse(
                value,
                NumberStyles.None,
                CultureInfo.InvariantCulture,
                out var number))
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }
        return number;
    }

    private static uint DateTimePictureMonthNumber(string value, string function)
    {
        string[] months =
        [
            "january",
            "february",
            "march",
            "april",
            "may",
            "june",
            "july",
            "august",
            "september",
            "october",
            "november",
            "december",
        ];
        var normalized = new string(value
            .Select(character => character is >= 'A' and <= 'Z'
                ? (char)(character + ('a' - 'A'))
                : character)
            .ToArray());
        var matches = months
            .Select((month, index) => (month, index))
            .Where(item => item.month.StartsWith(normalized, StringComparison.Ordinal))
            .ToArray();
        if (matches.Length != 1)
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }
        return (uint)matches[0].index + 1;
    }

    private static bool DateTimePicturePeriod(string value, string function)
    {
        var normalized = new string(value
            .Where(character => character is >= 'A' and <= 'Z' or >= 'a' and <= 'z')
            .Select(character => character is >= 'A' and <= 'Z'
                ? (char)(character + ('a' - 'A'))
                : character)
            .ToArray());
        return normalized switch
        {
            "am" => false,
            "pm" => true,
            _ => throw InvalidArgument(function, SupportedPictureDetail),
        };
    }

    private static string DateTimePictureTimezone(
        string value,
        bool requiresGmt,
        string function)
    {
        if (requiresGmt)
        {
            if (!value.StartsWith("GMT", StringComparison.Ordinal))
            {
                throw InvalidArgument(function, SupportedPictureDetail);
            }
            value = value[3..];
        }
        ValidateTimezone(value, function, SupportedPictureDetail);
        return value;
    }

    private static (uint Month, uint Day) DateTimePictureMonthDayFromOrdinal(
        uint year,
        uint ordinal,
        string function)
    {
        var remaining = ordinal;
        for (uint month = 1; month <= 12; month++)
        {
            var days = DateTimePictureDaysInMonth(year, month);
            if (remaining <= days)
            {
                return (month, remaining);
            }
            remaining = remaining > days ? remaining - days : 0;
        }
        throw InvalidArgument(function, SupportedPictureDetail);
    }

    private static uint DateTimePictureDaysInMonth(uint year, uint month)
    {
        var leap = year % 400 == 0 || year % 4 == 0 && year % 100 != 0;
        return month switch
        {
            1 or 3 or 5 or 7 or 8 or 10 or 12 => 31,
            4 or 6 or 9 or 11 => 30,
            2 when leap => 29,
            2 => 28,
            _ => 0,
        };
    }

    private static void SetDateTimePictureOnce<T>(
        ref T? slot,
        T value,
        string function)
        where T : struct
    {
        if (slot is not null)
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }
        slot = value;
    }

    private static void SetDateTimePictureOnce(
        ref string? slot,
        string value,
        string function)
    {
        if (slot is not null)
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }
        slot = value;
    }

    private sealed class ParsedDateTimePicture
    {
        private (uint Value, int Width)? _year;
        private uint? _month;
        private uint? _day;
        private uint? _dayOfYear;
        private uint? _hour24;
        private uint? _hour12;
        private bool? _period;
        private uint? _minute;
        private uint? _second;
        private string? _fraction;
        private string? _timezone;

        internal string? Fraction => _fraction;

        internal string? Timezone => _timezone;

        internal void Set(DateTimePictureField field, string value, string function)
        {
            switch (field)
            {
                case DateTimePictureField.Year:
                    SetDateTimePictureOnce(
                        ref _year,
                        (DateTimePictureNumber(value, function), value.Length),
                        function);
                    break;
                case DateTimePictureField.Month:
                    SetDateTimePictureOnce(
                        ref _month,
                        DateTimePictureNumber(value, function),
                        function);
                    break;
                case DateTimePictureField.MonthName:
                    SetDateTimePictureOnce(
                        ref _month,
                        DateTimePictureMonthNumber(value, function),
                        function);
                    break;
                case DateTimePictureField.Day:
                    SetDateTimePictureOnce(
                        ref _day,
                        DateTimePictureNumber(value, function),
                        function);
                    break;
                case DateTimePictureField.DayOfYear:
                    SetDateTimePictureOnce(
                        ref _dayOfYear,
                        DateTimePictureNumber(value, function),
                        function);
                    break;
                case DateTimePictureField.Hour24:
                    SetDateTimePictureOnce(
                        ref _hour24,
                        DateTimePictureNumber(value, function),
                        function);
                    break;
                case DateTimePictureField.Hour12:
                    SetDateTimePictureOnce(
                        ref _hour12,
                        DateTimePictureNumber(value, function),
                        function);
                    break;
                case DateTimePictureField.Period:
                    SetDateTimePictureOnce(
                        ref _period,
                        DateTimePicturePeriod(value, function),
                        function);
                    break;
                case DateTimePictureField.Minute:
                    SetDateTimePictureOnce(
                        ref _minute,
                        DateTimePictureNumber(value, function),
                        function);
                    break;
                case DateTimePictureField.Second:
                    SetDateTimePictureOnce(
                        ref _second,
                        DateTimePictureNumber(value, function),
                        function);
                    break;
                case DateTimePictureField.Fraction:
                    if (!value.All(IsAsciiDigit))
                    {
                        throw InvalidArgument(function, SupportedPictureDetail);
                    }
                    SetDateTimePictureOnce(ref _fraction, value, function);
                    break;
                case DateTimePictureField.Timezone:
                    SetDateTimePictureOnce(
                        ref _timezone,
                        DateTimePictureTimezone(value, requiresGmt: false, function),
                        function);
                    break;
                case DateTimePictureField.GmtTimezone:
                    SetDateTimePictureOnce(
                        ref _timezone,
                        DateTimePictureTimezone(value, requiresGmt: true, function),
                        function);
                    break;
                default:
                    throw InvalidArgument(function, SupportedPictureDetail);
            }
        }

        internal (uint Year, uint Month, uint Day) Date(string function)
        {
            if (_year is null)
            {
                throw InvalidArgument(function, SupportedPictureDetail);
            }
            var year = _year.Value.Value;
            if (_year.Value.Width == 2)
            {
                year += 2000;
            }

            uint month;
            uint day;
            if (_month is not null && _day is not null && _dayOfYear is null)
            {
                month = _month.Value;
                day = _day.Value;
            }
            else if (_month is null && _day is null && _dayOfYear is not null)
            {
                (month, day) = DateTimePictureMonthDayFromOrdinal(
                    year,
                    _dayOfYear.Value,
                    function);
            }
            else
            {
                throw InvalidArgument(function, SupportedPictureDetail);
            }
            if (year == 0 ||
                month is < 1 or > 12 ||
                day == 0 ||
                day > DateTimePictureDaysInMonth(year, month))
            {
                throw InvalidArgument(function, SupportedPictureDetail);
            }
            return (year, month, day);
        }

        internal (uint Hour, uint Minute, uint Second) Time(
            string function,
            bool allowDefault)
        {
            var hasTime = _hour24 is not null ||
                _hour12 is not null ||
                _minute is not null ||
                _second is not null;
            if (!allowDefault && !hasTime)
            {
                throw InvalidArgument(function, SupportedPictureDetail);
            }

            uint hour;
            if (_hour24 is not null && _hour12 is null && _period is null && _hour24 <= 23)
            {
                hour = _hour24.Value;
            }
            else if (_hour24 is null &&
                     _hour12 is not null &&
                     _period is not null &&
                     _hour12 is >= 1 and <= 12)
            {
                hour = (_hour12.Value % 12) + (_period.Value ? 12u : 0u);
            }
            else if (_hour24 is null && _hour12 is null && _period is null && allowDefault)
            {
                hour = 0;
            }
            else
            {
                throw InvalidArgument(function, SupportedPictureDetail);
            }

            var minute = _minute ?? 0;
            var second = _second ?? 0;
            if (minute > 59 || second > 59)
            {
                throw InvalidArgument(function, SupportedPictureDetail);
            }
            return (hour, minute, second);
        }
    }
}
