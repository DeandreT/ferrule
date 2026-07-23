using System.Globalization;

namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
    private const string DateTimeAddFunction = "datetime_add";
    private const string DateTimeAddInvalidDetail =
        "requires an xs:date or xs:dateTime followed by one or more xs:duration values";
    private const int DateTimeAddMaximumFractionDigits = 18;

    private enum DateTimeAddTemporalKind
    {
        Date,
        DateTime,
    }

    private sealed class DateTimeAddValue
    {
        private DateTimeAddTemporalKind _kind;
        private long _year;
        private uint _month;
        private uint _day;
        private uint _hour;
        private uint _minute;
        private uint _second;
        private Int128 _fraction;
        private int _fractionDigits;
        private string? _timezone;

        private DateTimeAddValue()
        {
        }

        public static DateTimeAddValue Parse(string value)
        {
            DateTimeAddTemporalKind kind;
            string date;
            string time;
            string? timezone;
            var separator = value.IndexOf('T', StringComparison.Ordinal);
            if (separator >= 0)
            {
                kind = DateTimeAddTemporalKind.DateTime;
                date = value[..separator];
                (time, timezone) = SplitIsoTimezone(
                    value[(separator + 1)..],
                    DateTimeAddFunction,
                    DateTimeAddInvalidDetail);
            }
            else
            {
                kind = DateTimeAddTemporalKind.Date;
                (date, timezone) = SplitIsoTimezone(
                    value,
                    DateTimeAddFunction,
                    DateTimeAddInvalidDetail);
                time = "00:00:00";
            }

            ValidateIsoDate(date, DateTimeAddFunction, DateTimeAddInvalidDetail);
            var endOfDay = kind == DateTimeAddTemporalKind.DateTime &&
                time.StartsWith("24:", StringComparison.Ordinal);
            if (endOfDay)
            {
                var normalized = "00" + time[2..];
                ValidateIsoTime(normalized, DateTimeAddFunction, DateTimeAddInvalidDetail);
                var minute = time[3..5];
                var secondPart = time[6..];
                var fractionSeparator = secondPart.IndexOf('.');
                var second = fractionSeparator >= 0
                    ? secondPart[..fractionSeparator]
                    : secondPart;
                var fraction = fractionSeparator >= 0
                    ? secondPart[(fractionSeparator + 1)..]
                    : string.Empty;
                if (minute != "00" ||
                    second != "00" ||
                    ContainsNonzeroDigit(fraction))
                {
                    throw DateTimeAddInvalid();
                }
            }
            else
            {
                ValidateIsoTime(time, DateTimeAddFunction, DateTimeAddInvalidDetail);
            }

            var yearEnd = date.Length - 6;
            if (!long.TryParse(
                    date[..yearEnd],
                    NumberStyles.AllowLeadingSign,
                    CultureInfo.InvariantCulture,
                    out var year))
            {
                throw DateTimeAddInvalid();
            }

            var wholeTime = time;
            var fractionText = string.Empty;
            var timeFractionSeparator = time.IndexOf('.');
            if (timeFractionSeparator >= 0)
            {
                wholeTime = time[..timeFractionSeparator];
                fractionText = time[(timeFractionSeparator + 1)..];
            }
            fractionText = fractionText.TrimEnd('0');
            if (fractionText.Length > DateTimeAddMaximumFractionDigits)
            {
                throw DateTimeAddInvalid();
            }

            return new DateTimeAddValue
            {
                _kind = kind,
                _year = year,
                _month = ParseUInt(date[(yearEnd + 1)..^3]),
                _day = ParseUInt(date[^2..]),
                _hour = ParseUInt(wholeTime[..2]),
                _minute = ParseUInt(wholeTime[3..5]),
                _second = ParseUInt(wholeTime[6..]),
                _fraction = ParseDateTimeAddDigits(fractionText),
                _fractionDigits = fractionText.Length,
                _timezone = timezone,
            };
        }

        public void Add(DateTimeAddDuration duration)
        {
            try
            {
                var signedMonths = duration.Negative
                    ? checked(-duration.Months)
                    : duration.Months;
                AddMonths(signedMonths);

                var digits = Math.Max(_fractionDigits, duration.FractionDigits);
                var scale = DateTimeAddPowerOfTen(digits);
                var currentScale = DateTimeAddPowerOfTen(_fractionDigits);
                var durationScale = DateTimeAddPowerOfTen(duration.FractionDigits);
                var ordinal = DateTimeAddDateOrdinal(_year, _month, _day);
                var seconds = checked(
                    checked(ordinal * 86_400) +
                    checked((Int128)_hour * 3_600) +
                    checked((Int128)_minute * 60) +
                    _second);
                var baseValue = checked(
                    checked(seconds * scale) +
                    checked(_fraction * (scale / currentScale)));
                var delta = checked(
                    checked(duration.Seconds * scale) +
                    checked(duration.Fraction * (scale / durationScale)));
                if (duration.Negative)
                {
                    delta = checked(-delta);
                }
                var total = checked(baseValue + delta);
                var dayUnits = checked(scale * 86_400);
                ordinal = DateTimeAddFloorDivide(total, dayUnits);
                var withinDay = DateTimeAddEuclideanRemainder(total, dayUnits);
                var wholeSeconds = withinDay / scale;
                (_year, _month, _day) = DateTimeAddDateFromOrdinal(ordinal);
                _hour = checked((uint)(wholeSeconds / 3_600));
                _minute = checked((uint)(wholeSeconds % 3_600 / 60));
                _second = checked((uint)(wholeSeconds % 60));
                _fraction = withinDay % scale;
                _fractionDigits = digits;
            }
            catch (OverflowException)
            {
                throw DateTimeAddInvalid();
            }
        }

        public string Render()
        {
            var magnitude = _year < 0
                ? (-(Int128)_year).ToString(CultureInfo.InvariantCulture)
                : _year.ToString(CultureInfo.InvariantCulture);
            var year = (_year < 0 ? "-" : string.Empty) + magnitude.PadLeft(4, '0');
            var output = $"{year}-{_month:00}-{_day:00}";
            if (_kind == DateTimeAddTemporalKind.DateTime)
            {
                output += $"T{_hour:00}:{_minute:00}:{_second:00}";
                if (_fraction != 0)
                {
                    var fraction = _fraction
                        .ToString(CultureInfo.InvariantCulture)
                        .PadLeft(_fractionDigits, '0')
                        .TrimEnd('0');
                    output += "." + fraction;
                }
            }
            return output + _timezone;
        }

        private void AddMonths(Int128 months)
        {
            try
            {
                var astronomicalYear = _year < 0 ? (Int128)_year + 1 : _year;
                var monthIndex = checked(
                    checked(astronomicalYear * 12) +
                    ((Int128)_month - 1) +
                    months);
                astronomicalYear = DateTimeAddFloorDivide(monthIndex, 12);
                var year = astronomicalYear <= 0
                    ? checked(astronomicalYear - 1)
                    : astronomicalYear;
                _year = checked((long)year);
                _month = checked((uint)(DateTimeAddEuclideanRemainder(monthIndex, 12) + 1));
                _day = Math.Min(_day, DateTimeAddDaysInMonth(_year, _month));
            }
            catch (OverflowException)
            {
                throw DateTimeAddInvalid();
            }
        }
    }

    private sealed class DateTimeAddDuration
    {
        public Int128 Months { get; private set; }

        public Int128 Seconds { get; private set; }

        public Int128 Fraction { get; private set; }

        public int FractionDigits { get; private set; }

        public bool Negative { get; private init; }

        public static DateTimeAddDuration Parse(string input)
        {
            var negative = input.StartsWith("-", StringComparison.Ordinal);
            var value = negative ? input[1..] : input;
            if (!value.StartsWith('P') || value.Length == 1)
            {
                throw DateTimeAddInvalid();
            }

            var result = new DateTimeAddDuration { Negative = negative };
            var index = 1;
            var inTime = false;
            var lastRank = 0;
            var sawValue = false;
            var timeValues = 0;
            while (index < value.Length)
            {
                if (value[index] == 'T')
                {
                    if (inTime || !sawValue && index != 1)
                    {
                        throw DateTimeAddInvalid();
                    }
                    inTime = true;
                    index++;
                    lastRank = 3;
                    continue;
                }

                var start = index;
                while (index < value.Length && IsAsciiDigit(value[index]))
                {
                    index++;
                }
                if (start == index)
                {
                    throw DateTimeAddInvalid();
                }
                var whole = ParseDateTimeAddDigits(value[start..index]);
                var fraction = string.Empty;
                if (index < value.Length && value[index] == '.')
                {
                    index++;
                    var fractionStart = index;
                    while (index < value.Length && IsAsciiDigit(value[index]))
                    {
                        index++;
                    }
                    if (fractionStart == index)
                    {
                        throw DateTimeAddInvalid();
                    }
                    fraction = value[fractionStart..index];
                }
                if (index >= value.Length)
                {
                    throw DateTimeAddInvalid();
                }

                var designator = value[index++];
                var rank = (inTime, designator) switch
                {
                    (false, 'Y') => 1,
                    (false, 'M') => 2,
                    (false, 'D') => 3,
                    (true, 'H') => 4,
                    (true, 'M') => 5,
                    (true, 'S') => 6,
                    _ => throw DateTimeAddInvalid(),
                };
                if (rank <= lastRank || fraction.Length > 0 && designator != 'S')
                {
                    throw DateTimeAddInvalid();
                }
                lastRank = rank;
                sawValue = true;
                if (inTime)
                {
                    timeValues++;
                }
                result.Set(rank, whole, fraction);
            }

            if (!sawValue || inTime && timeValues == 0)
            {
                throw DateTimeAddInvalid();
            }
            return result;
        }

        private void Set(int rank, Int128 whole, string fraction)
        {
            try
            {
                switch (rank)
                {
                    case 1:
                        Months = checked(whole * 12);
                        break;
                    case 2:
                        Months = checked(Months + whole);
                        break;
                    case 3:
                        Seconds = checked(whole * 86_400);
                        break;
                    case 4:
                        Seconds = checked(Seconds + checked(whole * 3_600));
                        break;
                    case 5:
                        Seconds = checked(Seconds + checked(whole * 60));
                        break;
                    case 6:
                        Seconds = checked(Seconds + whole);
                        fraction = fraction.TrimEnd('0');
                        if (fraction.Length > DateTimeAddMaximumFractionDigits)
                        {
                            throw DateTimeAddInvalid();
                        }
                        Fraction = ParseDateTimeAddDigits(fraction);
                        FractionDigits = fraction.Length;
                        break;
                    default:
                        throw DateTimeAddInvalid();
                }
            }
            catch (OverflowException)
            {
                throw DateTimeAddInvalid();
            }
        }
    }

    private static FerruleValue DateTimeAdd(IReadOnlyList<FerruleValue> arguments)
    {
        if (arguments.Count < 2)
        {
            throw Arity(DateTimeAddFunction, 2, arguments.Count);
        }
        if (arguments[0].Kind != FerruleValueKind.String)
        {
            throw Type(DateTimeAddFunction, arguments[0]);
        }

        var result = DateTimeAddValue.Parse(arguments[0].StringValue);
        var sawDuration = false;
        for (var index = 1; index < arguments.Count; index++)
        {
            var duration = arguments[index];
            if (duration.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
            {
                continue;
            }
            if (duration.Kind != FerruleValueKind.String)
            {
                throw Type(DateTimeAddFunction, duration);
            }
            sawDuration = true;
            result.Add(DateTimeAddDuration.Parse(duration.StringValue));
        }
        if (!sawDuration)
        {
            throw DateTimeAddInvalid();
        }
        return FerruleValue.FromString(result.Render());
    }

    private static Int128 DateTimeAddDateOrdinal(long year, uint month, uint day)
    {
        try
        {
            Int128 beforeYear;
            if (year > 0)
            {
                var years = (Int128)year - 1;
                beforeYear = checked(365 * years + years / 4 - years / 100 + years / 400);
            }
            else
            {
                var years = checked(-(Int128)year);
                beforeYear = checked(-(365 * years + years / 4 - years / 100 + years / 400));
            }

            Int128 beforeMonth = 0;
            for (uint currentMonth = 1; currentMonth < month; currentMonth++)
            {
                beforeMonth = checked(beforeMonth + DateTimeAddDaysInMonth(year, currentMonth));
            }
            return checked(beforeYear + beforeMonth + day - 1);
        }
        catch (OverflowException)
        {
            throw DateTimeAddInvalid();
        }
    }

    private static (long Year, uint Month, uint Day) DateTimeAddDateFromOrdinal(Int128 ordinal)
    {
        var low = long.MinValue;
        var high = long.MaxValue;
        while (low < high)
        {
            var distance = (Int128)high - low;
            var middle = checked((long)((Int128)low + (distance + 1) / 2));
            if (DateTimeAddDateOrdinal(middle, 1, 1) <= ordinal)
            {
                low = middle;
            }
            else
            {
                high = middle - 1;
            }
        }
        if (low == 0)
        {
            throw DateTimeAddInvalid();
        }

        var remaining = ordinal - DateTimeAddDateOrdinal(low, 1, 1);
        for (uint month = 1; month <= 12; month++)
        {
            var days = (Int128)DateTimeAddDaysInMonth(low, month);
            if (remaining < days)
            {
                return (low, month, checked((uint)(remaining + 1)));
            }
            remaining -= days;
        }
        throw DateTimeAddInvalid();
    }

    private static uint DateTimeAddDaysInMonth(long year, uint month) => month switch
    {
        1 or 3 or 5 or 7 or 8 or 10 or 12 => 31,
        4 or 6 or 9 or 11 => 30,
        2 when year % 400 == 0 || year % 4 == 0 && year % 100 != 0 => 29,
        2 => 28,
        _ => 0,
    };

    private static Int128 ParseDateTimeAddDigits(string value)
    {
        if (value.Length == 0)
        {
            return 0;
        }
        if (!Int128.TryParse(
                value,
                NumberStyles.None,
                CultureInfo.InvariantCulture,
                out var parsed))
        {
            throw DateTimeAddInvalid();
        }
        return parsed;
    }

    private static Int128 DateTimeAddPowerOfTen(int digits)
    {
        Int128 result = 1;
        try
        {
            for (var index = 0; index < digits; index++)
            {
                result = checked(result * 10);
            }
        }
        catch (OverflowException)
        {
            throw DateTimeAddInvalid();
        }
        return result;
    }

    private static Int128 DateTimeAddFloorDivide(Int128 value, Int128 divisor)
    {
        var quotient = value / divisor;
        var remainder = value % divisor;
        return remainder < 0 ? quotient - 1 : quotient;
    }

    private static Int128 DateTimeAddEuclideanRemainder(Int128 value, Int128 divisor)
    {
        var remainder = value % divisor;
        return remainder < 0 ? remainder + divisor : remainder;
    }

    private static bool ContainsNonzeroDigit(string value)
    {
        foreach (var character in value)
        {
            if (character != '0')
            {
                return true;
            }
        }
        return false;
    }

    private static FerruleRuntimeException DateTimeAddInvalid() =>
        InvalidArgument(DateTimeAddFunction, DateTimeAddInvalidDetail);
}
