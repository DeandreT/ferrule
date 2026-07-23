using System.Text;
using System.Text.RegularExpressions;

namespace Ferrule.Runtime;

/// <summary>Scalar functions supported by generated mappings.</summary>
public static partial class FerruleFunctions
{
    private const int MaximumRegexPatternBytes = 64 * 1024;

    public static FerruleValue Call(string function, IReadOnlyList<FerruleValue> arguments)
    {
        ArgumentNullException.ThrowIfNull(function);
        ArgumentNullException.ThrowIfNull(arguments);
        return function switch
        {
            "concat" => Concat(arguments),
            "upper" => UnaryString(function, arguments, static value => value.ToUpperInvariant()),
            "lower" => UnaryString(function, arguments, static value => value.ToLowerInvariant()),
            "normalize_space" => UnaryString(
                function,
                arguments,
                NormalizeXmlSpace),
            "trim" => UnaryString(function, arguments, TrimRustWhitespace),
            "left" => EdgeCharacters(function, arguments, left: true),
            "right" => EdgeCharacters(function, arguments, left: false),
            "left_trim" => UnaryString(
                function,
                arguments,
                static value => value[CountLeadingXmlWhitespace(value)..]),
            "right_trim" => UnaryString(
                function,
                arguments,
                static value => value[..^CountTrailingXmlWhitespace(value)]),
            "length" => Length(arguments),
            "sql_like" => BinaryString(function, arguments, SqlLike),
            "pad_string_left" => PadString(function, arguments, left: true),
            "pad_string_right" => PadString(function, arguments, left: false),
            "substring" => Substring(arguments),
            "substring_before" => SplitString(function, arguments, before: true),
            "substring_after" => SplitString(function, arguments, before: false),
            "string" => StringValue(arguments),
            "is_numeric" => IsNumeric(arguments),
            "to_number" => ToNumber(arguments),
            "format_number" => FormatNumber(arguments),
            "round" => Round(arguments),
            "delay_passthrough" => DelayPassthrough(arguments),
            "date_from_datetime" => UnaryString(
                function,
                arguments,
                DateFromDateTime),
            "year_from_datetime" => YearFromDateTime(arguments),
            "month_from_datetime" => MonthFromDateTime(arguments),
            "day_from_datetime" => DayFromDateTime(arguments),
            "weekday" => Weekday(arguments),
            "hours_from_datetime" => HoursFromDateTime(arguments),
            "minutes_from_datetime" => MinutesFromDateTime(arguments),
            "time_from_datetime" => TimeFromDateTime(arguments),
            "datetime_from_date_and_time" => DateTimeFromDateAndTime(arguments),
            "datetime_from_parts" => DateTimeFromParts(arguments),
            "coerce_datetime" => CoerceDateTime(arguments),
            "parse_date" => ParseDate(arguments),
            "parse_datetime" => ParseDateTime(arguments),
            "parse_time" => ParseTime(arguments),
            "datetime_add" => DateTimeAdd(arguments),
            "edifact_to_datetime" => EdifactToDateTime(arguments),
            "substitute_missing" => SubstituteMissing(arguments),
            "substitute_missing_with_xml_nil" => SubstituteMissingWithXmlNil(arguments),
            "is_xml_nil" => IsXmlNil(arguments),
            "get_folder" => UnaryString(function, arguments, GetFolder),
            "remove_folder" => UnaryString(function, arguments, RemoveFolder),
            "get_fileext" => UnaryString(function, arguments, GetFileExtension),
            "resolve_filepath" => ResolveFilePath(arguments),
            "isbn10_to_isbn13" => Isbn10ToIsbn13(arguments),
            "and" => BinaryBoolean(function, arguments, (left, right) => left && right),
            "or" => BinaryBoolean(function, arguments, (left, right) => left || right),
            "not" => UnaryBoolean(function, arguments, value => !value),
            "exists" => Exists(arguments),
            "is_empty" => IsEmpty(arguments),
            "starts_with" => BinaryScalarString(function, arguments, static (left, right) =>
                left.StartsWith(right, StringComparison.Ordinal)),
            "ends_with" => BinaryScalarString(function, arguments, static (left, right) =>
                left.EndsWith(right, StringComparison.Ordinal)),
            "contains" => BinaryScalarString(function, arguments, static (left, right) =>
                left.Contains(right, StringComparison.Ordinal)),
            "matches" => Matches(arguments),
            "add" => Numeric(function, arguments, NumericOperation.Add),
            "subtract" => Numeric(function, arguments, NumericOperation.Subtract),
            "multiply" => Numeric(function, arguments, NumericOperation.Multiply),
            "divide" => Divide(arguments),
            "equal" => Comparison(function, arguments, ordering => ordering == 0),
            "not_equal" => Comparison(function, arguments, ordering => ordering != 0),
            "less_than" => Comparison(function, arguments, ordering => ordering < 0),
            "greater_than" => Comparison(function, arguments, ordering => ordering > 0),
            "less_or_equal" => Comparison(function, arguments, ordering => ordering <= 0),
            "greater_or_equal" => Comparison(function, arguments, ordering => ordering >= 0),
            _ => throw new FerruleRuntimeException(
                FerruleRuntimeError.UnknownFunction,
                $"Unknown function '{function}'.",
                function: function),
        };
    }

