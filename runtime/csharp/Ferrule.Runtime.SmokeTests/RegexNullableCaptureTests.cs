using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexNullableUnboundedCaptures()
    {
        foreach (var (pattern, expected) in new[]
        {
            ("^(a?)+(b)$", "[aaaaaab:a:b:]"),
            ("^(a*)+(b)$", "[aaaaaab:aaaaaa:b:]"),
            ("^(a?)*(b)$", "[aaaaaab:a:b:]"),
            ("^(a*)*(b)$", "[aaaaaab:aaaaaa:b:]"),
            ("^((?:a{1,2}?)?)+(b)$", "[aaaaaab:a:b:]"),
            ("^((a)?)+(b)$", "[aaaaaab:a:a:b]"),
            ("^(?:(a)?)+(b)$", "[aaaaaab:a:b:]"),
            ("^((?:a|)?)+(b)$", "[aaaaaab:a:b:]"),
            ("^(?<keep>a?)+(b)$", "[aaaaaab:a:b:]"),
            ("^(?P<keep>a?)+(b)$", "[aaaaaab:a:b:]"),
        })
        {
            CallEquals(Bool(true), "matches", Text("aaaaaab"), Text(pattern));
            CallEquals(Text(expected), "replace", Text("aaaaaab"), Text(pattern), Text("[$0:$1:$2:$3]"));
            Equal("|", string.Join('|', FerruleSequences.TokenizeRegex(
                Text("aaaaaab"), Text(pattern), null).Select(ValueText)));
        }
        CallEquals(Text("[🙂🙂🙂b:🙂:b]"), "replace", Text("🙂🙂🙂b"),
            Text("^(🙂?)+(b)$"), Text("[$0:$1:$2]"));
        CallEquals(Text("[aaaaaab:a:b]"), "replace", Text("aaaaaab"),
            Text(" ^ ( a ? ) + ( b ) $ # final"), Text("[$0:$1:$2]"), Text("x"));
        CallEquals(Text("[aaaaaab:aaaaaa:b]"), "replace", Text("aaaaaab"),
            Text(@"^(\p{gc=Letter}*)+(b)$"), Text("[$0:$1:$2]"));
        CallEquals(Text("[aac:a:c]"), "replace", Text("aac"),
            Text("^(a|b?)+(c)$"), Text("[$0:$1:$2]"));
    }

    private static void RegexNullableCaptureControlsAndIteration()
    {
        foreach (var (pattern, expected) in new[]
        {
            ("^(a?)+?(b)$", "[aaaaaab:a:b]"),
            ("^(a*){1,2}(b)$", "[aaaaaab::b]"),
            ("^(a+)+(b)$", "[aaaaaab:aaaaaa:b]"),
            ("^(?:a?)+(b)$", "[aaaaaab:b:]"),
            ("^((a?)+){0}(aaaaaab)$", "[aaaaaab::]"),
        })
        {
            CallEquals(Text(expected), "replace", Text("aaaaaab"), Text(pattern), Text("[$0:$1:$2]"));
        }
        CallEquals(Text("x[aaaaaab:a:b]y[ab:a:b]z"), "replace", Text("xaaaaaabyabz"),
            Text("(a?)+(b)"), Text("[$0:$1:$2]"));
        Equal("x|y|z", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("xaaaaaabyabz"), Text("(a?)+(b)"), null).Select(ValueText)));
        CallEquals(Bool(false), "matches", Text("cccc"), Text("^(a?)+(b)$"));
        CallEquals(Text("cccc"), "replace", Text("cccc"), Text("^(a?)+(b)$"), Text("[$1]"));
        Equal("cccc", string.Join('|', FerruleSequences.TokenizeRegex(
            Text("cccc"), Text("^(a?)+(b)$"), null).Select(ValueText)));
        CallEquals(Bool(true), "matches", Text(string.Empty), Text("^(a?)+$"));
        AssertInvalidArgument("replace", "pattern matches a zero-length string",
            Text("aaa"), Text("^(a?)+$"), Text("[$1]"));
        Error(FerruleRuntimeError.ZeroWidthTokenizeRegex,
            () => FerruleSequences.TokenizeRegex(Text("aaa"), Text("^(a?)+$"), null));
    }

    private static void RegexNullableCaptureHostFallbacks()
    {
        // These existing ordinary host extensions stay on the host path. Their
        // nullable-loop captures are deliberately outside this portable slice.
        foreach (var (input, pattern) in new[]
        {
            ("aaaab", "^(a?(?#host comment))+(b)$"),
            ("aaaab", @"(a?)+(b)\Z"),
            ("aaaab", "^(?'keep'a?)+(b)$"),
            ("aaaab", @"^(\p{IsBasicLatin}?)+(b)$"),
            ("aaaab", "^([a-z-[c]]?)+(b)$"),
            ("<<b", @"^([\<]?)+(b)$"),
            (">>b", @"^([\>]?)+(b)$"),
            ("\u0001\u0001b", @"^([\cA]?)+(b)$"),
            ("\0\0b", @"^([\0]?)+(b)$"),
            ("🙂🙂b", @"^(\uD83D\uDE42?)+(b)$"),
        })
        {
            CallEquals(Bool(true), "matches", Text(input), Text(pattern));
            CallEquals(Text(string.Empty), "replace", Text(input), Text(pattern), Text("$1"));
        }
        CallEquals(Text(string.Empty), "replace", Text("aaaab"),
            Text("(?n)^(?<keep>a?)+b$"), Text("$1"));
        // A failed speculative parse cannot tighten the existing deep ordinary
        // host profile. Group 301 is the loop body and keeps its old empty tag.
        var deep = "^" + new string('(', 300) + "(a?)+" + new string(')', 300) + "b$";
        CallEquals(Bool(true), "matches", Text("aaaab"), Text(deep));
        CallEquals(Text(string.Empty), "replace", Text("aaaab"), Text(deep), Text("$301"));
        foreach (var invalid in new[] { "^(a?)+(", "^(a?)+b{2,1}$", "^(a?)+*{word}b$" })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit",
                Text("aaaab"), Text(invalid));
        }
    }

    private static void RegexNullableCaptureVmCapacityFailures()
    {
        // Once eligible capture routing is established, VM capacity failures
        // cannot fall back to a host with a different resource profile.
        foreach (var pattern in new[]
        {
            "^(a?)+a{200000}b$",
            "^(a?)+" + string.Concat(Enumerable.Repeat("(a)", 1800)) + "b$",
        })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit",
                Text("aaaab"), Text(pattern));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("aaaab"), Text(pattern), null));
        }
    }
}
