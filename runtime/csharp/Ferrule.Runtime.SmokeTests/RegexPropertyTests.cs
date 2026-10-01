using Ferrule.Runtime;

namespace Ferrule.Runtime.SmokeTests;

internal static partial class Program
{
    private static void RegexPropertyCategoryAliases()
    {
        foreach (var (category, names) in new[] {
            ("C", "c other"),
            ("LC", "casedletter lc"),
            ("Cc", "cc cntrl control"),
            ("Cf", "cf format"),
            ("Pe", "closepunctuation pe"),
            ("Cn", "cn unassigned"),
            ("Co", "co privateuse"),
            ("M", "combiningmark m mark"),
            ("Pc", "connectorpunctuation pc"),
            ("Cs", "cs surrogate"),
            ("Sc", "currencysymbol sc"),
            ("Pd", "dashpunctuation pd"),
            ("Nd", "decimalnumber digit nd"),
            ("Me", "enclosingmark me"),
            ("Pf", "finalpunctuation pf"),
            ("Pi", "initialpunctuation pi"),
            ("L", "l letter"),
            ("Nl", "letternumber nl"),
            ("Zl", "lineseparator zl"),
            ("Ll", "ll lowercaseletter"),
            ("Lm", "lm modifierletter"),
            ("Lo", "lo otherletter"),
            ("Lt", "lt titlecaseletter"),
            ("Lu", "lu uppercaseletter"),
            ("Sm", "mathsymbol sm"),
            ("Mc", "mc spacingmark"),
            ("Mn", "mn nonspacingmark"),
            ("Sk", "modifiersymbol sk"),
            ("N", "n number"),
            ("No", "no othernumber"),
            ("Ps", "openpunctuation ps"),
            ("Po", "otherpunctuation po"),
            ("So", "othersymbol so"),
            ("P", "p punct punctuation"),
            ("Zp", "paragraphseparator zp"),
            ("S", "s symbol"),
            ("Z", "separator z"),
            ("Zs", "spaceseparator zs"),
        })
        {
            foreach (var name in names.Split(' '))
            {
                if (category == "Cs")
                {
                    AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text("\\p{" + name + "}"));
                    continue;
                }
                foreach (var input in new[] { "Aé́🙂𐐨", "\u0378 \n0_" })
                {
                    var expected = FerruleFunctions.Call("matches", new[] { Text(input), Text("\\p{" + category + "}") });
                    CallEquals(expected, "matches", Text(input), Text("\\p{" + name + "}"));
                }
            }
        }
        CallEquals(Bool(true), "matches", Text("a𐐀𐐨"), Text(@"^\p{Cased_Letter}+$"));
        CallEquals(Bool(false), "matches", Text("α中"), Text(@"^\p{LC}+$"));
        CallEquals(Text("0[a][𐐨]🙂"), "replace", Text("0a𐐨🙂"), Text(@"(\p{Letter})"), Text("[$1]"));
        Equal("a|𐐨|🙂", string.Join('|', FerruleSequences.TokenizeRegex(Text("a0𐐨9🙂"), Text(@"\p{Decimal_Number}"), null).Select(ValueText)));
        CallEquals(Bool(true), "matches", Text("𐐨"), Text(@"^\p{Uppercase_Letter}$"), Text("i"));
        CallEquals(Bool(false), "matches", Text("ſ"), Text(@"^\P{Uppercase_Letter}$"), Text("i"));
    }

    private static void RegexPropertyQueryGrammar()
    {
        foreach (var pattern in new[] {
            @"^\pL+$", @"^\p{Letter}+$", @"^\p{IsLetter}+$", @"^\p{Léetter}+$",
            @"^\p{gc=Letter}+$", @"^\p{General_Category:Letter}+$", @"^\p{g c = L}+$",
        })
        {
            CallEquals(Bool(true), "matches", Text("a𐐨"), Text(pattern));
            CallEquals(Bool(false), "matches", Text("a🙂"), Text(pattern));
        }
        CallEquals(Bool(true), "matches", Text("🙂"), Text(@"^\PL$"));
        CallEquals(Bool(false), "matches", Text("𐐨"), Text(@"^\PL$"));
        CallEquals(Bool(true), "matches", Text("a"), Text(@"^\p{gc!=Lu}$"));
        CallEquals(Bool(false), "matches", Text("A"), Text(@"^\p{gc!=Lu}$"));
        CallEquals(Bool(true), "matches", Text("A"), Text(@"^\P{gc!=Lu}$"));
        CallEquals(Bool(false), "matches", Text("a"), Text(@"^\P{gc!=Lu}$"));
        CallEquals(Bool(true), "matches", Text("𐐨"), Text(@"^\P{gc!=Lu}$"), Text("i"));
        CallEquals(Bool(false), "matches", Text("𐐨"), Text(@"^\p{gc!=Lu}$"), Text("i"));
        CallEquals(Bool(true), "matches", Text("é"), Text(@"^[\p{Letter}&&[^a-z]]$"));
        CallEquals(Bool(false), "matches", Text("ſ"), Text(@"^[\p{Letter}&&[^a-z]]$"), Text("i"));
        CallEquals(Bool(true), "matches", Text("A"), Text("(?x)^\\p { General_ # query\n Category : Uppercase_ Letter }$"));
        CallEquals(Bool(true), "matches", Text("A"), Text("(?x)^\\p # query\n L$"));
        CallEquals(Bool(true), "matches", Text("A"), Text("(?x)^\\p{L\t}$"));
    }

    private static void RegexPropertySpecialSetsAndHostBlocks()
    {
        CallEquals(Bool(true), "matches", Text("\0é🙂"), Text(@"^\p{Any}+$"));
        CallEquals(Bool(false), "matches", Text("a"), Text(@"\P{Any}"));
        CallEquals(Bool(true), "matches", Text("\0A\u007F"), Text(@"^\p{ASCII}+$"));
        CallEquals(Bool(false), "matches", Text("é"), Text(@"\p{ASCII}"));
        CallEquals(Bool(true), "matches", Text("é"), Text(@"\P{ASCII}"));
        CallEquals(Bool(true), "matches", Text("ſK"), Text(@"^\p{ASCII}+$"), Text("i"));
        CallEquals(Bool(true), "matches", Text("aé🙂"), Text(@"^\p{Assigned}+$"));
        CallEquals(Bool(false), "matches", Text("\u0378"), Text(@"\p{Assigned}"));
        CallEquals(Bool(true), "matches", Text("\u0378"), Text(@"\P{Assigned}"));
        // Recognized Is-prefixed category aliases override the old host block meaning.
        CallEquals(Bool(true), "matches", Text("\uE000\U000F0000\U00100000"), Text(@"^\p{IsPrivateUse}+$"));
        // Raw host blocks remain accepted extensions, outside category aliases.
        CallEquals(Bool(true), "matches", Text("A"), Text(@"^\p{IsBasicLatin}$"));
        CallEquals(Bool(false), "matches", Text("é"), Text(@"^\p{IsBasicLatin}$"));
        CallEquals(Bool(true), "matches", Text("Ϣ"), Text(@"^\p{IsGreek}$"));
        CallEquals(Bool(false), "matches", Text("ἀ"), Text(@"^\p{IsGreek}$"));
    }

    private static void RegexPropertyInvalidAndUnsupportedQueries()
    {
        foreach (var pattern in new[] {
            @"\p", @"\P", @"\p{}", @"\p{Letter", @"\p\L", @"\p{gc=}", @"\p{=Lu}",
            @"\p{gc!Lu}", @"\p{gc<>Lu}", @"\p{gc==Lu}", @"\p{gc=NoSuchValue}",
            @"\p{gc=Letter=Letter}", @"\p{^Lu}", @"\p{L&}", @"\p{IsC}", @"\p{isc}",
            @"\p{Cs}", @"\P{Cs}", @"\p{Surrogate}", @"\P{Surrogate}", @"\p{gc!=Surrogate}",
            "\\p{L\t}",
        })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern));
            AssertInvalidArgument("replace", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern), Text("x"));
            Error(FerruleRuntimeError.InvalidTokenizeRegex,
                () => FerruleSequences.TokenizeRegex(Text("a"), Text(pattern), null));
        }
        // These valid Rust names remain outside the existing host vocabulary.
        foreach (var pattern in new[] { @"\p{Script=Greek}", @"\p{Alphabetic}", @"\p{upper}", @"\p{lower}" })
        {
            AssertInvalidArgument("matches", "pattern is invalid or exceeds the compiled-size limit", Text("a"), Text(pattern));
        }
    }
}
