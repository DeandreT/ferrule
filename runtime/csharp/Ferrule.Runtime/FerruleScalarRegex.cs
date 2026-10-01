using System.Collections.Concurrent;
using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;

namespace Ferrule.Runtime;

/// <summary>
/// Lowers consuming regex atoms to complete Unicode scalars while retaining
/// the host non-backtracking engine's capture, alternation and repetition rules.
/// All added groups are noncapturing; match offsets still address the input UTF-16.
/// </summary>
internal static class FerruleScalarRegex
{
    private const int MaximumTranslatedCharacters = 10 * 1024 * 1024;
    private const int MaximumClassDepth = 256;

    internal static Regex Compile(string source, RegexOptions options) =>
        // A neutral line-anchor alternative establishes the host's newline context.
        // Without it, NonBacktracking loses a final LF for large scalar sets.
        new("(?m:^|)(?:" + new Translator(source, options).Translate() + ")", options);

    private readonly record struct Range(int First, int Last);

    private sealed class ScalarSet
    {
        internal static readonly ScalarSet Empty = new(Array.Empty<Range>());
        internal static readonly ScalarSet All = new(new[] {
            new Range(0, 0xD7FF), new Range(0xE000, 0x10FFFF),
        });
        internal readonly Range[] Ranges;

        internal ScalarSet(IEnumerable<Range> ranges)
        {
            var ordered = ranges.OrderBy(range => range.First).ToArray();
            var merged = new List<Range>();
            foreach (var range in ordered)
            {
                if (range.First > range.Last) { continue; }
                if (merged.Count != 0 && range.First <= merged[^1].Last + 1)
                {
                    merged[^1] = new Range(merged[^1].First, Math.Max(merged[^1].Last, range.Last));
                }
                else { merged.Add(range); }
            }
            Ranges = merged.ToArray();
        }

        internal static ScalarSet Between(int first, int last)
        {
            if (!Rune.IsValid(first) || !Rune.IsValid(last) || first > last)
            {
                throw Invalid("invalid Unicode scalar range");
            }
            var ranges = new List<Range>();
            if (first <= 0xD7FF) { ranges.Add(new Range(first, Math.Min(last, 0xD7FF))); }
            if (last >= 0xE000) { ranges.Add(new Range(Math.Max(first, 0xE000), last)); }
            return new ScalarSet(ranges);
        }

        internal bool Contains(int scalar)
        {
            var low = 0;
            var high = Ranges.Length - 1;
            while (low <= high)
            {
                var middle = low + (high - low) / 2;
                var range = Ranges[middle];
                if (scalar < range.First) { high = middle - 1; }
                else if (scalar > range.Last) { low = middle + 1; }
                else { return true; }
            }
            return false;
        }

        internal ScalarSet Union(ScalarSet other) => new(Ranges.Concat(other.Ranges));

        internal ScalarSet Except(ScalarSet other)
        {
            var result = new List<Range>();
            var excludedIndex = 0;
            foreach (var range in Ranges)
            {
                var first = range.First;
                while (excludedIndex < other.Ranges.Length && other.Ranges[excludedIndex].Last < first)
                {
                    excludedIndex++;
                }
                for (var index = excludedIndex; index < other.Ranges.Length; index++)
                {
                    var excluded = other.Ranges[index];
                    if (excluded.First > range.Last) { break; }
                    if (excluded.First > first) { result.Add(new Range(first, excluded.First - 1)); }
                    first = Math.Max(first, excluded.Last + 1);
                    if (first > range.Last) { break; }
                }
                if (first <= range.Last) { result.Add(new Range(first, range.Last)); }
            }
            return new ScalarSet(result);
        }

        internal ScalarSet Complement() => All.Except(this);

        internal ScalarSet FoldCase()
        {
            var ranges = new List<Range>(Ranges);
            foreach (var group in CaseTables.Value.Groups)
            {
                if (!group.Any(Contains)) { continue; }
                ranges.AddRange(group.Select(value => new Range(value, value)));
            }
            return new ScalarSet(ranges);
        }
    }

