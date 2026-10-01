using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexAsciiModeClassesAndLiterals()
    {
        foreach (var (input, pattern, expected, tokens) in new[] {
            ("Aé_9🙂", @"(?-u:\w+)", "[A:::]é[_9:::]🙂", "|é|🙂"),
            ("1٢３9", @"(?-u:\d+)", "[1:::]٢３[9:::]", "|٢３|"),
            ("a\t\v\n\ré\u00A0 ", @"(?-u:\s+)", "a[\t\v\n\r:::]é\u00A0[ :::]", "a|é\u00A0|"),
            ("KkK", "(?i-u:K)", "[K:::][k:::]K", "||K"),
            ("Ssſ", "(?i-u:S)", "[S:::][s:::]ſ", "||ſ"),
            ("ab-c", "(?-u:[a-z-[b]c])", "[a:::][b:::][-:::][c:::]", "||||"),
            ("ab-c", "[a-z-[b]c](?u)", "[a:::][b:::][-:::][c:::]", "||||"),
            ("ab-c", "(?-u:[a-z--[b]])", "[a:::]b-[c:::]", "|b-|"),
        })
        {
            CallEquals(Bool(true), "matches", Text(input), Text(pattern));
            CallEquals(Text(expected), "replace", Text(input), Text(pattern), Text("[$0:$1:$2:$3]"));
            Equal(tokens, string.Join('|', FerruleSequences.TokenizeRegex(Text(input), Text(pattern), null).Select(ValueText)));
        }
        foreach (var (name, included, excluded) in new[] {
            ("alnum", "9", "_"), ("alpha", "z", "é"), ("ascii", "\u007F", "\u0080"),
            ("blank", "\t", "\n"), ("cntrl", "\0", " "), ("digit", "9", "٢"),
            ("graph", "~", " "), ("lower", "a", "A"), ("print", " ", "\t"),
            ("punct", "[", "z"), ("space", "\v", "\u00A0"), ("upper", "A", "a"),
            ("word", "_", "é"), ("xdigit", "F", "G"),
        })
        {
            var pattern = "(?-u:^[[:" + name + ":]]$)";
            CallEquals(Bool(true), "matches", Text(included), Text(pattern));
            CallEquals(Bool(false), "matches", Text(excluded), Text(pattern));
        }
        foreach (var pattern in new[] { "(?i-u:é)", @"(?i-u:\x{E9})", @"(?i-u:\u00E9)", @"(?i-u:\u{E9})", @"(?i-u:\U000000E9)" })
        {
            CallEquals(Bool(true), "matches", Text("é"), Text(pattern));
            CallEquals(Bool(false), "matches", Text("É"), Text(pattern));
        }
        CallEquals(Bool(true), "matches", Text("🙂"), Text(@"(?i-u:\U0001F642)"));
        CallEquals(Bool(false), "matches", Text("𐐨"), Text("(?i-u:𐐀)"));
        CallEquals(Bool(false), "matches", Text("K"), Text("(?i-u:[[:alpha:]])"));
        CallEquals(Bool(true), "matches", Text("A"), Text("(?i-u:[a&&A])"));
        CallEquals(Bool(false), "matches", Text("A"), Text("(?i-u:[a--A])"));
    }

    private static void RegexAsciiModeBoundariesAndCaptures()
    {
        foreach (var left in new[] { "a", "9", "_", "!", "é", "🙂", "𐐀", "\u0301" })
        foreach (var right in new[] { "a", "_", "!", "é", "🙂", "𐐀" })
        {
            var before = left is "a" or "9" or "_";
            var after = right is "a" or "_";
            foreach (var (assertion, expected) in new[] {
                (@"\b", before != after), (@"\B", before == after),
                (@"\<", !before && after), (@"\>", before && !after),
                (@"\b{start}", !before && after), (@"\b{end}", before && !after),
                (@"\b{start-half}", !before), (@"\b{end-half}", !after),
            })
            {
                var pattern = "(?-u:^" + left + assertion + right + "$)";
                CallEquals(Bool(expected), "matches", Text(left + right), Text(pattern));
                CallEquals(Text(expected ? "x" : left + right), "replace", Text(left + right), Text(pattern), Text("x"));
            }
        }
        CallEquals(Text("é[A:A::]é"), "replace", Text("éAé"), Text(@"(?-u:\b(\w+)\b)"), Text("[$0:$1:$2:$3]"));
        CallEquals(Text("[aéB:a:é:B]"), "replace", Text("aéB"),
            Text(@"(?-u:(\w+)(?u:(\w+))(\w+))"), Text("[$0:$1:$2:$3]"));
        CallEquals(Text("[aaaaaab:a:b:]"), "replace", Text("aaaaaab"),
            Text(@"(?-u:^((?:a{1,2}?)?)+(b)$)"), Text("[$0:$1:$2:$3]"));
        CallEquals(Text("[K][k][K][K][k][K]"), "replace", Text("KkKKkK"),
            Text("(?i)(?-u:K)|(?u:K)"), Text("[$0]"));
        CallEquals(Text("[kK]"), "replace", Text("kK"), Text("(?i)(?-u:K)(K)"), Text("[$0]"));
        CallEquals(Bool(true), "matches", Text("é"), Text(@"(?-u:a|(?u)\w)"));
        CallEquals(Bool(true), "matches", Text("𐐨"), Text("(?i)(?-u:a)(?u:𐐀)|𐐀"));
        CallEquals(Bool(false), "matches", Text(string.Empty), Text(@"(?-u:\b)"));
        foreach (var assertion in new[] { @"\B", @"\b{start-half}", @"\b{end-half}" })
        {
            var pattern = Text("(?-u:" + assertion + ")");
            CallEquals(Bool(true), "matches", Text(string.Empty), pattern);
            AssertInvalidArgument("replace", "pattern matches a zero-length string", Text("🙂"), pattern, Text("x"));
            Error(FerruleRuntimeError.ZeroWidthTokenizeRegex, () => FerruleSequences.TokenizeRegex(Text("🙂"), pattern, null));
        }
        CallEquals(Bool(true), "matches", Text("é"), Text(@"(?-u:\B){2}{3}é"));
    }

    private static void RegexAsciiModeStrictGrammarAndHostControls()
    {
        RegexAsciiModeIgnoredGroupHeaders();
        foreach (var pattern in new[] {
            @"(?-u:.)", @"(?s-u:.)", @"(?-u:\W)", @"(?-u:\D)", @"(?-u:\S)",
            @"(?-u:[^a])", @"(?-u:[[:^ascii:]])", @"(?-u:[^[:^ascii:]])", @"(?-u:[\W&&[:ascii:]])",
            @"(?-u:[é])", @"(?-u:[a-é])", @"(?-u:[🙂])", @"(?-u:[\u00E9])", @"(?-u:\xE9)",
            @"(?-u:\x80)", @"(?-u:\p{ASCII})", @"(?-u:\P{Lu})", @"(?-u:.{0})", @"(?-u:\W{0})",
            @"(?-u:\p{ASCII}{0})", "(?-u:[a-[a]])", @"(?u:[\b])", @"(?u:[\<])", @"(?u:[\>])",
            @"(?u:\e)", @"\e(?u)", @"(?u:\cA)", @"(?u:\0)", @"(?u:\07)", @"(?u:\Z)",
            @"(?u:\uD83D\uDE42)", @"\uD83D\uDE42(?u)", "(?u:(?#x)a)", "(?#x)a(?u)",
            "(?un:a)", "(?n:a)(?u)", "(?uu:a)", "(?u-u:a)", "(?--u:a)", "(?u--i:a)",
            "(?u-:a)", "(?u)(?ii:a)", "(?x-u :a)", "(?x-u#x\n:a)", "(?-u){2}a",
            @"(?u:\p{IsBasicLatin})", @"(?u:(?=a)a)", @"(?u:(a)\1)",
        })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern));
            AssertInvalidArgument("replace", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern), Text("x"));
            Error(FerruleRuntimeError.InvalidTokenizeRegex, () => FerruleSequences.TokenizeRegex(Text("a"), Text(pattern), null));
        }
        // A literal/comment containing the flag spelling keeps the host profile.
        CallEquals(Bool(true), "matches", Text("\b"), Text(@"[\b(?u)]"));
        CallEquals(Bool(true), "matches", Text("\b"), Text("(?x)[\\b] # (?u)"));
        CallEquals(Bool(true), "matches", Text("(?u)"), Text(@"\(\?u\)"));
        CallEquals(Bool(true), "matches", Text("<"), Text(@"[\<]"));
        CallEquals(Bool(false), "matches", Text("b"), Text("[a-z-[b]]"));
        CallEquals(Bool(true), "matches", Text("a"), Text("(?n:a)"));
        CallEquals(Bool(true), "matches", Text("a"), Text("(?#u)a"));
        AssertInvalidArgument("matches", "flags contain an unsupported value", Text("a"), Text("a"), Text("u"));
        CallEquals(Bool(true), "matches", Text("ab"), Text("[^^][a-z-[b]c](?u)"));
        CallEquals(Bool(true), "matches", Text("-b"), Text("[--][a-z-[b]c](?u)"));
        CallEquals(Bool(true), "matches", Text("a"), Text("(?x-u:a # (?u)\n)"));
        CallEquals(Bool(true), "matches", Text("a"), Text("(?x-u:\\x{6 # hex\n 1})"));
    }

    private static void RegexAsciiModeIgnoredGroupHeaders()
    {
        foreach (var (input, pattern, flags, replaced, tokens) in new[] {
            ("a", "(?x)( ?-u: a )", "", "[a:::]", "|"),
            ("a", "(?x)( # header comment\n ?-u: a )", "", "[a:::]", "|"),
            ("a", "(?x)( ?u: a )", "", "[a:::]", "|"),
            ("ab", "(?x)( ?<first> a )(b)(?u)", "", "[ab:a:b:]", "|"),
            ("ab", "(?x)( # name comment\n ?P<first> a )(b)(?u)", "", "[ab:a:b:]", "|"),
            ("a", "(?x)( ?: a )(?u)", "", "[a:::]", "|"),
            ("k", "(?ix)( ?-u)K", "", "[k:::]", "|"),
            ("K", "(?ix)( ?u)K", "", "[K:::]", "|"),
            ("kK", "(?ix)( ?-u: K )(K)", "", "[kK:K::]", "|"),
            ("KK", "(?ix)( ?-u: ( ?u: K ) )(K)", "", "[KK:K::]", "|"),
            (" a ", "(?x)( ?-x: a )(?u)", "", "[ a :::]", "|"),
            ("a", "(?x)( ?-x:(?-u:a))", "", "[a:::]", "|"),
            ("a", "(?x)(\u00A0?-u: a )", "", "[a:::]", "|"),
            ("a", "( # header comment\n ?-u: a )", "x", "[a:::]", "|"),
            ("a", "(?x)( # (?u) is ignored\n a )", "", "[a:a::]", "|"),
            ("(?u)", @"(?x)\( \?u \)", "", "[(?u):::]", "|"),
            ("u", "(?x)[ (?u) ]", "", "[u:::]", "|"),
        })
        {
            CallEquals(Bool(true), "matches", Text(input), Text(pattern), Text(flags));
            CallEquals(Text(replaced), "replace", Text(input), Text(pattern), Text("[$0:$1:$2:$3]"), Text(flags));
            Equal(tokens, string.Join('|', FerruleSequences.TokenizeRegex(Text(input), Text(pattern), Text(flags)).Select(ValueText)));
        }
        CallEquals(Bool(false), "matches", Text("K"), Text("(?ix)( ?-u)K"));
        CallEquals(Bool(false), "matches", Text("a"), Text("(?x)( ?-x: a )(?u)"));
        // A host-only escape hidden after a comment keeps the ordinary route.
        CallEquals(Text("[\b:\b]"), "replace", Text("\b"), Text("(?x)( # (?u) is ignored\n [\\b])"), Text("[$0:$1]"));
        // Ordinary host groups retain their old immediate-header behavior.
        AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text("(?x)( ?:a)"));
        CallEquals(Bool(false), "matches", Text("a"), Text("(?x)( ?-x:( ?-u:a))(?u)"));
        CallEquals(Text("[-u:a:-u:a]"), "replace", Text("-u:a"), Text("(?x)( ?-x:( ?-u:a))(?u)"), Text("[$0:$1]"));
        foreach (var pattern in new[] { "(?x)( ?-u :a)", "(?x)( ?-u# bad flag\n:a)" })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern));
            Error(FerruleRuntimeError.InvalidTokenizeRegex, () => FerruleSequences.TokenizeRegex(Text("a"), Text(pattern), null));
        }
    }

    private static void RegexAsciiModeBounds()
    {
        CallEquals(Bool(true), "matches", Text("a"), Text(string.Concat(Enumerable.Repeat("(?u)(?-u)", 7_000)) + "a"));
        CallEquals(Bool(false), "matches", Text("a"), Text(string.Concat(Enumerable.Repeat("(?-u:a)", 8_000))));
        CallEquals(Bool(true), "matches", Text("a"), Text(@"(?-u:(a{4294967295}){0})"));
        CallEquals(Bool(true), "matches", Text("a"), Text("(?-u:" + string.Concat(Enumerable.Repeat("(?:", 249)) + "a" + new string(')', 250)));
        foreach (var pattern in new[] {
            "(?-u:" + string.Concat(Enumerable.Repeat("(?:", 250)) + "a" + new string(')', 251),
            "(?-u:" + string.Concat(Enumerable.Repeat("(?:", 249)) + "[a]" + new string(')', 250),
            "(?-u:" + new string('a', 8_192) + ")", "(?-u:a{1000000})",
            "(?-u:" + string.Concat(Enumerable.Repeat("(a?)", 128)) + "a{131072})",
        })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern));
            Error(FerruleRuntimeError.InvalidTokenizeRegex, () => FerruleSequences.TokenizeRegex(Text("a"), Text(pattern), null));
        }
        AssertInvalidArgument("matches", "pattern exceeds 64 KiB", Text("a"), Text(string.Concat(Enumerable.Repeat("(?-u:a)", 10_000))));
        var costly = Text(@"(?-u:\B(?:a?){1024}b)");
        AssertInvalidArgument("matches", "word-boundary regex exceeds its work limit", Text(new string('a', 20_000)), costly);
        CallEquals(Bool(true), "matches", Text("éAé"), Text(@"(?-u:\bA\b)"));
    }
}
