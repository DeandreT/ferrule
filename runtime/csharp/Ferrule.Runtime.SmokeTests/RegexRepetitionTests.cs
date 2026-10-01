using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexConsecutiveRepetitions()
    {
        foreach (var suffix in new[] { string.Empty, @"\b" })
        {
            foreach (var repeated in new[] { "a{2}{3}", "(?:a){2}{3}", "a{2,3}{2}", "a++", "a{2}?+", "a+?+" })
            {
                var pattern = repeated + suffix;
                CallEquals(Bool(true), "matches", Text(" aaaaaa "), Text(pattern));
                CallEquals(Text(" [aaaaaa] "), "replace", Text(" aaaaaa "), Text(pattern), Text("[$0]"));
                Equal(" | ", string.Join('|', FerruleSequences.TokenizeRegex(
                    Text(" aaaaaa "), Text(pattern), null).Select(ValueText)));
                CallEquals(Bool(false), "matches", Text("bbb"), Text(pattern));
            }
            CallEquals(Bool(true), "matches", Text("aaaaaa"), Text("^a{2}{3}" + suffix + "$"));
            CallEquals(Bool(false), "matches", Text("aaaaa"), Text("^a{2}{3}" + suffix + "$"));
            CallEquals(Bool(false), "matches", Text("aaaaaaa"), Text("^a{2}{3}" + suffix + "$"));
        }
        CallEquals(Text("[🙂🙂🙂🙂:🙂]"), "replace", Text("🙂🙂🙂🙂"),
            Text(@"^(🙂){2}{2}\B$"), Text("[$0:$1]"));
        CallEquals(Bool(true), "matches", Text("🙂🙂🙂🙂"), Text("^🙂{2}{2}$"));
        CallEquals(Bool(false), "matches", Text("🙂🙂🙂"), Text("^🙂{2}{2}$"));
        CallEquals(Bool(true), "matches", Text("abcd"), Text(@"^\p{gc=letter}{2}{2}$"));
        CallEquals(Bool(true), "matches", Text("abcd"), Text(@"^\p{gc=letter}{2}{2}\b$"));
        CallEquals(Bool(true), "matches", Text("a}"), Text("^a{1}}$"));
        CallEquals(Bool(true), "matches", Text("{{"), Text(@"^\{{1}{2}$"));
    }

    private static void RegexRepeatedCaptureOrderAndLazySuffixes()
    {
        foreach (var suffix in new[] { string.Empty, @"\b" })
        {
            foreach (var header in new[] { "?<", "?P<" })
            {
                CallEquals(Text("aaaaaab|a|b|"), "replace", Text("aaaaaab"),
                    Text("^(" + header + "first>a){2}{3}(b)" + suffix + "$"), Text("$0|$1|$2|$3"));
            }
            CallEquals(Text("aaaaaab|aaaaaa|aa|b"), "replace", Text("aaaaaab"),
                Text("^((a|aa){2}{2})(b)" + suffix + "$"), Text("$0|$1|$2|$3"));
            CallEquals(Text("aaaaaab|a|b"), "replace", Text("aaaaaab"),
                Text("^(a+?)+?(b)" + suffix + "$"), Text("$0|$1|$2"));
            CallEquals(Text("aaaaaab|a|b"), "replace", Text("aaaaaab"),
                Text("^(a{1,2}?)+(b)" + suffix + "$"), Text("$0|$1|$2"));
            CallEquals(Text("aa|a"), "replace", Text("aa"), Text("^((a){1}{2})" + suffix + "$"), Text("$1|$2"));
        }
        // The scalar boundary machine retains the last selected nonempty
        // capture when a lazy optional body is repeated again.
        CallEquals(Text("aaaaaab|a|b"), "replace", Text("aaaaaab"),
            Text(@"^(a{1,2}??)+(b)\b$"), Text("$0|$1|$2"));
        CallEquals(Text("[aaaaaab:a:b]"), "replace", Text("aaaaaab"),
            Text("^(a+ # inner lazy\n ?)+ # outer lazy\n ?(b)\\b$"), Text("[$0:$1:$2]"), Text("x"));
    }

    private static void RegexRepeatedFlagsAndZeroWidthRules()
    {
        foreach (var suffix in new[] { string.Empty, @"\b" })
        {
            CallEquals(Bool(true), "matches", Text("AAAAAA"), Text("^(?i:a){2}{3}" + suffix + "$"));
            CallEquals(Bool(true), "matches", Text("AAAAAA"), Text("^(?i)a{2}{3}" + suffix + "$"));
            CallEquals(Bool(true), "matches", Text("aaaaaa"),
                Text("^a {2} # next repeat\n {3}" + suffix + "$"), Text("x"));
            CallEquals(Bool(true), "matches", Text("aaaaaa"),
                Text("^a {2} \u2003 # lazy\n ? {3}" + suffix + "$"), Text("x"));
            CallEquals(Bool(true), "matches", Text("𐐀𐐨𐐀𐐨"),
                Text(@"^(?i:\p{general_category=lowercaseletter}){2}{2}" + suffix + "$"));
            CallEquals(Bool(true), "matches", Text("aaaaaa"), Text("^a{2}(?:){1}a{2}{2}" + suffix + "$"));
        }
        foreach (var repeated in new[] { "a**", "a*+", "a+*", "a{2}??", "a{1,2}??", "a???", "a{1}{0}" })
        {
            CallEquals(Bool(true), "matches", Text(string.Empty), Text(repeated));
            AssertInvalidArgument("replace", "pattern matches a zero-length string", Text("ba"), Text(repeated), Text("x"));
            Error(FerruleRuntimeError.ZeroWidthTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("ba"), Text(repeated), null));
            var boundary = Text(repeated + @"\b");
            CallEquals(Bool(true), "matches", Text("ba"), boundary);
            AssertInvalidArgument("replace", "pattern produced a zero-length match", Text("ba"), boundary, Text("x"));
            Error(FerruleRuntimeError.ZeroWidthTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("ba"), boundary, null));
        }
    }

    private static void RegexRepetitionBoundsAndOperandErrors()
    {
        foreach (var suffix in new[] { string.Empty, @"\b" })
        {
            foreach (var invalid in new[] { "*", "+", "?", "{2}", "a|+", "a|{2}", "a(?i)*", "a(?i)+",
                "a(?i)?", "a(?i){2}", "a{2}(?i)?", "a{2}(?i){2}", "a(?i:{2})", "a{2}{word}", "a{2}{3,1}" })
            {
                var pattern = Text(invalid + suffix);
                AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("aaaaaa"), pattern);
                AssertInvalidArgument("replace", "pattern is invalid or exceeds the compiled-size limit", Text("aaaaaa"), pattern, Text("x"));
                Error(FerruleRuntimeError.InvalidTokenizeRegex,
                    () => FerruleSequences.TokenizeRegex(Text("aaaaaa"), pattern, null));
            }
        }
        var plainLimit = "^a" + string.Concat(Enumerable.Repeat("{1}", 255)) + "$";
        CallEquals(Bool(true), "matches", Text("a"), Text(plainLimit));
        var structuralLimit = "^a" + string.Concat(Enumerable.Repeat("{1}", 255)) + @"\b$";
        CallEquals(Bool(true), "matches", Text("a"), Text(structuralLimit));
        var capturedLimit = "^(a)" + string.Concat(Enumerable.Repeat("{1}", 254)) + @"\b$";
        CallEquals(Text("[a]"), "replace", Text("a"), Text(capturedLimit), Text("[$1]"));
        foreach (var invalid in new[] {
            "^a" + string.Concat(Enumerable.Repeat("{1}", 256)) + "$",
            "^a" + string.Concat(Enumerable.Repeat("{1}", 256)) + @"\b$",
            "^(a)" + string.Concat(Enumerable.Repeat("{1}", 255)) + @"\b$",
            "\\b" + string.Concat(Enumerable.Repeat("(", 130)) + "a" + string.Concat(Enumerable.Repeat(")?", 130)),
            @"\ba{163840}",
        })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(invalid));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("a"), Text(invalid), null));
        }
        // New repetition limits do not alter ordinary host group depth.
        CallEquals(Bool(true), "matches", Text("a"), Text(new string('(', 300) + "a" + new string(')', 300)));
        // A zero outer count discards its body before expansion accounting.
        CallEquals(Bool(true), "matches", Text("a"), Text("a{2147483647}{0}"));
        CallEquals(Bool(true), "matches", Text("a"), Text(@"a{4294967295}{0}\b"));
        CallEquals(Bool(true), "matches", Text("a"), Text(@"(a{4294967295}{0}){1}\b"));
        Error(FerruleRuntimeError.ZeroWidthTokenizeRegex,
            () => FerruleSequences.TokenizeRegex(Text("a"), Text(@"a{4294967295}{0}\b"), null));
        // A failed parse or budget attempt cannot poison a later call.
        CallEquals(Text("[aa]"), "replace", Text("aa"), Text(@"(a){1}{2}\b"), Text("[$0]"));
    }
}
