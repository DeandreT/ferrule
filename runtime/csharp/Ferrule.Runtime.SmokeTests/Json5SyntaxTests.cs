using System.Reflection;
using System.Runtime.ExceptionServices;
using System.Text;
using System.Text.Json;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private sealed record Json5Captured(string? Output, Exception? Error);

    private static Json5Captured CaptureJson5(string? input, Func<string> action)
    {
        string? output = null;
        Exception? error = null;
        try { output = action(); }
        catch (Exception observed) { error = observed; }
        var typed = error as FerruleJson5SyntaxException;
        // Code units retain every original, including unpaired surrogates,
        // without asking the JSON log encoder to normalize invalid text.
        Console.WriteLine(JsonSerializer.Serialize(new
        {
            originalUtf16Units = input?.Select(value => (int)value).ToArray(),
            output,
            fullError = error?.ToString(),
            errorType = error?.GetType().FullName,
            failure = typed?.Failure.ToString(),
            kind = typed?.Kind?.ToString(),
            resource = typed?.Resource?.ToString(),
            offset = typed?.Offset,
            requested = typed?.Requested,
            maximum = typed?.Maximum,
            utf16Index = typed?.Utf16Index,
            innerType = error?.InnerException?.GetType().FullName,
            innerSpanIndex = (error?.InnerException as EncoderFallbackException)?.Index,
        }));
        return new(output, error);
    }

    private static void Json5Good(string input, string expected)
    {
        var observed = CaptureJson5(input, () => FerruleJson5Syntax.Normalize(input));
        Equal((Exception?)null, observed.Error);
        Equal(expected, observed.Output);
    }

    private static FerruleJson5SyntaxException Json5Bad(
        string input, FerruleJson5SyntaxKind kind, long? offset = null)
    {
        var observed = CaptureJson5(input, () => FerruleJson5Syntax.Normalize(input));
        Equal((string?)null, observed.Output);
        if (observed.Error is not FerruleJson5SyntaxException error)
        {
            throw new InvalidOperationException("Expected the original JSON5 syntax exception.", observed.Error);
        }
        Equal(FerruleJson5SyntaxFailure.Syntax, error.Failure);
        Equal<FerruleJson5SyntaxKind?>(kind, error.Kind);
        Equal<FerruleJson5SyntaxResource?>(null, error.Resource);
        Equal<long?>(null, error.Requested);
        Equal<long?>(null, error.Maximum);
        Equal<int?>(null, error.Utf16Index);
        Equal((Exception?)null, error.InnerException);
        if (offset is { } exact) { Equal(exact, error.Offset); }
        else if (error.Offset < 0 || error.Offset > Encoding.UTF8.GetByteCount(input))
        {
            throw new InvalidOperationException("Syntax offset is outside the original UTF-8 input.");
        }
        return error;
    }

    private static void Json5SyntaxCorpus()
    {
        var successes = 0;
        var refusals = 0;
        foreach (var item in Json5SyntaxCases)
        {
            Console.WriteLine($"JSON5 authored case: {item.Id}");
            if (item.Normalized is { } expected && item.Error is null)
            {
                Json5Good(item.Input, expected);
                successes++;
            }
            else if (item.Normalized is null && item.Error is { } kind)
            {
                Json5Bad(item.Input, kind);
                refusals++;
            }
            else { throw new InvalidOperationException($"Incomplete literal expectation: {item.Id}"); }
        }
        Equal((75, 46, 29), (Json5SyntaxCases.Length, successes, refusals));
    }

    private static void Json5SyntaxGrammar()
    {
        foreach (var (input, expected) in new (string, string)[]
        {
            (" /*before*/ [1,{true:null, false:0, $x_1:'雪',},[],] //eof", "[1,{\"true\":null,\"false\":0,\"$x_1\":\"雪\"},[]]"),
            ("{a:1,/*between*/a:{a:2,},}", "{\"a\":1,\"a\":{\"a\":2}}"),
            ("{a:1,a:false,a:null}", "{\"a\":1,\"a\":false,\"a\":null}"),
            ("[+1,-0,1.,.25,1E+01,]", "[1,0,1.0,0.25,1E+01]"),
            ("null", "null"), ("'scalar'", "\"scalar\""), ("{}", "{}"), ("[]", "[]"),
        }) { Json5Good(input, expected); }
        Json5Bad(
            " /*before*/ [1,{true:null, false:0, $x_1:'雪',},[],], //eof",
            FerruleJson5SyntaxKind.TrailingRootValue, 53);
        foreach (var input in new[]
        {
            "", "/*only*/", "{", "[", "[1,,]", "{a:}", "{a:1 b:2}", "{a:1]", "[1}", "{,}", "[,]",
        }) { Json5Bad(input, FerruleJson5SyntaxKind.UnexpectedToken); }
    }

    private static void Json5SyntaxNumbers()
    {
        foreach (var (input, expected) in new (string, string)[]
        {
            ("18446744073709551615", "18446744073709551615"),
            ("0xffffffffffffffff", "18446744073709551615"),
            ("-9223372036854775808", "-9223372036854775808"),
            ("-0x8000000000000000", "-9223372036854775808"),
            ("-0e0", "-0e0"), ("+0x0", "0"), ("-0X0", "0"),
            ("+1.2300E-02", "1.2300E-02"), ("-.5e+01", "-0.5e+01"), ("5.e0", "5.0e0"),
            ("1e-400", "1e-400"), ("1.7976931348623157e308", "1.7976931348623157e308"),
            ("0e999999999999999999999", "0e999999999999999999999"),
            ("-0.000e999999999999999999999", "-0.000e999999999999999999999"),
        }) { Json5Good(input, expected); }
        foreach (var input in new[]
        {
            "18446744073709551616", "-9223372036854775809", "0x10000000000000000", "-0x8000000000000001",
        }) { Json5Bad(input, FerruleJson5SyntaxKind.IntegerOutOfRange, 0); }
        foreach (var input in new[] { "00", "00.1", "0xG", "1e", "1e-", ".", "+", "--1", "1.2.3", "1_0" })
        {
            Json5Bad(input, FerruleJson5SyntaxKind.InvalidNumber, 0);
        }
        foreach (var input in new[]
        {
            "Infinity", "+Infinity", "-Infinity", "NaN", "+NaN", "-NaN", "1e400", "1.7976931348623159e308",
        }) { Json5Bad(input, FerruleJson5SyntaxKind.NonFiniteNumber, 0); }
        Json5Bad("{ignored:Infinity,n:7}", FerruleJson5SyntaxKind.NonFiniteNumber, 9);
        Json5Bad("{n:NaN,n:null}", FerruleJson5SyntaxKind.NonFiniteNumber, 3);
        Json5Bad("[0,1e400]", FerruleJson5SyntaxKind.NonFiniteNumber, 3);
    }

    private static void Json5SyntaxStrings()
    {
        foreach (var space in new[]
        {
            '\t', '\n', '\u000b', '\u000c', '\r', ' ', '\u00a0', '\u1680',
            '\u2000', '\u2001', '\u2002', '\u2003', '\u2004', '\u2005', '\u2006', '\u2007',
            '\u2008', '\u2009', '\u200a', '\u2028', '\u2029', '\u202f', '\u205f', '\u3000', '\ufeff',
        }) { Json5Good($"{space}{{n:7}}{space}", "{\"n\":7}"); }
        Json5Good("{\\u006e0:7}", "{\"n0\":7}");
        Json5Good("{'雪':'\\uD83D\\uDE00'}", "{\"雪\":\"😀\"}");
        Json5Good("{n:'a\\\rb\\\u2029c'}", "{\"n\":\"abc\"}");
        Json5Good("{n:'\u0000\u001f'}", "{\"n\":\"\\u0000\\u001f\"}");
        Json5Good("{n:'\\\"\\\\\\/\\b\\f\\r'}", "{\"n\":\"\\\"\\\\/\\b\\f\\r\"}");
        Json5Bad("\u0085{}", FerruleJson5SyntaxKind.UnexpectedToken, 0);
        Json5Bad("\ufeff/*雪*/{n:NaN}", FerruleJson5SyntaxKind.NonFiniteNumber, 13);
        Json5Bad("{n:7}\u00a0[]", FerruleJson5SyntaxKind.TrailingRootValue, 7);
        Json5Bad("{n:7}/*雪", FerruleJson5SyntaxKind.UnterminatedComment, 5);
        Json5Bad(" {n:'a\\1'}", FerruleJson5SyntaxKind.InvalidEscape, 6);
        Json5Bad("{雪:7}", FerruleJson5SyntaxKind.UnsupportedIdentifier, 1);
        Json5Bad("{'雪':'\\uD800'}", FerruleJson5SyntaxKind.InvalidEscape, 8);
        Json5Bad("{n 7}", FerruleJson5SyntaxKind.UnexpectedToken, 3);
        Json5Bad("'unfinished", FerruleJson5SyntaxKind.UnterminatedString, 0);
        Json5Bad("/", FerruleJson5SyntaxKind.UnexpectedToken, 0);
        Json5Bad("null/", FerruleJson5SyntaxKind.UnexpectedToken, 4);
        Json5Bad("{\\:1}", FerruleJson5SyntaxKind.InvalidEscape, 1);
        Json5Bad("{\\uD800:1}", FerruleJson5SyntaxKind.InvalidEscape, 1);
    }

    private static void Json5SyntaxEncoding()
    {
        var nullInput = CaptureJson5(null, () => FerruleJson5Syntax.Normalize(null!));
        Equal((string?)null, nullInput.Output);
        if (nullInput.Error is not ArgumentNullException argument)
        {
            throw new InvalidOperationException("Null text must retain its argument failure.", nullInput.Error);
        }
        Equal("source", argument.ParamName);
        Equal((Exception?)null, argument.InnerException);
        foreach (var (input, utf16Index, utf8Prefix) in new (string, int, long)[]
        {
            ("\ud800", 0, 0), ("\udc00", 0, 0), ("雪\ud800", 1, 3),
            ("'😀'\udc00", 4, 6), ("NaN\ud800", 3, 3),
        })
        {
            var observed = CaptureJson5(input, () => FerruleJson5Syntax.Normalize(input));
            Equal((string?)null, observed.Output);
            if (observed.Error is not FerruleJson5SyntaxException error)
            {
                throw new InvalidOperationException("Expected the typed original encoding error.", observed.Error);
            }
            Equal(FerruleJson5SyntaxFailure.Encoding, error.Failure);
            Equal<FerruleJson5SyntaxKind?>(null, error.Kind);
            Equal<FerruleJson5SyntaxResource?>(null, error.Resource);
            Equal<long?>(null, error.Requested);
            Equal<long?>(null, error.Maximum);
            Equal<int?>(utf16Index, error.Utf16Index);
            Equal(utf8Prefix, error.Offset);
            if (error.InnerException is not EncoderFallbackException encoding)
            {
                throw new InvalidOperationException("The actual throwing-encoder cause was not retained.");
            }
            // This Index belongs to the one-code-unit offending span.
            Equal(0, encoding.Index);
            Equal(input[utf16Index], encoding.CharUnknown);
            Equal(false, encoding.IsUnknownSurrogate());
        }
        Json5Good("'😀 雪'", "\"😀 雪\"");
    }

    private static string Json5Limited(string input, long original, long normalized, int depth, long work)
    {
        var method = typeof(FerruleJson5Syntax).GetMethod("NormalizeLimited", BindingFlags.Static | BindingFlags.NonPublic)
            ?? throw new InvalidOperationException("Private scaled-limit control seam is missing.");
        try
        {
            return (string)(method.Invoke(null, [input, original, normalized, depth, work])
                ?? throw new InvalidOperationException("Private normalizer returned no string."));
        }
        catch (TargetInvocationException wrapper) when (wrapper.InnerException is { } originalError)
        {
            Console.WriteLine($"Original reflection wrapper before unwrapping: {wrapper}");
            ExceptionDispatchInfo.Capture(originalError).Throw();
            throw;
        }
    }

    private static void Json5Limit(
        string input, long original, long normalized, int depth, long work,
        FerruleJson5SyntaxResource resource, long offset, long requested, long maximum)
    {
        var observed = CaptureJson5(input, () => Json5Limited(input, original, normalized, depth, work));
        Equal((string?)null, observed.Output);
        if (observed.Error is not FerruleJson5SyntaxException error)
        {
            throw new InvalidOperationException("Expected the original bounded syntax failure.", observed.Error);
        }
        Equal(FerruleJson5SyntaxFailure.Limit, error.Failure);
        Equal<FerruleJson5SyntaxKind?>(null, error.Kind);
        Equal<FerruleJson5SyntaxResource?>(resource, error.Resource);
        Equal(offset, error.Offset);
        Equal<long?>(requested, error.Requested);
        Equal<long?>(maximum, error.Maximum);
        Equal<int?>(null, error.Utf16Index);
        Equal((Exception?)null, error.InnerException);
    }

    private static void Json5SyntaxBudgets()
    {
        const long document = 67_108_864;
        const long work = 536_870_912;
        Equal((document, document, 127, work),
            (FerruleJson5Syntax.MaximumOriginalBytes, FerruleJson5Syntax.MaximumNormalizedBytes,
                FerruleJson5Syntax.MaximumContainerDepth, FerruleJson5Syntax.MaximumSyntaxWork));
        var exact = new string('[', 127) + "0" + new string(']', 127);
        Json5Good(exact, exact);
        Json5Limit(new string('[', 128) + "0" + new string(']', 128), document, document, 127, work,
            FerruleJson5SyntaxResource.ContainerDepth, 127, 128, 127);
        Json5Limit("/*[[*/['[[',[]]", document, document, 1, work,
            FerruleJson5SyntaxResource.ContainerDepth, 12, 2, 1);
        var original = CaptureJson5("\ufeffnull", () => Json5Limited("\ufeffnull", 7, document, 127, work));
        Equal((Exception?)null, original.Error);
        Equal("null", original.Output);
        Json5Limit("\ufeffnull ", 7, document, 127, work,
            FerruleJson5SyntaxResource.OriginalDocumentBytes, 0, 8, 7);
        var normalized = CaptureJson5("{n:'\\0'}", () => Json5Limited("{n:'\\0'}", document, 14, 127, work));
        Equal((Exception?)null, normalized.Error);
        Equal("{\"n\":\"\\u0000\"}", normalized.Output);
        Json5Limit("{n:'\\0'}", document, 13, 127, work,
            FerruleJson5SyntaxResource.NormalizedDocumentBytes, 8, 14, 13);
        // Shared null ledger: dispatch2 + lookahead11 + consumed4 + literal
        // comparisons12 + append4 + state1 + EOF2 =36. UTF16 census is separate.
        var syntaxWork = CaptureJson5("null", () => Json5Limited("null", document, document, 127, 36));
        Equal((Exception?)null, syntaxWork.Error);
        Equal("null", syntaxWork.Output);
        Json5Limit("null", document, document, 127, 35,
            FerruleJson5SyntaxResource.SyntaxWork, 4, 36, 35);
        // These are scaled controls, not actual default64MiB cap qualification.
    }
}
