using System.Globalization;
using System.Numerics;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>Scalar functions supported by generated mappings.</summary>
public static class FerruleFunctions
{
    public static FerruleValue Call(string function, IReadOnlyList<FerruleValue> arguments)
    {
        ArgumentNullException.ThrowIfNull(function);
        ArgumentNullException.ThrowIfNull(arguments);
        return function switch
        {
            "concat" => Concat(arguments),
            "normalize_space" => UnaryString(
                function,
                arguments,
                NormalizeXmlSpace),
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
            "round" => Round(arguments),
            "date_from_datetime" => UnaryString(
                function,
                arguments,
                DateFromDateTime),
            "substitute_missing" => SubstituteMissing(arguments),
            "is_xml_nil" => IsXmlNil(arguments),
            "get_folder" => UnaryString(function, arguments, GetFolder),
            "remove_folder" => UnaryString(function, arguments, RemoveFolder),
            "resolve_filepath" => ResolveFilePath(arguments),
            "isbn10_to_isbn13" => Isbn10ToIsbn13(arguments),
            "and" => BinaryBoolean(function, arguments, (left, right) => left && right),
            "or" => BinaryBoolean(function, arguments, (left, right) => left || right),
            "not" => UnaryBoolean(function, arguments, value => !value),
            "exists" => Exists(arguments),
            "is_empty" => IsEmpty(arguments),
            "starts_with" => BinaryString(function, arguments, static (left, right) =>
                left.StartsWith(right, StringComparison.Ordinal)),
            "contains" => BinaryString(function, arguments, static (left, right) =>
                left.Contains(right, StringComparison.Ordinal)),
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
        return FerruleValue.FromBoolean(RequireString(arguments[0], "is_empty").Length == 0);
    }

    private static FerruleValue Concat(IReadOnlyList<FerruleValue> arguments)
    {
        var result = new StringBuilder();
        foreach (var argument in arguments)
        {
            if (argument.Kind is not (FerruleValueKind.Null or FerruleValueKind.XmlNil))
            {
                result.Append(ScalarText(argument));
            }
        }

        return FerruleValue.FromString(result.ToString());
    }

    private static FerruleValue UnaryString(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        Func<string, string> operation)
    {
        RequireArity(function, arguments, 1);
        return FerruleValue.FromString(operation(RequireString(arguments[0], function)));
    }

    private static FerruleValue Length(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("length", arguments, 1);
        long length = 0;
        foreach (var _ in ScalarText(arguments[0]).EnumerateRunes())
        {
            length++;
        }

        return FerruleValue.FromInt64(length);
    }

    private static FerruleValue Substring(IReadOnlyList<FerruleValue> arguments)
    {
        if (arguments.Count > 0 && arguments[0].Kind != FerruleValueKind.String)
        {
            throw Type("substring", arguments[0]);
        }
        if (arguments.Count is not (2 or 3))
        {
            throw Arity("substring", 2, arguments.Count);
        }

        var value = arguments[0].StringValue;
        var start = SaturatingInt64(RustRound(NumberArgument(arguments[1], "substring")));
        long? end = arguments.Count == 3
            ? SaturatingAdd(
                start,
                SaturatingInt64(RustRound(NumberArgument(arguments[2], "substring"))))
            : null;
        var result = new StringBuilder(value.Length);
        long position = 1;
        foreach (var rune in value.EnumerateRunes())
        {
            if (position >= start && (!end.HasValue || position < end.Value))
            {
                result.Append(rune.ToString());
            }
            position++;
        }
        return FerruleValue.FromString(result.ToString());
    }

    private static bool SqlLike(string value, string pattern)
    {
        var valueRunes = Runes(value);
        var previous = new bool[valueRunes.Count + 1];
        previous[0] = true;
        foreach (var token in pattern.EnumerateRunes())
        {
            var current = new bool[valueRunes.Count + 1];
            if (token.Value == '%')
            {
                current[0] = previous[0];
                for (var index = 1; index <= valueRunes.Count; index++)
                {
                    current[index] = previous[index] || current[index - 1];
                }
            }
            else if (token.Value == '_')
            {
                for (var index = 1; index <= valueRunes.Count; index++)
                {
                    current[index] = previous[index - 1];
                }
            }
            else
            {
                for (var index = 1; index <= valueRunes.Count; index++)
                {
                    current[index] = previous[index - 1] &&
                        EqualIgnoringAsciiCase(valueRunes[index - 1], token);
                }
            }
            previous = current;
        }
        return previous[valueRunes.Count];
    }

    private static FerruleValue PadString(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        bool left)
    {
        RequireArity(function, arguments, 3);
        var desiredLength = arguments[1].Kind switch
        {
            FerruleValueKind.Int64 => arguments[1].Int64Value,
            FerruleValueKind.Double when double.IsFinite(arguments[1].DoubleValue) =>
                SaturatingInt64(arguments[1].DoubleValue),
            FerruleValueKind.Double => throw InvalidArgument(
                function,
                "requires a finite desired length"),
            _ => throw Type(function, arguments[1]),
        };
        if (desiredLength > 1_000_000)
        {
            throw InvalidArgument(
                function,
                "requested output exceeds 1000000 characters");
        }

        var paddingRunes = Runes(ScalarText(arguments[2]));
        if (paddingRunes.Count != 1)
        {
            throw InvalidArgument(function, "requires one padding character");
        }

        var value = ScalarText(arguments[0]);
        var valueLength = RuneCount(value);
        var paddingCount = desiredLength > valueLength
            ? (int)(desiredLength - valueLength)
            : 0;
        var paddingText = paddingRunes[0].ToString();
        var padding = new StringBuilder(paddingCount * paddingText.Length);
        for (var index = 0; index < paddingCount; index++)
        {
            padding.Append(paddingText);
        }

        return FerruleValue.FromString(left
            ? padding + value
            : value + padding.ToString());
    }

    private static FerruleValue Round(IReadOnlyList<FerruleValue> arguments)
    {
        if (arguments.Count == 1 && arguments[0].Kind == FerruleValueKind.Int64)
        {
            return arguments[0];
        }
        if (arguments.Count == 1)
        {
            return FerruleValue.FromDouble(
                RustRound(NumberArgument(arguments[0], "round")));
        }
        if (arguments.Count == 2)
        {
            var value = NumberArgument(arguments[0], "round");
            var digits = SaturatingInt32(
                RustRound(NumberArgument(arguments[1], "round")));
            var factor = PowInteger(10.0, digits);
            return FerruleValue.FromDouble(RustRound(value * factor) / factor);
        }
        throw Arity("round", 1, arguments.Count);
    }

    private static string DateFromDateTime(string value)
    {
        var separator = value.IndexOf('T');
        var date = separator >= 0 ? value[..separator] : value;
        var start = 0;
        while (start < date.Length && IsRustWhitespace(date[start]))
        {
            start++;
        }
        var end = date.Length;
        while (end > start && IsRustWhitespace(date[end - 1]))
        {
            end--;
        }
        return date[start..end];
    }

    private static FerruleValue Isbn10ToIsbn13(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("isbn10_to_isbn13", arguments, 1);
        var input = RequireString(arguments[0], "isbn10_to_isbn13");
        var normalized = new StringBuilder(10);
        foreach (var character in input)
        {
            if (character is not (' ' or '-'))
            {
                normalized.Append(character);
            }
        }

        var isbn = normalized.ToString();
        if (isbn.Length != 10 ||
            isbn[..9].Any(character => character is < '0' or > '9') ||
            !(isbn[9] is >= '0' and <= '9' or 'X' or 'x'))
        {
            throw InvalidArgument(
                "isbn10_to_isbn13",
                "expected a 10-character ISBN with an optional final X check digit");
        }

        var isbn10Sum = 0;
        for (var index = 0; index < 9; index++)
        {
            isbn10Sum += (isbn[index] - '0') * (10 - index);
        }
        isbn10Sum += isbn[9] is 'X' or 'x' ? 10 : isbn[9] - '0';
        if (isbn10Sum % 11 != 0)
        {
            throw InvalidArgument(
                "isbn10_to_isbn13",
                "ISBN-10 check digit is invalid");
        }

        var output = "978" + isbn[..9];
        var weighted = 0;
        for (var index = 0; index < output.Length; index++)
        {
            weighted += (output[index] - '0') * (index % 2 == 0 ? 1 : 3);
        }
        var checkDigit = (char)('0' + ((10 - weighted % 10) % 10));
        return FerruleValue.FromString(output + checkDigit);
    }

    private static FerruleValue SplitString(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        bool before)
    {
        RequireArity(function, arguments, 2);
        var value = RequireString(arguments[0], function);
        var separator = RequireString(arguments[1], function);
        var index = value.IndexOf(separator, StringComparison.Ordinal);
        if (index < 0)
        {
            return FerruleValue.FromString(string.Empty);
        }

        var result = before
            ? value[..index]
            : value[(index + separator.Length)..];
        return FerruleValue.FromString(result);
    }

    private static FerruleValue StringValue(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("string", arguments, 1);
        return FerruleValue.FromString(ScalarText(arguments[0]));
    }

    private static FerruleValue SubstituteMissing(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("substitute_missing", arguments, 2);
        return arguments[0].Kind is FerruleValueKind.Null or FerruleValueKind.XmlNil
            ? arguments[1]
            : arguments[0];
    }

    private static FerruleValue IsXmlNil(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("is_xml_nil", arguments, 1);
        return FerruleValue.FromBoolean(arguments[0].Kind == FerruleValueKind.XmlNil);
    }

    private static string NormalizeXmlSpace(string value)
    {
        var result = new StringBuilder(value.Length);
        var pendingSeparator = false;
        foreach (var character in value)
        {
            if (IsXmlWhitespace(character))
            {
                pendingSeparator = result.Length > 0;
                continue;
            }

            if (pendingSeparator)
            {
                result.Append(' ');
                pendingSeparator = false;
            }
            result.Append(character);
        }

        return result.ToString();
    }

    private static int CountLeadingXmlWhitespace(string value)
    {
        var count = 0;
        while (count < value.Length && IsXmlWhitespace(value[count]))
        {
            count++;
        }
        return count;
    }

    private static int CountTrailingXmlWhitespace(string value)
    {
        var count = 0;
        while (count < value.Length && IsXmlWhitespace(value[^(count + 1)]))
        {
            count++;
        }
        return count;
    }

    private static bool IsXmlWhitespace(char character) =>
        character is ' ' or '\t' or '\r' or '\n';

    private static string GetFolder(string path)
    {
        var separator = LastSeparator(path);
        return separator >= 0 ? path[..(separator + 1)] : string.Empty;
    }

    private static string RemoveFolder(string path)
    {
        var separator = LastSeparator(path);
        return separator >= 0 ? path[(separator + 1)..] : path;
    }

    private static FerruleValue ResolveFilePath(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("resolve_filepath", arguments, 2);
        var basePath = RequireString(arguments[0], "resolve_filepath");
        var path = RequireString(arguments[1], "resolve_filepath");
        if (IsAbsolute(path))
        {
            return FerruleValue.FromString(path);
        }

        var delimiter = CommonSeparator(basePath, path);
        var folder = basePath;
        if (folder.Length > 0 && !EndsWithSeparator(folder))
        {
            folder += delimiter;
        }

        var relative = StripCurrentDirectories(path);
        while (TryParentRemainder(relative, out var remainder))
        {
            folder = ParentFolder(folder, delimiter);
            relative = remainder;
        }

        return FerruleValue.FromString(folder + relative);
    }

    private static int LastSeparator(string path) => path.LastIndexOfAny(['/', '\\']);

    private static char? PathSeparator(string path)
    {
        var forward = path.Contains('/');
        var backward = path.Contains('\\');
        return (forward, backward) switch
        {
            (true, false) => '/',
            (false, true) => '\\',
            _ => null,
        };
    }

    private static char CommonSeparator(string basePath, string path)
    {
        var left = PathSeparator(basePath);
        var right = PathSeparator(path);
        if (left.HasValue && right.HasValue && left == right)
        {
            return left.Value;
        }
        if (left.HasValue && !right.HasValue)
        {
            return left.Value;
        }
        if (!left.HasValue && right.HasValue)
        {
            return right.Value;
        }
        return '\\';
    }

    private static bool IsAbsolute(string path) =>
        StartsWithSeparator(path) || path.Contains(':');

    private static string StripCurrentDirectories(string path)
    {
        while (true)
        {
            if (path == ".")
            {
                return string.Empty;
            }
            if (path.StartsWith("./", StringComparison.Ordinal) ||
                path.StartsWith(".\\", StringComparison.Ordinal))
            {
                path = path[2..];
                continue;
            }
            return path;
        }
    }

    private static bool TryParentRemainder(string path, out string remainder)
    {
        if (path == "..")
        {
            remainder = string.Empty;
            return true;
        }
        if (path.StartsWith("../", StringComparison.Ordinal) ||
            path.StartsWith("..\\", StringComparison.Ordinal))
        {
            remainder = path[3..];
            return true;
        }

        remainder = string.Empty;
        return false;
    }

    private static string ParentFolder(string folder, char delimiter)
    {
        var trimmed = folder.TrimEnd('/', '\\');
        if (trimmed.Length == 0 && StartsWithSeparator(folder))
        {
            return delimiter.ToString();
        }
        if (IsDrivePrefix(trimmed))
        {
            return trimmed + delimiter;
        }

        var separator = LastSeparator(trimmed);
        if (separator >= 0)
        {
            return trimmed[..(separator + 1)];
        }
        return trimmed.Length == 0 ? $"..{delimiter}" : string.Empty;
    }

    private static bool StartsWithSeparator(string path) =>
        path.Length > 0 && path[0] is '/' or '\\';

    private static bool EndsWithSeparator(string path) =>
        path.Length > 0 && path[^1] is '/' or '\\';

    private static bool IsDrivePrefix(string path) =>
        path.Length == 2 &&
        ((path[0] is >= 'a' and <= 'z') || (path[0] is >= 'A' and <= 'Z')) &&
        path[1] == ':';

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

    private static FerruleValue Numeric(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        NumericOperation operation)
    {
        if (arguments.Count < 2)
        {
            throw Arity(function, 2, arguments.Count);
        }

        var operands = arguments.Select(value => NumericOperand.From(value, function)).ToArray();
        if (operands.Any(operand => operand.IsDouble))
        {
            var result = operands[0].AsDouble();
            for (var index = 1; index < operands.Length; index++)
            {
                var operand = operands[index].AsDouble();
                result = operation switch
                {
                    NumericOperation.Add => result + operand,
                    NumericOperation.Subtract => result - operand,
                    NumericOperation.Multiply => result * operand,
                    _ => result,
                };
            }

            if (operation == NumericOperation.Multiply && TryExactDecimalProduct(arguments, out var exact))
            {
                result = exact;
            }

            return FerruleValue.FromDouble(result);
        }

        var integer = operands[0].Integer;
        try
        {
            for (var index = 1; index < operands.Length; index++)
            {
                integer = operation switch
                {
                    NumericOperation.Add => checked(integer + operands[index].Integer),
                    NumericOperation.Subtract => checked(integer - operands[index].Integer),
                    NumericOperation.Multiply => checked(integer * operands[index].Integer),
                    _ => integer,
                };
            }
        }
        catch (OverflowException exception)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.IntegerOverflow,
                $"`{function}` integer arithmetic overflowed.",
                exception,
                function: function);
        }

