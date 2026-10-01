using System.Text;
using System.Text.RegularExpressions;

namespace Ferrule.Runtime;

internal static partial class FerruleScalarRegex
{
    private const int MaximumBoundaryAstNodes = 8192;
    private const int MaximumBoundaryDepth = 256;

    private enum BoundaryKind { Empty, Consume, Assertion, Sequence, Alternate, Repeat, Capture }
    private enum BoundaryAssertion {
        Start, End, LineStart, LineEnd, Word, NotWord,
        WordStart, WordEnd, WordStartHalf, WordEndHalf,
        AsciiWord, AsciiNotWord, AsciiWordStart, AsciiWordEnd, AsciiWordStartHalf, AsciiWordEndHalf,
    }

    private sealed record BoundaryNode(
        BoundaryKind Kind,
        ScalarSet? Set = null,
        BoundaryAssertion Assertion = default,
        BoundaryNode[]? Children = null,
        uint Minimum = 0,
        uint? Maximum = 0,
        bool Lazy = false,
        int Capture = 0,
        int Height = 0,
        int SyntaxHeight = 0,
        bool IsNullable = true,
        bool HasCapture = false,
        bool HasNullableCaptureLoop = false);

    private sealed partial class Translator
    {
        // Identify actual flag headers before choosing a grammar. Escaped text,
        // bracket classes and retained host comments cannot select the profile.
        internal bool ContainsExplicitUnicode()
        {
            var saved = new Stack<RegexOptions>();
            while (_index < _source.Length)
            {
                SkipIgnored();
                if (_index == _source.Length) { break; }
                var kind = _source[_index++];
                if (kind == '\\') { if (_index < _source.Length) { _index++; } continue; }
                if (kind == '[')
                {
                    var opening = new Stack<int>(); opening.Push(0);
                    while (_index < _source.Length && opening.Count != 0)
                    {
                        SkipIgnored();
                        if (_index == _source.Length) { break; }
                        kind = _source[_index++];
                        var first = opening.Pop();
                        if (kind == '\\')
                        { if (_index < _source.Length) { _index++; } opening.Push(2); }
                        else if (kind == '[') { opening.Push(2); opening.Push(0); }
                        else if (kind == ']' && first == 2) { }
                        else { opening.Push(kind == '^' && first == 0 ? 1 : 2); }
                    }
                    continue;
                }
                if (kind == ')') { if (saved.TryPop(out var parent)) { _options = parent; } continue; }
                if (kind != '(') { continue; }
                // Rust x mode skips ignored source after the opening parenthesis
                // before identifying a group header, using the enclosing mode.
                SkipIgnored();
                if (_index == _source.Length || _source[_index] != '?') { saved.Push(_options); continue; }
                _index++;
                if (_index < _source.Length && _source[_index] == '#')
                {
                    while (_index < _source.Length && _source[_index] != ')') { _index++; }
                    if (_index < _source.Length) { _index++; }
                    continue;
                }
                if (ReadFlags(out var updated, out _, out var scoped, out _))
                {
                    if (_hasExplicitUnicode) { return true; }
                    if (scoped) { saved.Push(_options); }
                    _options = updated;
                }
                else { saved.Push(_options); }
            }
            return _hasExplicitUnicode;
        }


        private bool ReadFlags(out RegexOptions options, out bool unicode, out bool scoped, out string hostFlags)
        {
            options = _options; unicode = _unicode; scoped = false; hostFlags = string.Empty;
            var start = _index;
            var enabled = true;
            var seen = new HashSet<char>();
            var duplicate = false;
            var dashed = false;
            var malformedDash = false;
            var afterDash = false;
            var hasFlag = false;
            var hasUnicode = false;
            var hasHostCapture = false;
            var host = new StringBuilder(6);
            while (_index < _source.Length)
            {
                var flag = _source[_index];
                if (flag == '-')
                {
                    malformedDash |= dashed; dashed = true; enabled = false;
                    host.Append(flag); _index++; continue;
                }
                var option = flag switch {
                    'i' => RegexOptions.IgnoreCase, 'm' => RegexOptions.Multiline,
                    's' => RegexOptions.Singleline, 'x' => RegexOptions.IgnorePatternWhitespace,
                    'n' => RegexOptions.ExplicitCapture, _ => RegexOptions.None,
                };
                if (flag != 'u' && option == RegexOptions.None) { break; }
                hasFlag = true; afterDash |= dashed; duplicate |= !seen.Add(flag);
                if (flag == 'u') { hasUnicode = true; unicode = enabled; }
                else
                {
                    hasHostCapture |= flag == 'n'; host.Append(flag);
                    options = enabled ? options | option : options & ~option;
                }
                _index++;
            }
            _hasExplicitUnicode |= hasUnicode;
            if (!hasFlag || _index == _source.Length || _source[_index] is not (':' or ')'))
            { _index = start; return false; }
            if (duplicate || malformedDash || dashed && !afterDash || hasHostCapture)
            { _captureRoutingEligible = false; }
            if ((_strictUnicodeProfile || hasUnicode) && (duplicate || malformedDash || dashed && !afterDash || hasHostCapture))
            { throw Invalid("invalid explicit Unicode flag header"); }
            scoped = _source[_index++] == ':';
            hostFlags = host.ToString().TrimEnd('-');
            return true;
        }

