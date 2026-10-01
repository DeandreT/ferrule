using System.Collections.Concurrent;
using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;

namespace Ferrule.Runtime;

internal static partial class FerruleScalarRegex
{
    private enum PropertyDomain { Existing, Script, ScriptExtensions, Binary }
    private readonly record struct PropertyIdentity(PropertyDomain Domain, string Name);
    private static readonly ConcurrentDictionary<string, ScalarSet> NamedProperties = new(StringComparer.Ordinal);
    private static readonly ConcurrentDictionary<(PropertyIdentity Identity, bool Folded, bool Complemented), ScalarSet> ModifiedProperties = new();

    private static (PropertyIdentity Identity, bool Complemented) PropertyQuery(string query, bool complemented, bool strict)
    {
        if (!strict)
        {
            var existing = ExistingPropertyQuery(query, complemented);
            return (new PropertyIdentity(PropertyDomain.Existing, existing.Name), existing.Complemented);
        }
        var separator = query.IndexOf("!=", StringComparison.Ordinal);
        var width = 2;
        var inverted = separator >= 0;
        if (separator < 0) { separator = query.IndexOf(':'); width = 1; }
        if (separator < 0) { separator = query.IndexOf('='); }
        var domain = PropertyDomain.Script;
        var value = query;
        if (separator >= 0)
        {
            var property = NormalizePropertyName(query[..separator]);
            domain = property switch {
                "gc" or "generalcategory" => PropertyDomain.Existing,
                "sc" or "script" => PropertyDomain.Script,
                "scx" or "scriptextensions" => PropertyDomain.ScriptExtensions,
                _ => throw Invalid("unsupported property query"),
            };
            value = query[(separator + width)..];
            complemented ^= inverted;
        }
        var normalized = NormalizePropertyName(value);
        // Reachable bare binary names precede categories/scripts. The pinned
        // binary aliases exclude cf/lc/sc, which remain general categories.
        // Do not accept Boolean-valued binary queries or unreachable InCB data.
        if (separator < 0 && BinaryAliases.TryGetValue(normalized, out var binary))
        {
            return (new PropertyIdentity(PropertyDomain.Binary, binary), complemented);
        }
        // Bare general categories take precedence over script aliases.
        if ((separator < 0 || domain == PropertyDomain.Existing)
            && CategoryAliases.TryGetValue(normalized, out var category))
        {
            if (category == "Cs") { throw Invalid("surrogate category has no Unicode scalar property data"); }
            return (new PropertyIdentity(PropertyDomain.Existing, category), complemented);
        }
        if (domain == PropertyDomain.Existing) { throw Invalid("unrecognized general category"); }
        if (!ScriptAliases.TryGetValue(normalized, out var script))
        {
            throw Invalid("unsupported Unicode script property in the explicit Unicode profile");
        }
        return (new PropertyIdentity(domain, script), complemented);
    }

    private static ScalarSet ModifiedProperty(PropertyIdentity identity, bool folded, bool complemented) =>
        ModifiedProperties.GetOrAdd((identity, folded, complemented), static key => {
            var value = key.Identity.Domain switch {
                PropertyDomain.Existing => Property(key.Identity.Name),
                PropertyDomain.Script => ScriptSets.Value[key.Identity.Name],
                PropertyDomain.ScriptExtensions => ScriptExtensionSets.Value[key.Identity.Name],
                PropertyDomain.Binary => BinarySets.Value[key.Identity.Name],
                _ => throw Invalid("unsupported property domain"),
            };
            if (key.Folded) { value = value.FoldCase(); }
            return key.Complemented ? value.Complement() : value;
        });

