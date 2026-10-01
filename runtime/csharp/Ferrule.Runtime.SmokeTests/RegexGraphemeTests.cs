using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexGraphemeMembershipAndAliases()
    {
        foreach (var profile in new[] { "(?u)", "(?U)", "(?R)" })
        {
            foreach (var (input, query, expected) in new[]
            {
                ("\u0345", "GCB=Extend", true), ("\u0345", "Grapheme_Cluster_Break:ex", true),
                ("\u0903", "gcb=SpacingMark", true), ("\u0903", "gcb=Extend", false),
                ("\u200D", "gcb=ZWJ", true), ("\u200D", "gcb=Control", false),
                ("\r", "gcb=CR", true), ("\r", "gcb=Control", false), ("\r", "Control", true),
                ("\n", "gcb=LF", true), ("\0", "gcb=Control", true), ("\u0600", "gcb=Prepend", true),
                ("ᄀ", "gcb=L", true), ("ᅡ", "gcb=V", true), ("ᆨ", "gcb=T", true),
                ("가", "gcb=LV", true), ("각", "gcb=LVT", true), ("🇺", "gcb=RI", true),
                ("🙂", "gcb=RI", false), ("A", "gcb!=Extend", true),
                ("\u0345", "Is_Grapheme_Cluster_Break=Is_Ex_tend", true),
            })
            {
                CallEquals(Bool(expected), "matches", Text(input), Text(profile + @"\A\p{" + query + @"}\z"));
            }
        }
        CallEquals(Bool(true), "matches", Text("\u0903"),
            Text("(?ux)\\p{Gra#comment\npheme_Cluster_Break : Spacing Mark}"));
    }

    private static void RegexGraphemeClassesCapturesAndFolding()
    {
        CallEquals(Text("[ͅ]"), "replace", Text("ͅ"), Text(@"(?u)([\p{GCB=Extend}&&\p{Alphabetic}])"), Text("[$1]"));
        CallEquals(Text("[̀]"), "replace", Text("̀"), Text(@"(?u)([\p{GCB=Extend}--\p{Alphabetic}])"), Text("[$1]"));
        CallEquals(Text("[ः]"), "replace", Text("ः"), Text(@"(?u)([\p{M}~~\p{GCB=Extend}])"), Text("[$1]"));
        foreach (var input in new[] { "ͅ", "Ι", "ι" })
        {
            CallEquals(Bool(true), "matches", Text(input), Text(@"(?iu)\p{GCB=Extend}"));
            CallEquals(Bool(false), "matches", Text(input), Text(@"(?iu)\P{GCB=Extend}"));
            CallEquals(Bool(false), "matches", Text(input), Text(@"(?iu)\p{GCB!=Extend}"));
            CallEquals(Bool(true), "matches", Text(input), Text(@"(?iu)\P{GCB!=Extend}"));
        }
        CallEquals(Text("a[̀]b[ͅ]!"), "replace", Text("àbͅ!"), Text(@"(?u)(?<marks>\p{GCB=Extend}+)"), Text("[$1]"));
        Equal("a|b|!", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("àbͅ!"), Text(@"(?u)\p{GCB=Extend}+"), null).Select(ValueText)));
        CallEquals(Text("a[\r\n]b"), "replace", Text("a\r\nb"), Text(@"(?R)([\p{GCB=CR}\p{GCB=LF}]+)"), Text("[$1]"));
        Equal("a|b", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("a\r\nb"), Text(@"(?R)[\p{GCB=CR}\p{GCB=LF}]+"), null).Select(ValueText)));
        CallEquals(Text("[̀][ͅ]"), "replace", Text("̀ͅ"), Text(@"(?U)(\p{GCB=Extend}+)"), Text("[$1]"));
        CallEquals(Text("[̀ͅ]"), "replace", Text("̀ͅ"), Text(@"(?U)(\p{GCB=Extend}+?)"), Text("[$1]"));
    }

    private static void RegexGraphemeProfileAndErrors()
    {
        CallEquals(Text("[A/][/ͅ]"), "replace", Text("Aͅ"), Text(@"(?u)(?-u:(A))|(\p{GCB=Extend})"), Text("[$1/$2]"));
        CallEquals(Bool(true), "matches", Text("ͅ"), Text(@"(?R)(?-R:\p{GCB=Extend})"));
        // Bare categories and ordinary host properties retain separate meaning/cache.
        CallEquals(Bool(true), "matches", Text("\r"), Text(@"\p{Control}"));
        CallEquals(Bool(false), "matches", Text("\r"), Text(@"(?u)\p{GCB=Control}"));
        CallEquals(Bool(true), "matches", Text("\r"), Text(@"\p{Control}"));
        foreach (var value in new[] { "eb", "E_Base", "ebasegaz", "ebg", "em", "E_Modifier", "gaz", "Glue_After_Zwj", "other", "xx" })
        {
            var pattern = @"(?u)\p{GCB=" + value + "}";
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("A"), Text(pattern));
            AssertInvalidArgument("replace", "pattern is invalid or exceeds the compiled-size limit", Text("A"), Text(pattern), Text("x"));
            Error(FerruleRuntimeError.InvalidTokenizeRegex, () => FerruleSequences.TokenizeRegex(Text("A"), Text(pattern), null));
        }
        foreach (var pattern in new[] {
            @"\p{GCB=Extend}", @"\p{Grapheme_Cluster_Break=Extend}", @"(?u)\p{Extend}", @"(?u)\p{GCB}",
            @"(?u)\p{GCB=Alphabetic}", @"(?u)\p{gc=Extend}", @"(?u)\p{sc=Extend}",
            @"(?u)\p{WB=Extend}", @"(?u)\p{SB=Extend}", @"(?u)\p{Age=16.0}",
            @"(?u-u:\p{GCB=Extend})", @"(?U-u:[\P{GCB=Control}])", @"(?R-u:\p{GCB=CR})",
        })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("A"), Text(pattern));
        }
    }

    private static void RegexGraphemeBoundedProperties()
    {
        var aliases = string.Concat(Enumerable.Range(0, 1_500).Select(index => index % 2 == 0 ? @"\p{gcb=ex}" : @"\p{Grapheme_Cluster_Break=Extend}"));
        CallEquals(Bool(true), "matches", Text("ͅ"), Text("(?u)[" + aliases + "]"));
        static string Intersections(int leaves, string item)
        {
            if (leaves == 1) { return item; }
            var half = leaves / 2;
            return "[" + Intersections(half, item) + "&&" + Intersections(leaves - half, item) + "]";
        }
        CallEquals(Bool(true), "matches", Text("가"), Text("(?u)" + Intersections(512, @"\p{gcb=LV}")));
        // New domain participates in existing shared class budgets; the enclosed
        // large binary intersection exceeds that budget without a source overflow.
        var expensive = @"(?u)[\p{gcb=LV}&&" + Intersections(4_096, @"\p{GrBase}") + "]";
        AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("가"), Text(expensive));
        Error(FerruleRuntimeError.InvalidTokenizeRegex, () => FerruleSequences.TokenizeRegex(Text("가"), Text(expensive), null));
        AssertInvalidArgument("matches", "pattern exceeds 64 KiB", Text("ͅ"), Text(@"(?u)\p{GCB=Extend}" + new string('a', 65_536)));
        AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("ͅ"), Text(@"(?u)\p{GCB=Extend}{1000000}"));
        Error(FerruleRuntimeError.ZeroWidthTokenizeRegex, () => FerruleSequences.TokenizeRegex(Text("ͅ"), Text(@"(?u)\p{GCB=Extend}?"), null));
    }
}