    private sealed record CaseTable(int[][] Groups, IReadOnlyDictionary<int, int[]> ByScalar);
    private static readonly Lazy<CaseTable> CaseTables = new(BuildCaseTable);
    private static readonly Lazy<IReadOnlyDictionary<UnicodeCategory, ScalarSet>> Categories = new(BuildCategories);
    private static readonly ConcurrentDictionary<string, ScalarSet> NamedProperties = new(StringComparer.Ordinal);
    private static readonly ConcurrentDictionary<(string Name, bool Folded, bool Complemented), ScalarSet> ModifiedProperties = new();
    private static readonly Lazy<IReadOnlyDictionary<char, ScalarSet>> Shorthands = new(BuildShorthands);

    private static CaseTable BuildCaseTable()
    {
        var parents = new Dictionary<int, int>();
        int Find(int scalar)
        {
            if (!parents.TryGetValue(scalar, out var parent)) { parents[scalar] = scalar; return scalar; }
            if (parent != scalar) { parents[scalar] = Find(parent); }
            return parents[scalar];
        }
        void Join(int left, int right)
        {
            if (left == right) { return; }
            // Native casing can know newer assignments than the runtime's
            // Unicode category table. Such scalars have no case pair in the
            // interpreter's Unicode domain and must remain independent.
            if (Rune.GetUnicodeCategory(new Rune(left)) == UnicodeCategory.OtherNotAssigned
                || Rune.GetUnicodeCategory(new Rune(right)) == UnicodeCategory.OtherNotAssigned) { return; }
            parents[Find(left)] = Find(right);
        }
        for (var scalar = 0; scalar <= 0x10FFFF; scalar++)
        {
            if (!Rune.IsValid(scalar)) { continue; }
            // Default Unicode folding keeps dotted/dotless Turkish I apart
            // from I/i regardless of the host globalization configuration.
            if (scalar is 0x0130 or 0x0131) { continue; }
            var rune = new Rune(scalar);
            Join(scalar, Rune.ToUpperInvariant(rune).Value);
            Join(scalar, Rune.ToLowerInvariant(rune).Value);
        }
        // These default simple-fold equivalences do not follow invariant
        // upper/lower casing (Unicode 16; includes long s and Greek variants).
        Join(0x017F, 's');
        Join(0x0390, 0x1FD3);
        Join(0x03B0, 0x1FE3);
        Join(0xFB05, 0xFB06);
        var groups = parents.Keys.ToArray().GroupBy(Find).Select(group => group.ToArray()).ToArray();
        return new CaseTable(groups, groups.SelectMany(group => group.Select(value => (value, group)))
            .ToDictionary(item => item.value, item => item.group));
    }

