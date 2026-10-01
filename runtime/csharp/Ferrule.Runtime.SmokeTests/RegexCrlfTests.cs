using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexCrlfDotAndLiteralSpans()
    {
        foreach (var separator in new[] { "\r", "\n", "\r\n" })
        {
            CallEquals(Bool(false), "matches", Text(separator), Text("(?R)(.)"));
            CallEquals(Text(separator), "replace", Text(separator), Text("(?R)(.)"), Text("[$1]"));
            Equal(separator, string.Join('|', FerruleSequences.TokenizeRegex(
                Text(separator), Text("(?R)(.)"), null).Select(ValueText)));
        }
        foreach (var scalar in new[] { "\u0085", "\u2028", "\u2029", "🙂" })
        {
            CallEquals(Text("[" + scalar + "]"), "replace", Text(scalar), Text("(?R)(.)"), Text("[$1]"));
        }
        CallEquals(Text("[\r][\n]"), "replace", Text("\r\n"), Text("(?sR)(.)"), Text("[$1]"));
        CallEquals(Text("[\r][\n]"), "replace", Text("\r\n"), Text(@"(?R)([\r\n])"), Text("[$1]"));
        CallEquals(Text("[\r]"), "replace", Text("\r"), Text(@"(?R)([^\n])"), Text("[$1]"));
        CallEquals(Text("[a\r\nb]"), "replace", Text("a\r\nb"), Text(@"(?R)(a\r\nb)"), Text("[$1]"));
        CallEquals(Text("left[\r\n]middle[\r]right[\n]end"), "replace",
            Text("left\r\nmiddle\rright\nend"), Text(@"(?R)([\r\n]+)"), Text("[$1]"));
        Equal("left|middle|right|end", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("left\r\nmiddle\rright\nend"), Text(@"(?R)([\r\n]+)"), null).Select(ValueText)));
        CallEquals(Text("[\r]"), "replace", Text("\r"), Text("(?R)(?-R:(.))"), Text("[$1]"));
        CallEquals(Text("[/\r][a/]"), "replace", Text("\ra"), Text("(?R:(a))|(.)"), Text("[$1/$2]"));
        CallEquals(Bool(false), "matches", Text("\r"), Text("(?R)(?-R:a)|(.)"));
        // s overrides R only in its group; R is restored for the second atom.
        CallEquals(Text("[\r/a]"), "replace", Text("\ra"), Text("(?R)(?s:(.))(.)"), Text("[$1/$2]"));
        CallEquals(Bool(false), "matches", Text("\r\r"), Text("(?R)(?s:(.))(.)"));
    }

    private static void RegexCrlfAnchorsAndRestoration()
    {
        foreach (var (input, pattern, replacement, expected) in new[]
        {
            ("a\r\nb\rc\nd", "(?mR)(^.+$)", "[$1]", "[a]\r\n[b]\r[c]\n[d]"),
            ("a\r\nb\rc\nd", "(?m)(^.+$)", "[$1]", "[a\r]\n[b\rc]\n[d]"),
            ("a\r\n", @"(?mR)(^a\r$)", "[$1]", "a\r\n"),
            ("a\r\n", @"(?mR)(^a\r\n$)", "[$1]", "[a\r\n]"),
            ("a\rb\rc", "(?m)(?R:(^a$))|(^b$)", "[$1/$2]", "[a/]\rb\rc"),
            ("a\rb\rc", "(?m)((?R)^a$)|(^b$)", "[$1/$2]", "[a/]\rb\rc"),
            ("a\r\nb", @"(?mR)(^a$)(?-R:\r\n)(^b$)", "[$1/$2]", "[a/b]"),
            ("a\r\n", "(?R)(?m:(^a$))", "[$1]", "[a]\r\n"),
            ("🙂\r\né\rb", "(?mR)(^.+$)", "[$1]", "[🙂]\r\n[é]\r[b]"),
            ("aaaa\r\nbbbb", "(?mRU)(^(.+))", "[$1/$2]", "[a/a]aaa\r\n[b/b]bbb"),
            ("aaaa\r\nbbbb", "(?mRU)(^(.+?)$)", "[$1/$2]", "[aaaa/aaaa]\r\n[bbbb/bbbb]"),
            ("éa\r\nb", @"(?mR-u:\b([a-z]+)\b$)", "[$1]", "é[a]\r\n[b]"),
            ("🙂🙂\r\néé", "(?mRuU)(^.+$)", "[$1]", "[🙂🙂]\r\n[éé]"),
        })
        {
            CallEquals(Text(expected), "replace", Text(input), Text(pattern), Text(replacement));
        }
        foreach (var pattern in new[] { @"(?mR)(\r)$([\n])", @"(?mR)(\r)^([\n])" })
        {
            CallEquals(Bool(false), "matches", Text("\r\n"), Text(pattern));
        }
        // Isolated terminators allow the corresponding edge; the CRLF middle does not.
        CallEquals(Bool(true), "matches", Text("\ra"), Text(@"(?mR)(\r)(^)a"));
        CallEquals(Bool(true), "matches", Text("\r"), Text(@"(?mR)(\r)($)"));
        CallEquals(Bool(false), "matches", Text("\r\na"), Text(@"(?mR)(\r)(^)\na"));
        CallEquals(Bool(false), "matches", Text("a\r"), Text("(?R)^a$"));
        CallEquals(Bool(false), "matches", Text("a\n"), Text("(?R)^a$"));
        CallEquals(Bool(true), "matches", Text("a"), Text(@"(?R)\Aa\z"));
        CallEquals(Text("[a]\r\nB"), "replace", Text("a\r\nB"), Text("(?R)(^a$)"), Text("[$1]"), Text("im"));
        Equal("|\r\n|\r|\n|", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("a\r\nb\rc\nd"), Text("(?mR)(^.+$)"), null).Select(ValueText)));
        CallEquals(Bool(true), "matches", Text(string.Empty), Text("(?mR)^$"));
    }

    private static void RegexCrlfDetectionAndConditionalGrammar()
    {
        foreach (var pattern in new[] { @"\(\?R\)", "[(?R)]+", @"\x28\x3fR\x29" })
        {
            CallEquals(Text("[(?R)]"), "replace", Text("(?R)"), Text(pattern), Text("[$0]"));
        }
        CallEquals(Text("[a]\r\n[b]"), "replace", Text("a\r\nb"),
            Text("(?x)( # before header\n ?mR:(^.+$))"), Text("[$1]"));
        CallEquals(Text("[aaaab]"), "replace", Text("aaaab"), Text("a # (?R) ignored\n+b"), Text("[$0]"), Text("x"));
        CallEquals(Text(string.Empty), "replace", Text("aaaab"), Text("^(a?(?# (?R))+b$"), Text("$1"));
        CallEquals(Text(string.Empty), "replace", Text("<<b"), Text(@"^([\<]?)+b$"), Text("$1"));
        CallEquals(Text(string.Empty), "replace", Text("aaaab"), Text("(?n)^(?<keep>a?)+b$"), Text("$1"));
        CallEquals(Text("[🙂]"), "replace", Text("🙂"), Text(@"\U0001F642"), Text("[$0]"));
        CallEquals(Text("[-R:a:-R:a]"), "replace", Text("-R:a"),
            Text("(?x)( ?-x:( ?-R:a))(?R)"), Text("[$0:$1]"));
        CallEquals(Text("[a]"), "replace", Text("a"), Text("(?R)(?<λ>a)"), Text("[$1]"));
        CallEquals(Bool(true), "matches", Text("\r"), Text("(?-R)(.)"));
        foreach (var pattern in new[]
        {
            "(?RR)a", "(?R-)a", "(?R--i)a", "(?RQ)a", "(?Rn)a",
            "(?R)(?#comment)a", "(?R)(?'name'a)", @"(?R)[\<]", @"(?R)[\cA]",
            @"(?R)[\0]", @"(?R)\p{IsBasicLatin}", @"(?R)\p{Greek}", "(?R)(?<9name>a)",
            @"(?R-u:.)", "(?xR)(? R:a)", "(?xR)(?R :a)", "(?R)a{2,1}",
        })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("a"), Text(pattern), null));
        }
        AssertInvalidArgument("matches", "flags contain an unsupported value", Text("a"), Text("a"), Text("R"));
    }

    private static void RegexCrlfLimitsAndZeroWidth()
    {
        CallEquals(Bool(true), "matches", Text("a"), Text(string.Concat(Enumerable.Repeat("(?R)(?-R)", 7_000)) + "a"));
        CallEquals(Bool(true), "matches", Text("a"), Text(@"(?R:(a{4294967295}){0})"));
        CallEquals(Bool(true), "matches", Text("a"),
            Text("(?R:" + string.Concat(Enumerable.Repeat("(?:", 249)) + "a" + new string(')', 250)));
        foreach (var pattern in new[]
        {
            "(?R:" + string.Concat(Enumerable.Repeat("(?:", 250)) + "a" + new string(')', 251),
            "(?R:" + new string('a', 8_192) + ")",
            "(?R:a{1000000})",
            "(?R:(a?)+" + string.Concat(Enumerable.Repeat("(a)", 1800)) + "b)",
        })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("a"), Text(pattern), null));
        }
        AssertInvalidArgument("matches", "pattern exceeds 64 KiB", Text("a"), Text("(?R)" + new string('a', 65_536)));
        CallEquals(Bool(true), "matches", Text("\r\n"), Text("(?mR)^$"));
        AssertInvalidArgument("replace", "pattern matches a zero-length string", Text("\r\n"), Text("(?mR)^$"), Text("x"));
        Error(FerruleRuntimeError.ZeroWidthTokenizeRegex,
            () => FerruleSequences.TokenizeRegex(Text("\r\n"), Text("(?mR)^$"), null));
        CallEquals(Bool(true), "matches", Text("b"), Text("(?R)(a?)+b"));
    }
}