        private bool TryReadWordAssertion(out BoundaryAssertion assertion)
        {
            assertion = default;
            if (_index + 1 >= _source.Length || _source[_index] != '\\') { return false; }
            var escape = _source[_index + 1];
            switch (escape)
            {
                case 'b': assertion = BoundaryAssertion.Word; break;
                case 'B': assertion = BoundaryAssertion.NotWord; break;
                case '<': assertion = BoundaryAssertion.WordStart; break;
                case '>': assertion = BoundaryAssertion.WordEnd; break;
                default: return false;
            }
            _index += 2;
            // Only an immediately adjacent brace can name a special assertion.
            // Ignored x whitespace/comments are allowed inside that brace.
            if (escape != 'b' || _index == _source.Length || _source[_index] != '{')
            { assertion = WordMode(assertion); return true; }
            var brace = _index++;
            SkipIgnored();
            if (_index == _source.Length) { throw Invalid("unterminated special word assertion"); }
            static bool NameCharacter(char value) => value is >= 'a' and <= 'z' or >= 'A' and <= 'Z' or '-';
            if (!NameCharacter(_source[_index]))
            {
                // Numeric braces belong to the ordinary repetition reader.
                _index = brace; assertion = WordMode(assertion); return true;
            }
            var name = new StringBuilder(10);
            while (_index < _source.Length && NameCharacter(_source[_index]))
            {
                if (name.Length == 10) { throw Invalid("unrecognized special word assertion"); }
                name.Append(_source[_index++]); SkipIgnored();
            }
            if (_index == _source.Length || _source[_index++] != '}')
            {
                throw Invalid("unterminated special word assertion");
            }
            assertion = name.ToString() switch {
                "start" => BoundaryAssertion.WordStart,
                "end" => BoundaryAssertion.WordEnd,
                "start-half" => BoundaryAssertion.WordStartHalf,
                "end-half" => BoundaryAssertion.WordEndHalf,
                _ => throw Invalid("unrecognized special word assertion"),
            };
            assertion = WordMode(assertion);
            return true;
        }

        private BoundaryAssertion WordMode(BoundaryAssertion assertion) => _unicode ? assertion : assertion switch {
            BoundaryAssertion.Word => BoundaryAssertion.AsciiWord,
            BoundaryAssertion.NotWord => BoundaryAssertion.AsciiNotWord,
            BoundaryAssertion.WordStart => BoundaryAssertion.AsciiWordStart,
            BoundaryAssertion.WordEnd => BoundaryAssertion.AsciiWordEnd,
            BoundaryAssertion.WordStartHalf => BoundaryAssertion.AsciiWordStartHalf,
            BoundaryAssertion.WordEndHalf => BoundaryAssertion.AsciiWordEndHalf,
            _ => throw Invalid("invalid Unicode-disabled word assertion"),
        };

        private int _boundaryDepth;
        private int _boundaryNodes;
        private int _boundaryCaptures;
        private readonly HashSet<string> _boundaryCaptureNames = new(StringComparer.Ordinal);

        internal (BoundaryNode Root, int Captures) ParseBoundary()
        {
            var root = BoundaryExpression();
            SkipIgnored();
            if (_index != _source.Length) { throw Invalid("unmatched closing parenthesis"); }
            return (root, _boundaryCaptures);
        }

