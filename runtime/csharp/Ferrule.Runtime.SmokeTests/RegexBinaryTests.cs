using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexBinaryMembershipAndCategories()
    {
        foreach (var profile in new[] { "(?u)", "(?U)", "(?R)" })
        {
            foreach (var (input, query, expected) in new[]
            {
                ("\u0345", "Alphabetic", true), ("\u0345", "L", false),
                ("\u0345", "Lowercase", true), ("\u0345", "Ll", false),
                ("Ⅻ", "Uppercase", true), ("Ⅻ", "Lu", false), ("ⅻ", "lower", true),
                ("🙂", "Emoji", true), ("🙂", "Extended_Pictographic", true),
                ("0", "Emoji", true), ("0", "Emoji_Presentation", false),
                ("\uFE0F", "Variation_Selector", true), ("\uFE0F", "Emoji_Component", true),
                ("\u200D", "Join_Control", true), ("\u200D", "Default_Ignorable_Code_Point", true),
                ("\u200D", "cf", true), ("$", "sc", true), ("A", "lc", true),
                ("\u00A0", "White_Space", true), ("\u00A0", "Pattern_White_Space", false),
                ("\u0085", "space", true), ("\U0001F1FA", "Regional_Indicator", true),
                ("\uFDD0", "Noncharacter_Code_Point", true), ("=", "Pattern_Syntax", true),
                ("Ａ", "Hex_Digit", true), ("Ａ", "ASCII_Hex_Digit", false),
                ("+", "Math", true), ("α", "XID_Start", true), ("9", "XID_Start", false),
                ("9", "XID_Continue", true), ("\u0345", "Is_Alpha_betic", true),
                ("٠", "Decimal_Number", true),
            })
            {
                try { CallEquals(Bool(expected), "matches", Text(input), Text(profile + @"\A\p{" + query + @"}\z")); }
                catch (Exception error) { throw new InvalidOperationException($"binary membership {profile} {query}", error); }
            }
        }
        foreach (var value in new[] { "\t", "\n", "\r", "\v", "\f", "\u0085", "\u00A0", "\u1680", "\u2000", "\u200A", "\u2028", "\u2029", "\u202F", "\u205F", "\u3000" })
        {
            CallEquals(Bool(true), "matches", Text(value), Text(@"(?u)\p{wspace}"));
            CallEquals(Bool(true), "matches", Text(value), Text(@"(?u)\s"));
        }
        CallEquals(Bool(false), "matches", Text("\u200B"), Text(@"(?u)\p{WhiteSpace}"));
        CallEquals(Bool(true), "matches", Text("ͅ"), Text("(?ux)\\p{Alpha#comment\nbetic}"));
    }

    private static void RegexBinaryClassesCapturesAndFolding()
    {
        CallEquals(Text("[ͅ]"), "replace", Text("ͅ"), Text(@"(?u)([\p{Alphabetic}--\p{L}])"), Text("[$1]"));
        CallEquals(Text("aA[ͅ]"), "replace", Text("aAͅ"), Text(@"(?u)([\p{lower}~~\p{Ll}])"), Text("[$1]"));
        CallEquals(Text("[α]"), "replace", Text("α"), Text(@"(?u)([\p{Alphabetic}&&\p{Greek}])"), Text("[$1]"));
        CallEquals(Bool(false), "matches", Text("·"), Text(@"(?u)[\p{Alphabetic}&&\p{scx=Greek}]"));
        CallEquals(Text("🙂[A]"), "replace", Text("🙂A"), Text(@"(?u)([^\p{Emoji}])"), Text("[$1]"));
        CallEquals(Text("1[αβ]🙂[Z]!"), "replace", Text("1αβ🙂Z!"), Text(@"(?u)(?<letters>\p{Alphabetic}+)"), Text("[$1]"));
        Equal("1|🙂|!", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("1αβ🙂Z!"), Text(@"(?u)\p{Alphabetic}+"), null).Select(ValueText)));
        foreach (var (value, query) in new[] { ("𐐨", "Uppercase"), ("A", "Lowercase"), ("ͅ", "Alphabetic"), ("µ", "Uppercase") })
        {
            CallEquals(Bool(true), "matches", Text(value), Text(@"(?iu)\p{" + query + "}"));
            CallEquals(Bool(false), "matches", Text(value), Text(@"(?iu)\P{" + query + "}"));
        }
        CallEquals(Text("a[\r\n]b"), "replace", Text("a\r\nb"), Text(@"(?R)(\p{WhiteSpace}+)"), Text("[$1]"));
        Equal("a|b", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("a\r\nb"), Text(@"(?R)\p{WhiteSpace}+"), null).Select(ValueText)));
        CallEquals(Text("[a][b]"), "replace", Text("ab"), Text(@"(?U)(\p{Alphabetic}+)"), Text("[$1]"));
        CallEquals(Text("[ab]"), "replace", Text("ab"), Text(@"(?U)(\p{Alphabetic}+?)"), Text("[$1]"));
    }

    private static void RegexBinaryProfileAndErrors()
    {
        // Strict cached binary sets never widen the ordinary host vocabulary.
        foreach (var query in new[] { "Alphabetic", "lower", "upper", "Emoji", "WhiteSpace" })
        {
            CallEquals(Bool(true), "matches", Text(query == "Emoji" ? "🙂" : query == "WhiteSpace" ? " " : "A"), Text(@"(?iu)\p{" + query + "}"));
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("A"), Text(@"\p{" + query + "}"));
        }
        CallEquals(Bool(true), "matches", Text("Ϣ"), Text(@"\p{IsGreek}"));
        CallEquals(Bool(false), "matches", Text("Ϣ"), Text(@"(?u)\p{IsGreek}"));
        CallEquals(Bool(true), "matches", Text("Ϣ"), Text(@"\p{IsGreek}"));
        CallEquals(Text("[A/][/🙂]"), "replace", Text("A🙂"), Text(@"(?u)(?-u:(A))|(\p{Emoji})"), Text("[$1/$2]"));
        CallEquals(Bool(true), "matches", Text("🙂"), Text(@"(?R)(?-R:\p{Emoji})"));
        foreach (var query in new[] {
            "InCB", "incb", "Indic_Conjunct_Break", "InCB=Linker", "InCB!=None", "incb:Consonant",
            "Alphabetic=Yes", "Alphabetic:True", "Alpha!=No", "WhiteSpace=Yes", "Emoji=False",
            "DecimalNumber=Yes", "Lowercase=Y", "gc=Alphabetic", "sc=Alphabetic", "Age=16", "Block=Greek",
        })
        {
            var pattern = @"(?u)\p{" + query + "}";
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("A"), Text(pattern));
            AssertInvalidArgument("replace", "pattern is invalid or exceeds the compiled-size limit", Text("A"), Text(pattern), Text("x"));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("A"), Text(pattern), null));
        }
        foreach (var pattern in new[] { @"(?u-u:\p{Emoji})", @"(?U-u:[\P{Alpha}])", @"(?R-u:\p{Lowercase})" })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("A"), Text(pattern));
        }
    }

    private static void RegexBinaryBoundedClassWork()
    {
        // Canonical aliases share one set; duplicate unions stay bounded and valid.
        var aliases = string.Concat(Enumerable.Range(0, 1_500).Select(index => index % 2 == 0 ? @"\p{GrBase}" : @"\p{Grapheme_Base}"));
        CallEquals(Bool(true), "matches", Text("A"), Text("(?u)[" + aliases + "]"));
        static string Intersections(int leaves)
        {
            if (leaves == 1) { return @"\p{GrBase}"; }
            var half = leaves / 2;
            return "[" + Intersections(half) + "&&" + Intersections(leaves - half) + "]";
        }
        CallEquals(Bool(true), "matches", Text("A"), Text("(?u)" + Intersections(512)));
        // Actual interval operations, not repeated spelling, exceed the shared class budget.
        var expensive = "(?u)" + Intersections(4_096);
        AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("A"), Text(expensive));
        AssertInvalidArgument("replace", "pattern is invalid or exceeds the compiled-size limit", Text("A"), Text(expensive), Text("x"));
        Error(FerruleRuntimeError.InvalidTokenizeRegex,
            () => FerruleSequences.TokenizeRegex(Text("A"), Text(expensive), null));
        AssertInvalidArgument("matches", "pattern exceeds 64 KiB", Text("A"), Text(@"(?u)\p{Emoji}" + new string('a', 65_536)));
        AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("A"), Text(@"(?u)\p{Alphabetic}{1000000}"));
        Error(FerruleRuntimeError.ZeroWidthTokenizeRegex,
            () => FerruleSequences.TokenizeRegex(Text("A"), Text(@"(?u)\p{Alphabetic}?"), null));
    }
}