    private static IReadOnlyDictionary<UnicodeCategory, ScalarSet> BuildCategories()
    {
        var categories = new Dictionary<UnicodeCategory, List<Range>>();
        for (var scalar = 0; scalar <= 0x10FFFF; scalar++)
        {
            if (!Rune.IsValid(scalar)) { continue; }
            var category = Rune.GetUnicodeCategory(new Rune(scalar));
            if (!categories.TryGetValue(category, out var ranges))
            {
                ranges = new List<Range>(); categories.Add(category, ranges);
            }
            if (ranges.Count != 0 && ranges[^1].Last + 1 == scalar)
            {
                ranges[^1] = new Range(ranges[^1].First, scalar);
            }
            else { ranges.Add(new Range(scalar, scalar)); }
        }
        return categories.ToDictionary(item => item.Key, item => new ScalarSet(item.Value));
    }

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
            ["L"] = new[] { UnicodeCategory.UppercaseLetter, UnicodeCategory.LowercaseLetter, UnicodeCategory.TitlecaseLetter, UnicodeCategory.ModifierLetter, UnicodeCategory.OtherLetter },
            ["M"] = new[] { UnicodeCategory.NonSpacingMark, UnicodeCategory.SpacingCombiningMark, UnicodeCategory.EnclosingMark },
            ["N"] = new[] { UnicodeCategory.DecimalDigitNumber, UnicodeCategory.LetterNumber, UnicodeCategory.OtherNumber },
            ["P"] = new[] { UnicodeCategory.ConnectorPunctuation, UnicodeCategory.DashPunctuation, UnicodeCategory.OpenPunctuation, UnicodeCategory.ClosePunctuation, UnicodeCategory.InitialQuotePunctuation, UnicodeCategory.FinalQuotePunctuation, UnicodeCategory.OtherPunctuation },
            ["S"] = new[] { UnicodeCategory.MathSymbol, UnicodeCategory.CurrencySymbol, UnicodeCategory.ModifierSymbol, UnicodeCategory.OtherSymbol },
            ["Z"] = new[] { UnicodeCategory.SpaceSeparator, UnicodeCategory.LineSeparator, UnicodeCategory.ParagraphSeparator },
            ["C"] = new[] { UnicodeCategory.Control, UnicodeCategory.Format, UnicodeCategory.Surrogate, UnicodeCategory.PrivateUse, UnicodeCategory.OtherNotAssigned },
        };

    private static ScalarSet Property(string name) => NamedProperties.GetOrAdd(name, static key => {
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

    private static IReadOnlyDictionary<char, ScalarSet> BuildShorthands()
    {
        var result = new Dictionary<char, ScalarSet> {
            ['d'] = Property("Nd"),
            ['w'] = new ScalarSet(new[] { Property("L"), Property("M"), Property("Nl"), Property("Nd"), Property("Pc"), ScalarSet.Between(0x200C, 0x200D),
                // Other_Alphabetic symbols outside the letter/mark categories.
                ScalarSet.Between(0x24B6, 0x24E9), ScalarSet.Between(0x1F130, 0x1F149),
                ScalarSet.Between(0x1F150, 0x1F169), ScalarSet.Between(0x1F170, 0x1F189),
            }
                .SelectMany(set => set.Ranges)),
            ['s'] = new ScalarSet(new[] {
                new Range(9, 13), new Range(0x20, 0x20), new Range(0x85, 0x85), new Range(0xA0, 0xA0),
                new Range(0x1680, 0x1680), new Range(0x2000, 0x200A), new Range(0x2028, 0x2029),
                new Range(0x202F, 0x202F), new Range(0x205F, 0x205F), new Range(0x3000, 0x3000),
            }),
        };
        foreach (var lower in new[] { 'd', 'w', 's' }) { result.Add(char.ToUpperInvariant(lower), result[lower].Complement()); }
        return result;
    }

    private static ScalarSet Shorthand(char kind) => Shorthands.Value[kind];

    private static string Atom(ScalarSet set)
    {
        if (set.Ranges.Length == 0) { return @"[\u0000-[\u0000]]"; }
        var branches = new List<string>();
        var bmp = new StringBuilder();
        foreach (var range in set.Ranges)
        {
            if (range.First <= 0xFFFF) { AppendRange(bmp, range.First, Math.Min(range.Last, 0xFFFF)); }
            if (range.Last < 0x10000) { continue; }
            var first = Math.Max(range.First, 0x10000) - 0x10000;
            var last = range.Last - 0x10000;
            var firstHigh = 0xD800 + (first >> 10);
            var lastHigh = 0xD800 + (last >> 10);
            var firstLow = 0xDC00 + (first & 0x3FF);
            var lastLow = 0xDC00 + (last & 0x3FF);
            if (firstHigh == lastHigh) { branches.Add(Pair(firstHigh, firstHigh, firstLow, lastLow)); continue; }
            if (firstLow != 0xDC00) { branches.Add(Pair(firstHigh, firstHigh, firstLow, 0xDFFF)); firstHigh++; }
            if (lastLow != 0xDFFF) { branches.Add(Pair(lastHigh, lastHigh, 0xDC00, lastLow)); lastHigh--; }
            if (firstHigh <= lastHigh) { branches.Add(Pair(firstHigh, lastHigh, 0xDC00, 0xDFFF)); }
        }
        if (bmp.Length != 0) { branches.Insert(0, "[" + bmp + "]"); }
        // Fold in scalar space before complements; disable host code-unit folding.
        return "(?-i:" + string.Join('|', branches) + ")";
    }

    private static string Pair(int firstHigh, int lastHigh, int firstLow, int lastLow)
    {
        var builder = new StringBuilder("["); AppendRange(builder, firstHigh, lastHigh);
        builder.Append("]["); AppendRange(builder, firstLow, lastLow); builder.Append(']');
        return builder.ToString();
    }

    private static void AppendRange(StringBuilder output, int first, int last)
    {
        output.Append(@"\u").Append(first.ToString("X4", CultureInfo.InvariantCulture));
        if (first != last) { output.Append(@"-\u").Append(last.ToString("X4", CultureInfo.InvariantCulture)); }
    }

    private sealed class Translator
    {
        private readonly string _source;
        private readonly StringBuilder _output = new();
        private readonly Stack<RegexOptions> _groups = new();
        private readonly Dictionary<(string Source, bool IgnoreCase, bool IgnoreWhitespace), string> _classes = new();
        private readonly Dictionary<(int Scalar, bool IgnoreCase), string> _literals = new();
        private RegexOptions _options;
        private int _index;

        internal Translator(string source, RegexOptions options) { _source = source; _options = options; }
        private bool IgnoreCase => (_options & RegexOptions.IgnoreCase) != 0;

        internal string Translate()
        {
            while (_index < _source.Length)
            {
                var current = _source[_index];
                if ((_options & RegexOptions.IgnorePatternWhitespace) != 0 && current == '#')
                {
                    while (_index < _source.Length && _source[_index] != '\n') { _index++; }
                    // Comments carry no regex semantics. Dropping them also
                    // keeps an EOF comment from swallowing the host wrapper.
                    continue;
                }
                switch (current)
                {
                    case '(':
                        Group(); break;
                    case ')':
                        _index++; Append(")");
                        if (_groups.TryPop(out var previous)) { _options = previous; }
                        break;
                    case '[':
                        var start = _index;
                        var set = Class(0);
                        var key = (_source[start.._index], IgnoreCase,
                            (_options & RegexOptions.IgnorePatternWhitespace) != 0);
                        if (!_classes.TryGetValue(key, out var atom)) { atom = Atom(set); _classes.Add(key, atom); }
                        Append(atom); break;
                    case '.':
                        _index++; Append(Atom((_options & RegexOptions.Singleline) != 0
                            ? ScalarSet.All : ScalarSet.All.Except(ScalarSet.Between('\n', '\n')))); break;
                    case '$':
                        _index++; Append((_options & RegexOptions.Multiline) != 0 ? "$" : @"\z"); break;
                    case '\\':
                        EscapeOutside(); break;
                    case '{':
                        Quantifier(); break;
                    case '^': case '|': case '*': case '+': case '?': case '}':
                        _index++; Append(current.ToString()); break;
                    default:
                        if ((_options & RegexOptions.IgnorePatternWhitespace) != 0 && char.IsWhiteSpace(current))
                        {
                            _index++;
                        }
                        else { Append(Literal(ReadScalar())); }
                        break;
                }
            }
            return _output.ToString();
        }

        private void Append(string value)
        {
            if (value.Length > MaximumTranslatedCharacters - _output.Length)
            {
                throw Invalid("pattern exceeds the compiled-size limit after scalar lowering");
            }
            _output.Append(value);
        }

        private string Literal(int scalar)
        {
            var key = (scalar, IgnoreCase);
            if (!_literals.TryGetValue(key, out var atom))
            {
                var set = ScalarSet.Between(scalar, scalar);
                if (IgnoreCase && CaseTables.Value.ByScalar.TryGetValue(scalar, out var group))
                {
                    set = new ScalarSet(group.Select(value => new Range(value, value)));
                }
                atom = Atom(set); _literals.Add(key, atom);
            }
            return atom;
        }

        private void Group()
        {
            var start = _index++;
            if (_index == _source.Length || _source[_index] != '?') { _groups.Push(_options); Append("("); return; }
            _index++;
            if (_index < _source.Length && _source[_index] == '#')
            {
                while (_index < _source.Length && _source[_index] != ')') { _index++; }
                if (_index < _source.Length) { _index++; }
                Append(_source[start.._index]); return;
            }
            var updated = _options;
            var enabled = true;
            var hasFlag = false;
            while (_index < _source.Length)
            {
                var flag = _source[_index];
                if (flag == '-') { enabled = false; _index++; continue; }
                var option = flag switch {
                    'i' => RegexOptions.IgnoreCase, 'm' => RegexOptions.Multiline, 's' => RegexOptions.Singleline,
                    'x' => RegexOptions.IgnorePatternWhitespace, 'n' => RegexOptions.ExplicitCapture,
                    _ => RegexOptions.None,
                };
                if (option == RegexOptions.None) { break; }
                hasFlag = true; updated = enabled ? updated | option : updated & ~option; _index++;
            }
            if (hasFlag && _index < _source.Length && _source[_index] is ':' or ')')
            {
                if (_source[_index] == ':') { _groups.Push(_options); }
                _options = updated; _index++; Append(_source[start.._index]); return;
            }
            _index = start + 2;
            _groups.Push(_options);
            if (_index < _source.Length && _source[_index] is ':' or '=' or '!' or '>') { _index++; }
            else if (_index < _source.Length && _source[_index] is '<' or '\'')
            {
                var closer = _source[_index++] == '<' ? '>' : '\'';
                if (_index < _source.Length && _source[_index] is '=' or '!') { _index++; }
                else
                {
                    while (_index < _source.Length && _source[_index] != closer) { _index++; }
                    if (_index < _source.Length) { _index++; }
                }
            }
            Append(_source[start.._index]);
        }

        private void Quantifier()
        {
            var start = _index++;
            while (_index < _source.Length && _source[_index] is >= '0' and <= '9') { _index++; }
            var hasMinimum = _index > start + 1;
            if (_index < _source.Length && _source[_index] == ',')
            {
                _index++;
                while (_index < _source.Length && _source[_index] is >= '0' and <= '9') { _index++; }
            }
            if (hasMinimum && _index < _source.Length && _source[_index] == '}')
            {
                _index++; Append(_source[start.._index]);
            }
            else { _index = start + 1; Append(Literal('{')); }
        }

        private void EscapeOutside()
        {
            var start = _index++;
            if (_index >= _source.Length) { throw Invalid("trailing backslash"); }
            var escape = _source[_index];
            if (escape is 'b' or 'B' or 'A' or 'Z' or 'z' or 'G' or 'k' or >= '1' and <= '9')
            {
                _index++;
                if (escape is >= '1' and <= '9')
                {
                    while (_index < _source.Length && _source[_index] is >= '0' and <= '9') { _index++; }
                }
                Append(_source[start.._index]); return;
            }
            _index = start;
            var item = ClassItem();
            Append(item.Scalar.HasValue ? Literal(item.Scalar.Value) : Atom(item.Set));
        }

        private readonly record struct Item(ScalarSet Set, int? Scalar);

        private Item ClassItem()
        {
            if (_index >= _source.Length) { throw Invalid("missing class item"); }
            if (_source[_index] != '\\')
            {
                var scalar = ReadScalar(); return new Item(ScalarSet.Between(scalar, scalar), scalar);
            }
            _index++;
            if (_index >= _source.Length) { throw Invalid("trailing backslash"); }
            var kind = _source[_index++];
            if (kind is 'd' or 'D' or 's' or 'S' or 'w' or 'W') { return new Item(Shorthand(kind), null); }
            if (kind is 'p' or 'P')
            {
                if (_index >= _source.Length || _source[_index++] != '{') { throw Invalid("missing property name"); }
                var start = _index;
                while (_index < _source.Length && _source[_index] != '}') { _index++; }
                if (_index == _source.Length) { throw Invalid("missing property terminator"); }
                var key = (_source[start.._index++], IgnoreCase, kind == 'P');
                var set = ModifiedProperties.GetOrAdd(key, static key => {
                    var value = Property(key.Name);
                    if (key.Folded) { value = value.FoldCase(); }
                    return key.Complemented ? value.Complement() : value;
                });
                return new Item(set, null);
            }
            var scalarValue = kind switch {
                'a' => 7, 'b' => 8, 'e' => 27, 'f' => 12, 'n' => 10, 'r' => 13, 't' => 9, 'v' => 11,
                'u' => HexScalar(), 'x' => Hex(2), 'c' => Control(),
                >= '0' and <= '7' => Octal(kind),
                _ when !char.IsLetterOrDigit(kind) => kind,
                _ => throw Invalid("unrecognized escape"),
            };
            if (!Rune.IsValid(scalarValue)) { throw Invalid("escape is not a Unicode scalar"); }
            return new Item(ScalarSet.Between(scalarValue, scalarValue), scalarValue);
        }

        private ScalarSet Class(int depth)
        {
            if (depth > MaximumClassDepth) { throw Invalid("character-class nesting exceeds its bounded depth"); }
            _index++;
            SkipIgnored();
            var complemented = _index < _source.Length && _source[_index] == '^';
            if (complemented) { _index++; }
            // Collect first, then normalize/fold once. Irregular large classes
            // must not repeatedly clone and sort every previously parsed item.
            var scalarRanges = new List<Range>();
            var composites = new HashSet<ScalarSet>();
            ScalarSet Finish()
            {
                var scalars = new ScalarSet(scalarRanges);
                if (IgnoreCase) { scalars = scalars.FoldCase(); }
                var combined = new ScalarSet(scalars.Ranges.Concat(composites.SelectMany(item => item.Ranges)));
                return complemented ? combined.Complement() : combined;
            }
            var first = true;
            while (_index < _source.Length)
            {
                SkipIgnored();
                if (_index == _source.Length) { break; }
                if (_source[_index] == ']' && !first)
                {
                    _index++; return Finish();
                }
                if (!first && _source[_index] == '-')
                {
                    var dash = _index++;
                    SkipIgnored();
                    if (_index < _source.Length && _source[_index] == '[')
                    {
                        var excluded = Class(depth + 1);
                        SkipIgnored();
                        if (_index >= _source.Length || _source[_index++] != ']') { throw Invalid("subtraction must end the class"); }
                        return Finish().Except(excluded);
                    }
                    _index = dash;
                }
                var item = ClassItem();
                first = false;
                SkipIgnored();
                if (_index < _source.Length && _source[_index] == '-')
                {
                    var dash = _index++;
                    SkipIgnored();
                    if (_index < _source.Length && _source[_index] is not (']' or '['))
                    {
                        var last = ClassItem();
                        if (!item.Scalar.HasValue || !last.Scalar.HasValue) { throw Invalid("range endpoints must be scalars"); }
                        scalarRanges.AddRange(ScalarSet.Between(item.Scalar.Value, last.Scalar.Value).Ranges);
                        continue;
                    }
                    _index = dash;
                }
                if (item.Scalar.HasValue) { scalarRanges.AddRange(item.Set.Ranges); }
                else { composites.Add(item.Set); }
            }
            throw Invalid("unterminated character class");
        }

        private void SkipIgnored()
        {
            if ((_options & RegexOptions.IgnorePatternWhitespace) == 0) { return; }
            while (_index < _source.Length)
            {
                if (char.IsWhiteSpace(_source[_index])) { _index++; }
                else if (_source[_index] == '#')
                {
                    while (_index < _source.Length && _source[_index] != '\n') { _index++; }
                }
                else { break; }
            }
        }

        private int ReadScalar()
        {
            var rune = Rune.GetRuneAt(_source, _index); _index += rune.Utf16SequenceLength; return rune.Value;
        }

        private int HexScalar()
        {
            var value = Hex(4);
            if (value is >= 0xD800 and <= 0xDBFF)
            {
                if (_index + 2 > _source.Length || _source[_index] != '\\' || _source[_index + 1] != 'u')
                {
                    throw Invalid("isolated surrogate escape");
                }
                _index += 2; var low = Hex(4);
                if (low is not (>= 0xDC00 and <= 0xDFFF)) { throw Invalid("isolated surrogate escape"); }
                return 0x10000 + ((value - 0xD800) << 10) + low - 0xDC00;
            }
            if (!Rune.IsValid(value)) { throw Invalid("isolated surrogate escape"); }
            return value;
        }

        private int Hex(int digits)
        {
            if (_source.Length - _index < digits) { throw Invalid("incomplete hex escape"); }
            var value = 0;
            for (var digit = 0; digit < digits; digit++)
            {
                var current = _source[_index++];
                var number = current switch {
                    >= '0' and <= '9' => current - '0', >= 'a' and <= 'f' => current - 'a' + 10,
                    >= 'A' and <= 'F' => current - 'A' + 10, _ => throw Invalid("invalid hex escape"),
                };
                value = value * 16 + number;
            }
            return value;
        }

        private int Control()
        {
            if (_index == _source.Length) { throw Invalid("missing control escape"); }
            var value = char.ToUpperInvariant(_source[_index++]);
            if (value is < '@' or > '_') { throw Invalid("invalid control escape"); }
            return value - '@';
        }

        private int Octal(char first)
        {
            var value = first - '0';
            for (var count = 1; count < 3 && _index < _source.Length && _source[_index] is >= '0' and <= '7'; count++)
            {
                value = value * 8 + _source[_index++] - '0';
            }
            // The host dialect's three-digit octal escapes use the low byte.
            return value & 0xFF;
        }
    }

    private static ArgumentException Invalid(string message) => new(message);
}
