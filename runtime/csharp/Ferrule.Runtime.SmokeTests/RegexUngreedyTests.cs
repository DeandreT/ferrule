using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexUngreedyCapturesAndRepetitions()
    {
        foreach (var (pattern, expected) in new[]
        {
            ("(?U)(a+)(a*b)", "[aaaaab:a:aaaab:]"),
            ("(?U)(a+?)(a*b)", "[aaaaab:aaaaa:b:]"),
            ("(?U)(a*)(a+b)", "[aaaaab::aaaaab:]"),
            ("(?U)^(a?)(a*b)$", "[aaaaab::aaaaab:]"),
            ("(?U)^(a??)(a*b)$", "[aaaaab:a:aaaab:]"),
            ("(?U)(a{2,4})(a*b)", "[aaaaab:aa:aaab:]"),
            ("(?U)(a{2,4}?)(a*b)", "[aaaaab:aaaa:ab:]"),
            ("(?U)(a{2,})(a*b)", "[aaaaab:aa:aaab:]"),
            ("(?U)(a{2,}?)(a*b)", "[aaaaab:aaaaa:b:]"),
            ("(?U)(a{2})(a*b)", "[aaaaab:aa:aaab:]"),
            ("(?U)(a{2}?)(a*b)", "[aaaaab:aa:aaab:]"),
            ("(?U)^(a?)+(b)$", "[aaaaab:a:b:]"),
            ("(?U)^((a)?)+(b)$", "[aaaaab:a:a:b]"),
        })
        {
            CallEquals(Bool(true), "matches", Text("aaaaab"), Text(pattern));
            CallEquals(Text(expected), "replace", Text("aaaaab"), Text(pattern), Text("[$0:$1:$2:$3]"));
            Equal("|", string.Join('|', FerruleSequences.TokenizeRegex(
                Text("aaaaab"), Text(pattern), null).Select(ValueText)));
        }
        foreach (var (pattern, expected) in new[]
        {
            ("(?U)(a++)", "x[a][a][a][a][a][a]y"),
            ("(?U)(a+?+)", "x[aaaaaa]y"),
            ("(?U)(a++?)", "x[aaaaaa]y"),
            ("(?U)(a{1,2}{2,3})", "x[aa][aa][aa]y"),
            ("(?U)(a{1,2}?{2,3})", "x[aaaa][aa]y"),
            ("(?U)(a{1,2}{2,3}?)", "x[aaa][aaa]y"),
        })
        {
            CallEquals(Text(expected), "replace", Text("xaaaaaay"), Text(pattern), Text("[$1]"));
        }
        CallEquals(Text("x[a][a][a]y[a][a][a]Z"), "replace", Text("xaaayaaaZ"), Text("(?U)(a+)"), Text("[$1]"));
        Equal("x|||y|||Z", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("xaaayaaaZ"), Text("(?U)(a+)"), null).Select(ValueText)));
        CallEquals(Text("x[aaa]y[aaa]Z"), "replace", Text("xaaayaaaZ"), Text("(?U)(a+?)"), Text("[$1]"));
        Equal("x|y|Z", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("xaaayaaaZ"), Text("(?U)(a+?)"), null).Select(ValueText)));
    }

    private static void RegexUngreedyScopeAndDetection()
    {
        foreach (var (input, pattern, flags, expected) in new[]
        {
            ("aaaaa", "(?U:(a+))(a+)", "", "[aaaaa:a:aaaa:]"),
            ("aaaaa", "((?U)a+)(a+)", "", "[aaaaa:a:aaaa:]"),
            ("aaaaab", "(?U)(a+)(?-U:(a+))(a*b)", "", "[aaaaab:a:aaaa:b]"),
            ("aaaaa", "(?U)(a+)(?-U)(a+)", "", "[aaaaa:a:aaaa:]"),
            ("aaaaab", "(?-U)(a+)(a*b)", "", "[aaaaab:aaaaa:b:]"),
            ("aaaaa", "(?x)( # before header\n ?U:(a+)) (a+)", "", "[aaaaa:a:aaaa:]"),
            ("AAAAAb", "(?iU)(a+)(a*b)", "", "[AAAAAb:A:AAAAb:]"),
            ("🙂🙂🙂b", "(?U)(🙂+)(🙂*b)", "", "[🙂🙂🙂b:🙂:🙂🙂b:]"),
            ("Aé_9", @"(?U-u:(\w+))(?Uu:(\w+))", "", "[Aé:A:é:][_9:_:9:]"),
        })
        {
            CallEquals(Bool(true), "matches", Text(input), Text(pattern), Text(flags));
            CallEquals(Text(expected), "replace", Text(input), Text(pattern), Text("[$0:$1:$2:$3]"), Text(flags));
        }
        CallEquals(Text("[a:a:][a:a:][a:a:][bbb::bbb]"), "replace", Text("aaabbb"),
            Text("(?U:(a+))|(b+)"), Text("[$0:$1:$2]"));
        foreach (var pattern in new[] { @"\(\?U\)", "[(?U)]+", @"\x28\x3fU\x29" })
        {
            CallEquals(Text("[(?U)]"), "replace", Text("(?U)"), Text(pattern), Text("[$0]"));
        }
        CallEquals(Text("[🙂]"), "replace", Text("🙂"), Text(@"\U0001F642"), Text("[$0]"));
        CallEquals(Text("[aaaab]"), "replace", Text("aaaab"), Text("a # (?U) is ignored\n+b"), Text("[$0]"), Text("x"));
        CallEquals(Text(string.Empty), "replace", Text("aaaab"), Text("^(a?(?# (?U))+b$"), Text("$1"));
        // Real U always selects the closed scalar grammar; the same ordinary
        // host-only forms without U retain their existing output.
        CallEquals(Text(string.Empty), "replace", Text("<<b"), Text(@"^([\<]?)+b$"), Text("$1"));
        CallEquals(Text(string.Empty), "replace", Text("aaaab"), Text("(?n)^(?<keep>a?)+b$"), Text("$1"));
    }

    private static void RegexUngreedyConditionalGrammar()
    {
        foreach (var pattern in new[]
        {
            "(?UU)a", "(?U-)a", "(?U--i)a", "(?UQ)a",
            "(?Un)a", "(?U)(?#comment)a", "(?U)(?'name'a)",
            @"(?U)[\<]", @"(?U)[\cA]", @"(?U)[\0]", @"(?U)\p{IsBasicLatin}",
            @"(?U)\p{Greek}", "(?U)(?<9name>a)", @"(?U-u:.)",
            "(?xU)(? U:a)", "(?xU)(?U :a)",
            "(?U)a{2,1}", "(?U)a{4294967296}",
        })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("a"), Text(pattern), null));
        }
        CallEquals(Bool(true), "matches", Text("a"), Text("(?U)(a)"), Text("i"));
        CallEquals(Text("[a:a]"), "replace", Text("a"), Text("(?U)(?<λ>a)"), Text("[$0:$1]"));
        CallEquals(Bool(false), "matches", Text("a"), Text("(?x)( ?-x:( ?-U:a))(?U)"));
        CallEquals(Text("[-U:a:-U:a]"), "replace", Text("-U:a"), Text("(?x)( ?-x:( ?-U:a))(?U)"), Text("[$0:$1]"));
        CallEquals(Text("[a:][a:][b:][b:]"), "replace", Text("aabb"), Text("((?U)a+|b+)"), Text("[$1:]"));
        AssertInvalidArgument("matches", "flags contain an unsupported value", Text("a"), Text("a"), Text("U"));
        CallEquals(Bool(true), "matches", Text("a"), Text("(?UR)a"));
        CallEquals(Bool(false), "matches", Text("\r"), Text("(?UR)."));
    }

    private static void RegexUngreedyLimitsAndZeroWidth()
    {
        CallEquals(Bool(true), "matches", Text("a"), Text(string.Concat(Enumerable.Repeat("(?U)(?-U)", 7_000)) + "a"));
        CallEquals(Bool(true), "matches", Text("a"), Text(@"(?U:(a{4294967295}){0})"));
        CallEquals(Bool(true), "matches", Text("a"),
            Text("(?U:" + string.Concat(Enumerable.Repeat("(?:", 249)) + "a" + new string(')', 250)));
        foreach (var pattern in new[]
        {
            "(?U:" + string.Concat(Enumerable.Repeat("(?:", 250)) + "a" + new string(')', 251),
            "(?U:" + new string('a', 8_192) + ")",
            "(?U:a{1000000})",
            "(?U:(a?)+" + string.Concat(Enumerable.Repeat("(a)", 1800)) + "b)",
        })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("a"), Text(pattern), null));
        }
        AssertInvalidArgument("matches", "pattern exceeds 64 KiB", Text("a"), Text("(?U)" + new string('a', 65_536)));
        CallEquals(Bool(true), "matches", Text(string.Empty), Text("(?U)(a*)"));
        AssertInvalidArgument("replace", "pattern matches a zero-length string", Text("aaa"), Text("(?U)(a*)"), Text("[$1]"));
        Error(FerruleRuntimeError.ZeroWidthTokenizeRegex,
            () => FerruleSequences.TokenizeRegex(Text("aaa"), Text("(?U)(a*)"), null));
        CallEquals(Bool(true), "matches", Text("b"), Text("(?U)(a?)+b"));
    }
}