    public static bool RequireBoolean(FerruleValue value, uint conditionNode)
    {
        if (value.Kind != FerruleValueKind.Bool)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.NotABool,
                $"Graph node {conditionNode} expected a bool, found {TypeName(value)}.",
                node: conditionNode,
                foundKind: value.Kind);
        }

        return value.BooleanValue;
    }

    private static FerruleValue Exists(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("exists", arguments, 1);
        return FerruleValue.FromBoolean(arguments[0].Kind != FerruleValueKind.Null);
    }

    private static FerruleValue IsEmpty(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("is_empty", arguments, 1);
        return FerruleValue.FromBoolean(ScalarText(arguments[0]).Length == 0);
    }

    private static FerruleValue Matches(IReadOnlyList<FerruleValue> arguments)
    {
        if (arguments.Count is not (2 or 3))
        {
            throw Arity("matches", 2, arguments.Count);
        }
        var input = ScalarText(arguments[0]);
        var pattern = ScalarText(arguments[1]);
        if (Encoding.UTF8.GetByteCount(pattern) > MaximumRegexPatternBytes)
        {
            throw InvalidArgument("matches", "pattern exceeds 64 KiB");
        }
        var flags = arguments.Count == 3 ? ScalarText(arguments[2]) : string.Empty;
        var options = RegexOptions.CultureInvariant | RegexOptions.NonBacktracking;
        foreach (var flag in flags)
        {
            options |= flag switch
            {
                'i' => RegexOptions.IgnoreCase,
                'm' => RegexOptions.Multiline,
                's' => RegexOptions.Singleline,
                'x' => RegexOptions.IgnorePatternWhitespace,
                _ => throw InvalidArgument("matches", "flags contain an unsupported value"),
            };
        }
        try
        {
            return FerruleValue.FromBoolean(Regex.IsMatch(input, pattern, options));
        }
        catch (Exception error) when (error is ArgumentException or NotSupportedException)
        {
            throw InvalidArgument(
                "matches",
                "pattern is invalid or exceeds the compiled-size limit");
        }
    }

    private static FerruleValue UnaryBoolean(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        Func<bool, bool> operation)
    {
        RequireArity(function, arguments, 1);
        return FerruleValue.FromBoolean(operation(RequireBooleanArgument(arguments[0], function)));
    }

    private static FerruleValue BinaryBoolean(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        Func<bool, bool, bool> operation)
    {
        RequireArity(function, arguments, 2);
        var left = RequireBooleanArgument(arguments[0], function);
        var right = RequireBooleanArgument(arguments[1], function);
        return FerruleValue.FromBoolean(operation(left, right));
    }

    private static FerruleValue BinaryString(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        Func<string, string, bool> operation)
    {
        RequireArity(function, arguments, 2);
        var left = RequireString(arguments[0], function);
        var right = RequireString(arguments[1], function);
        return FerruleValue.FromBoolean(operation(left, right));
    }

    private static FerruleValue BinaryScalarString(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        Func<string, string, bool> operation)
    {
        RequireArity(function, arguments, 2);
        var left = ScalarText(arguments[0]);
        var right = ScalarText(arguments[1]);
        return FerruleValue.FromBoolean(operation(left, right));
    }

    private static bool RequireBooleanArgument(FerruleValue value, string function)
    {
        if (value.Kind != FerruleValueKind.Bool)
        {
            throw Type(function, value);
        }
        return value.BooleanValue;
    }

    private static string RequireString(FerruleValue value, string function)
    {
        if (value.Kind != FerruleValueKind.String)
        {
            throw Type(function, value);
        }
        return value.StringValue;
    }

    private static void RequireArity(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        int expected)
    {
        if (arguments.Count != expected)
        {
            throw Arity(function, expected, arguments.Count);
        }
    }

    private static FerruleRuntimeException Arity(string function, int expected, int actual) =>
        new(
            FerruleRuntimeError.FunctionArity,
            $"`{function}` expected {expected} argument(s), got {actual}.",
            function: function,
            expectedArity: expected,
            actualArity: actual);

    private static FerruleRuntimeException Type(string function, FerruleValue value) =>
        new(
            FerruleRuntimeError.FunctionType,
            $"`{function}` cannot accept a {TypeName(value)} argument.",
            function: function,
            foundKind: value.Kind);

    private static FerruleRuntimeException InvalidArgument(string function, string detail) =>
        new(
            FerruleRuntimeError.FunctionInvalidArgument,
            $"`{function}` {detail}.",
            function: function,
            detail: detail);

    private static string TypeName(FerruleValue value) => value.Kind switch
    {
        FerruleValueKind.Null => "null",
        FerruleValueKind.XmlNil => "xml nil",
        FerruleValueKind.Bool => "bool",
        FerruleValueKind.Int64 => "int",
        FerruleValueKind.Double => "float",
        FerruleValueKind.String => "string",
        _ => "unknown",
    };

}
