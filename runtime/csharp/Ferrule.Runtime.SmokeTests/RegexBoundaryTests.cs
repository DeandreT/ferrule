using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexScalarWordBoundaries()
    {
        foreach (var word in new[] { "a", "𐐀", "Ⓐ", "🅰", "\u0301", "\u20DD", "\u200C", "\u200D", "١" })
        {
            CallEquals(Bool(true), "matches", Text(word), Text(@"^\b" + word + @"\b$"));
            CallEquals(Bool(false), "matches", Text("a" + word), Text(@"^a\b" + word + "$"));
            CallEquals(Bool(true), "matches", Text("a" + word), Text(@"^a\B" + word + "$"));
            CallEquals(Bool(true), "matches", Text(word), Text(@"^\b\b" + word + @"\b\b$"));
        }
        CallEquals(Bool(true), "matches", Text("🙂"), Text(@"^\B🙂\B$"));
        CallEquals(Bool(false), "matches", Text("🙂"), Text(@"^\b🙂\b$"));
        CallEquals(Bool(true), "matches", Text(string.Empty), Text(@"\B"));
        CallEquals(Bool(false), "matches", Text(string.Empty), Text(@"\b"));
        CallEquals(Bool(true), "matches", Text("\n𐐀\n"), Text(@"(?m)^\b𐐀\b$"));
        CallEquals(Bool(true), "matches", Text("𐐨"), Text(@"(?i)^\b𐐀\b$"));
        CallEquals(Bool(true), "matches", Text("b"), Text(@"^\b[a-z&&[^aeiou]]\b$"));
        CallEquals(Bool(false), "matches", Text("a"), Text(@"^\b[a-z&&[^aeiou]]\b$"));
        CallEquals(Bool(true), "matches", Text("a"), Text(" ^ \\b [ [:alpha:] # comment\n ] \\b $ # eof"), Text("x"));
        // Escaped backslash, class backspace and comments carry no word assertion.
        CallEquals(Bool(true), "matches", Text(@"\b"), Text(@"^\\b$"));
        CallEquals(Bool(true), "matches", Text("\b"), Text(@"^[\b]$"));
        CallEquals(Bool(true), "matches", Text("a"), Text("a # \\b"), Text("x"));
    }

    private static void RegexBoundaryCapturesAndIteration()
    {
        CallEquals(Text("𐐀|𐐀|"), "replace", Text("𐐀"),
            Text(@"^(?:(\b𐐀)|(\B𐐀))$"), Text("$0|$1|$2"));
        CallEquals(Text("|a𐐀 |a𐐀"), "replace", Text("a𐐀 a𐐀"),
            Text(@"(?:(a\b𐐀)|(a\B𐐀))"), Text("$1|$2"));
        CallEquals(Text("a[𐐀]"), "replace", Text("a𐐀"), Text(@"^a\b𐐀|𐐀$"), Text("[$0]"));
        CallEquals(Text("aaa|"), "replace", Text("aaa"), Text(@"\b(a+)(a*)"), Text("$1|$2"));
        CallEquals(Text("a|aa"), "replace", Text("aaa"), Text(@"\b(a+?)(a*)"), Text("$1|$2"));
        CallEquals(Text("a/b a/"), "replace", Text("ab a"), Text(@"\b(?<first>a)(b)?"), Text("$1/$2"));
        CallEquals(Text("a:b:c"), "replace", Text("abc"),
            Text(@"\b(?<first>a)(b)(?P<third>c)"), Text("$1:$2:$3"));
        CallEquals(Text("a|b"), "replace", Text("aba"), Text(@"\b(a(b)?)+"), Text("$1|$2"));
        CallEquals(Text("[a]"), "replace", Text("a"), Text(@"\ba*"), Text("[$0]"));
        CallEquals(Text("[aaa]"), "replace", Text("aaa"), Text(@"\b((a*)*)"), Text("[$1]"));
        Equal("x|z", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("x🙂𐐀🙂z"), Text(@"🙂\b𐐀\b🙂"), null).Select(ValueText)));
        Equal("left | right", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("left 𐐀 right"), Text(@"\b𐐀\b"), null).Select(ValueText)));
        Equal("a|", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("a𐐀"), Text(@"^a\b𐐀|𐐀$"), null).Select(ValueText)));
        foreach (var pattern in new[] { @"\B", @"\b(?:a?)*", @"\b((a)?|(b)?)*" })
        {
            Error(FerruleRuntimeError.FunctionInvalidArgument,
                () => FerruleFunctions.Call("replace", new[] { Text("ba"), Text(pattern), Text("x") }));
            Error(FerruleRuntimeError.ZeroWidthTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("ba"), Text(pattern), null));
        }
    }

    private static void RegexBoundaryBoundsAndUnsupportedSyntax()
    {
        foreach (var pattern in new[] { @"\ba{4294967295}",
            @"\b" + new string('a', 8192),
            @"\b" + string.Concat(Enumerable.Repeat("(a?)", 128)) + "a{131072}",
            @"\b(?=a)a", @"\b(?>a)", @"\b(a)\1", @"\b\Ga", @"\b\Za",
            @"\b(?<same>a)(?<same>b)", @"\b(?'host'a)" })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern));
            AssertInvalidArgument("replace", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern), Text("x"));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("a"), Text(pattern), null));
        }
        var input = Text(new string('a', 20_000));
        var costly = Text(@"\B(?:a?){1024}b");
        AssertInvalidArgument("matches", "word-boundary regex exceeds its work limit", input, costly);
        AssertInvalidArgument("replace", "word-boundary regex exceeds its work limit", input, costly, Text("x"));
        Error(FerruleRuntimeError.InvalidTokenizeRegex,
            () => FerruleSequences.TokenizeRegex(input, costly, null));
        var globalInput = Text(string.Concat(Enumerable.Repeat(new string('a', 128) + "b ", 4_000)));
        var globalPattern = Text(@"\b(?:a?){128}b");
        // A first-match query succeeds without precharging unused input. Global
        // replacement/tokenization must share work across every selected span.
        CallEquals(Bool(true), "matches", globalInput, globalPattern);
        AssertInvalidArgument("replace", "word-boundary regex exceeds its work limit", globalInput, globalPattern, Text("x"));
        Error(FerruleRuntimeError.InvalidTokenizeRegex,
            () => FerruleSequences.TokenizeRegex(globalInput, globalPattern, null));
        // Failed searches and compile attempts leave no process-global budget.
        CallEquals(Bool(true), "matches", Text("a"), Text(@"\ba\b"));
        CallEquals(Text("[a]"), "replace", Text("a"), Text(@"\b(a)\b"), Text("[$1]"));
    }
}
