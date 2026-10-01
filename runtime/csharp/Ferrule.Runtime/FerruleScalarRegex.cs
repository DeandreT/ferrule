using System.Collections.Concurrent;
using System.Globalization;
using System.Text;
using System.Text.RegularExpressions;

namespace Ferrule.Runtime;

/// <summary>
/// Lowers consuming regex atoms to complete Unicode scalars while retaining
/// a non-backtracking engine with source-order captures and scalar assertions.
/// All added groups are noncapturing; match offsets still address the input UTF-16.
/// </summary>
internal static partial class FerruleScalarRegex
{
    private const int MaximumTranslatedCharacters = 10 * 1024 * 1024;
    private const int MaximumClassDepth = 256;
    private const long MaximumClassWork = 100_000_000;

    internal static Regex Compile(string source, RegexOptions options) => Compile(source, options, out _);

    internal static Regex Compile(string source, RegexOptions options, out int[] captureGroups)
    {
        var translator = new Translator(source, options);
        // A neutral line-anchor alternative establishes the host's newline context.
        // Without it, NonBacktracking loses a final LF for large scalar sets.
        var regex = new Regex("(?m:^|)(?:" + translator.Translate() + ")", options);
        captureGroups = translator.CaptureGroups(regex);
        return regex;
    }

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

        internal ScalarSet Intersect(ScalarSet other)
        {
            var result = new List<Range>();
            var left = 0;
            var right = 0;
            while (left < Ranges.Length && right < other.Ranges.Length)
            {
                var first = Math.Max(Ranges[left].First, other.Ranges[right].First);
                var last = Math.Min(Ranges[left].Last, other.Ranges[right].Last);
                if (first <= last) { result.Add(new Range(first, last)); }
                if (Ranges[left].Last <= other.Ranges[right].Last) { left++; }
                else { right++; }
            }
            return new ScalarSet(result);
        }

        internal ScalarSet SymmetricDifference(ScalarSet other) => Except(other).Union(other.Except(this));

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

    private static readonly IReadOnlyDictionary<string, ScalarSet> AsciiClasses =
        new Dictionary<string, ScalarSet>(StringComparer.Ordinal) {
            ["alnum"] = new(new[] { new Range('0', '9'), new Range('A', 'Z'), new Range('a', 'z') }),
            ["alpha"] = new(new[] { new Range('A', 'Z'), new Range('a', 'z') }),
            ["ascii"] = ScalarSet.Between(0, 0x7F),
            ["blank"] = new(new[] { new Range(9, 9), new Range(0x20, 0x20) }),
            ["cntrl"] = new(new[] { new Range(0, 0x1F), new Range(0x7F, 0x7F) }),
            ["digit"] = ScalarSet.Between('0', '9'),
            ["graph"] = ScalarSet.Between(0x21, 0x7E),
            ["lower"] = ScalarSet.Between('a', 'z'),
            ["print"] = ScalarSet.Between(0x20, 0x7E),
            ["punct"] = new(new[] { new Range(0x21, 0x2F), new Range(0x3A, 0x40), new Range(0x5B, 0x60), new Range(0x7B, 0x7E) }),
            ["space"] = new(new[] { new Range(9, 13), new Range(0x20, 0x20) }),
            ["upper"] = ScalarSet.Between('A', 'Z'),
            ["word"] = new(new[] { new Range('0', '9'), new Range('A', 'Z'), new Range('_', '_'), new Range('a', 'z') }),
            ["xdigit"] = new(new[] { new Range('0', '9'), new Range('A', 'F'), new Range('a', 'f') }),
        };
    private static readonly ConcurrentDictionary<(string Name, bool Folded, bool Complemented), ScalarSet> ModifiedAsciiClasses = new();

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

    private sealed partial class Translator
    {
        private readonly string _source;
        private readonly StringBuilder _output = new();
        private readonly Stack<RegexOptions> _groups = new();
        private readonly Dictionary<(string Source, bool IgnoreCase, bool IgnoreWhitespace), string> _classes = new();
        private readonly Dictionary<(int Scalar, bool IgnoreCase), string> _literals = new();
        private readonly List<(int AnonymousIndex, string? Name)> _captures = new();
        private readonly HashSet<string> _captureNames = new(StringComparer.Ordinal);
        private readonly HashSet<string> _pythonCaptureNames = new(StringComparer.Ordinal);
        private int _anonymousCaptures;
        private bool _hasOperand;
        private RegexOptions _options;
        private int _index;
        private bool _hasWordBoundary;
        internal bool HasWordBoundary => _hasWordBoundary;
        private long _classWork;

        internal Translator(string source, RegexOptions options) { _source = source; _options = options; }
        private bool IgnoreCase => (_options & RegexOptions.IgnoreCase) != 0;

