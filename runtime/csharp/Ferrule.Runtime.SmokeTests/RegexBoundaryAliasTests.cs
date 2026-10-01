using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexBoundaryAliasTruthTable()
    {
        foreach (var (left, right) in new[] { ("a", "a"), ("a", "!"), ("!", "a"), ("!", "!"),
            ("𐐀", "🙂"), ("🙂", "𐐀"), ("🙂", "🙂"), ("𐐀", "𐐨"), ("\u0301", "a"), ("a", "\u200D") })
        {
            var before = left != "!" && left != "🙂";
            var after = right != "!" && right != "🙂";
            foreach (var (alias, expected) in new[] {
                (@"\b{start}", !before && after), (@"\<", !before && after),
                (@"\b{end}", before && !after), (@"\>", before && !after),
                (@"\b{start-half}", !before), (@"\b{end-half}", !after),
            })
            {
                var input = left + right;
                var pattern = "^(" + left + ")" + alias + "(" + right + ")$";
                CallEquals(Bool(expected), "matches", Text(input), Text(pattern));
                CallEquals(Text(expected ? left + "/" + right : input), "replace", Text(input), Text(pattern), Text("$1/$2"));
                Equal(expected ? "|" : input, string.Join('|', FerruleSequences.TokenizeRegex(
                    Text(input), Text(pattern), null).Select(ValueText)));
            }
        }
        CallEquals(Bool(true), "matches", Text("a"), Text(@"^\<a\>$"));
        CallEquals(Bool(false), "matches", Text("<a>"), Text(@"^\<a\>$"));
        CallEquals(Bool(true), "matches", Text("<a>"), Text("^<a>$"));
        CallEquals(Bool(true), "matches", Text(@"\<a\>"), Text(@"^\\<a\\>$"));
        CallEquals(Bool(true), "matches", Text("𐐨"), Text(@"(?i)^\<𐐀\>$"));
        CallEquals(Bool(true), "matches", Text("\n𐐀\n"), Text(@"(?m)^\b{start}𐐀\b{end}$"));
    }

    private static void RegexBoundaryAliasCapturesAndZeroWidth()
    {
        foreach (var (start, end) in new[] { (@"\b{start}", @"\b{end}"), (@"\<", @"\>"),
            (@"\b{start-half}", @"\b{end-half}") })
        {
            var pattern = start + @"(?<word>\w+)" + end;
            CallEquals(Text(" [a:a:]🙂[𐐀:𐐀:] "), "replace", Text(" a🙂𐐀 "), Text(pattern), Text("[$0:$1:$2]"));
            Equal(" |🙂| ", string.Join('|', FerruleSequences.TokenizeRegex(
                Text(" a🙂𐐀 "), Text(pattern), null).Select(ValueText)));
        }
        CallEquals(Text("a/b"), "replace", Text("ab"), Text(@"^\b{start}(?<first>a)(b)\b{end}$"), Text("$1/$2"));
        CallEquals(Text("/aaaaaa"), "replace", Text("aaaaaa"),
            Text(@"^(?<edge>\b{start}){2}{3}(?<word>a+)\b{end}$"), Text("$1/$2"));
        CallEquals(Text("/aaa"), "replace", Text("aaa"),
            Text(@"^(?<edge>\b{start-half})+*(?<word>a+)\b{end}$"), Text("$1/$2"));
        CallEquals(Text("a/aa"), "replace", Text("aaa"), Text(@"\b{start}(a+?)(a*)\b{end}"), Text("$1/$2"));
        CallEquals(Text("[🙂]"), "replace", Text("🙂"), Text(@"^\b{start}{0}(🙂)\b{end}{0}$"), Text("[$1]"));
        CallEquals(Bool(true), "matches", Text("a"), Text(@"^\b{ 2 }{3}a\b{end}$"));
        foreach (var assertion in new[] { @"\b{start}", @"\b{end}", @"\<", @"\>" })
        {
            CallEquals(Bool(false), "matches", Text(string.Empty), Text(assertion));
            CallEquals(Bool(true), "matches", Text("a"), Text(assertion));
            CallEquals(Text(string.Empty), "replace", Text(string.Empty), Text(assertion), Text("x"));
            Equal(0, FerruleSequences.TokenizeRegex(Text(string.Empty), Text(assertion), null).Count);
            AssertInvalidArgument("replace", "pattern produced a zero-length match", Text("a"), Text(assertion), Text("x"));
            Error(FerruleRuntimeError.ZeroWidthTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("a"), Text(assertion), null));
        }
        foreach (var assertion in new[] { @"\b{start-half}", @"\b{end-half}", @"\<{0}", @"\b{end}{0}" })
        {
            CallEquals(Bool(true), "matches", Text(string.Empty), Text(assertion));
            AssertInvalidArgument("replace", "pattern matches a zero-length string", Text("a"), Text(assertion), Text("x"));
            Error(FerruleRuntimeError.ZeroWidthTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("a"), Text(assertion), null));
        }
    }

    private static void RegexBoundaryAliasLexicalRules()
    {
        foreach (var spelling in new[] { @"\b{ s t a r t }", "\\b{\u2003start\u2003}",
            "\\b{# open\nsta# middle\nrt# close\n}", @"\b{start}" })
        {
            CallEquals(Bool(true), "matches", Text("aa"), Text(spelling + "(a+)\\b{ e n d }# eof"), Text("x"));
            CallEquals(Text("[aa]"), "replace", Text("aa"), Text(spelling + "(a+)\\b{ e n d }# eof"), Text("[$1]"), Text("x"));
        }
        CallEquals(Bool(true), "matches", Text("🙂"), Text(@"^\b{ s t a r t - h a l f }🙂\b{ e n d - h a l f }$"), Text("x"));
        CallEquals(Bool(true), "matches", Text("a"), Text("^\\b{start} # repeat\n {2}{3} a \\b{end}$"), Text("x"));
        foreach (var flags in new[] { string.Empty, "x" })
        {
            foreach (var malformed in new[] { @"\b {start}", "\\b # gap\n {start}", @"\b{START}", @"\b{start_half}",
                @"\b{word}", @"\b{start!}", @"\b{starts}", @"\b{}", @"\b{", @"\b{start", @"\b{# eof",
                @"\B{start}", @"\<{start}", @"\>{end}", @"\b{start}{word}", @"\b{start}{2,1}",
                @"\b{start}{4294967296}", @"\b{start}(?i){2}" })
            {
                AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(malformed), Text(flags));
                AssertInvalidArgument("replace", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(malformed), Text("x"), Text(flags));
                Error(FerruleRuntimeError.InvalidTokenizeRegex,
                    () => FerruleSequences.TokenizeRegex(Text("a"), Text(malformed), Text(flags)));
            }
        }
        foreach (var malformed in new[] { @"\b{ start }", @"\b{s t a r t}" })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(malformed));
        }
        // Failed specialized parsing cannot change a later plain/count assertion.
        CallEquals(Bool(true), "matches", Text("a"), Text(@"\b{2}a\B{0}\b{end}"));
    }

    private static void RegexBoundaryAliasBounds()
    {
        CallEquals(Bool(true), "matches", Text("a"), Text(@"\b{start}" + string.Concat(Enumerable.Repeat("{1}", 255)) + "a"));
        foreach (var malformed in new[] { @"\b{start}" + string.Concat(Enumerable.Repeat("{1}", 256)) + "a",
            @"\b{" + new string('a', 60_000) + "}", string.Concat(Enumerable.Repeat(@"\<", 8192)) + "a", @"\<a{163840}" })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(malformed));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("a"), Text(malformed), null));
        }
        var oversized = Text(@"\b{start-half}" + new string(' ', 65_536));
        AssertInvalidArgument("matches", "pattern exceeds 64 KiB", Text("a"), oversized);
        Error(FerruleRuntimeError.TokenizeRegexPatternTooLarge,
            () => FerruleSequences.TokenizeRegex(Text("a"), oversized, Text("x")));
        CallEquals(Bool(true), "matches", Text("a"), Text(@"\<{4294967295}{0}a\>{0}"));
        // Keep existing host class extensions outside this assertion reader.
        CallEquals(Bool(true), "matches", Text("<"), Text(@"^[\<]$"));
        CallEquals(Bool(true), "matches", Text("\b"), Text(@"^[\b]$"));
        CallEquals(Bool(true), "matches", Text("a"), Text(new string('(', 300) + "a" + new string(')', 300)));
    }
}