        private BoundaryNode BoundaryNew(BoundaryNode node)
        {
            if (++_boundaryNodes > MaximumBoundaryAstNodes)
            {
                throw Invalid("word-boundary regex exceeds its AST limit");
            }
            // Source group depth alone does not bound nested repetitions and
            // other structural nodes. Compute height bottom-up before either
            // recursive compiler pass can visit the completed tree.
            var height = node.Children is null ? 0 : 1 + node.Children.Max(child => child.Height);
            if (height > MaximumBoundaryDepth)
            {
                throw Invalid("word-boundary regex exceeds its structural nesting limit");
            }
            var syntaxHeight = Math.Max(node.SyntaxHeight,
                node.Children is null ? 0 : 1 + node.Children.Max(child => child.SyntaxHeight));
            if (_strictUnicodeProfile && syntaxHeight > 250)
            { throw Invalid("explicit Unicode regex exceeds the Rust syntax nesting limit"); }
            var nullable = node.Kind switch {
                BoundaryKind.Consume => false,
                BoundaryKind.Sequence => node.Children!.All(child => child.IsNullable),
                BoundaryKind.Alternate => node.Children!.Any(child => child.IsNullable),
                BoundaryKind.Capture => node.Children![0].IsNullable,
                BoundaryKind.Repeat => node.Minimum == 0 || node.Children![0].IsNullable,
                _ => true,
            };
            var capture = node.Kind == BoundaryKind.Capture
                || node.Children?.Any(child => child.HasCapture) == true;
            var captureLoop = node.Children?.Any(child => child.HasNullableCaptureLoop) == true;
            if (node.Kind == BoundaryKind.Repeat)
            {
                // A zero outer count cannot execute its captured inner loop.
                if (node.Maximum == 0) { captureLoop = false; }
                else if (!node.Maximum.HasValue && node.Children![0].IsNullable && node.Children[0].HasCapture)
                {
                    captureLoop = true;
                }
            }
            return node with { Height = height, SyntaxHeight = syntaxHeight, IsNullable = nullable,
                HasCapture = capture, HasNullableCaptureLoop = captureLoop };
        }

        private BoundaryNode BoundaryExpression()
        {
            if (++_boundaryDepth > MaximumBoundaryDepth)
            {
                throw Invalid("word-boundary regex exceeds its nesting limit");
            }
            var branches = new List<BoundaryNode> { BoundarySequence() };
            SkipIgnored();
            while (_index < _source.Length && _source[_index] == '|')
            {
                _index++;
                branches.Add(BoundarySequence());
                SkipIgnored();
            }
            _boundaryDepth--;
            return branches.Count == 1 ? branches[0] : BoundaryNew(new(
                BoundaryKind.Alternate, Children: branches.ToArray()));
        }

        private BoundaryNode BoundarySequence()
        {
            var items = new List<BoundaryNode>();
            var syntaxItems = 0;
            while (true)
            {
                SkipIgnored();
                if (_index == _source.Length || _source[_index] is ')' or '|') { break; }
                var atom = BoundaryAtom();
                syntaxItems++;
                if (atom is null) { continue; }
                SkipIgnored();
                while (_index < _source.Length && _source[_index] is '*' or '+' or '?' or '{')
                {
                    var repetition = ReadRepetition();
                    atom = BoundaryNew(new(BoundaryKind.Repeat, Children: new[] { atom },
                        Minimum: repetition.Minimum, Maximum: repetition.Maximum, Lazy: repetition.Lazy));
                    SkipIgnored();
                }
                items.Add(atom);
            }
            var result = items.Count switch {
                0 => BoundaryNew(new(BoundaryKind.Empty)),
                1 => items[0],
                _ => BoundaryNew(new(BoundaryKind.Sequence, Children: items.ToArray())),
            };
            // Global directives remain source AST nodes without adding VM work.
            var syntaxHeight = (items.Count == 0 ? 0 : items.Max(item => item.SyntaxHeight)) + (syntaxItems > 1 ? 1 : 0);
            if (_strictUnicodeProfile && syntaxHeight > 250)
            { throw Invalid("explicit Unicode regex exceeds the Rust syntax nesting limit"); }
            return result with { SyntaxHeight = syntaxHeight };

        }

