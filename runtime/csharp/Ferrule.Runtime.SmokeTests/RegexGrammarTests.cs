using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexCaptureOpeningOrder()
    {
        foreach (var header in new[] { "?<", "?P<" })
        {
            CallEquals(Text("a-b"), "replace", Text("ab"), Text("(" + header + "first>a)(b)"), Text("$1-$2"));
            CallEquals(Text("abc|a|b|c|"), "replace", Text("abc"),
                Text("^(" + header + "first>a)(b)(" + header + "last>c)$"), Text("$0|$1|$2|$3|$4"));
            CallEquals(Text("abc|ab|b|c"), "replace", Text("abc"),
                Text("^(" + header + "outer>a(" + header + "inner>b))(c)$"), Text("$0|$1|$2|$3"));
            CallEquals(Text("|b"), "replace", Text("b"), Text("(a)?(" + header + "second>b)"), Text("$1|$2"));
            CallEquals(Text("a0:a2:a"), "replace", Text("abc"),
                Text("^(" + header + "first>a)(b)(c)$"), Text("$10:$12:$01"));
            var pattern = string.Concat(Enumerable.Range(1, 12).Select(index => index % 2 == 1
                ? "(" + header + "capture" + index + ">" + (char)('a' + index - 1) + ")"
                : "(" + (char)('a' + index - 1) + ")"));
            CallEquals(Text("a:b:c:d:e:f:g:h:i:j:k:l"), "replace", Text("abcdefghijkl"), Text(pattern),
                Text(string.Join(':', Enumerable.Range(1, 12).Select(index => "$" + index))));
            CallEquals(Bool(true), "matches", Text("ab"), Text("(" + header + "first>a)(b)"));
            Equal("x|z", string.Join('|', FerruleSequences.TokenizeRegex(
                Text("xabz"), Text("(" + header + "first>a)(b)"), null).Select(ValueText)));
        }
        foreach (var name in new[] { "_", "é", "𐐀", "a.b", "a[0]", "a[", "a]", "ferrule0061", "Ⅰ" })
        {
            foreach (var header in new[] { "?<", "?P<" })
            {
                CallEquals(Text("a-b"), "replace", Text("ab"), Text("(" + header + name + ">a)(b)"), Text("$1-$2"));
            }
        }
        foreach (var pattern in new[] { "(?P<1>a)(b)", "(?P<bad-name>a)", "(?P<>a)",
            "(?P<same>a)(?P<same>b)", "(?<same>a)(?P<same>b)", "(?P<same>a)(?<same>b)" })
        {
            AssertInvalidArgument("replace", "pattern is invalid or exceeds the compiled-size limit", Text("ab"), Text(pattern), Text("$1"));
        }
        // The previously accepted host n flag still suppresses ordinary captures.
        CallEquals(Text("b|"), "replace", Text("abc"), Text("(?n)(a)(?<second>b)(c)"), Text("$1|$2"));
        CallEquals(Text("[🙂]"), "replace", Text("🙂"), Text("(?P<scalar>.)"), Text("[$1]"));
    }

    private static void RegexBracedUnicodeEscapes()
    {
        foreach (var prefix in new[] { "x", "u", "U" })
        {
            var escaped = @"\" + prefix + "{1F642}";
            CallEquals(Bool(true), "matches", Text("🙂"), Text("^" + escaped + "$"));
            CallEquals(Bool(true), "matches", Text("🙂🙂"), Text("^" + escaped + "{2}$"));
            CallEquals(Bool(true), "matches", Text("🙂"), Text("^[" + escaped + "]$"));
            CallEquals(Bool(true), "matches", Text("m"), Text("^[\\" + prefix + "{61}-\\" + prefix + "{7A}]$"));
            CallEquals(Bool(false), "matches", Text("🙂"), Text("^[^" + escaped + "]$"));
            CallEquals(Bool(true), "matches", Text("a"), Text(@"\" + prefix + "{" + new string('0', 200) + "61}"));
            CallEquals(Text("[🙂]"), "replace", Text("🙂"), Text("(" + escaped + ")"), Text("[$1]"));
            Equal("a|b", string.Join('|', FerruleSequences.TokenizeRegex(Text("a🙂b"), Text(escaped), null).Select(ValueText)));
            CallEquals(Bool(true), "matches", Text("a"), Text(@"\" + prefix + "{6 # hex\n 1}".Replace("\\n", "\n")), Text("x"));
            foreach (var body in new[] { "", "D800", "DFFF", "110000", "FFFFFFFFFFFFFFFF", "-61", "0x61", "G", "6 1" })
            {
                var invalid = @"\" + prefix + "{" + body + "}";
                AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(invalid));
                Error(FerruleRuntimeError.InvalidTokenizeRegex,
                    () => FerruleSequences.TokenizeRegex(Text("a"), Text(invalid), null));
            }
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(@"\" + prefix + "{61"));
        }
        CallEquals(Bool(true), "matches", Text("🙂"), Text(@"^\U0001F642$"));
        CallEquals(Bool(true), "matches", Text("a"), Text("\\x6 # digit\n 1"), Text("x"));
        CallEquals(Bool(true), "matches", Text("\U0010FFFF"), Text(@"^\x{10FFFF}$"));
    }

    private static void RegexBraceValidation()
    {
        foreach (var pattern in new[] { "a{word}", "a{}", "a{,2}", "{2}", "a|{2}", "(?i){2}", "a{", "a{2", "a{2,x}",
            "a{2,1}", "a{1,,2}", "a{+2}", "a{1 2}", "a{1, }", "a{4294967296}" })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("aa"), Text(pattern));
            AssertInvalidArgument("replace", "pattern is invalid or exceeds the compiled-size limit", Text("aa"), Text(pattern), Text("x"));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("aa"), Text(pattern), null));
        }
        foreach (var pattern in new[] { "^a{ 2 }$", "^a{\t2\n}$", "^a{\u20032\u2003}$", "^a{ 1 , 2 }$", "^a{0000000000002}$" })
        {
            CallEquals(Bool(true), "matches", Text("aa"), Text(pattern));
        }
        CallEquals(Bool(true), "matches", Text("aaaaaaaaaaaa"), Text("^a{1 2}$"), Text("x"));
        CallEquals(Bool(true), "matches", Text("aa"), Text("^a{1, }$"), Text("x"));
        CallEquals(Bool(true), "matches", Text("aa"), Text("^a{2# count\n}$"), Text("x"));
        CallEquals(Bool(true), "matches", Text("a{word}"), Text(@"^a\{word\}$"));
        CallEquals(Bool(true), "matches", Text("a}"), Text("^a}$"));
        CallEquals(Bool(true), "matches", Text("{}"), Text("^[{}]{2}$"));
        CallEquals(Bool(true), "matches", Text("🙂"), Text(" ^ ( . ) $ # trailing capture"), Text("x"));
        CallEquals(Text("[🙂]"), "replace", Text("🙂"), Text(" ^ ( . ) $ # trailing capture"), Text("[$1]"), Text("x"));
    }
}
