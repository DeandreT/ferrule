using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexClassSetOperations()
    {
        foreach (var input in new[] { "a", "d", "&" })
        {
            CallEquals(Bool(false), "matches", Text(input), Text("^[a-c&&b-d]$"));
        }
        foreach (var input in new[] { "b", "c" })
        {
            CallEquals(Bool(true), "matches", Text(input), Text("^[a-c&&b-d]$"));
        }
        CallEquals(Text("a[b][c]d&~🙂🙃🙏"), "replace", Text("abcd&~🙂🙃🙏"), Text("([a-c&&b-d])"), Text("[$1]"));
        Equal("a||d", string.Join('|', FerruleSequences.TokenizeRegex(Text("abcd"), Text("[a-c&&b-d]"), null).Select(ValueText)));
        CallEquals(Text("[a]b[c]d&~🙂🙃🙏"), "replace", Text("abcd&~🙂🙃🙏"), Text("([a-c--b])"), Text("[$1]"));
        CallEquals(Text("[a]b[c]d&~🙂🙃🙏"), "replace", Text("abcd&~🙂🙃🙏"), Text("([ab~~bc])"), Text("[$1]"));
        CallEquals(Text("a[b][c][d]&~🙂🙃🙏"), "replace", Text("abcd&~🙂🙃🙏"), Text("([a-z&&[^aeiou]])"), Text("[$1]"));
        CallEquals(Bool(true), "matches", Text("acz"), Text("^[[a-c][x-z]]+$"));
        foreach (var input in new[] { "b", "c", "d", "e" })
        {
            CallEquals(Bool(true), "matches", Text(input), Text("^[a-c&&b-d~~d-e]$"));
        }
        CallEquals(Bool(false), "matches", Text("a"), Text("^[a-c&&b-d~~d-e]$"));
        CallEquals(Bool(true), "matches", Text("c"), Text("^[ab~~bc&&cd]$"));
        CallEquals(Bool(false), "matches", Text("a"), Text("^[ab~~bc&&cd]$"));
        CallEquals(Bool(false), "matches", Text("b"), Text("^[ab~~bc&&cd]$"));
        foreach (var pattern in new[] { "[a&&]", "[&&a]" })
        {
            CallEquals(Bool(false), "matches", Text("a"), Text(pattern));
            CallEquals(Text("a"), "replace", Text("a"), Text(pattern), Text("x"));
            Equal("a", string.Join('|', FerruleSequences.TokenizeRegex(Text("a"), Text(pattern), null).Select(ValueText)));
        }
        CallEquals(Bool(true), "matches", Text("a"), Text("[a~~]"));
        CallEquals(Bool(true), "matches", Text("a"), Text("[a--]"));
        CallEquals(Bool(true), "matches", Text("-"), Text("^[--a]$"));
        CallEquals(Bool(false), "matches", Text("b"), Text("^[--a]$"));
        foreach (var (input, pattern) in new[] { ("&", @"^[a\&\&b]$"), ("-", @"^[a\-\-b]$"), ("~", @"^[a\~\~b]$"), ("[", @"^[\[]$") })
        {
            CallEquals(Bool(true), "matches", Text(input), Text(pattern));
        }
        // Single-dash host subtraction remains an explicit accepted extension.
        CallEquals(Bool(true), "matches", Text("b"), Text("^[a-z-[aeiou]]$"));
        CallEquals(Bool(false), "matches", Text("a"), Text("^[a-z-[aeiou]]$"));
    }

    private static void RegexClassSetUnicodeAndFlags()
    {
        CallEquals(Bool(true), "matches", Text("🙃"), Text("^[🙂🙃&&🙃🙏]$"));
        CallEquals(Bool(false), "matches", Text("🙂"), Text("^[🙂🙃&&🙃🙏]$"));
        CallEquals(Bool(false), "matches", Text("🙂"), Text("^[🙂🙃--🙂]$"));
        CallEquals(Bool(true), "matches", Text("🙃"), Text("^[🙂🙃--🙂]$"));
        CallEquals(Text("a[🙂]🙃[🙏]b"), "replace", Text("a🙂🙃🙏b"), Text("([🙂🙃~~🙃🙏])"), Text("[$1]"));
        Equal("a|🙃|b", string.Join('|', FerruleSequences.TokenizeRegex(Text("a🙂🙃🙏b"), Text("[🙂🙃~~🙃🙏]"), null).Select(ValueText)));
        foreach (var input in new[] { "a", "d", "🙂" })
        {
            CallEquals(Bool(true), "matches", Text(input), Text("^[^a-c&&b-d]$"));
        }
        CallEquals(Bool(false), "matches", Text("b"), Text("^[^a-c&&b-d]$"));
        foreach (var input in new[] { "b", "B", "c", "C" })
        {
            CallEquals(Bool(true), "matches", Text(input), Text("^[a-c&&B-D]$"), Text("i"));
        }
        CallEquals(Bool(false), "matches", Text("A"), Text("^[a-c&&B-D]$"), Text("i"));
        CallEquals(Bool(true), "matches", Text("A"), Text("^[a&&A]$"), Text("i"));
        CallEquals(Bool(false), "matches", Text("a"), Text("^[a--A]$"), Text("i"));
        CallEquals(Bool(false), "matches", Text("A"), Text("^[^a&&A]$"), Text("i"));
        CallEquals(Bool(true), "matches", Text("🙂"), Text("^[^a&&A]$"), Text("i"));
        CallEquals(Bool(true), "matches", Text("𐐨"), Text(@"^[\p{Lu}&&[𐐀-𐐧]]$"), Text("i"));
        CallEquals(Bool(false), "matches", Text("𐐨"), Text(@"^[\P{Lu}&&[𐐀-𐐧]]$"), Text("i"));
        CallEquals(Bool(true), "matches", Text("é"), Text(@"^[\w&&[^a-z]]$"));
        CallEquals(Bool(false), "matches", Text("ſ"), Text(@"^[\w&&[^a-z]]$"), Text("i"));
        CallEquals(Bool(false), "matches", Text("🙂"), Text(@"^[\w&&[^a-z]]$"));
        CallEquals(Bool(true), "matches", Text("B"), Text("(?x)^[ a-c && [ B-D ] ]$"), Text("i"));
        CallEquals(Bool(true), "matches", Text("b"), Text("(?x)^[a-c # left\n && [b-d]]$"));
        CallEquals(Bool(true), "matches", Text("&"), Text("(?x)^[a& &b]$"));
        CallEquals(Bool(true), "matches", Text("🙂"), Text(@"^[\x{1F642}\x{1F643}--\x{1F643}]$"));
    }

    private static void RegexClassSetAsciiOperands()
    {
        var examples = new[] {
            ("alnum", "09AZaz", "_é🙂"),
            ("alpha", "AZaz", "09_é🙂"),
            ("ascii", "\0\t A~\u007F", "\u0080é🙂"),
            ("blank", "\t ", "\n\r\u00A0a🙂"),
            ("cntrl", "\0\t\u001F\u007F", " ~\u0080🙂"),
            ("digit", "09", "/:a٠🙂"),
            ("graph", "!AZaz09~", " \t\u007Fé🙂"),
            ("lower", "az", "AZ0é🙂"),
            ("print", " !AZaz09~", "\0\t\u007Fé🙂"),
            ("punct", "!/@[\\`{~", " AZaz09é🙂"),
            ("space", "\t\n\v\f\r ", "\0\u0085\u00A0a🙂"),
            ("upper", "AZ", "az0É🙂"),
            ("word", "09AZaz_", "!é🙂"),
            ("xdigit", "09AFaf", "Ggzé🙂"),
        };
        foreach (var (name, included, excluded) in examples)
        {
            foreach (var (input, expected) in new[] { (included, true), (excluded, false) })
            {
                foreach (var scalar in input.EnumerateRunes())
                {
                    CallEquals(Bool(expected), "matches", Text(scalar.ToString()), Text("^[[:" + name + ":]]$"));
                    CallEquals(Bool(!expected), "matches", Text(scalar.ToString()), Text("^[[:^" + name + ":]]$"));
                }
            }
        }
        foreach (var name in new[] { "alnum", "alpha", "ascii", "graph", "lower", "print", "upper", "word" })
        {
            foreach (var input in new[] { "A", "a", "ſ", "K" })
            {
                CallEquals(Bool(true), "matches", Text(input), Text("^[[:" + name + ":]]$"), Text("i"));
                CallEquals(Bool(false), "matches", Text(input), Text("^[[:^" + name + ":]]$"), Text("i"));
            }
        }
        CallEquals(Bool(false), "matches", Text("é"), Text("^[[:alpha:]]$"), Text("i"));
        CallEquals(Text("a[B][c]0🙂"), "replace", Text("aBc0🙂"), Text("([[:alpha:]&&[^aeiou]])"), Text("[$1]"), Text("i"));
        Equal("a|🙂|z", string.Join('|', FerruleSequences.TokenizeRegex(Text("a0🙂9z"), Text("[[:digit:]]"), null).Select(ValueText)));
        // Unknown/malformed POSIX spellings are nested literal classes in the Rust grammar.
        CallEquals(Bool(true), "matches", Text("f"), Text("^[[:foobar:]]$"));
        CallEquals(Bool(true), "matches", Text(":"), Text("^[[:foobar:]]$"));
        CallEquals(Bool(false), "matches", Text("z"), Text("^[[:foobar:]]$"));
        CallEquals(Bool(true), "matches", Text("a"), Text("(?x)^[[: alpha:]]$"));
        CallEquals(Bool(false), "matches", Text("z"), Text("(?x)^[[: alpha:]]$"));
        CallEquals(Bool(true), "matches", Text("z"), Text("(?x)^[[:alpha:]]$"));
    }

    private static void RegexClassSetBoundedCompilation()
    {
        var depth = new string('[', 258) + "a" + new string(']', 258);
        var work = "[" + string.Concat(Enumerable.Repeat(@"\p{L}&&", 7000)) + @"\p{L}]";
        foreach (var pattern in new[] { depth, work, "[a&&[b]", "[a-z&&[z-a]]" })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("a"), Text(pattern), null));
        }
        CallEquals(Bool(true), "matches", Text("a"), Text(new string('[', 32) + "a" + new string(']', 32)));
        CallEquals(Bool(true), "matches", Text("a"), Text("[" + string.Concat(Enumerable.Repeat("a&&", 128)) + "a]"));
    }
}
