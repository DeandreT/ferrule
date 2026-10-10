using System.Reflection;
using System.Runtime.ExceptionServices;
using System.Text;
using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void Json5UnicodeIdentifiers()
    {
        Json5UnicodeIdentifierDocuments();
        Json5UnicodeIdentifierMembership();
        Json5UnicodeIdentifierEncoding();
        Json5UnicodeIdentifierBudgets();
    }

    private static void Json5UnicodeIdentifierDocuments()
    {
        foreach (var (id, input, normalized, kind, offset, message) in
            new (string, string, string?, FerruleJson5SyntaxKind?, long, string?)[]
        {
            ("ascii-keyword-conservation", "{$x_0:0,true:1,null:2,default:3}", "{\"$x_0\":0,\"true\":1,\"null\":2,\"default\":3}", null, 0, null),
            ("start-lu", "{\u00c0:0}", "{\"\u00c0\":0}", null, 0, null),
            ("start-ll", "{\u03b1:0}", "{\"\u03b1\":0}", null, 0, null),
            ("start-lt", "{\u01c5:0}", "{\"\u01c5\":0}", null, 0, null),
            ("start-lm", "{\u02b0:0}", "{\"\u02b0\":0}", null, 0, null),
            ("start-lo", "{\u96ea:0}", "{\"\u96ea\":0}", null, 0, null),
            ("start-nl", "{\u2160:0}", "{\"\u2160\":0}", null, 0, null),
            ("continue-mn", "{a\u0301:0}", "{\"a\u0301\":0}", null, 0, null),
            ("continue-mc", "{a\u0903:0}", "{\"a\u0903\":0}", null, 0, null),
            ("continue-nd", "{a\u0661:0}", "{\"a\u0661\":0}", null, 0, null),
            ("continue-pc", "{a\u203f:0}", "{\"a\u203f\":0}", null, 0, null),
            ("continue-join-controls", "{a\u200c\u200d:0}", "{\"a\u200c\u200d\":0}", null, 0, null),
            ("raw-supplementary-lu", "{\ud801\udc00:0}", "{\"\ud801\udc00\":0}", null, 0, null),
            ("unicode17-tai-yo-lo", "{\ud839\udec0:0}", "{\"\ud839\udec0\":0}", null, 0, null),
            ("unicode16-cyrillic-lu", "{\u1c89:0}", "{\"\u1c89\":0}", null, 0, null),
            ("unicode17-supplementary-nd-continue", "{a\ud807\udde0:0}", "{\"a\ud807\udde0\":0}", null, 0, null),
            ("escaped-lu", "{\\u00C0:0}", "{\"\u00c0\":0}", null, 0, null),
            ("escaped-mn-continue", "{a\\u0301:0}", "{\"a\u0301\":0}", null, 0, null),
            ("escaped-nl", "{\\u2160:0}", "{\"\u2160\":0}", null, 0, null),
            ("paired-escape-supplementary-lu", "{\\uD801\\uDC00:0}", "{\"\ud801\udc00\":0}", null, 0, null),
            ("paired-escape-unicode17-lo", "{\\uD839\\uDEC0:0}", "{\"\ud839\udec0\":0}", null, 0, null),
            ("escaped-join-controls", "{a\\u200C\\u200D:0}", "{\"a\u200c\u200d\":0}", null, 0, null),
            ("escaped-underscore", "{\\u005F0:0}", "{\"_0\":0}", null, 0, null),
            ("composed-decomposed-distinct", "{\u00e9:0,e\u0301:1}", "{\"\u00e9\":0,\"e\u0301\":1}", null, 0, null),
            ("duplicate-decoded-name", "{\u96ea:0,\\u96EA:1}", "{\"\u96ea\":0,\"\u96ea\":1}", null, 0, null),
            ("mixed-case-hex", "{\\u96eA:0}", "{\"\u96ea\":0}", null, 0, null),
            ("multibyte-prefix-success", "{\"\u00e9\":0,\ud801\udc00:1}", "{\"\u00e9\":0,\"\ud801\udc00\":1}", null, 0, null),
            ("quoted-excluded-scalars", "{'\ud83d\ude00':0,'\u2118':1,'\u00b7':2}", "{\"\ud83d\ude00\":0,\"\u2118\":1,\"\u00b7\":2}", null, 0, null),
            ("quoted-supplementary-pair", "{'\\uD801\\uDC00':0}", "{\"\ud801\udc00\":0}", null, 0, null),
            ("quoted-value-supplementary-pair", "{n:'\\uD83D\\uDE00'}", "{\"n\":\"\ud83d\ude00\"}", null, 0, null),
            ("continue-variation-selector", "{a\ufe0f:0}", "{\"a\ufe0f\":0}", null, 0, null),
            ("continue-supplementary-mn", "{a\udb40\udd00:0}", "{\"a\udb40\udd00\":0}", null, 0, null),
            ("start-mn", "{\u0301a:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("start-mc", "{\u0903a:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("start-arabic-nd", "{\u0661a:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("start-pc", "{\u203fa:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("start-zwnj", "{\u200ca:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("start-zwj", "{\u200da:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("excluded-enclosing-mark", "{a\u20dd:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 2, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 2"),
            ("excluded-other-id-start-sm", "{\u2118:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("excluded-other-id-start-sk", "{\u309b:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("excluded-other-id-continue", "{a\u00b7:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 2, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 2"),
            ("raw-emoji-start", "{\ud83d\ude00:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("excluded-format-continue", "{a\u061c:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 2, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 2"),
            ("excluded-private-use", "{\ue000:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("excluded-unassigned", "{\u0378:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("excluded-last-noncharacter", "{\udbff\udfff:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("ascii-leading-digit", "{9a:0}", null, FerruleJson5SyntaxKind.UnexpectedToken, 1, "JSON5 syntax UnexpectedToken at original UTF-8 byte 1"),
            ("escaped-leading-digit", "{\\u0039a:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("escaped-space", "{\\u0020:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("escaped-colon", "{\\u003A:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("escaped-bmp-noncharacter", "{\\uFFFF:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("braced-escape", "{\\u{10400}:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("x-escape", "{\\xC0:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("uppercase-u-marker", "{\\U00010400:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("nonhex-digit", "{\\u00G0:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("nonascii-hex-digit", "{\\u00\uff260:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("truncated-hex", "{\\u12", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("lone-high", "{\\uD800:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("lone-low", "{\\uDC00:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("two-highs", "{\\uD800\\uD800:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("reversed-pair", "{\\uDC00\\uD800:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("high-followed-bmp", "{\\uD801\\u0041:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("pair-bad-second-hex", "{\\uD801\\uDC0Z:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("pair-wrong-second-marker", "{\\uD801\\xDC00:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("wellformed-pair-excluded-emoji", "{\\uD83D\\uDE00:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("multibyte-prefix-excluded", "{\"\u00e9\":0,a\ud83d\ude00:1}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 9, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 9"),
            ("multibyte-prefix-invalid-escape", "{\"\u00e9\":0,a\\u00G0:1}", null, FerruleJson5SyntaxKind.InvalidEscape, 9, "JSON5 syntax InvalidEscape at original UTF-8 byte 9"),
            ("low-at-continuation", "{a\\uDC00:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 2, "JSON5 syntax InvalidEscape at original UTF-8 byte 2"),
            ("bom-comment-offset", "\ufeff/*\u96ea*/{\u2118:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 11, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 11"),
            ("supplementary-nd-start", "{\ud807\udde0:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("supplementary-mn-start", "{\udb40\udd00:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 1, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 1"),
            ("excluded-zero-width-space", "{a\u200b:0}", null, FerruleJson5SyntaxKind.UnsupportedIdentifier, 2, "JSON5 syntax UnsupportedIdentifier at original UTF-8 byte 2"),
            ("bare-backslash", "{\\:0}", null, FerruleJson5SyntaxKind.InvalidEscape, 1, "JSON5 syntax InvalidEscape at original UTF-8 byte 1"),
            ("ascii-punctuation-after-key", "{a-b:0}", null, FerruleJson5SyntaxKind.UnexpectedToken, 2, "JSON5 syntax UnexpectedToken at original UTF-8 byte 2"),
        })
        {
            Console.WriteLine($"Unicode identifier original case: {id}");
            if (normalized is { } expected)
            {
                var observed = CaptureJson5(input, () => FerruleJson5Syntax.Normalize(input));
                Equal((Exception?)null, observed.Error);
                Equal(expected, observed.Output);
                Equal(true, Encoding.UTF8.GetBytes(expected).SequenceEqual(Encoding.UTF8.GetBytes(observed.Output!)));
            }
            else
            {
                var error = Json5Bad(input, kind!.Value, offset);
                Equal(message, error.Message);
            }
        }
    }

    private static void Json5UnicodeIdentifierMembership()
    {
        var helper = typeof(FerruleJson5Syntax).Assembly.GetType("Ferrule.Runtime.Json5Identifiers")
            ?? throw new InvalidOperationException("Generated identifier helper is missing.");
        var start = helper.GetMethod("IsStart", BindingFlags.Static | BindingFlags.NonPublic)
            ?? throw new InvalidOperationException("Start lookup is missing.");
        var continuation = helper.GetMethod("IsContinue", BindingFlags.Static | BindingFlags.NonPublic)
            ?? throw new InvalidOperationException("Continue lookup is missing.");
        static bool Lookup(MethodInfo method, int scalar)
        {
            try
            {
                var original = method.Invoke(null, [scalar]);
                Console.WriteLine($"U+{scalar:X6}: {method.Name} original result={original}");
                return original is bool result ? result : throw new InvalidOperationException("Lookup did not return bool.");
            }
            catch (TargetInvocationException wrapper) when (wrapper.InnerException is { } originalError)
            {
                Console.WriteLine($"Original lookup reflection wrapper: {wrapper}");
                ExceptionDispatchInfo.Capture(originalError).Throw();
                throw;
            }
        }
        foreach (var (scalar, expectedStart, expectedContinue) in new (int, bool, bool)[]
        {
            (0x24, true, true),
            (0x5F, true, true),
            (0x41, true, true),
            (0x61, true, true),
            (0x30, false, true),
            (0xC0, true, true),
            (0x3B1, true, true),
            (0x1C5, true, true),
            (0x2B0, true, true),
            (0x96EA, true, true),
            (0x2160, true, true),
            (0x301, false, true),
            (0x903, false, true),
            (0x661, false, true),
            (0x203F, false, true),
            (0x200C, false, true),
            (0x200D, false, true),
            (0x10400, true, true),
            (0x1E6C0, true, true),
            (0x1C89, true, true),
            (0x11DE0, false, true),
            (0xFE0F, false, true),
            (0xE0100, false, true),
            (0x2118, false, false),
            (0x309B, false, false),
            (0xB7, false, false),
            (0x20DD, false, false),
            (0x1F600, false, false),
            (0x61C, false, false),
            (0x200B, false, false),
            (0xE000, false, false),
            (0x378, false, false),
            (0xFFFF, false, false),
            (0x10FFFF, false, false),
            (0x20, false, false),
            (0x3A, false, false),
            (0x5C, false, false),
            (0xD800, false, false),
            (0xDC00, false, false),
            (0x110000, false, false),
        })
        {
            Equal(expectedStart, Lookup(start, scalar));
            Equal(expectedContinue, Lookup(continuation, scalar));
        }
    }

    private static void Json5UnicodeIdentifierEncoding()
    {
        foreach (var (units, index, offset, unknown, message) in new (char[], int, long, char, string)[]
        {
            (new char[] { (char)0xd800 }, 0, 0, (char)0xd800, "JSON5 text has an unpaired UTF-16 surrogate at index 0; valid-prefix UTF-8 byte 0"),
            (new char[] { (char)0xdc00 }, 0, 0, (char)0xdc00, "JSON5 text has an unpaired UTF-16 surrogate at index 0; valid-prefix UTF-8 byte 0"),
            (new char[] { (char)0x96ea, (char)0xd800 }, 1, 3, (char)0xd800, "JSON5 text has an unpaired UTF-16 surrogate at index 1; valid-prefix UTF-8 byte 3"),
            (new char[] { (char)0x0027, (char)0xd83d, (char)0xde00, (char)0x0027, (char)0xdc00 }, 4, 6, (char)0xdc00, "JSON5 text has an unpaired UTF-16 surrogate at index 4; valid-prefix UTF-8 byte 6"),
            (new char[] { (char)0x004e, (char)0x0061, (char)0x004e, (char)0xd800 }, 3, 3, (char)0xd800, "JSON5 text has an unpaired UTF-16 surrogate at index 3; valid-prefix UTF-8 byte 3"),
            (new char[] { (char)0x007b, (char)0x0022, (char)0x00e9, (char)0x0022, (char)0x003a, (char)0x0030, (char)0x002c, (char)0x0061, (char)0xd800 }, 8, 9, (char)0xd800, "JSON5 text has an unpaired UTF-16 surrogate at index 8; valid-prefix UTF-8 byte 9"),
        })
        {
            var input = new string(units);
            var observed = CaptureJson5(input, () => FerruleJson5Syntax.Normalize(input));
            Equal((string?)null, observed.Output);
            if (observed.Error is not FerruleJson5SyntaxException error)
            { throw new InvalidOperationException("Expected the complete original Encoding error.", observed.Error); }
            Equal(FerruleJson5SyntaxFailure.Encoding, error.Failure);
            Equal<FerruleJson5SyntaxKind?>(null, error.Kind);
            Equal<FerruleJson5SyntaxResource?>(null, error.Resource);
            Equal<long?>(null, error.Requested);
            Equal<long?>(null, error.Maximum);
            Equal<int?>(index, error.Utf16Index);
            Equal(offset, error.Offset);
            Equal(message, error.Message);
            if (error.InnerException is not EncoderFallbackException inner)
            { throw new InvalidOperationException("Complete encoder cause was not retained."); }
            Console.WriteLine($"Complete encoder original Message={inner.Message}; ToString={inner}");
            Equal(0, inner.Index);
            Equal(unknown, inner.CharUnknown);
            Equal(false, inner.IsUnknownSurrogate());
            Equal((Exception?)null, inner.InnerException);
        }
    }

    private static void Json5UnicodeIdentifierBudgets()
    {
        {
            var input = "null";
            var observed = CaptureJson5(input, () => Json5Limited(input, 67108864, 67108864, 127, 36));
            Equal((Exception?)null, observed.Error);
            Equal("null", observed.Output);
        }
        {
            var input = "null";
            var observed = CaptureJson5(input, () => Json5Limited(input, 67108864, 67108864, 127, 35));
            Equal((string?)null, observed.Output);
            if (observed.Error is not FerruleJson5SyntaxException error)
            { throw new InvalidOperationException("Expected the complete original Limit error.", observed.Error); }
            Equal(FerruleJson5SyntaxFailure.Limit, error.Failure);
            Equal<FerruleJson5SyntaxKind?>(null, error.Kind);
            Equal<FerruleJson5SyntaxResource?>(FerruleJson5SyntaxResource.SyntaxWork, error.Resource);
            Equal(4L, error.Offset);
            Equal<long?>(36, error.Requested);
            Equal<long?>(35, error.Maximum);
            Equal("JSON5 SyntaxWork limit at original UTF-8 byte 4: requested 36, max 35", error.Message);
            Equal<int?>(null, error.Utf16Index);
            Equal((Exception?)null, error.InnerException);
        }
        {
            var input = "\ufeffnull";
            var observed = CaptureJson5(input, () => Json5Limited(input, 7, 67108864, 127, 536870912));
            Equal((Exception?)null, observed.Error);
            Equal("null", observed.Output);
        }
        {
            var input = "\ufeffnull ";
            var observed = CaptureJson5(input, () => Json5Limited(input, 7, 67108864, 127, 536870912));
            Equal((string?)null, observed.Output);
            if (observed.Error is not FerruleJson5SyntaxException error)
            { throw new InvalidOperationException("Expected the complete original Limit error.", observed.Error); }
            Equal(FerruleJson5SyntaxFailure.Limit, error.Failure);
            Equal<FerruleJson5SyntaxKind?>(null, error.Kind);
            Equal<FerruleJson5SyntaxResource?>(FerruleJson5SyntaxResource.OriginalDocumentBytes, error.Resource);
            Equal(0L, error.Offset);
            Equal<long?>(8, error.Requested);
            Equal<long?>(7, error.Maximum);
            Equal("JSON5 OriginalDocumentBytes limit at original UTF-8 byte 0: requested 8, max 7", error.Message);
            Equal<int?>(null, error.Utf16Index);
            Equal((Exception?)null, error.InnerException);
        }
        {
            var input = "{n:'\\0'}";
            var observed = CaptureJson5(input, () => Json5Limited(input, 67108864, 14, 127, 536870912));
            Equal((Exception?)null, observed.Error);
            Equal("{\"n\":\"\\u0000\"}", observed.Output);
        }
        {
            var input = "{n:'\\0'}";
            var observed = CaptureJson5(input, () => Json5Limited(input, 67108864, 13, 127, 536870912));
            Equal((string?)null, observed.Output);
            if (observed.Error is not FerruleJson5SyntaxException error)
            { throw new InvalidOperationException("Expected the complete original Limit error.", observed.Error); }
            Equal(FerruleJson5SyntaxFailure.Limit, error.Failure);
            Equal<FerruleJson5SyntaxKind?>(null, error.Kind);
            Equal<FerruleJson5SyntaxResource?>(FerruleJson5SyntaxResource.NormalizedDocumentBytes, error.Resource);
            Equal(8L, error.Offset);
            Equal<long?>(14, error.Requested);
            Equal<long?>(13, error.Maximum);
            Equal("JSON5 NormalizedDocumentBytes limit at original UTF-8 byte 8: requested 14, max 13", error.Message);
            Equal<int?>(null, error.Utf16Index);
            Equal((Exception?)null, error.InnerException);
        }
        {
            var input = "[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[0]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]";
            var observed = CaptureJson5(input, () => Json5Limited(input, 67108864, 67108864, 127, 536870912));
            Equal((Exception?)null, observed.Error);
            Equal("[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[0]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]", observed.Output);
        }
        {
            var input = "[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[[0]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]]";
            var observed = CaptureJson5(input, () => Json5Limited(input, 67108864, 67108864, 127, 536870912));
            Equal((string?)null, observed.Output);
            if (observed.Error is not FerruleJson5SyntaxException error)
            { throw new InvalidOperationException("Expected the complete original Limit error.", observed.Error); }
            Equal(FerruleJson5SyntaxFailure.Limit, error.Failure);
            Equal<FerruleJson5SyntaxKind?>(null, error.Kind);
            Equal<FerruleJson5SyntaxResource?>(FerruleJson5SyntaxResource.ContainerDepth, error.Resource);
            Equal(127L, error.Offset);
            Equal<long?>(128, error.Requested);
            Equal<long?>(127, error.Maximum);
            Equal("JSON5 ContainerDepth limit at original UTF-8 byte 127: requested 128, max 127", error.Message);
            Equal<int?>(null, error.Utf16Index);
            Equal((Exception?)null, error.InnerException);
        }
    }
}
