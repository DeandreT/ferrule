using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexUnicodeMatching()
    {
        CallEquals(Bool(true), "matches", Text("🙂"), Text("^.$"));
        CallEquals(Bool(true), "matches", Text("🙂"), Text("^[^a]$"));
        CallEquals(Bool(true), "matches", Text("🙂🙂"), Text("^🙂{2}$"));
        CallEquals(Bool(true), "matches", Text("A🙂é"), Text("^.{3}$"));
        CallEquals(Bool(false), "matches", Text("A🙂é"), Text("^.{4}$"));
        CallEquals(Bool(false), "matches", Text("🙂\n"), Text("^.$"));
        CallEquals(Bool(true), "matches", Text("🙂\n"), Text("^.$"), Text("m"));
        CallEquals(Bool(true), "matches", Text("🙂\n"), Text("^.{2}$"), Text("s"));
        CallEquals(Bool(true), "matches", Text("🙂"), Text(@"^\uD83D\uDE42$"));
        CallEquals(Bool(true), "matches", Text("🙂"), Text(@"^[\uD83D\uDE42]$"));
        CallEquals(Bool(true), "matches", Text("🙂"), Text("(?x) ^ # scalar\n . $"));
        CallEquals(Bool(true), "matches", Text("🙂#"), Text("(?x) ^ . \\# $"));
        CallEquals(Bool(true), "matches", Text("🙂"), Text(" ^ ( . ) $ # scalar capture"), Text("x"));
        foreach (var whitespace in new[] { "\v", "\u0085", "\u00A0", "\u2000", "\u2028", "\u3000" })
        {
            CallEquals(Bool(true), "matches", Text("ab"), Text("^a" + whitespace + "b$"), Text("x"));
            CallEquals(Bool(false), "matches", Text(whitespace), Text("^[a" + whitespace + "]$"), Text("x"));
        }
        CallEquals(Bool(true), "matches", Text("m"), Text("^[a # range comment\n - z]$"), Text("x"));
        CallEquals(Bool(false), "matches", Text("#"), Text("^[a # range comment\n b]$"), Text("x"));
        CallEquals(Bool(true), "matches", Text(" "), Text(@"^[a\ \#]$"), Text("x"));
        CallEquals(Bool(false), "matches", Text("y "), Text("x[a ]|(?x:y[a ])"));
        CallEquals(Bool(true), "matches", Text("x "), Text("(?x:y[a ])|x[a ]"));
    }

    private static void RegexUnicodeScalarSets()
    {
        foreach (var value in new[] { "a", "Ω", "\uFFFF", "😀", "🙂", "🙏" })
        {
            CallEquals(Bool(true), "matches", Text(value), Text("^[a-🙏]$"));
        }
        CallEquals(Bool(false), "matches", Text("0"), Text("^[a-🙏]$"));
        CallEquals(Bool(false), "matches", Text("🙃"), Text("^[a-🙂]$"));
        CallEquals(Bool(true), "matches", Text("😀🙂🙏"), Text("^[😀-🙏]+$"));
        CallEquals(Bool(false), "matches", Text("🙂"), Text("^[^🙂]$"));
        CallEquals(Bool(true), "matches", Text("🙃a"), Text("^[^🙂]+$"));
        CallEquals(Bool(true), "matches", Text("🙂é"), Text("^[^a-b]+$"));
        CallEquals(Bool(true), "matches", Text("🙂"), Text(@"^\P{L}$"));
        CallEquals(Bool(false), "matches", Text("𐐨"), Text(@"^\P{L}$"));
        CallEquals(Bool(true), "matches", Text("𐐨"), Text(@"^\p{L}$"));
        CallEquals(Bool(true), "matches", Text("𐐨"), Text(@"^\w$"));
        foreach (var word in new[] { "Ⓐ", "🄰", "🅐", "🅰" })
        {
            CallEquals(Bool(true), "matches", Text(word), Text(@"^\w$"));
            CallEquals(Bool(false), "matches", Text(word), Text(@"^\W$"));
        }
        CallEquals(Bool(true), "matches", Text("🙂"), Text(@"^\W$"));
        CallEquals(Bool(true), "matches", Text("\U0001D7CE"), Text(@"^\d$"));
        CallEquals(Bool(false), "matches", Text("\U0001D7CE"), Text(@"^\D$"));
        CallEquals(Bool(true), "matches", Text("\u00A0"), Text(@"^\s$"));
        CallEquals(Bool(true), "matches", Text("Ω"), Text(@"^\p{IsGreek}$"));
        CallEquals(Bool(false), "matches", Text("🙂"), Text(@"^\p{Cs}$"));
        foreach (var pattern in new[] { @"^\P{L}+$", @"^[\P{L}]+$", @"^\W+$" })
        {
            CallEquals(Bool(true), "matches", Text("🙂\n"), Text(pattern));
            CallEquals(Bool(true), "matches", Text("🙂\n"), Text(pattern), Text("i"));
        }
        // Host class subtraction remains accepted without introducing captures.
        CallEquals(Bool(true), "matches", Text("b"), Text("^[a-z-[aeiou]]$"));
        CallEquals(Bool(false), "matches", Text("a"), Text("^[a-z-[aeiou]]$"));
        CallEquals(Bool(true), "matches", Text("ÿ"), Text(@"^[\777]$"));
        CallEquals(Bool(false), "matches", Text("🙂"), Text("^[😀-🙏-[🙂]]$"));
        CallEquals(Bool(true), "matches", Text("🙃"), Text("^[😀-🙏-[🙂]]$"));

        foreach (var pattern in new[] { "^𐐀$", "^[𐐀]$", "^[𐐀-𐐧]$", @"^\p{Lu}$" })
        {
            CallEquals(Bool(true), "matches", Text("𐐨"), Text(pattern), Text("i"));
        }
        foreach (var (left, right) in new[] { ("s", "ſ"), ("K", "K"), ("ΐ", "ΐ"), ("ΰ", "ΰ"), ("ﬅ", "ﬆ") })
        {
            CallEquals(Bool(true), "matches", Text(right), Text("^" + left + "$"), Text("i"));
            CallEquals(Bool(true), "matches", Text(left), Text("(?i)^" + right + "$"));
            CallEquals(Bool(true), "matches", Text(right), Text("(?i:[" + left + "])"));
            CallEquals(Bool(false), "matches", Text(right), Text("(?i)^[^" + left + "]$"));
        }
        foreach (var isolated in new[] { "ı", "İ" })
        {
            CallEquals(Bool(false), "matches", Text(isolated), Text("^i$"), Text("i"));
            CallEquals(Bool(false), "matches", Text("i"), Text("^" + isolated + "$"), Text("i"));
            CallEquals(Bool(true), "matches", Text(isolated), Text("(?i)^[^i]$"));
        }
        // Almost 64 KiB of irregular ranges exercises batched class parsing.
        var largeClass = "^[" + string.Concat(Enumerable.Range(0, 10_500).Select(index =>
            @"\u" + ((index * 7919) % 0xD700).ToString("X4", System.Globalization.CultureInfo.InvariantCulture))) + "]$";
        CallEquals(Bool(true), "matches", Text(char.ConvertFromUtf32(7919)), Text(largeClass), Text("i"));
        CallEquals(Bool(true), "matches", Text("I"), Text("^i$"), Text("i"));
        CallEquals(Bool(true), "matches", Text("ſ"), Text("(?i)^[a-z𐐀-𐐧]+$"));
        CallEquals(Bool(true), "matches", Text("𐐨"), Text("(?i:𐐀)"));
        CallEquals(Bool(true), "matches", Text("𐐨"), Text("(?i)[𐐀]"));
        CallEquals(Bool(false), "matches", Text("𐐨"), Text("(?i)^[^𐐀]$"));
        CallEquals(Bool(false), "matches", Text("𐐨"), Text(@"^\P{Lu}$"), Text("i"));
        CallEquals(Bool(false), "matches", Text("𐐨𐐨"), Text("(?i:𐐀)(?-i:𐐀)"));
        CallEquals(Bool(true), "matches", Text("𐐨𐐀"), Text("(?i:𐐀)(?-i:𐐀)"));
        foreach (var (upper, lower) in new[] { ("\uA7CE", "\uA7CF"), ("\U00016EA0", "\U00016EBB") })
        {
            CallEquals(Bool(false), "matches", Text(lower), Text("^" + upper + "$"), Text("i"));
            CallEquals(Bool(false), "matches", Text(upper), Text("^" + lower + "$"), Text("i"));
            CallEquals(Bool(false), "matches", Text(upper), Text(@"^\p{L}$"));
        }
    }

    private static void RegexUnicodeCaptureReplacement()
    {
        CallEquals(Text("x"), "replace", Text("🙂"), Text("."), Text("x"));
        CallEquals(Text("[🙂]"), "replace", Text("🙂"), Text("(.)"), Text("[$1]"));
        CallEquals(Text("[A][🙂][é]"), "replace", Text("A🙂é"), Text("(.)"), Text("[$1]"));
        CallEquals(Text("🙃🙂"), "replace", Text("🙂🙃"), Text("(.)(.)"), Text("$2$1"));
        CallEquals(Text("[🙃]"), "replace", Text("🙂🙃"), Text("(.)+"), Text("[$1]"));
        CallEquals(Text("[🙂]🙃"), "replace", Text("🙂🙃"), Text("(🙂|🙂🙃)"), Text("[$1]"));
        CallEquals(Text("[🙂🙃]"), "replace", Text("🙂🙃"), Text("(🙂🙃|🙂)"), Text("[$1]"));
        CallEquals(Text("!🙂🙃!"), "replace", Text("🙂🙃z"), Text("(.*?)z"), Text("!$1!"));
        CallEquals(Text("[🙂][🙃]"), "replace", Text("🙂🙃"), Text("(.+?)"), Text("[$1]"));
        CallEquals(Text("[🙂🙃]"), "replace", Text("🙂🙃"), Text("(.+)"), Text("[$1]"));
        CallEquals(Text("[𐐨]"), "replace", Text("𐐨"), Text("(?i:(𐐀))"), Text("[$1]"));
        CallEquals(Text("[𐐨]"), "replace", Text("𐐨"), Text(@"(\p{L})"), Text("[$1]"));
        CallEquals(Text("🙂"), "replace", Text("🙂"), Text(@"\p{Cs}"), Text("x"));
    }

    private static void RegexUnicodeTokenizationAndErrors()
    {
        void Tokens(string expected, string input, string pattern, string? flags = null) => Equal(
            expected, string.Join('|', FerruleSequences.TokenizeRegex(
                Text(input), Text(pattern), flags is null ? null : Text(flags)).Select(ValueText)));
        Tokens("a|b", "a🙂b", "[🙂]");
        Tokens("||", "🙂\n", @"\P{L}");
        Tokens("||", "🙂\n", @"\W");
        Tokens("a|b", "aſb", "s", "i");
        Tokens("|", "🙂", ".");
        Tokens("A|é|Z", "A🙂é🙃Z", "[🙂-🙃]");
        Tokens("|🙂|", "A🙂B", "[^🙂]");
        Tokens("a|b", "a𐐨b", "𐐀", "i");
        Tokens("a|b", "a𐐨b", "(?i:[𐐀])");
        Tokens("|🙂|", "𐐨🙂𐐀", @"\p{L}");
        Tokens("𐐨|𐐀", "𐐨🙂𐐀", @"\P{L}");
        Tokens("|🙂|", "𐐨🙂𐐀", @"\w");
        foreach (var pattern in new[] { "$", "(?m:$)", "a*", @"\b" })
        {
            Error(FerruleRuntimeError.ZeroWidthTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("🙂a"), Text(pattern), null));
        }
        AssertInvalidArgument("replace", "pattern matches a zero-length string", Text("🙂"), Text(".*?"), Text("x"));
        var nestedSubtraction = "[a" + string.Concat(Enumerable.Repeat("-[a", 258)) + new string(']', 259);
        AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(nestedSubtraction));
        Error(FerruleRuntimeError.InvalidTokenizeRegex,
            () => FerruleSequences.TokenizeRegex(Text("a"), Text(nestedSubtraction), null));
        foreach (var pattern in new[] { @"\uD800", @"[\uDC00]", "[🙃-🙂]", "[", "(?=🙂)" })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("🙂"), Text(pattern));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("🙂"), Text(pattern), null));
        }
    }
}
