using System.Globalization;
using System.Numerics;

namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
    private const string DateOrDateTimeDetail = "requires a valid ISO date or dateTime";
    private const string DateTimeDetail = "requires a valid ISO dateTime";
    private const string SupportedPictureDetail =
        "requires a value matching a supported date/time picture";

    private readonly record struct LocalDate(
        string Year,
        uint Month,
        uint Day,
        bool EndOfDay)
    {
        public bool RollsYear => EndOfDay && Month == 12 && Day == 31;

        public (uint Month, uint Day) NormalizedMonthDay()
        {
            if (!EndOfDay)
            {
                return (Month, Day);
            }

            var lastDay = DaysInMonth(Year, Month);
            if (Day < lastDay)
            {
                return (Month, Day + 1);
            }
            return Month < 12 ? (Month + 1, 1u) : (1u, 1u);
        }
    }

    private readonly record struct LocalTime(string Value, bool EndOfDay);

    private static FerruleValue YearFromDateTime(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "year_from_datetime";
        var value = NullableStringArgument(arguments, function);
        if (value is null)
        {
            return FerruleValue.Null;
        }

        var date = ValidatedLocalDate(value, function);
        if (!long.TryParse(date.Year, out var year))
        {
            throw InvalidArgument(function, "requires a year within the signed 64-bit integer range");
        }
        if (date.RollsYear)
        {
            if (year == -1)
            {
                year = 1;
            }
            else if (year == long.MaxValue)
            {
                throw InvalidArgument(function, "requires a year within the signed 64-bit integer range");
            }
            else
            {
                year++;
            }
        }

        return FerruleValue.FromInt64(year);
    }

    private static FerruleValue MonthFromDateTime(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "month_from_datetime";
        var value = NullableStringArgument(arguments, function);
        if (value is null)
        {
            return FerruleValue.Null;
        }

        var (month, _) = ValidatedLocalDate(value, function).NormalizedMonthDay();
        return FerruleValue.FromInt64(month);
    }

    private static FerruleValue DayFromDateTime(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "day_from_datetime";
        var value = NullableStringArgument(arguments, function);
        if (value is null)
        {
            return FerruleValue.Null;
        }

        var (_, day) = ValidatedLocalDate(value, function).NormalizedMonthDay();
        return FerruleValue.FromInt64(day);
    }

    private static FerruleValue HoursFromDateTime(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "hours_from_datetime";
        var value = NullableStringArgument(arguments, function);
        if (value is null)
        {
            return FerruleValue.Null;
        }

        var time = ValidatedLocalDateTimeTime(value, function);
        return FerruleValue.FromInt64(time.EndOfDay ? 0 : ParseUInt(time.Value[..2]));
    }

    private static FerruleValue MinutesFromDateTime(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "minutes_from_datetime";
        var value = NullableStringArgument(arguments, function);
        if (value is null)
        {
            return FerruleValue.Null;
        }

        var time = ValidatedLocalDateTimeTime(value, function);
        return FerruleValue.FromInt64(time.EndOfDay ? 0 : ParseUInt(time.Value[3..5]));
    }

    private static FerruleValue TimeFromDateTime(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "time_from_datetime";
        RequireArity(function, arguments, 1);
        var value = RequireString(arguments[0], function);
        var separator = value.IndexOf('T', StringComparison.Ordinal);
        if (separator < 0)
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }

        var date = value[..separator];
        var time = value[(separator + 1)..];
        ValidateIsoDate(date, function, SupportedPictureDetail);
        ValidateIsoTime(time, function, SupportedPictureDetail);
        return FerruleValue.FromString(time);
    }

    private static FerruleValue DateTimeFromDateAndTime(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "datetime_from_date_and_time";
        string date;
        string time;
        if (arguments.Count == 1)
        {
            date = RequireString(arguments[0], function);
            time = "00:00:00";
        }
        else if (arguments.Count == 2)
        {
            if (arguments[0].Kind != FerruleValueKind.String)
            {
                throw Type(function, arguments[0]);
            }
            date = arguments[0].StringValue;
            time = arguments[1].Kind switch
            {
                FerruleValueKind.String => arguments[1].StringValue,
                FerruleValueKind.Null => "00:00:00",
                _ => throw Type(function, arguments[1]),
            };
        }
        else
        {
            throw Arity(function, 1, arguments.Count);
        }

        var (dateValue, dateZone) = SplitIsoTimezone(date, function, SupportedPictureDetail);
        var (timeValue, timeZone) = SplitIsoTimezone(time, function, SupportedPictureDetail);
        ValidateIsoDate(dateValue, function, SupportedPictureDetail);
        ValidateIsoTime(timeValue, function, SupportedPictureDetail);
        if (dateZone is not null &&
            timeZone is not null &&
            DateTimeTimezoneOffset(dateZone) != DateTimeTimezoneOffset(timeZone))
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }

        var zone = dateZone ?? timeZone;
        return FerruleValue.FromString($"{dateValue}T{timeValue}{zone}");
    }

    private static FerruleValue CoerceDateTime(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "coerce_datetime";
        RequireArity(function, arguments, 1);
        if (arguments[0].Kind is FerruleValueKind.Null or FerruleValueKind.XmlNil)
        {
            return arguments[0];
        }
        var value = RequireString(arguments[0], function);
        var separator = value.IndexOf('T', StringComparison.Ordinal);
        if (separator >= 0)
        {
            ValidateIsoDate(value[..separator], function, SupportedPictureDetail);
            ValidateIsoTime(value[(separator + 1)..], function, SupportedPictureDetail);
            return arguments[0];
        }

        var (date, timezone) = SplitIsoTimezone(value, function, SupportedPictureDetail);
        ValidateIsoDate(date, function, SupportedPictureDetail);
        return FerruleValue.FromString($"{date}T00:00:00{timezone}");
    }

    private static FerruleValue DateTimeFromParts(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "datetime_from_parts";
        if (arguments.Count is < 3 or > 8)
        {
            throw Arity(function, 3, arguments.Count);
        }

        var year = DateTimeIntegerPart(arguments[0], function);
        var month = DateTimeIntegerPart(arguments[1], function);
        var day = DateTimeIntegerPart(arguments[2], function);
        long OptionalInteger(int index) => index >= arguments.Count ||
            arguments[index].Kind == FerruleValueKind.Null
                ? 0
                : DateTimeIntegerPart(arguments[index], function);
        var hour = OptionalInteger(3);
        var minute = OptionalInteger(4);
        var second = OptionalInteger(5);
        var millisecond = arguments.Count <= 6 || arguments[6].Kind == FerruleValueKind.Null
            ? 0.0
            : DateTimeDecimalPart(arguments[6], function);
        long? timezone = arguments.Count <= 7 || arguments[7].Kind == FerruleValueKind.Null
            ? null
            : DateTimeIntegerPart(arguments[7], function);

        if (month is < 0 or > uint.MaxValue ||
            day is < 0 or > uint.MaxValue ||
            hour is < 0 or > 23 ||
            minute is < 0 or > 59 ||
            second is < 0 or > 59 ||
            !double.IsFinite(millisecond) ||
            millisecond is < 0.0 or >= 1000.0)
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }

        var yearText = FormatDateTimeYear(year);
        var date = $"{yearText}-{month:00}-{day:00}";
        ValidateIsoDate(date, function, SupportedPictureDetail);
        var output = $"{date}T{hour:00}:{minute:00}:{second:00}";
        if (millisecond != 0.0)
        {
            var fraction = (millisecond / 1000.0)
                .ToString("F15", CultureInfo.InvariantCulture)
                .TrimStart('0')
                .TrimEnd('0');
            if (fraction != ".")
            {
                output += fraction;
            }
        }

        if (timezone is not null && timezone != -32_768)
        {
            if (timezone is < -840 or > 840)
            {
                throw InvalidArgument(function, SupportedPictureDetail);
            }
            if (timezone == 0)
            {
                output += "Z";
            }
            else
            {
                var sign = timezone < 0 ? '-' : '+';
                var absolute = Math.Abs(timezone.Value);
                output += $"{sign}{absolute / 60:00}:{absolute % 60:00}";
            }
        }
        return FerruleValue.FromString(output);
    }

    private static long DateTimeIntegerPart(FerruleValue value, string function)
    {
        if (value.Kind == FerruleValueKind.Int64)
        {
            return value.Int64Value;
        }
        if (value.Kind == FerruleValueKind.Double)
        {
            var number = value.DoubleValue;
            if (double.IsFinite(number) &&
                Math.Truncate(number) == number &&
                number >= long.MinValue &&
                number < -(double)long.MinValue)
            {
                return (long)number;
            }
            throw Type(function, value);
        }
        if (value.Kind == FerruleValueKind.String &&
            long.TryParse(
                TrimRustWhitespace(value.StringValue),
                NumberStyles.AllowLeadingSign,
                CultureInfo.InvariantCulture,
                out var parsed))
        {
            return parsed;
        }
        throw Type(function, value);
    }

    private static double DateTimeDecimalPart(FerruleValue value, string function)
    {
        if (value.Kind == FerruleValueKind.Int64)
        {
            return value.Int64Value;
        }
        if (value.Kind == FerruleValueKind.Double)
        {
            return value.DoubleValue;
        }
        if (value.Kind == FerruleValueKind.String)
        {
            var text = TrimRustWhitespace(value.StringValue);
            if (text is "inf" or "+inf")
            {
                return double.PositiveInfinity;
            }
            if (text == "-inf")
            {
                return double.NegativeInfinity;
            }
            if (text == "NaN")
            {
                return double.NaN;
            }
            if (double.TryParse(
                    text,
                    NumberStyles.Float,
                    CultureInfo.InvariantCulture,
                    out var parsed))
            {
                return parsed;
            }
        }
        throw Type(function, value);
    }

    private static string FormatDateTimeYear(long year)
    {
        var magnitude = BigInteger.Abs(new BigInteger(year))
            .ToString(CultureInfo.InvariantCulture)
            .PadLeft(4, '0');
        return year < 0 ? "-" + magnitude : magnitude;
    }

    private static int DateTimeTimezoneOffset(string timezone)
    {
        if (timezone == "Z")
        {
            return 0;
        }
        var sign = timezone[0] == '-' ? -1 : 1;
        return sign * ((int)ParseUInt(timezone[1..3]) * 60 + (int)ParseUInt(timezone[4..]));
    }

    private static string? NullableStringArgument(
        IReadOnlyList<FerruleValue> arguments,
        string function)
    {
        RequireArity(function, arguments, 1);
        if (arguments[0].Kind == FerruleValueKind.Null)
        {
            return null;
        }
        return RequireString(arguments[0], function);
    }

    private static LocalDate ValidatedLocalDate(string value, string function)
    {
        if (!IsAscii(value))
        {
            throw InvalidArgument(function, DateOrDateTimeDetail);
        }

        string date;
        bool endOfDay;
        var separator = value.IndexOf('T', StringComparison.Ordinal);
        if (separator >= 0)
        {
            date = value[..separator];
            endOfDay = ValidatedLocalTime(value[(separator + 1)..], function).EndOfDay;
        }
        else
        {
            (date, _) = SplitIsoTimezone(value, function, DateOrDateTimeDetail);
            endOfDay = false;
        }

        ValidateIsoDate(date, function, DateOrDateTimeDetail);
        var yearEnd = date.Length - 6;
        return new LocalDate(
            date[..yearEnd],
            ParseUInt(date[(yearEnd + 1)..(date.Length - 3)]),
            ParseUInt(date[^2..]),
            endOfDay);
    }

    private static LocalTime ValidatedLocalDateTimeTime(string value, string function)
    {
        var separator = value.IndexOf('T', StringComparison.Ordinal);
        if (separator < 0)
        {
            throw InvalidArgument(function, DateTimeDetail);
        }

        ValidateIsoDate(value[..separator], function, DateTimeDetail);
        return ValidatedLocalTime(value[(separator + 1)..], function);
    }

    private static LocalTime ValidatedLocalTime(string value, string function)
    {
        if (!IsAscii(value))
        {
            throw InvalidArgument(function, DateTimeDetail);
        }

        var (time, _) = SplitIsoTimezone(value, function, DateTimeDetail);
        var endOfDay = time.StartsWith("24:", StringComparison.Ordinal);
        if (endOfDay)
        {
            var normalized = "00" + time[2..];
            ValidateIsoTime(normalized, function, DateTimeDetail);
            var first = time.IndexOf(':');
            var second = first >= 0 ? time.IndexOf(':', first + 1) : -1;
            if (first < 0 || second < 0)
            {
                throw InvalidArgument(function, DateTimeDetail);
            }

            var minute = time[(first + 1)..second];
            var secondPart = time[(second + 1)..];
            var fractionStart = secondPart.IndexOf('.');
            var seconds = fractionStart >= 0 ? secondPart[..fractionStart] : secondPart;
            var fraction = fractionStart >= 0 ? secondPart[(fractionStart + 1)..] : string.Empty;
            if (minute != "00" ||
                seconds != "00" ||
                fraction.Any(character => character != '0'))
            {
                throw InvalidArgument(function, DateTimeDetail);
            }
        }
        else
        {
            ValidateIsoTime(value, function, DateTimeDetail);
        }

        return new LocalTime(time, endOfDay);
    }

    private static void ValidateIsoDate(string value, string function, string detail)
    {
        if (!IsAscii(value) || value.Length < 10)
        {
            throw InvalidArgument(function, detail);
        }

        var yearEnd = value.Length - 6;
        if (value[yearEnd] != '-' || value[^3] != '-')
        {
            throw InvalidArgument(function, detail);
        }

        var year = value[..yearEnd];
        var digits = year.StartsWith("-", StringComparison.Ordinal) ? year[1..] : year;
        if (digits.Length < 4 ||
            !digits.All(IsAsciiDigit) ||
            digits.All(character => character == '0') ||
            digits.Length > 4 && digits[0] == '0')
        {
            throw InvalidArgument(function, detail);
        }

        var monthText = value[(yearEnd + 1)..^3];
        var dayText = value[^2..];
        if (!monthText.All(IsAsciiDigit) || !dayText.All(IsAsciiDigit))
        {
            throw InvalidArgument(function, detail);
        }

        var month = ParseUInt(monthText);
        var day = ParseUInt(dayText);
        if (month is < 1 or > 12 || day == 0 || day > DaysInMonth(year, month))
        {
            throw InvalidArgument(function, detail);
        }
    }

    private static void ValidateIsoTime(string value, string function, string detail)
    {
        if (!IsAscii(value))
        {
            throw InvalidArgument(function, detail);
        }

        var timezoneStart = value.Length;
        for (var index = 1; index < value.Length; index++)
        {
            if (value[index] is 'Z' or '+' or '-')
            {
                timezoneStart = index;
                break;
            }
        }

        var time = value[..timezoneStart];
        var zone = value[timezoneStart..];
        var fractionStart = time.IndexOf('.');
        var whole = fractionStart >= 0 ? time[..fractionStart] : time;
        var fraction = fractionStart >= 0 ? time[(fractionStart + 1)..] : null;
        if (whole.Length != 8 || whole[2] != ':' || whole[5] != ':')
        {
            throw InvalidArgument(function, detail);
        }

        var hourText = whole[..2];
        var minuteText = whole[3..5];
        var secondText = whole[6..];
        if (!hourText.All(IsAsciiDigit) ||
            !minuteText.All(IsAsciiDigit) ||
            !secondText.All(IsAsciiDigit))
        {
            throw InvalidArgument(function, detail);
        }

        var hour = ParseUInt(hourText);
        var minute = ParseUInt(minuteText);
        var second = ParseUInt(secondText);
        if (hour > 23 || minute > 59 || second > 59)
        {
            throw InvalidArgument(function, detail);
        }
        if (fraction is not null && (fraction.Length == 0 || !fraction.All(IsAsciiDigit)))
        {
            throw InvalidArgument(function, detail);
        }
        if (zone.Length > 0)
        {
            ValidateTimezone(zone, function, detail);
        }
    }

    private static (string Value, string? Zone) SplitIsoTimezone(
        string value,
        string function,
        string detail)
    {
        if (value.EndsWith('Z'))
        {
            return (value[..^1], "Z");
        }
        if (value.Length >= 6)
        {
            var start = value.Length - 6;
            var candidate = value[start..];
            if (candidate[0] is '+' or '-' && candidate[3] == ':')
            {
                ValidateTimezone(candidate, function, detail);
                return (value[..start], candidate);
            }
        }

        return (value, null);
    }

    private static void ValidateTimezone(string value, string function, string detail)
    {
        if (value == "Z")
        {
            return;
        }
        if (value.Length != 6 ||
            value[0] is not ('+' or '-') ||
            value[3] != ':' ||
            !value[1..3].All(IsAsciiDigit) ||
            !value[4..6].All(IsAsciiDigit))
        {
            throw InvalidArgument(function, detail);
        }

        var hour = ParseUInt(value[1..3]);
        var minute = ParseUInt(value[4..6]);
        if (hour > 14 || minute > 59 || hour == 14 && minute != 0)
        {
            throw InvalidArgument(function, detail);
        }
    }

    private static uint DaysInMonth(string year, uint month)
    {
        var digits = year.StartsWith("-", StringComparison.Ordinal) ? year[1..] : year;
        var leap = DecimalMod(digits, 400) == 0 ||
            DecimalMod(digits, 4) == 0 && DecimalMod(digits, 100) != 0;
        return month switch
        {
            1 or 3 or 5 or 7 or 8 or 10 or 12 => 31,
            4 or 6 or 9 or 11 => 30,
            2 when leap => 29,
            2 => 28,
            _ => 0,
        };
    }

    private static uint DecimalMod(string digits, uint modulus)
    {
        uint value = 0;
        foreach (var digit in digits)
        {
            value = (value * 10 + digit - '0') % modulus;
        }
        return value;
    }

    private static bool IsAscii(string value)
    {
        foreach (var character in value)
        {
            if (character > 0x7f)
            {
                return false;
            }
        }
        return true;
    }

    private static bool IsAsciiDigit(char character) => character is >= '0' and <= '9';

    private static uint ParseUInt(string value)
    {
        uint result = 0;
        foreach (var character in value)
        {
            result = result * 10 + (uint)(character - '0');
        }
        return result;
    }
}