        internal int[] CaptureGroups(Regex regex) => new[] { 0 }.Concat(_captures.Select(capture =>
            capture.Name is null ? capture.AnonymousIndex : regex.GroupNumberFromName(capture.Name))).ToArray();

        private void Capture(string? name = null)
        {
            _captures.Add((name is null ? ++_anonymousCaptures : 0, name));
        }

        private string HostCaptureName(string name, bool python)
        {
            var portable = PortableCaptureName(name);
            if (python && !portable) { throw Invalid("invalid Python capture name"); }
            if (!_captureNames.Add(name) && (python || _pythonCaptureNames.Contains(name)))
            {
                throw Invalid("duplicate Python capture name");
            }
            if (python) { _pythonCaptureNames.Add(name); }
            // Numeric replacements are the only public capture references.
            // Encode supported Rust names into collision-free ASCII host names
            // so dots, brackets and supplementary letters need no extra group.
            var host = portable ? "ferrule" + string.Concat(name.Select(character =>
                ((int)character).ToString("X4", CultureInfo.InvariantCulture))) : name;
            Capture(host);
            return host;
        }

        private static bool PortableCaptureName(string name)
        {
            var first = true;
            foreach (var rune in name.EnumerateRunes())
            {
                var alphabetic = Rune.IsLetter(rune) || Rune.GetUnicodeCategory(rune) == UnicodeCategory.LetterNumber;
                if (!(alphabetic || rune.Value == '_' || !first && (Rune.IsNumber(rune) || rune.Value is '.' or '[' or ']')))
                {
                    return false;
                }
                first = false;
            }
            return !first;
        }

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
                        _hasOperand = true; break;
                    case '[':
                        var start = _index;
                        var set = Class(0);
                        var key = (_source[start.._index], IgnoreCase,
                            (_options & RegexOptions.IgnorePatternWhitespace) != 0);
                        if (!_classes.TryGetValue(key, out var atom)) { atom = Atom(set); _classes.Add(key, atom); }
                        Append(atom); _hasOperand = true; break;
                    case '.':
                        _index++; Append(Atom((_options & RegexOptions.Singleline) != 0
                            ? ScalarSet.All : ScalarSet.All.Except(ScalarSet.Between('\n', '\n')))); _hasOperand = true; break;
                    case '$':
                        _index++; Append((_options & RegexOptions.Multiline) != 0 ? "$" : @"\z"); _hasOperand = true; break;
                    case '\\':
                        EscapeOutside(); _hasOperand = true; break;
                    case '{':
                        Quantifier(); break;
                    case '|':
                        _index++; Append("|"); _hasOperand = false; break;
                    case '*': case '+': case '?':
                        _index++; Append(current.ToString()); break;
                    case '^': case '}':
                        _index++; Append(current.ToString()); _hasOperand = true; break;
                    default:
                        if ((_options & RegexOptions.IgnorePatternWhitespace) != 0 && char.IsWhiteSpace(current))
                        {
                            _index++;
                        }
                        else { Append(Literal(ReadScalar())); _hasOperand = true; }
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
            if (_index == _source.Length || _source[_index] != '?')
            {
                _groups.Push(_options);
                if ((_options & RegexOptions.ExplicitCapture) == 0) { Capture(); }
                Append("("); _hasOperand = false; return;
            }
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
                _options = updated; _index++; Append(_source[start.._index]); _hasOperand = false; return;
            }
            _index = start + 2;
            // Rust/Python named captures share the host angle-bracket header.
            if (_index + 1 < _source.Length && _source[_index] == 'P' && _source[_index + 1] == '<')
            {
                _groups.Push(_options); _index += 2;
                var nameStart = _index;
                while (_index < _source.Length && _source[_index] != '>') { _index++; }
                if (_index == _source.Length) { throw Invalid("unterminated capture name"); }
                var name = HostCaptureName(_source[nameStart.._index++], true);
                Append("(?<" + name + ">"); _hasOperand = false; return;
            }
            _groups.Push(_options);
            if (_index < _source.Length && _source[_index] is ':' or '=' or '!' or '>') { _index++; }
            else if (_index < _source.Length && _source[_index] is '<' or '\'')
            {
                var closer = _source[_index++] == '<' ? '>' : '\'';
                if (_index < _source.Length && _source[_index] is '=' or '!') { _index++; }
                else
                {
                    var nameStart = _index;
                    while (_index < _source.Length && _source[_index] != closer) { _index++; }
                    var name = HostCaptureName(_source[nameStart.._index], false);
                    if (_index == _source.Length) { throw Invalid("unterminated capture name"); }
                    _index++;
                    Append("(?<" + name + ">"); _hasOperand = false; return;
                }
            }
            Append(_source[start.._index]); _hasOperand = false;
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

