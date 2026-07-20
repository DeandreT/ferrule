using System.Text;

namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
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

    private static string TrimRustWhitespace(string value)
    {
        var start = 0;
        while (start < value.Length && IsRustWhitespace(value[start]))
        {
            start++;
        }

        var end = value.Length;
        while (end > start && IsRustWhitespace(value[end - 1]))
        {
            end--;
        }
        return value[start..end];
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

}
