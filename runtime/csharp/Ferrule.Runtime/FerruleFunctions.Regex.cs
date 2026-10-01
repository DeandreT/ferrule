using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;

namespace Ferrule.Runtime;

public static partial class FerruleFunctions
{
    private const int MaximumRegexPatternBytes = 64 * 1024;
    private const int MaximumRegexReplacementBytes = 64 * 1024;
    private const int MaximumRegexResultBytes = 64 * 1024 * 1024;

    private readonly record struct RegexReplacementToken(
        string? Literal,
        int Group,
        string Suffix);

    private static FerruleValue Matches(IReadOnlyList<FerruleValue> arguments)
    {
        if (arguments.Count is not (2 or 3))
        {
            throw Arity("matches", 2, arguments.Count);
        }
        var input = ScalarText(arguments[0]);
        var pattern = ScalarText(arguments[1]);
        var flags = arguments.Count == 3 ? ScalarText(arguments[2]) : string.Empty;
        var regex = CompileRegex("matches", pattern, flags);
        try { return FerruleValue.FromBoolean(regex.IsMatch(input, new FerruleScalarRegex.BoundaryWork())); }
        catch (FerruleScalarRegex.WorkLimitException error)
        {
            throw InvalidArgument("matches", error.Message);
        }
    }

    private static FerruleValue Replace(IReadOnlyList<FerruleValue> arguments)
    {
        if (arguments.Count is not (3 or 4))
        {
            throw Arity("replace", 3, arguments.Count);
        }
        var input = ScalarText(arguments[0]);
        var pattern = ScalarText(arguments[1]);
        var replacement = ScalarText(arguments[2]);
        var flags = arguments.Count == 4 ? ScalarText(arguments[3]) : string.Empty;
        if (Encoding.UTF8.GetByteCount(replacement) > MaximumRegexReplacementBytes)
        {
            throw InvalidArgument("replace", "replacement exceeds 64 KiB");
        }
        var regex = CompileRegex("replace", pattern, flags, out var captureGroups);
        var work = new FerruleScalarRegex.BoundaryWork();
        try
        {
            if (regex.IsMatch(string.Empty, work))
            {
                throw InvalidArgument("replace", "pattern matches a zero-length string");
            }
            var tokens = ParseReplacement(replacement, captureGroups.Length - 1);
            var output = new StringBuilder();
            var outputBytes = 0;
            var end = 0;
            foreach (var match in regex.Matches(input, work))
            {
                if (match.Length == 0)
                {
                    throw InvalidArgument("replace", "pattern produced a zero-length match");
                }
                AppendBounded(output, input[end..match.Index], ref outputBytes);
                foreach (var token in tokens)
                {
                    if (token.Literal is not null)
                    {
                        AppendBounded(output, token.Literal, ref outputBytes);
                        continue;
                    }
                    if (token.Group < captureGroups.Length)
                    {
                        var group = match.Groups[captureGroups[token.Group]];
                        if (group.Success) { AppendBounded(output, group.Value, ref outputBytes); }
                    }
                    AppendBounded(output, token.Suffix, ref outputBytes);
                }
                end = match.Index + match.Length;
            }
            AppendBounded(output, input[end..], ref outputBytes);
            return FerruleValue.FromString(output.ToString());
        }
        catch (FerruleScalarRegex.WorkLimitException error)
        {
            throw InvalidArgument("replace", error.Message);
        }
    }

    private static FerruleScalarRegex.ScalarRegexProgram CompileRegex(string function, string pattern, string flags) =>
        CompileRegex(function, pattern, flags, out _);

    private static FerruleScalarRegex.ScalarRegexProgram CompileRegex(
        string function,
        string pattern,
        string flags,
        out int[] captureGroups)
    {
        if (Encoding.UTF8.GetByteCount(pattern) > MaximumRegexPatternBytes)
        {
            throw InvalidArgument(function, "pattern exceeds 64 KiB");
        }
        var options = RegexOptions.CultureInvariant | RegexOptions.NonBacktracking;
        foreach (var flag in flags)
        {
            options |= flag switch
            {
                'i' => RegexOptions.IgnoreCase,
                'm' => RegexOptions.Multiline,
                's' => RegexOptions.Singleline,
                'x' => RegexOptions.IgnorePatternWhitespace,
                _ => throw InvalidArgument(function, "flags contain an unsupported value"),
            };
        }
        try
        {
            var program = FerruleScalarRegex.CompileProgram(pattern, options);
            captureGroups = Enumerable.Range(0, program.CaptureCount).ToArray();
            return program;
        }
        catch (Exception error) when (error is ArgumentException or NotSupportedException)
        {
            throw InvalidArgument(
                function,
                "pattern is invalid or exceeds the compiled-size limit");
        }
    }

    private static IReadOnlyList<RegexReplacementToken> ParseReplacement(
        string replacement,
        int groupCount)
    {
        var tokens = new List<RegexReplacementToken>();
        var literal = new StringBuilder();
        for (var index = 0; index < replacement.Length; index++)
        {
            switch (replacement[index])
            {
                case '\\':
                    if (++index >= replacement.Length || replacement[index] is not ('\\' or '$'))
                    {
                        throw InvalidReplacement();
                    }
                    literal.Append(replacement[index]);
                    break;
                case '$':
                    var start = index + 1;
                    if (start >= replacement.Length || !IsRegexAsciiDigit(replacement[start]))
                    {
                        throw InvalidReplacement();
                    }
                    if (literal.Length > 0)
                    {
                        tokens.Add(new RegexReplacementToken(literal.ToString(), -1, string.Empty));
                        literal.Clear();
                    }
                    index = start;
                    while (index + 1 < replacement.Length && IsRegexAsciiDigit(replacement[index + 1]))
                    {
                        index++;
                    }
                    var digits = replacement[start..(index + 1)];
                    var (group, suffix) = ResolveGroup(digits, groupCount);
                    tokens.Add(new RegexReplacementToken(null, group, suffix));
                    break;
                default:
                    literal.Append(replacement[index]);
                    break;
            }
        }
        if (literal.Length > 0)
        {
            tokens.Add(new RegexReplacementToken(literal.ToString(), -1, string.Empty));
        }
        return tokens;
    }

    private static (int Group, string Suffix) ResolveGroup(string digits, int groupCount)
    {
        var prefixLength = digits.Length;
        while (true)
        {
            if (int.TryParse(
                    digits.AsSpan(0, prefixLength),
                    NumberStyles.None,
                    CultureInfo.InvariantCulture,
                    out var group)
                && (group <= groupCount || group <= 9))
            {
                return (group, digits[prefixLength..]);
            }
            prefixLength--;
        }
    }

    private static void AppendBounded(StringBuilder output, string value, ref int outputBytes)
    {
        var added = Encoding.UTF8.GetByteCount(value);
        if (added > MaximumRegexResultBytes - outputBytes)
        {
            throw InvalidArgument("replace", "result exceeds 64 MiB");
        }
        output.Append(value);
        outputBytes += added;
    }

    private static bool IsRegexAsciiDigit(char value) => value is >= '0' and <= '9';

    private static FerruleRuntimeException InvalidReplacement() => InvalidArgument(
        "replace",
        "replacement has an invalid dollar or backslash escape");
}
