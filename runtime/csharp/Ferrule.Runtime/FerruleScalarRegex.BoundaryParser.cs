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
        bool IsNullable = true);

    private sealed partial class Translator
    {
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
            var nullable = node.Kind switch {
                BoundaryKind.Consume => false,
                BoundaryKind.Sequence => node.Children!.All(child => child.IsNullable),
                BoundaryKind.Alternate => node.Children!.Any(child => child.IsNullable),
                BoundaryKind.Capture => node.Children![0].IsNullable,
                BoundaryKind.Repeat => node.Minimum == 0 || node.Children![0].IsNullable,
                _ => true,
            };
            return node with { Height = height, IsNullable = nullable };
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
            while (true)
            {
                SkipIgnored();
                if (_index == _source.Length || _source[_index] is ')' or '|') { break; }
                var atom = BoundaryAtom();
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
            return items.Count switch {
                0 => BoundaryNew(new(BoundaryKind.Empty)),
                1 => items[0],
                _ => BoundaryNew(new(BoundaryKind.Sequence, Children: items.ToArray())),
            };
        }

        private BoundaryNode? BoundaryAtom()
        {
            var kind = _source[_index];
            switch (kind)
            {
                case '(':
                    return BoundaryGroup();
                case '[':
                    return BoundaryNew(new(BoundaryKind.Consume, Set: Class(0)));
                case '.':
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
                    var item = ClassItem();
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
            return IgnoreCase && CaseTables.Value.ByScalar.TryGetValue(scalar, out var group)
                ? new ScalarSet(group.Select(value => new Range(value, value)))
                : ScalarSet.Between(scalar, scalar);
        }

        private BoundaryNode? BoundaryGroup()
        {
            _index++;
            var saved = _options;
            var capture = 0;
            if (_index < _source.Length && _source[_index] == '?')
            {
                _index++;
                if (_index < _source.Length && _source[_index] == '#')
                {
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
                    var enabled = true;
                    var hasFlag = false;
                    while (_index < _source.Length)
                    {
                        var flag = _source[_index];
                        if (flag == '-') { enabled = false; _index++; continue; }
                        var option = flag switch {
                            'i' => RegexOptions.IgnoreCase, 'm' => RegexOptions.Multiline,
                            's' => RegexOptions.Singleline, 'x' => RegexOptions.IgnorePatternWhitespace,
                            'n' => RegexOptions.ExplicitCapture, _ => RegexOptions.None,
                        };
                        if (option == RegexOptions.None) { break; }
                        hasFlag = true; _options = enabled ? _options | option : _options & ~option; _index++;
                    }
                    if (!hasFlag || _index == _source.Length || _source[_index] is not (':' or ')'))
                    {
                        throw Invalid("host-only group is unsupported with word assertions");
                    }
                    if (_source[_index++] == ')') { return null; }
                }
            }
            else if ((_options & RegexOptions.ExplicitCapture) == 0) { capture = ++_boundaryCaptures; }
            var body = BoundaryExpression();
            SkipIgnored();
            if (_index == _source.Length || _source[_index++] != ')') { throw Invalid("unterminated regex group"); }
            _options = saved;
            return capture == 0 ? body : BoundaryNew(new(BoundaryKind.Capture,
                Children: new[] { body }, Capture: capture));
        }
    }
}