        return FerruleValue.FromInt64(integer);
    }

    private static FerruleValue Divide(IReadOnlyList<FerruleValue> arguments)
    {
        RequireArity("divide", arguments, 2);
        var left = NumericOperand.From(arguments[0], "divide").AsDouble();
        var right = NumericOperand.From(arguments[1], "divide").AsDouble();
        if (right == 0.0)
        {
            throw new FerruleRuntimeException(
                FerruleRuntimeError.DivideByZero,
                "Division by zero.");
        }

        return FerruleValue.FromDouble(left / right);
    }

    private static FerruleValue Comparison(
        string function,
        IReadOnlyList<FerruleValue> arguments,
        Func<int, bool> matches)
    {
        RequireArity(function, arguments, 2);
        var left = arguments[0];
        var right = arguments[1];
        if (left.Kind is FerruleValueKind.Null or FerruleValueKind.XmlNil ||
            right.Kind is FerruleValueKind.Null or FerruleValueKind.XmlNil)
        {
            return FerruleValue.FromBoolean(false);
        }

        var ordering = ValueOrdering(left, right);
        if (ordering is null)
        {
            throw Type(function, right);
        }

        return FerruleValue.FromBoolean(matches(ordering.Value));
    }

    private static int? ValueOrdering(FerruleValue left, FerruleValue right)
    {
        if (left.Kind == FerruleValueKind.Int64 && right.Kind == FerruleValueKind.Int64)
        {
            return left.Int64Value.CompareTo(right.Int64Value);
        }
        if (left.Kind == FerruleValueKind.Double && right.Kind == FerruleValueKind.Double)
        {
            return PartialDoubleComparison(left.DoubleValue, right.DoubleValue);
        }
        if (left.Kind == FerruleValueKind.Int64 && right.Kind == FerruleValueKind.Double)
        {
            return PartialDoubleComparison(left.Int64Value, right.DoubleValue);
        }
        if (left.Kind == FerruleValueKind.Double && right.Kind == FerruleValueKind.Int64)
        {
            return PartialDoubleComparison(left.DoubleValue, right.Int64Value);
        }
        if (left.Kind == FerruleValueKind.String && right.Kind == FerruleValueKind.String)
        {
            return CompareUnicodeScalars(left.StringValue, right.StringValue);
        }
        if (left.Kind == FerruleValueKind.String)
        {
            return CompareUnicodeScalars(left.StringValue, ScalarText(right));
        }
        if (right.Kind == FerruleValueKind.String)
        {
            return CompareUnicodeScalars(ScalarText(left), right.StringValue);
        }
        if (left.Kind == FerruleValueKind.Bool && right.Kind == FerruleValueKind.Bool)
        {
            return left.BooleanValue.CompareTo(right.BooleanValue);
        }

        return null;
    }

    private static int? PartialDoubleComparison(double left, double right) =>
        double.IsNaN(left) || double.IsNaN(right) ? null : left.CompareTo(right);

    private static int CompareUnicodeScalars(string left, string right)
    {
        var leftRunes = left.EnumerateRunes().GetEnumerator();
        var rightRunes = right.EnumerateRunes().GetEnumerator();
        while (true)
        {
            var hasLeft = leftRunes.MoveNext();
            var hasRight = rightRunes.MoveNext();
            if (!hasLeft || !hasRight)
            {
                return hasLeft.CompareTo(hasRight);
            }

            var comparison = leftRunes.Current.Value.CompareTo(rightRunes.Current.Value);
            if (comparison != 0)
            {
                return comparison;
            }
        }
    }

    private static List<Rune> Runes(string value)
    {
        var runes = new List<Rune>(value.Length);
        foreach (var rune in value.EnumerateRunes())
        {
            runes.Add(rune);
        }
        return runes;
    }

    private static int RuneCount(string value)
    {
        var count = 0;
        foreach (var _ in value.EnumerateRunes())
        {
            count++;
        }
        return count;
    }

    private static bool EqualIgnoringAsciiCase(Rune left, Rune right) =>
        AsciiLower(left.Value) == AsciiLower(right.Value);

    private static int AsciiLower(int value) => value is >= 'A' and <= 'Z'
        ? value + ('a' - 'A')
        : value;

    private static double NumberArgument(FerruleValue value, string function) => value.Kind switch
    {
        FerruleValueKind.Int64 => value.Int64Value,
        FerruleValueKind.Double => value.DoubleValue,
        _ => throw Type(function, value),
    };

    private static double RustRound(double value) =>
        Math.Round(value, MidpointRounding.AwayFromZero);

    private static long SaturatingInt64(double value)
    {
        if (double.IsNaN(value))
        {
            return 0;
        }
        if (value >= -(double)long.MinValue)
        {
            return long.MaxValue;
        }
        if (value <= long.MinValue)
        {
            return long.MinValue;
        }
        return (long)Math.Truncate(value);
    }

    private static int SaturatingInt32(double value)
    {
        if (double.IsNaN(value))
        {
            return 0;
        }
        if (value >= int.MaxValue)
        {
            return int.MaxValue;
        }
        if (value <= int.MinValue)
        {
            return int.MinValue;
        }
        return (int)Math.Truncate(value);
    }

    private static long SaturatingAdd(long left, long right)
    {
        if (right > 0 && left > long.MaxValue - right)
        {
            return long.MaxValue;
        }
        if (right < 0 && left < long.MinValue - right)
        {
            return long.MinValue;
        }
        return left + right;
    }

    private static double PowInteger(double value, int exponent)
    {
        var reciprocal = exponent < 0;
        var result = 1.0;
        while (true)
        {
            if ((exponent & 1) != 0)
            {
                result *= value;
            }
            exponent /= 2;
            if (exponent == 0)
            {
                break;
            }
            value *= value;
        }
        return reciprocal ? 1.0 / result : result;
    }

    private static bool IsRustWhitespace(char character) => character is
        >= '\u0009' and <= '\u000D' or
        '\u0020' or
        '\u0085' or
        '\u00A0' or
        '\u1680' or
        >= '\u2000' and <= '\u200A' or
        '\u2028' or
        '\u2029' or
        '\u202F' or
        '\u205F' or
        '\u3000';

    internal static string ScalarText(FerruleValue value) => value.Kind switch
    {
        FerruleValueKind.Null or FerruleValueKind.XmlNil => string.Empty,
        FerruleValueKind.Bool => value.BooleanValue ? "true" : "false",
        FerruleValueKind.Int64 => value.Int64Value.ToString(CultureInfo.InvariantCulture),
        FerruleValueKind.Double => DoubleText(value.DoubleValue),
        FerruleValueKind.String => value.StringValue,
        _ => string.Empty,
    };

    private static string DoubleText(double value)
    {
        if (double.IsNaN(value))
        {
            return "NaN";
        }
        if (double.IsPositiveInfinity(value))
        {
            return "inf";
        }
        if (double.IsNegativeInfinity(value))
        {
            return "-inf";
        }

        var text = value.ToString("R", CultureInfo.InvariantCulture);
        var exponentMarker = text.IndexOfAny(['E', 'e']);
        if (exponentMarker < 0)
        {
            return text;
        }

        var mantissa = text[..exponentMarker];
        var exponent = int.Parse(text[(exponentMarker + 1)..], CultureInfo.InvariantCulture);
        var negative = mantissa.StartsWith("-", StringComparison.Ordinal);
        if (negative)
        {
            mantissa = mantissa[1..];
        }
        var decimalPoint = mantissa.IndexOf('.');
        var digitsBeforePoint = decimalPoint < 0 ? mantissa.Length : decimalPoint;
        var digits = mantissa.Replace(".", string.Empty, StringComparison.Ordinal);
        var targetPoint = digitsBeforePoint + exponent;
        var expanded = targetPoint switch
        {
            <= 0 => "0." + new string('0', -targetPoint) + digits,
            _ when targetPoint >= digits.Length => digits + new string('0', targetPoint - digits.Length),
            _ => digits.Insert(targetPoint, "."),
        };
        return negative ? "-" + expanded : expanded;
    }

    private static bool TryExactDecimalProduct(
        IReadOnlyList<FerruleValue> values,
        out double result)
    {
        result = 0.0;
        if (!ExactDecimal.TryFrom(values[0], out var product))
        {
            return false;
        }
        for (var index = 1; index < values.Count; index++)
        {
            if (!ExactDecimal.TryFrom(values[index], out var operand) ||
                !product.TryMultiply(operand, out product))
            {
                return false;
            }
        }

        return double.TryParse(
                   $"{product.Coefficient}e{product.Exponent}",
                   NumberStyles.Float,
                   CultureInfo.InvariantCulture,
                   out result) &&
               double.IsFinite(result);
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

    private enum NumericOperation
    {
        Add,
        Subtract,
        Multiply,
    }

    private readonly struct NumericOperand
    {
        private NumericOperand(long integer)
        {
            Integer = integer;
            Double = default;
            IsDouble = false;
        }

        private NumericOperand(double value)
        {
            Integer = default;
            Double = value;
            IsDouble = true;
        }

        internal long Integer { get; }

        internal double Double { get; }

        internal bool IsDouble { get; }

        internal double AsDouble() => IsDouble ? Double : Integer;

        internal static NumericOperand From(FerruleValue value, string function)
        {
            if (value.Kind == FerruleValueKind.Int64)
            {
                return new NumericOperand(value.Int64Value);
            }
            if (value.Kind == FerruleValueKind.Double)
            {
                return new NumericOperand(value.DoubleValue);
            }
            if (value.Kind == FerruleValueKind.String)
            {
                var text = value.StringValue.Trim();
                if (long.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var integer))
                {
                    return new NumericOperand(integer);
                }
                if (double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var number) &&
                    double.IsFinite(number))
                {
                    return new NumericOperand(number);
                }
            }

            throw Type(function, value);
        }
    }

    private readonly struct ExactDecimal
    {
        private static readonly BigInteger Minimum = -(BigInteger.One << 127);
        private static readonly BigInteger Maximum = (BigInteger.One << 127) - 1;

        private ExactDecimal(BigInteger coefficient, int exponent)
        {
            Coefficient = coefficient;
            Exponent = exponent;
        }

        internal BigInteger Coefficient { get; }

        internal int Exponent { get; }

        internal static bool TryFrom(FerruleValue value, out ExactDecimal result)
        {
            string? lexical = value.Kind switch
            {
                FerruleValueKind.Int64 => value.Int64Value.ToString(CultureInfo.InvariantCulture),
                FerruleValueKind.Double when value.DoubleValue == 0.0 &&
                                             double.IsNegative(value.DoubleValue) => null,
                FerruleValueKind.Double when double.IsFinite(value.DoubleValue) =>
                    DoubleText(value.DoubleValue),
                FerruleValueKind.String => CanonicalNumericString(value.StringValue),
                _ => null,
            };
            return TryParse(lexical, out result);
        }

        internal bool TryMultiply(ExactDecimal other, out ExactDecimal result)
        {
            var coefficient = Coefficient * other.Coefficient;
            var exponent = (long)Exponent + other.Exponent;
            if (!FitsInt128(coefficient) || exponent is < int.MinValue or > int.MaxValue)
            {
                result = default;
                return false;
            }
            result = Normalize(coefficient, (int)exponent);
            return true;
        }

        private static string? CanonicalNumericString(string value)
        {
            var text = value.Trim();
            if (long.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var integer))
            {
                return integer.ToString(CultureInfo.InvariantCulture);
            }
            return double.TryParse(text, NumberStyles.Float, CultureInfo.InvariantCulture, out var number) &&
                   double.IsFinite(number) &&
                   !(number == 0.0 && double.IsNegative(number))
                ? DoubleText(number)
                : null;
        }

        private static bool TryParse(string? lexical, out ExactDecimal result)
        {
            result = default;
            if (lexical is null)
            {
                return false;
            }
            var exponentMarker = lexical.IndexOfAny(['e', 'E']);
            var mantissa = exponentMarker < 0 ? lexical : lexical[..exponentMarker];
            var scientificExponent = 0;
            if (exponentMarker >= 0 &&
                !int.TryParse(
                    lexical[(exponentMarker + 1)..],
                    NumberStyles.Integer,
                    CultureInfo.InvariantCulture,
                    out scientificExponent))
            {
                return false;
            }
            var negative = mantissa.StartsWith("-", StringComparison.Ordinal);
            if (negative)
            {
                mantissa = mantissa[1..];
            }
            var parts = mantissa.Split('.', 2);
            var whole = parts[0];
            var fraction = parts.Length == 2 ? parts[1] : string.Empty;
            if (whole.Length == 0 ||
                whole.Any(character => character is < '0' or > '9') ||
                fraction.Any(character => character is < '0' or > '9') ||
                !BigInteger.TryParse(
                    whole + fraction,
                    NumberStyles.None,
                    CultureInfo.InvariantCulture,
                    out var coefficient))
            {
                return false;
            }
            if (negative)
            {
                coefficient = -coefficient;
            }
            var exponent = (long)scientificExponent - fraction.Length;
            if (!FitsInt128(coefficient) || exponent is < int.MinValue or > int.MaxValue)
            {
                return false;
            }
            result = Normalize(coefficient, (int)exponent);
            return true;
        }

        private static ExactDecimal Normalize(BigInteger coefficient, int exponent)
        {
            while (!coefficient.IsZero && coefficient % 10 == 0)
            {
                coefficient /= 10;
                exponent++;
            }
            return new ExactDecimal(coefficient, exponent);
        }

        private static bool FitsInt128(BigInteger value) => value >= Minimum && value <= Maximum;
    }
}