    private static readonly IReadOnlyDictionary<string, UnicodeCategory[]> CategoryNames =
        new Dictionary<string, UnicodeCategory[]>(StringComparer.Ordinal) {
            ["Lu"] = new[] { UnicodeCategory.UppercaseLetter }, ["Ll"] = new[] { UnicodeCategory.LowercaseLetter },
            ["Lt"] = new[] { UnicodeCategory.TitlecaseLetter }, ["Lm"] = new[] { UnicodeCategory.ModifierLetter },
            ["Lo"] = new[] { UnicodeCategory.OtherLetter }, ["Mn"] = new[] { UnicodeCategory.NonSpacingMark },
            ["Mc"] = new[] { UnicodeCategory.SpacingCombiningMark }, ["Me"] = new[] { UnicodeCategory.EnclosingMark },
            ["Nd"] = new[] { UnicodeCategory.DecimalDigitNumber }, ["Nl"] = new[] { UnicodeCategory.LetterNumber },
            ["No"] = new[] { UnicodeCategory.OtherNumber }, ["Pc"] = new[] { UnicodeCategory.ConnectorPunctuation },
            ["Pd"] = new[] { UnicodeCategory.DashPunctuation }, ["Ps"] = new[] { UnicodeCategory.OpenPunctuation },
            ["Pe"] = new[] { UnicodeCategory.ClosePunctuation }, ["Pi"] = new[] { UnicodeCategory.InitialQuotePunctuation },
            ["Pf"] = new[] { UnicodeCategory.FinalQuotePunctuation }, ["Po"] = new[] { UnicodeCategory.OtherPunctuation },
            ["Sm"] = new[] { UnicodeCategory.MathSymbol }, ["Sc"] = new[] { UnicodeCategory.CurrencySymbol },
            ["Sk"] = new[] { UnicodeCategory.ModifierSymbol }, ["So"] = new[] { UnicodeCategory.OtherSymbol },
            ["Zs"] = new[] { UnicodeCategory.SpaceSeparator }, ["Zl"] = new[] { UnicodeCategory.LineSeparator },
            ["Zp"] = new[] { UnicodeCategory.ParagraphSeparator }, ["Cc"] = new[] { UnicodeCategory.Control },
            ["Cf"] = new[] { UnicodeCategory.Format }, ["Cs"] = new[] { UnicodeCategory.Surrogate },
            ["Co"] = new[] { UnicodeCategory.PrivateUse }, ["Cn"] = new[] { UnicodeCategory.OtherNotAssigned },
            ["LC"] = new[] { UnicodeCategory.UppercaseLetter, UnicodeCategory.LowercaseLetter, UnicodeCategory.TitlecaseLetter },
            ["L"] = new[] { UnicodeCategory.UppercaseLetter, UnicodeCategory.LowercaseLetter, UnicodeCategory.TitlecaseLetter, UnicodeCategory.ModifierLetter, UnicodeCategory.OtherLetter },
            ["M"] = new[] { UnicodeCategory.NonSpacingMark, UnicodeCategory.SpacingCombiningMark, UnicodeCategory.EnclosingMark },
            ["N"] = new[] { UnicodeCategory.DecimalDigitNumber, UnicodeCategory.LetterNumber, UnicodeCategory.OtherNumber },
            ["P"] = new[] { UnicodeCategory.ConnectorPunctuation, UnicodeCategory.DashPunctuation, UnicodeCategory.OpenPunctuation, UnicodeCategory.ClosePunctuation, UnicodeCategory.InitialQuotePunctuation, UnicodeCategory.FinalQuotePunctuation, UnicodeCategory.OtherPunctuation },
            ["S"] = new[] { UnicodeCategory.MathSymbol, UnicodeCategory.CurrencySymbol, UnicodeCategory.ModifierSymbol, UnicodeCategory.OtherSymbol },
            ["Z"] = new[] { UnicodeCategory.SpaceSeparator, UnicodeCategory.LineSeparator, UnicodeCategory.ParagraphSeparator },
            ["C"] = new[] { UnicodeCategory.Control, UnicodeCategory.Format, UnicodeCategory.Surrogate, UnicodeCategory.PrivateUse, UnicodeCategory.OtherNotAssigned },
        };

    private static readonly IReadOnlyDictionary<string, string> CategoryAliases = BuildCategoryAliases();

    private static IReadOnlyDictionary<string, string> BuildCategoryAliases()
    {
        var aliases = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var (canonical, names) in new[] {
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
            ("Any", "any"), ("ASCII", "ascii"), ("Assigned", "assigned"),
        })
        {
            foreach (var name in names.Split(' ')) { aliases.Add(name, canonical); }
        }
        return aliases;
    }

    private static string NormalizePropertyName(string name)
    {
        var prefixed = name.Length >= 2 && name[0] is 'i' or 'I' && name[1] is 's' or 'S';
        var normalized = new StringBuilder(name.Length);
        for (var index = prefixed ? 2 : 0; index < name.Length; index++)
        {
            var value = name[index];
            if (value > 0x7F || value is ' ' or '_' or '-') { continue; }
            normalized.Append(value is >= 'A' and <= 'Z' ? (char)(value + ('a' - 'A')) : value);
        }
        // ISO_Comment's abbreviation is not the Other category after stripping "is".
        var result = normalized.ToString();
        return prefixed && result == "c" ? "isc" : result;
    }

    private static (string Name, bool Complemented) ExistingPropertyQuery(string query, bool complemented)
    {
        var separator = query.IndexOf("!=", StringComparison.Ordinal);
        var width = 2;
        var inverted = separator >= 0;
        if (separator < 0) { separator = query.IndexOf(':'); width = 1; }
        if (separator < 0) { separator = query.IndexOf('='); }
        var value = query;
        if (separator >= 0)
        {
            var property = NormalizePropertyName(query[..separator]);
            if (property is not ("gc" or "generalcategory")) { throw Invalid("unsupported property query"); }
            value = query[(separator + width)..];
            complemented ^= inverted;
        }
        if (CategoryAliases.TryGetValue(NormalizePropertyName(value), out var canonical))
        {
            if (canonical == "Cs") { throw Invalid("surrogate category has no Unicode scalar property data"); }
            return (canonical, complemented);
        }
        if (separator >= 0) { throw Invalid("unrecognized general category"); }
        // Retain raw accepted host blocks outside the selected category vocabulary.
        return (query, complemented);
    }

    private static ScalarSet Property(string name) => NamedProperties.GetOrAdd(name, static key => {
        if (key == "Any") { return ScalarSet.All; }
        if (key == "ASCII") { return ScalarSet.Between(0, 0x7F); }
        if (key == "Assigned") { return Property("Cn").Complement(); }
        if (CategoryNames.TryGetValue(key, out var categories))
        {
            return new ScalarSet(categories.SelectMany(category => Categories.Value.TryGetValue(category, out var set)
                ? set.Ranges : Array.Empty<Range>()));
        }
        // Retain accepted host block properties without expanding the host dialect.
        var regex = new Regex(@"\p{" + key + "}", RegexOptions.CultureInvariant | RegexOptions.NonBacktracking);
        var bmp = new StringBuilder(0x10000 - 0x800);
        for (var scalar = 0; scalar <= 0xFFFF; scalar++)
        {
            if (Rune.IsValid(scalar)) { bmp.Append((char)scalar); }
        }
        var text = bmp.ToString();
        var ranges = new List<Range>();
        foreach (var match in regex.EnumerateMatches(text))
        {
            var scalar = text[match.Index]; ranges.Add(new Range(scalar, scalar));
        }
        return new ScalarSet(ranges);
    });

}