        private BoundaryNode? BoundaryAtom()
        {
            var kind = _source[_index];
            switch (kind)
            {
                case '(':
                    return BoundaryGroup();
                case '[':
                    var set = Class(0);
                    return BoundaryNew(new(BoundaryKind.Consume, Set: set, SyntaxHeight: _lastClassSyntaxHeight));
                case '.':
                    if (!_unicode) { throw Invalid("Unicode-disabled dot can match invalid UTF-8"); }
                    _index++;
                    return BoundaryNew(new(BoundaryKind.Consume, Set:
                        (_options & RegexOptions.Singleline) != 0 ? ScalarSet.All
                            : ScalarSet.All.Except(ScalarSet.Between('\n', '\n'))));
                case '^': case '$':
                    _index++;
                    var multiline = (_options & RegexOptions.Multiline) != 0;
                    return BoundaryNew(new(BoundaryKind.Assertion, Assertion: kind == '^'
                        ? (multiline ? BoundaryAssertion.LineStart : BoundaryAssertion.Start)
                        : (multiline ? BoundaryAssertion.LineEnd : BoundaryAssertion.End)));
                case '\\':
                    if (_index + 1 == _source.Length) { throw Invalid("trailing backslash"); }
                    if (TryReadWordAssertion(out var wordAssertion))
                    {
                        return BoundaryNew(new(BoundaryKind.Assertion, Assertion: wordAssertion));
                    }
                    var escape = _source[_index + 1];
                    if (escape is 'A' or 'z')
                    {
                        _index += 2;
                        return BoundaryNew(new(BoundaryKind.Assertion, Assertion: escape switch {
                            'A' => BoundaryAssertion.Start, _ => BoundaryAssertion.End,
                        }));
                    }
                    if (escape is 'Z' or 'G' or 'k' or >= '1' and <= '9')
                    {
                        throw Invalid("host-only escape is unsupported with word assertions");
                    }
                    var item = ClassItem(false);
                    return BoundaryNew(new(BoundaryKind.Consume,
                        Set: item.Scalar.HasValue ? BoundaryLiteral(item.Scalar.Value) : item.Set));
                case '*': case '+': case '?': case '{':
                    throw Invalid("repetition quantifier has no operand");
                default:
                    return BoundaryNew(new(BoundaryKind.Consume, Set: BoundaryLiteral(ReadScalar())));
            }
        }

        private ScalarSet BoundaryLiteral(int scalar)
        {
            var set = ScalarSet.Between(scalar, scalar);
            if (!IgnoreCase) { return set; }
            return !_unicode ? set.FoldAsciiCase() : CaseTables.Value.ByScalar.TryGetValue(scalar, out var group)
                ? new ScalarSet(group.Select(value => new Range(value, value))) : set;
        }

        private BoundaryNode? BoundaryGroup()
        {
            _index++;
            // Only the explicit Unicode profile adopts Rust's x-mode group
            // header spacing. Flags in this group take effect after its header.
            if (_strictUnicodeProfile) { SkipIgnored(); }
            var saved = _options;
            var savedUnicode = _unicode;
            var capture = 0;
            if (_index < _source.Length && _source[_index] == '?')
            {
                _index++;
                if (_index < _source.Length && _source[_index] == '#')
                {
                    if (_strictUnicodeProfile) { throw Invalid("comment groups are not in the explicit Unicode profile"); }
                    while (_index < _source.Length && _source[_index] != ')') { _index++; }
                    if (_index == _source.Length) { throw Invalid("unterminated regex comment"); }
                    _index++; return null;
                }
                if (_index < _source.Length && _source[_index] == ':') { _index++; }
                else if (_index < _source.Length && (_source[_index] == '<'
                    || _source[_index] == 'P' && _index + 1 < _source.Length && _source[_index + 1] == '<'))
                {
                    _index += _source[_index] == 'P' ? 2 : 1;
                    var start = _index;
                    while (_index < _source.Length && _source[_index] != '>') { _index++; }
                    if (_index == _source.Length) { throw Invalid("unterminated capture name"); }
                    var name = _source[start.._index++];
                    if (!PortableCaptureName(name) || !_boundaryCaptureNames.Add(name))
                    {
                        throw Invalid("invalid or duplicate capture name with word assertions");
                    }
                    capture = ++_boundaryCaptures;
                }
                else
                {
                    if (!ReadFlags(out var updated, out var unicode, out var scoped, out _))
                    { throw Invalid("host-only group is unsupported with word assertions"); }
                    _options = updated; _unicode = unicode;
                    if (!scoped) { return null; }
                }
            }
            else if ((_options & RegexOptions.ExplicitCapture) == 0) { capture = ++_boundaryCaptures; }
            var body = BoundaryExpression();
            SkipIgnored();
            if (_index == _source.Length || _source[_index++] != ')') { throw Invalid("unterminated regex group"); }
            _options = saved; _unicode = savedUnicode;
            if (capture != 0) { return BoundaryNew(new(BoundaryKind.Capture, Children: new[] { body }, Capture: capture)); }
            var syntaxHeight = 1 + body.SyntaxHeight;
            if (_strictUnicodeProfile && syntaxHeight > 250)
            { throw Invalid("explicit Unicode regex exceeds the Rust syntax nesting limit"); }
            return body with { SyntaxHeight = syntaxHeight };
        }
    }
}
