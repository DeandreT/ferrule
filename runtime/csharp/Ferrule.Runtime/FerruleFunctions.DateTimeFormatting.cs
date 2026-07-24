using System.Globalization;
using System.Text;

namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
    private sealed class FormattableDateTimePicture
    {
        internal required string Year { get; init; }

        internal required uint Month { get; init; }

        internal required uint Day { get; init; }

        internal uint Hour { get; set; }

        internal uint Minute { get; set; }

        internal uint Second { get; set; }

        internal string? Fraction { get; set; }

        internal string? Timezone { get; set; }

        internal uint DayOfYear()
        {
            uint ordinal = Day;
            for (uint month = 1; month < Month; month++)
            {
                ordinal += DaysInMonth(Year, month);
            }
            return ordinal;
        }
    }

    private enum DateTimePictureInput
    {
        Date,
        DateTime,
        Time,
    }

    private static FerruleValue FormatDate(IReadOnlyList<FerruleValue> arguments) =>
        FormatDateTimePictureValue(arguments, "format_date", DateTimePictureInput.Date);

    private static FerruleValue FormatDateTime(IReadOnlyList<FerruleValue> arguments) =>
        FormatDateTimePictureValue(arguments, "format_datetime", DateTimePictureInput.DateTime);

    private static FerruleValue FormatTime(IReadOnlyList<FerruleValue> arguments) =>
        FormatDateTimePictureValue(arguments, "format_time", DateTimePictureInput.Time);

    private static FerruleValue FormatDateTimePictureValue(
        IReadOnlyList<FerruleValue> arguments,
        string function,
        DateTimePictureInput input)
    {
        var (value, picture) = FormatDateTimePictureArguments(arguments, function);
        FormattableDateTimePicture fields;
        if (input == DateTimePictureInput.Date)
        {
            var (date, timezone) = SplitIsoTimezone(value, function, DateOrDateTimeDetail);
            ValidateIsoDate(date, function, DateOrDateTimeDetail);
            fields = DateTimePictureDate(date);
            fields.Timezone = timezone;
        }
        else if (input == DateTimePictureInput.DateTime)
        {
            var separator = value.IndexOf('T', StringComparison.Ordinal);
            if (separator < 0)
            {
                throw InvalidArgument(function, SupportedPictureDetail);
            }
            var date = value[..separator];
            var time = value[(separator + 1)..];
            ValidateIsoDate(date, function, DateTimeDetail);
            ValidateIsoTime(time, function, DateTimeDetail);
            fields = DateTimePictureDate(date);
            SetDateTimePictureTime(fields, time, function);
        }
        else
        {
            ValidateIsoTime(value, function, DateTimeDetail);
            fields = new FormattableDateTimePicture
            {
                Year = "2000",
                Month = 1,
                Day = 1,
            };
            SetDateTimePictureTime(fields, value, function);
        }

        return FerruleValue.FromString(RenderDateTimePicture(fields, picture, function));
    }

    private static (string Value, string Picture) FormatDateTimePictureArguments(
        IReadOnlyList<FerruleValue> arguments,
        string function)
    {
        if (arguments.Count is < 2 or > 5)
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
        foreach (var argument in arguments.Skip(2))
        {
            if (argument.Kind is FerruleValueKind.Null or FerruleValueKind.JsonNull)
            {
                continue;
            }
            if (argument.Kind == FerruleValueKind.String && argument.StringValue.Length == 0)
            {
                continue;
            }
            throw InvalidArgument(
                function,
                "supports only default language, calendar, and place arguments");
        }
        return (arguments[0].StringValue, arguments[1].StringValue);
    }

    private static FormattableDateTimePicture DateTimePictureDate(string date)
    {
        var yearEnd = date.Length - 6;
        return new FormattableDateTimePicture
        {
            Year = date[..yearEnd],
            Month = ParseUInt(date[(yearEnd + 1)..^3]),
            Day = ParseUInt(date[^2..]),
        };
    }

    private static void SetDateTimePictureTime(
        FormattableDateTimePicture fields,
        string value,
        string function)
    {
        var (time, timezone) = SplitIsoTimezone(value, function, DateTimeDetail);
        var fractionStart = time.IndexOf('.');
        var whole = fractionStart < 0 ? time : time[..fractionStart];
        fields.Hour = ParseUInt(whole[..2]);
        fields.Minute = ParseUInt(whole[3..5]);
        fields.Second = ParseUInt(whole[6..]);
        fields.Fraction = fractionStart < 0 ? null : time[(fractionStart + 1)..];
        fields.Timezone = timezone;
    }

    private static string RenderDateTimePicture(
        FormattableDateTimePicture fields,
        string picture,
        string function)
    {
        var parts = DateTimePictureParts(picture);
        if (parts is null)
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }
        var output = new StringBuilder();
        foreach (var part in parts)
        {
            if (part is DateTimePictureLiteral literal)
            {
                output.Append(literal.Value);
            }
            else
            {
                output.Append(RenderDateTimePictureComponent(
                    fields,
                    (DateTimePictureComponent)part,
                    function));
            }
        }
        return output.ToString();
    }

    private static string RenderDateTimePictureComponent(
        FormattableDateTimePicture fields,
        DateTimePictureComponent component,
        string function)
    {
        string Numeric(uint value) => RenderDateTimePictureNumber(
            value.ToString(CultureInfo.InvariantCulture),
            component,
            truncateLeft: false,
            function);

        return component.Field switch
        {
            DateTimePictureField.Year => RenderDateTimePictureNumber(
                fields.Year,
                component,
                truncateLeft: true,
                function),
            DateTimePictureField.Month => Numeric(fields.Month),
            DateTimePictureField.MonthName => RenderDateTimePictureMonth(
                fields.Month,
                component,
                function),
            DateTimePictureField.Day => Numeric(fields.Day),
            DateTimePictureField.DayOfYear => Numeric(fields.DayOfYear()),
            DateTimePictureField.Hour24 => Numeric(fields.Hour),
            DateTimePictureField.Hour12 => Numeric(fields.Hour % 12 == 0 ? 12 : fields.Hour % 12),
            DateTimePictureField.Period => RenderDateTimePicturePeriod(fields.Hour, component),
            DateTimePictureField.Minute => Numeric(fields.Minute),
            DateTimePictureField.Second => Numeric(fields.Second),
            DateTimePictureField.Fraction => RenderDateTimePictureNumber(
                fields.Fraction ?? "0",
                component,
                truncateLeft: false,
                function),
            DateTimePictureField.Timezone => fields.Timezone ?? string.Empty,
            DateTimePictureField.GmtTimezone => fields.Timezone is null
                ? string.Empty
                : "GMT" + fields.Timezone,
            _ => throw InvalidArgument(function, SupportedPictureDetail),
        };
    }

    private static string RenderDateTimePictureMonth(
        uint month,
        DateTimePictureComponent component,
        string function)
    {
        string[] months =
        [
            "January",
            "February",
            "March",
            "April",
            "May",
            "June",
            "July",
            "August",
            "September",
            "October",
            "November",
            "December",
        ];
        if (month is < 1 or > 12)
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }
        var value = months[month - 1];
        value = component.LetterCase switch
        {
            DateTimePictureLetterCase.Upper => value.ToUpperInvariant(),
            DateTimePictureLetterCase.Lower => value.ToLowerInvariant(),
            _ => value,
        };
        if (component.FixedWidth is int width && value.Length > width)
        {
            value = value[..width];
        }
        if (value.Length < component.MinimumWidth || value.Length > component.MaximumWidth)
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }
        return value;
    }

    private static string RenderDateTimePicturePeriod(
        uint hour,
        DateTimePictureComponent component)
    {
        var value = hour >= 12 ? "PM" : "AM";
        return component.LetterCase switch
        {
            DateTimePictureLetterCase.Lower => value.ToLowerInvariant(),
            DateTimePictureLetterCase.Title => value[..1] + value[1..].ToLowerInvariant(),
            _ => value,
        };
    }

    private static string RenderDateTimePictureNumber(
        string value,
        DateTimePictureComponent component,
        bool truncateLeft,
        string function)
    {
        var negative = value.StartsWith("-", StringComparison.Ordinal);
        var digits = negative ? value[1..] : value;
        if (digits.Length == 0 || !digits.All(IsAsciiDigit))
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }
        if (component.FixedWidth is int width)
        {
            if (digits.Length > width)
            {
                if (!truncateLeft)
                {
                    throw InvalidArgument(function, SupportedPictureDetail);
                }
                digits = digits[^width..];
            }
            else if (digits.Length < width)
            {
                digits = digits.PadLeft(width, '0');
            }
        }
        if (digits.Length < component.MinimumWidth || digits.Length > component.MaximumWidth)
        {
            throw InvalidArgument(function, SupportedPictureDetail);
        }
        return negative ? "-" + digits : digits;
    }
}
