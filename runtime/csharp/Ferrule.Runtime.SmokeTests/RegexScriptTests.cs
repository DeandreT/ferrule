using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexScriptMembershipAndDomains()
    {
        foreach (var profile in new[] { "(?u)", "(?U)", "(?R)" })
        {
            foreach (var (input, query, expected) in new[]
            {
                ("α", "Greek", true), ("Ϣ", "Greek", false), ("\u0378", "IsGreek", false),
                ("\U0001D200", "sc=Grek", true), ("\u0342", "sc=Greek", false),
                ("\u0342", "scx=Greek", true), ("·", "Script_Extensions:Grek", true),
                ("ー", "sc=Common", true), ("ー", "scx=Common", false),
                ("ー", "scx=Hira", true), ("ー", "scx=Kana", true),
                ("\u0342", "sc=Inherited", true), ("\u0342", "scx=Inherited", false),
                ("\U00016D40", "Kirat_Rai", true), ("\U00011380", "Tulu_Tigalari", true),
                ("\U0001E5D0", "Ol_Onal", true), ("\U00011BF0", "Sunuwar", true),
                ("$", "sc", true), ("\u200D", "Cf", true), ("A", "Lc", true),
                ("α", "Script!=Latin", true), ("·", " sCrIpT_eXtEnSiOnS =Is_G_r_e_e_k ", true),
            })
            {
                try { CallEquals(Bool(expected), "matches", Text(input), Text(profile + @"\A\p{" + query + @"}\z")); }
                catch (Exception error) { throw new InvalidOperationException($"script membership {profile} {query}", error); }
            }
        }
        CallEquals(Bool(true), "matches", Text("α"), Text(@"(?u)\P{Script_Extensions!=Greek}"));
        CallEquals(Bool(true), "matches", Text("·"), Text(@"(?xU)\p { Script_Extensions = Greek }"));
    }

    private static void RegexScriptClassesCapturesAndFolding()
    {
        CallEquals(Text("αA[·]"), "replace", Text("αA·"),
            Text(@"(?u)([\p{scx=Greek}--\p{sc=Greek}])"), Text("[$1]"));
        CallEquals(Text("[α]AΩ"), "replace", Text("αAΩ"),
            Text(@"(?u)([\p{Greek}&&\p{Ll}])"), Text("[$1]"));
        CallEquals(Text("[α][A]"), "replace", Text("αA"),
            Text(@"(?u)([\p{Greek}~~\p{Latin}])"), Text("[$1]"));
        CallEquals(Text("α[🙂][A]"), "replace", Text("α🙂A"),
            Text(@"(?u)([^\p{Greek}])"), Text("[$1]"));
        CallEquals(Text("x[αβ]y[Ω]Z"), "replace", Text("xαβyΩZ"),
            Text(@"(?u)(?<letters>\p{Greek}+)"), Text("[$1]"));
        Equal("x|y|Z", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("xαβyΩZ"), Text(@"(?u)(\p{Greek}+)"), null).Select(ValueText)));
        CallEquals(Text("[𝈀][α]"), "replace", Text("𝈀α"), Text(@"(?u)(\p{sc=Greek})"), Text("[$1]"));
        foreach (var value in new[] { "µ", "\u0345", "\U00010428" })
        {
            var query = value == "\U00010428" ? "Deseret" : "Greek";
            CallEquals(Bool(true), "matches", Text(value), Text(@"(?iu)\p{" + query + "}"));
            CallEquals(Bool(false), "matches", Text(value), Text(@"(?iu)\P{" + query + "}"));
            CallEquals(Bool(false), "matches", Text(value), Text(@"(?iu)\p{sc!=" + query + "}"));
        }
        CallEquals(Bool(false), "matches", Text("µ"), Text(@"(?u)\p{Greek}"));
        CallEquals(Text("[α][β]"), "replace", Text("αβ"), Text(@"(?U)(\p{Greek}+)"), Text("[$1]"));
        CallEquals(Text("[αβ]"), "replace", Text("αβ"), Text(@"(?U)(\p{Greek}+?)"), Text("[$1]"));
        CallEquals(Text("[α]\r\n[β]"), "replace", Text("α\r\nβ"), Text(@"(?mR)(^\p{Greek}+$)"), Text("[$1]"));
    }

    private static void RegexScriptProfileAndHostPreservation()
    {
        // These checks intentionally prime both domains in one process.
        foreach (var value in new[] { "Ϣ", "\u0378" })
        {
            CallEquals(Bool(true), "matches", Text(value), Text(@"\p{IsGreek}"));
            CallEquals(Bool(false), "matches", Text(value), Text(@"(?u)\p{IsGreek}"));
            CallEquals(Bool(true), "matches", Text(value), Text(@"\p{IsGreek}"));
        }
        CallEquals(Bool(false), "matches", Text("𝈀"), Text(@"\p{IsGreek}"));
        CallEquals(Bool(true), "matches", Text("𝈀"), Text(@"(?R)\p{IsGreek}"));
        CallEquals(Bool(false), "matches", Text("𝈀"), Text(@"\p{IsGreek}"));
        CallEquals(Bool(true), "matches", Text("A"), Text(@"\p{IsBasicLatin}"));
        CallEquals(Bool(true), "matches", Text("Ж"), Text(@"\p{IsCyrillic}"));
        // A non-strict boundary/nullable VM does not enable script queries.
        CallEquals(Bool(true), "matches", Text("Ϣ "), Text(@"\b\p{IsGreek}\b"));
        CallEquals(Text(string.Empty), "replace", Text("aaaab"),
            Text(@"^(\p{IsBasicLatin}?)+(b)$"), Text("$1"));
        foreach (var pattern in new[] { @"\p{sc=Greek}", @"\b\p{scx=Greek}\b" })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("α"), Text(pattern));
        }
        // Grammar selection is whole-pattern even when a mode is disabled locally.
        CallEquals(Bool(true), "matches", Text("α"), Text(@"(?R)(?-R:\p{Greek})"));
        CallEquals(Bool(true), "matches", Text("·"), Text(@"(?U)(?-U:\p{scx=Greek})"));
        CallEquals(Text("[/α][A/][/β]"), "replace", Text("αAβ"),
            Text(@"(?u)(?-u:(A))|(\p{Greek})"), Text("[$1/$2]"));
    }

    private static void RegexScriptBoundsAndErrors()
    {
        foreach (var query in new[] {
            "sc=Unknown", "sc=Zzzz", "sc=Hrkt", "sc=Katakana_Or_Hiragana",
            "scx=Unknown", "scx=Zzzz", "scx=Hrkt", "scx=Katakana_Or_Hiragana",
            "sc=NoSuchScript", "scx=", "gc=Greek", "Script==Greek", "Script=Greek=Latin",
            "IsC", "isc", "sc=IsC", "Age=16", "Block=Greek", "Alphabetic",
        })
        {
            var pattern = @"(?u)\p{" + query + "}";
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("α"), Text(pattern));
            AssertInvalidArgument("replace", "pattern is invalid or exceeds the compiled-size limit", Text("α"), Text(pattern), Text("x"));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("α"), Text(pattern), null));
        }
        foreach (var pattern in new[] { @"(?u)(?-u:\p{Greek})", @"(?U-u:\P{scx=Latin})", @"(?R-u:[\p{Greek}])" })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("α"), Text(pattern));
        }
        AssertInvalidArgument("matches", "pattern exceeds 64 KiB", Text("α"), Text(@"(?u)\p{Greek}" + new string('a', 65_536)));
        AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("α"), Text(@"(?u)\p{Greek}{1000000}"));
        CallEquals(Bool(true), "matches", Text(string.Empty), Text(@"(?u)\p{Greek}?"));
        AssertInvalidArgument("replace", "pattern matches a zero-length string", Text("α"), Text(@"(?u)\p{Greek}?"), Text("x"));
        Error(FerruleRuntimeError.ZeroWidthTokenizeRegex,
            () => FerruleSequences.TokenizeRegex(Text("α"), Text(@"(?u)\p{Greek}?"), null));
    }
}
