namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
    private const string EdifactDateTimeInvalidDetail =
        "requires a value matching its UN/EDIFACT 2379 date/time format code";
    private const string EdifactDateTimeUnsupportedDetail =
        "supports UN/EDIFACT 2379 codes 102, 203, 204, 205, 303, and 304";
    private const string EdifactZoneUnsupportedDetail =
        "supports UTC, GMT, EST, EDT, CST, CDT, MST, MDT, PST, and PDT named zones";

    private static FerruleValue EdifactToDateTime(IReadOnlyList<FerruleValue> arguments)
    {
        const string function = "edifact_to_datetime";
        RequireArity(function, arguments, 2);
        var value = RequireString(arguments[0], function);
        var code = RequireString(arguments[1], function);
        if (!IsAscii(value))
        {
            throw InvalidArgument(function, EdifactDateTimeInvalidDetail);
        }

        int baseLength;
        bool hasSeconds;
        string zone;
        switch (code)
        {
            case "102" when value.Length == 8:
                baseLength = 8;
                hasSeconds = false;
                zone = string.Empty;
                break;
            case "203" when value.Length == 12:
                baseLength = 12;
                hasSeconds = false;
                zone = string.Empty;
                break;
            case "204" when value.Length == 14:
                baseLength = 14;
                hasSeconds = true;
                zone = string.Empty;
                break;
            case "205":
                if (value.Length != 17)
                {
                    throw InvalidArgument(function, EdifactDateTimeInvalidDetail);
                }
                baseLength = 12;
                hasSeconds = false;
                zone = NumericEdifactZone(value[12..]);
                break;
            case "303":
                if (value.Length != 15)
                {
                    throw InvalidArgument(function, EdifactDateTimeInvalidDetail);
                }
                baseLength = 12;
                hasSeconds = false;
                zone = NamedEdifactZone(value[12..]);
                break;
            case "304":
                if (value.Length != 17)
                {
                    throw InvalidArgument(function, EdifactDateTimeInvalidDetail);
                }
                baseLength = 14;
                hasSeconds = true;
                zone = NamedEdifactZone(value[14..]);
                break;
            case "102" or "203" or "204":
                throw InvalidArgument(function, EdifactDateTimeInvalidDetail);
            default:
                throw InvalidArgument(function, EdifactDateTimeUnsupportedDetail);
        }

        if (value.Length < baseLength)
        {
            throw InvalidArgument(function, EdifactDateTimeInvalidDetail);
        }
        var baseValue = value[..baseLength];
        if (!baseValue.All(IsAsciiDigit))
        {
            throw InvalidArgument(function, EdifactDateTimeInvalidDetail);
        }

        var date = $"{baseValue[..4]}-{baseValue[4..6]}-{baseValue[6..8]}";
        ValidateIsoDate(date, function, EdifactDateTimeInvalidDetail);
        string time;
        if (baseLength == 8)
        {
            time = "00:00:00";
        }
        else
        {
            var seconds = hasSeconds ? baseValue[12..14] : "00";
            time = $"{baseValue[8..10]}:{baseValue[10..12]}:{seconds}";
        }
        ValidateIsoTime(time + zone, function, EdifactDateTimeInvalidDetail);
        return FerruleValue.FromString($"{date}T{time}{zone}");
    }

    private static string NumericEdifactZone(string zone)
    {
        const string function = "edifact_to_datetime";
        if (zone.Length != 5 ||
            zone[0] is not ('+' or '-') ||
            !zone[1..].All(IsAsciiDigit))
        {
            throw InvalidArgument(function, EdifactDateTimeInvalidDetail);
        }
        if (zone[1..] == "0000")
        {
            return "Z";
        }
        return $"{zone[0]}{zone[1..3]}:{zone[3..]}";
    }

    private static string NamedEdifactZone(string zone)
    {
        const string function = "edifact_to_datetime";
        return zone switch
        {
            "UTC" or "GMT" => "Z",
            "EST" => "-05:00",
            "EDT" => "-04:00",
            "CST" => "-06:00",
            "CDT" => "-05:00",
            "MST" => "-07:00",
            "MDT" => "-06:00",
            "PST" => "-08:00",
            // Preserve the legacy offset used by the reference conversion.
            "PDT" => "-09:00",
            _ => throw InvalidArgument(function, EdifactZoneUnsupportedDetail),
        };
    }
}