        private void Quantifier()
        {
            if (!_hasOperand) { throw Invalid("repetition quantifier has no operand"); }
            _index++; SkipIgnored();
            var minimum = Decimal();
            uint? maximum = minimum;
            var comma = _index < _source.Length && _source[_index] == ',';
            if (comma)
            {
                _index++; SkipIgnored();
                maximum = _index < _source.Length && _source[_index] == '}' ? null : Decimal();
            }
            if (_index == _source.Length || _source[_index] != '}' || maximum.HasValue && minimum > maximum)
            {
                throw Invalid("malformed repetition quantifier");
            }
            _index++;
            Append("{" + minimum.ToString(CultureInfo.InvariantCulture)
                + (comma ? "," + maximum?.ToString(CultureInfo.InvariantCulture) : string.Empty) + "}");
        }

        private uint Decimal()
        {
            while (_index < _source.Length && char.IsWhiteSpace(_source[_index])) { _index++; }
            var present = false;
            uint value = 0;
            while (_index < _source.Length && _source[_index] is >= '0' and <= '9')
            {
                present = true;
                var digit = (uint)(_source[_index++] - '0');
                if (value > (uint.MaxValue - digit) / 10) { throw Invalid("repetition count exceeds its integer bound"); }
                value = value * 10 + digit;
                SkipIgnored();
            }
            while (_index < _source.Length && char.IsWhiteSpace(_source[_index])) { _index++; SkipIgnored(); }
            if (!present) { throw Invalid("missing repetition count"); }
            return value;
        }

        private void EscapeOutside()
        {
            var start = _index++;
            if (_index >= _source.Length) { throw Invalid("trailing backslash"); }
            var escape = _source[_index];
            if (escape is 'b' or 'B') { _hasWordBoundary = true; }
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
                'x' or 'u' or 'U' => EscapedHex(kind), 'c' => Control(),
                >= '0' and <= '7' => Octal(kind),
                _ when !char.IsLetterOrDigit(kind) => kind,
                _ => throw Invalid("unrecognized escape"),
            };
            if (!Rune.IsValid(scalarValue)) { throw Invalid("escape is not a Unicode scalar"); }
            return new Item(ScalarSet.Between(scalarValue, scalarValue), scalarValue);
        }

        private ScalarSet? AsciiClass()
        {
            if (_index + 1 >= _source.Length || _source[_index + 1] != ':') { return null; }
            var next = _index + 2;
            var complemented = next < _source.Length && _source[next] == '^';
            if (complemented) { next++; }
            var start = next;
            // Known names have at most six ASCII letters; malformed prefixes
            // cannot trigger repeated scans over the remainder of the pattern.
            while (next < _source.Length && next - start < 6 && _source[next] is >= 'a' and <= 'z') { next++; }
            if (next + 1 >= _source.Length || _source[next] != ':' || _source[next + 1] != ']') { return null; }
            var name = _source[start..next];
            if (!AsciiClasses.ContainsKey(name)) { return null; }
            _index = next + 2;
            ClassWork(1);
            return ModifiedAsciiClasses.GetOrAdd((name, IgnoreCase, complemented), static key => {
                var set = AsciiClasses[key.Name];
                if (key.Folded) { set = set.FoldCase(); }
                return key.Complemented ? set.Complement() : set;
            });
        }

        private void ClassWork(long amount)
        {
            if (amount > MaximumClassWork - _classWork)
            {
                throw Invalid("character-class compilation exceeds its bounded work limit");
            }
            _classWork += amount;
        }

        private ScalarSet Class(int depth)
        {
            if (depth > MaximumClassDepth) { throw Invalid("character-class nesting exceeds its bounded depth"); }
            ClassWork(1);
            _index++; SkipIgnored();
            var complemented = _index < _source.Length && _source[_index] == '^';
            if (complemented) { _index++; SkipIgnored(); }
            var scalarRanges = new List<Range>();
            var composites = new HashSet<ScalarSet>();
            ScalarSet TakeUnion()
            {
                var count = scalarRanges.Count + composites.Sum(item => item.Ranges.Length);
                // Charge the normalization sort and scalar-fold membership work
                // before allocating the combined interval set.
                ClassWork((long)count * (1 + (int)Math.Log2(Math.Max(count, 1))));
                var scalars = new ScalarSet(scalarRanges);
                if (IgnoreCase && scalars.Ranges.Length != 0)
                {
                    ClassWork(CaseTables.Value.Groups.Sum(group => (long)group.Length)
                        * (1 + (int)Math.Log2(Math.Max(scalars.Ranges.Length, 1))));
                    scalars = scalars.FoldCase();
                }
                var combined = new ScalarSet(scalars.Ranges.Concat(composites.SelectMany(item => item.Ranges)));
                scalarRanges.Clear(); composites.Clear();
                return combined;
            }
            ScalarSet Apply(ScalarSet left, ScalarSet right, char operation)
            {
                var count = (long)left.Ranges.Length + right.Ranges.Length;
                ClassWork(count * (operation == '~' ? 3 : 1) * (1 + (int)Math.Log2(Math.Max(count, 1))));
                return operation switch {
                    '&' => left.Intersect(right), '-' => left.Except(right),
                    '~' => left.SymmetricDifference(right), _ => right,
                };
            }
            // Rust treats opening dashes and the first closing bracket literally.
            while (_index < _source.Length && _source[_index] == '-')
            {
                scalarRanges.Add(new Range('-', '-')); _index++; SkipIgnored(); ClassWork(1);
            }
            if (scalarRanges.Count == 0 && _index < _source.Length && _source[_index] == ']')
            {
                scalarRanges.Add(new Range(']', ']')); _index++; SkipIgnored();
            }
            ScalarSet? accumulated = null;
            var operation = '\0';
            while (_index < _source.Length)
            {
                SkipIgnored();
                if (_index == _source.Length) { break; }
                ClassWork(1);
                if (_source[_index] == ']')
                {
                    _index++;
                    var finalUnion = TakeUnion();
                    var result = accumulated is null ? finalUnion : Apply(accumulated, finalUnion, operation);
                    return complemented ? result.Complement() : result;
                }
                if (_index + 1 < _source.Length && _source[_index] is '&' or '-' or '~'
                    && _source[_index + 1] == _source[_index])
                {
                    var next = _source[_index]; _index += 2;
                    var union = TakeUnion();
                    accumulated = accumulated is null ? union : Apply(accumulated, union, operation);
                    operation = next; continue;
                }
                // Retain the previously accepted single-dash host subtraction.
                if (_source[_index] == '-' && (accumulated is not null || scalarRanges.Count != 0 || composites.Count != 0))
                {
                    var dash = _index++;
                    SkipIgnored();
                    if (_index < _source.Length && _source[_index] == '[')
                    {
                        var excluded = Class(depth + 1);
                        SkipIgnored();
                        if (_index == _source.Length || _source[_index++] != ']') { throw Invalid("subtraction must end the class"); }
                        var union = TakeUnion();
                        var result = accumulated is null ? union : Apply(accumulated, union, operation);
                        if (complemented) { result = result.Complement(); }
                        return Apply(result, excluded, '-');
                    }
                    _index = dash;
                }
                if (_source[_index] == '[')
                {
                    composites.Add(AsciiClass() ?? Class(depth + 1)); continue;
                }
                var item = ClassItem();
                SkipIgnored();
                if (_index < _source.Length && _source[_index] == '-')
                {
                    var dash = _index++;
                    SkipIgnored();
                    if (_index < _source.Length && _source[_index] is not (']' or '[' or '-'))
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


        private int ReadScalar()
        {
            var rune = Rune.GetRuneAt(_source, _index); _index += rune.Utf16SequenceLength; return rune.Value;
        }

        private int EscapedHex(char kind)
        {
            SkipIgnored();
            if (_index < _source.Length && _source[_index] == '{') { return BracedScalar(); }
            return kind switch { 'x' => Hex(2), 'u' => HexScalar(), _ => Hex(8) };
        }

        private int BracedScalar()
        {
            _index++; SkipIgnored();
            var present = false;
            var value = 0;
            while (_index < _source.Length && _source[_index] != '}')
            {
                present = true;
                var current = _source[_index++];
                var digit = current switch {
                    >= '0' and <= '9' => current - '0', >= 'a' and <= 'f' => current - 'a' + 10,
                    >= 'A' and <= 'F' => current - 'A' + 10, _ => throw Invalid("invalid braced hex escape"),
                };
                if (value > (0x10FFFF - digit) / 16) { throw Invalid("escape exceeds the Unicode scalar range"); }
                value = value * 16 + digit;
                SkipIgnored();
            }
            if (!present || _index == _source.Length) { throw Invalid("incomplete braced hex escape"); }
            _index++;
            if (!Rune.IsValid(value)) { throw Invalid("escape is not a Unicode scalar"); }
            return value;
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
            long value = 0;
            for (var digit = 0; digit < digits; digit++)
            {
                SkipIgnored();
                if (_index == _source.Length) { throw Invalid("incomplete hex escape"); }
                var current = _source[_index++];
                var number = current switch {
                    >= '0' and <= '9' => current - '0', >= 'a' and <= 'f' => current - 'a' + 10,
                    >= 'A' and <= 'F' => current - 'A' + 10, _ => throw Invalid("invalid hex escape"),
                };
                value = value * 16 + number;
            }
            if (value > int.MaxValue) { throw Invalid("escape exceeds the Unicode scalar range"); }
            return (int)value;
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
