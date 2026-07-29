using System.Buffers;
using System.Text;

namespace Ferrule.Runtime;

/// <summary>
/// A bounded, host-independent matcher for Ferrule's portable JSON Schema
/// pattern subset.
/// </summary>
internal sealed class FerruleJsonPattern
{
    internal const int MaximumSourceBytes = 64 * 1024;
    internal const int MaximumParserDepth = 256;
    internal const int MaximumAstNodes = 8 * 1024;
    internal const int MaximumInstructions = 16 * 1024;
    internal const ulong MaximumBoundaryWork = 100_000_000;

    private readonly Instruction[] _instructions;
    private readonly int _start;

    private FerruleJsonPattern(Instruction[] instructions, int start)
    {
        _instructions = instructions;
        _start = start;
    }

    internal int InstructionCount => _instructions.Length;

    internal static FerruleJsonPattern Compile(string source)
    {
        ArgumentNullException.ThrowIfNull(source);
        var parser = new Parser(source);
        var expression = parser.Parse();
        return new Compiler().Compile(expression);
    }

    internal bool IsMatch(string value, ref ulong remainingWork)
    {
        ArgumentNullException.ThrowIfNull(value);
        var input = ScanInput(value);
        var scalarCount = Math.Max(1UL, checked((ulong)input.ScalarCount));
        var work = checked(scalarCount * (ulong)_instructions.Length);
        if (work > remainingWork)
        {
            throw Boundary(
                $"JSON pattern match requires {work} work units; " +
                $"only {remainingWork} remain from the {MaximumBoundaryWork}-unit bounded work limit.");
        }
        remainingWork -= work;

        var active = new StateSet(_instructions.Length);
        var next = new StateSet(_instructions.Length);
        AddClosure(active, _start, 0, input);
        if (active.ContainsMatch(_instructions))
        {
            return true;
        }

        var remaining = value.AsSpan();
        var position = 0;
        while (!remaining.IsEmpty)
        {
            var status = Rune.DecodeFromUtf16(
                remaining,
                out var rune,
                out var charsConsumed);
            if (status != OperationStatus.Done)
            {
                throw Boundary("JSON string contains an unpaired UTF-16 surrogate.");
            }
            next.Reset();
            foreach (var state in active.States)
            {
                var instruction = _instructions[state];
                if (instruction.Operation == Operation.Consume &&
                    instruction.CharacterSet is not null &&
                    instruction.CharacterSet.Contains(rune))
                {
                    AddClosure(next, instruction.First, position + 1, input);
                }
            }

            position++;
            remaining = remaining[charsConsumed..];
            AddClosure(next, _start, position, input);
            if (next.ContainsMatch(_instructions))
            {
                return true;
            }
            (active, next) = (next, active);
        }

        return false;
    }

    private void AddClosure(StateSet states, int initial, int position, InputInfo input)
    {
        states.PushPending(initial);
        while (states.TryPopPending(out var state))
        {
            if (!states.TryVisit(state))
            {
                continue;
            }

            var instruction = _instructions[state];
            switch (instruction.Operation)
            {
                case Operation.Split:
                    states.PushPending(instruction.Second);
                    states.PushPending(instruction.First);
                    break;
                case Operation.Jump:
                    states.PushPending(instruction.First);
                    break;
                case Operation.AssertStart:
                    if (position == 0)
                    {
                        states.PushPending(instruction.First);
                    }
                    break;
                case Operation.AssertEnd:
                    if (IsEndPosition(position, input))
                    {
                        states.PushPending(instruction.First);
                    }
                    break;
                case Operation.Consume:
                case Operation.Match:
                    states.Add(state);
                    break;
                default:
                    throw new InvalidOperationException("Unknown JSON pattern instruction.");
            }
        }
    }

    private static bool IsEndPosition(int position, InputInfo input)
    {
        return position == input.ScalarCount ||
               input.FinalLineTerminatorStart == position;
    }

    private static bool IsLineTerminator(int value) =>
        value is '\n' or '\r' or 0x2028 or 0x2029;

    private static InputInfo ScanInput(string value)
    {
        var count = 0;
        int? previous = null;
        int? last = null;
        var remaining = value.AsSpan();
        while (!remaining.IsEmpty)
        {
            var status = Rune.DecodeFromUtf16(
                remaining,
                out var rune,
                out var charsConsumed);
            if (status != OperationStatus.Done)
            {
                throw Boundary("JSON string contains an unpaired UTF-16 surrogate.");
            }
            count = checked(count + 1);
            previous = last;
            last = rune.Value;
            remaining = remaining[charsConsumed..];
        }

        int? finalLineTerminatorStart = null;
        if (last == '\n' && previous == '\r')
        {
            finalLineTerminatorStart = count - 2;
        }
        else if (last is not null && IsLineTerminator(last.Value))
        {
            finalLineTerminatorStart = count - 1;
        }
        return new InputInfo(count, finalLineTerminatorStart);
    }

    private static Rune[] DecodePatternScalars(string value)
    {
        var result = new List<Rune>(value.Length);
        var remaining = value.AsSpan();
        while (!remaining.IsEmpty)
        {
            var status = Rune.DecodeFromUtf16(
                remaining,
                out var rune,
                out var charsConsumed);
            if (status != OperationStatus.Done)
            {
                throw Boundary("JSON pattern contains an unpaired UTF-16 surrogate.");
            }
            result.Add(rune);
            remaining = remaining[charsConsumed..];
        }
        return result.ToArray();
    }

    private readonly record struct InputInfo(
        int ScalarCount,
        int? FinalLineTerminatorStart);

    private static FerruleRuntimeException Boundary(string message) =>
        new(FerruleRuntimeError.JsonBoundary, message, detail: message);

    private abstract record Expression;

    private sealed record EmptyExpression : Expression;

    private sealed record CharacterExpression(CharacterSet CharacterSet) : Expression;

    private sealed record StartExpression : Expression;

    private sealed record EndExpression : Expression;

    private sealed record GroupExpression(Expression Item) : Expression;

    private sealed record SequenceExpression(IReadOnlyList<Expression> Items) : Expression;

    private sealed record AlternationExpression(IReadOnlyList<Expression> Branches) : Expression;

    private sealed record RepeatExpression(
        Expression Item,
        int Minimum,
        int? Maximum) : Expression;

    private readonly record struct ScalarRange(int Start, int End);

    private sealed class CharacterSet
    {
        private readonly ScalarRange[] _ranges;

        internal CharacterSet(IEnumerable<ScalarRange> ranges, bool complemented)
        {
            _ranges = MergeRanges(ranges);
            Complemented = complemented;
        }

        internal bool Complemented { get; }

        internal bool Contains(Rune value)
        {
            var scalar = value.Value;
            var low = 0;
            var high = _ranges.Length - 1;
            while (low <= high)
            {
                var middle = low + ((high - low) / 2);
                var range = _ranges[middle];
                if (scalar < range.Start)
                {
                    high = middle - 1;
                }
                else if (scalar > range.End)
                {
                    low = middle + 1;
                }
                else
                {
                    return !Complemented;
                }
            }
            return Complemented;
        }

        internal static CharacterSet Single(Rune value) =>
            new([new ScalarRange(value.Value, value.Value)], false);

        internal static CharacterSet Dot() =>
            new(
                [
                    new ScalarRange('\n', '\n'),
                    new ScalarRange('\r', '\r'),
                    new ScalarRange(0x2028, 0x2029),
                ],
                true);

        private static ScalarRange[] MergeRanges(IEnumerable<ScalarRange> source)
        {
            var ordered = source
                .OrderBy(range => range.Start)
                .ThenBy(range => range.End)
                .ToArray();
            if (ordered.Length <= 1)
            {
                return ordered;
            }

            var merged = new List<ScalarRange>(ordered.Length);
            var current = ordered[0];
            foreach (var range in ordered.AsSpan(1))
            {
                if (range.Start <= current.End + 1)
                {
                    current = new ScalarRange(
                        current.Start,
                        Math.Max(current.End, range.End));
                }
                else
                {
                    merged.Add(current);
                    current = range;
                }
            }
            merged.Add(current);
            return merged.ToArray();
        }
    }

    private sealed class Parser
    {
        private readonly Rune[] _source;
        private int _index;
        private int _nodes;

        internal Parser(string source)
        {
            var sourceBytes = Encoding.UTF8.GetByteCount(source);
            if (sourceBytes > MaximumSourceBytes)
            {
                throw Boundary(
                    $"JSON pattern is {sourceBytes} bytes; maximum is {MaximumSourceBytes}.");
            }
            _source = DecodePatternScalars(source);
        }

        internal Expression Parse()
        {
            var expression = ParseAlternation(0);
            if (!AtEnd)
            {
                throw Invalid($"unexpected '{Display(Current)}'");
            }
            return expression;
        }

        private Expression ParseAlternation(int depth)
        {
            RequireDepth(depth);
            var branches = new List<Expression>
            {
                ParseSequence(depth),
            };
            while (Take('|'))
            {
                branches.Add(ParseSequence(depth));
            }
            return branches.Count == 1
                ? branches[0]
                : Count(new AlternationExpression(branches));
        }

        private Expression ParseSequence(int depth)
        {
            var items = new List<Expression>();
            while (!AtEnd && Current.Value is not (')' or '|'))
            {
                items.Add(ParsePiece(depth));
            }
            return items.Count switch
            {
                0 => Count(new EmptyExpression()),
                1 => items[0],
                _ => Count(new SequenceExpression(items)),
            };
        }

        private Expression ParsePiece(int depth)
        {
            var item = ParseAtom(depth);
            if (AtEnd || !IsQuantifierStart(Current.Value))
            {
                return item;
            }
            if (item is StartExpression or EndExpression)
            {
                throw Invalid("assertions cannot be quantified");
            }

            var (minimum, maximum) = ParseQuantifier();
            _ = Take('?');
            if (!AtEnd && IsQuantifierStart(Current.Value))
            {
                throw Invalid("an atom cannot have more than one quantifier");
            }
            return Count(new RepeatExpression(item, minimum, maximum));
        }

        private Expression ParseAtom(int depth)
        {
            if (AtEnd)
            {
                throw Invalid("expected an expression");
            }

            var current = Current;
            _index++;
            return current.Value switch
            {
                '.' => Count(new CharacterExpression(CharacterSet.Dot())),
                '^' => Count(new StartExpression()),
                '$' => Count(new EndExpression()),
                '(' => ParseGroup(depth),
                '[' => ParseClass(),
                '\\' => Count(new CharacterExpression(CharacterSet.Single(ParseEscape()))),
                '*' or '+' or '?' or '{' =>
                    throw Invalid("quantifier has no preceding atom"),
                ')' or '|' or ']' or '}' =>
                    throw Invalid($"unexpected '{Display(current)}'"),
                _ => Count(new CharacterExpression(CharacterSet.Single(current))),
            };
        }

        private Expression ParseGroup(int depth)
        {
            RequireDepth(depth + 1);
            if (Take('?'))
            {
                if (!Take(':'))
                {
                    throw Invalid(
                        "lookaround, named groups, and inline flags are unsupported");
                }
            }
            var expression = ParseAlternation(depth + 1);
            if (!Take(')'))
            {
                throw Invalid("group is missing ')'");
            }
            return Count(new GroupExpression(expression));
        }

        private Expression ParseClass()
        {
            var complemented = Take('^');
            var ranges = new List<ScalarRange>();
            var hasPriorMember = false;
            while (!AtEnd && Current.Value != ']')
            {
                RejectClassSetOperation();
                if (Current.Value == '-' &&
                    hasPriorMember &&
                    PeekValue(1) != ']')
                {
                    throw Invalid("raw '-' is only valid first or last in a character class");
                }
                var start = ParseClassScalar();
                if (!AtEnd &&
                    Current.Value == '-' &&
                    PeekValue(1) is not (null or ']'))
                {
                    RejectClassSetOperation();
                    _index++;
                    var end = ParseClassScalar();
                    if (start.Value > end.Value)
                    {
                        throw Invalid("character class range is reversed");
                    }
                    ranges.Add(new ScalarRange(start.Value, end.Value));
                }
                else
                {
                    ranges.Add(new ScalarRange(start.Value, start.Value));
                }
                hasPriorMember = true;
            }
            if (!Take(']'))
            {
                throw Invalid("character class is missing ']'");
            }
            return Count(new CharacterExpression(new CharacterSet(ranges, complemented)));
        }

        private Rune ParseClassScalar()
        {
            if (AtEnd || Current.Value == ']')
            {
                throw Invalid("character class item is missing");
            }
            var value = Current;
            _index++;
            if (value.Value == '\\')
            {
                return ParseEscape();
            }
            if (value.Value == '[')
            {
                throw Invalid("nested character classes are unsupported");
            }
            return value;
        }

        private void RejectClassSetOperation()
        {
            if (PeekValue(0) is '&' or '-' or '~' or '|' &&
                PeekValue(1) == PeekValue(0))
            {
                throw Invalid("character class set operations are unsupported");
            }
        }

        private (int Minimum, int? Maximum) ParseQuantifier()
        {
            if (Take('*'))
            {
                return (0, null);
            }
            if (Take('+'))
            {
                return (1, null);
            }
            if (Take('?'))
            {
                return (0, 1);
            }
            if (!Take('{'))
            {
                throw new InvalidOperationException("Expected a JSON pattern quantifier.");
            }

            var minimum = ParseRepetitionCount();
            if (Take('}'))
            {
                return (minimum, minimum);
            }
            if (!Take(','))
            {
                throw Invalid("bounded quantifier must contain ',' or '}'");
            }
            if (Take('}'))
            {
                return (minimum, null);
            }
            var maximum = ParseRepetitionCount();
            if (!Take('}'))
            {
                throw Invalid("bounded quantifier is missing '}'");
            }
            if (maximum < minimum)
            {
                throw Invalid("bounded quantifier range is reversed");
            }
            return (minimum, maximum);
        }

        private int ParseRepetitionCount()
        {
            if (AtEnd || !IsAsciiDigit(Current.Value))
            {
                throw Invalid("bounded quantifier requires an ASCII decimal count");
            }

            ulong value = 0;
            while (!AtEnd && IsAsciiDigit(Current.Value))
            {
                var digit = checked((uint)(Current.Value - '0'));
                if (value > ((ulong)MaximumInstructions - digit) / 10)
                {
                    throw Invalid(
                        $"bounded quantifier exceeds the {MaximumInstructions}-instruction limit");
                }
                value = (value * 10) + digit;
                _index++;
            }
            return checked((int)value);
        }

        private Rune ParseEscape()
        {
            if (AtEnd)
            {
                throw Invalid("escape is missing its value");
            }
            var escaped = Current;
            _index++;
            return escaped.Value switch
            {
                '^' or '$' or '\\' or '.' or '*' or '+' or '?' or '(' or ')' or
                    '[' or ']' or '{' or '}' or '|' or '/' => escaped,
                'n' => new Rune('\n'),
                'r' => new Rune('\r'),
                't' => new Rune('\t'),
                'f' => new Rune('\f'),
                'v' => new Rune('\v'),
                '0' => ParseNullEscape(),
                'x' => ParseFixedHexEscape(2, "x"),
                'u' => ParseUnicodeEscape(),
                'd' or 'D' or 's' or 'S' or 'w' or 'W' =>
                    throw Invalid("shorthand character classes are unsupported"),
                'p' or 'P' =>
                    throw Invalid("Unicode property escapes are unsupported"),
                'k' =>
                    throw Invalid("named backreferences are unsupported"),
                >= '1' and <= '9' =>
                    throw Invalid("backreferences and octal escapes are unsupported"),
                'c' =>
                    throw Invalid("control-letter escapes are unsupported"),
                _ => throw Invalid($"unsupported escape '\\{Display(escaped)}'"),
            };
        }

        private Rune ParseNullEscape()
        {
            if (!AtEnd && IsAsciiDigit(Current.Value))
            {
                throw Invalid("octal escapes are unsupported");
            }
            return new Rune(0);
        }

        private Rune ParseFixedHexEscape(int digits, string name)
        {
            var value = ParseHexDigits(digits, $"\\{name}");
            return new Rune(value);
        }

        private Rune ParseUnicodeEscape()
        {
            if (Take('{'))
            {
                var start = _index;
                var bracedValue = 0;
                while (!AtEnd && Current.Value != '}')
                {
                    if (_index - start >= 6 || !TryHexValue(Current.Value, out var digit))
                    {
                        throw Invalid("\\u{...} requires one to six hexadecimal digits");
                    }
                    bracedValue = checked((bracedValue * 16) + digit);
                    _index++;
                }
                if (_index == start || !Take('}'))
                {
                    throw Invalid("\\u{...} requires one to six hexadecimal digits and '}'");
                }
                if (!Rune.IsValid(bracedValue))
                {
                    throw Invalid("Unicode escape is not a Unicode scalar value");
                }
                return new Rune(bracedValue);
            }

            var value = ParseHexDigits(4, "\\u");
            if (value is >= 0xDC00 and <= 0xDFFF)
            {
                throw Invalid("Unicode escape contains an unpaired low surrogate");
            }
            if (value is not (>= 0xD800 and <= 0xDBFF))
            {
                return new Rune(value);
            }

            if (!Take('\\') || !Take('u'))
            {
                throw Invalid("Unicode escape contains an unpaired high surrogate");
            }
            var low = ParseHexDigits(4, "\\u");
            if (low is not (>= 0xDC00 and <= 0xDFFF))
            {
                throw Invalid("Unicode escape contains an unpaired high surrogate");
            }
            var scalar = 0x10000 + ((value - 0xD800) << 10) + (low - 0xDC00);
            return new Rune(scalar);
        }

        private int ParseHexDigits(int count, string name)
        {
            var value = 0;
            for (var offset = 0; offset < count; offset++)
            {
                if (AtEnd || !TryHexValue(Current.Value, out var digit))
                {
                    throw Invalid($"{name} escape requires exactly {count} hexadecimal digits");
                }
                value = checked((value * 16) + digit);
                _index++;
            }
            return value;
        }

        private Expression Count(Expression expression)
        {
            _nodes = checked(_nodes + 1);
            if (_nodes > MaximumAstNodes)
            {
                throw Invalid($"pattern exceeds the {MaximumAstNodes}-node syntax limit");
            }
            return expression;
        }

        private static bool IsQuantifierStart(int value) =>
            value is '*' or '+' or '?' or '{';

        private static bool IsAsciiDigit(int value) => value is >= '0' and <= '9';

        private static bool TryHexValue(int value, out int digit)
        {
            digit = value switch
            {
                >= '0' and <= '9' => value - '0',
                >= 'a' and <= 'f' => value - 'a' + 10,
                >= 'A' and <= 'F' => value - 'A' + 10,
                _ => -1,
            };
            return digit >= 0;
        }

        private void RequireDepth(int depth)
        {
            if (depth > MaximumParserDepth)
            {
                throw Invalid($"pattern nesting exceeds {MaximumParserDepth} levels");
            }
        }

        private bool Take(int expected)
        {
            if (!AtEnd && Current.Value == expected)
            {
                _index++;
                return true;
            }
            return false;
        }

        private int? PeekValue(int offset)
        {
            var index = _index + offset;
            return index < _source.Length ? _source[index].Value : null;
        }

        private bool AtEnd => _index >= _source.Length;

        private Rune Current => _source[_index];

        private static string Display(Rune value) =>
            value.Value == 0
                ? "\\0"
                : value.ToString();

        private static FerruleRuntimeException Invalid(string detail) =>
            Boundary($"Embedded JSON schema pattern is invalid: {detail}.");
    }

    private enum Operation
    {
        Consume,
        Split,
        Jump,
        AssertStart,
        AssertEnd,
        Match,
    }

    private sealed class Instruction
    {
        internal Instruction(
            Operation operation,
            CharacterSet? characterSet = null,
            int first = -1,
            int second = -1)
        {
            Operation = operation;
            CharacterSet = characterSet;
            First = first;
            Second = second;
        }

        internal Operation Operation { get; }

        internal CharacterSet? CharacterSet { get; }

        internal int First { get; set; }

        internal int Second { get; set; }
    }

    private readonly record struct Patch(int Instruction, bool Second);

    private sealed record Fragment(int Start, List<Patch> Outputs);

    private sealed class Compiler
    {
        private readonly List<Instruction> _instructions = [];

        internal FerruleJsonPattern Compile(Expression expression)
        {
            var fragment = CompileExpression(expression);
            var accept = Emit(new Instruction(Operation.Match));
            PatchOutputs(fragment.Outputs, accept);
            return new FerruleJsonPattern(_instructions.ToArray(), fragment.Start);
        }

        private Fragment CompileExpression(Expression expression) => expression switch
        {
            EmptyExpression => Epsilon(),
            CharacterExpression character => Consuming(character.CharacterSet),
            StartExpression => Assertion(Operation.AssertStart),
            EndExpression => Assertion(Operation.AssertEnd),
            GroupExpression group => CompileExpression(group.Item),
            SequenceExpression sequence => CompileSequence(sequence.Items),
            AlternationExpression alternation => CompileAlternation(alternation.Branches),
            RepeatExpression repeat => CompileRepeat(repeat),
            _ => throw new InvalidOperationException("Unknown JSON pattern expression."),
        };

        private Fragment Epsilon()
        {
            var instruction = Emit(new Instruction(Operation.Jump));
            return new Fragment(instruction, [new Patch(instruction, false)]);
        }

        private Fragment Consuming(CharacterSet characterSet)
        {
            var instruction = Emit(new Instruction(Operation.Consume, characterSet));
            return new Fragment(instruction, [new Patch(instruction, false)]);
        }

        private Fragment Assertion(Operation operation)
        {
            var instruction = Emit(new Instruction(operation));
            return new Fragment(instruction, [new Patch(instruction, false)]);
        }

        private Fragment CompileSequence(IReadOnlyList<Expression> items)
        {
            if (items.Count == 0)
            {
                return Epsilon();
            }
            var result = CompileExpression(items[0]);
            for (var index = 1; index < items.Count; index++)
            {
                result = Concatenate(result, CompileExpression(items[index]));
            }
            return result;
        }

        private Fragment CompileAlternation(IReadOnlyList<Expression> branches)
        {
            if (branches.Count == 0)
            {
                return Epsilon();
            }
            var result = CompileExpression(branches[0]);
            for (var index = 1; index < branches.Count; index++)
            {
                var right = CompileExpression(branches[index]);
                var split = Emit(
                    new Instruction(
                        Operation.Split,
                        first: result.Start,
                        second: right.Start));
                var outputs = new List<Patch>(result.Outputs.Count + right.Outputs.Count);
                outputs.AddRange(result.Outputs);
                outputs.AddRange(right.Outputs);
                result = new Fragment(split, outputs);
            }
            return result;
        }

        private Fragment CompileRepeat(RepeatExpression repeat)
        {
            Fragment? result = null;
            for (var index = 0; index < repeat.Minimum; index++)
            {
                result = Append(result, CompileExpression(repeat.Item));
            }

            if (repeat.Maximum is null)
            {
                var repeating = Star(CompileExpression(repeat.Item));
                return Append(result, repeating);
            }

            for (var index = repeat.Minimum; index < repeat.Maximum.Value; index++)
            {
                result = Append(result, Optional(CompileExpression(repeat.Item)));
            }
            return result ?? Epsilon();
        }

        private Fragment Star(Fragment item)
        {
            var split = Emit(
                new Instruction(
                    Operation.Split,
                    first: item.Start));
            PatchOutputs(item.Outputs, split);
            return new Fragment(split, [new Patch(split, true)]);
        }

        private Fragment Optional(Fragment item)
        {
            var split = Emit(
                new Instruction(
                    Operation.Split,
                    first: item.Start));
            var outputs = new List<Patch>(item.Outputs.Count + 1);
            outputs.AddRange(item.Outputs);
            outputs.Add(new Patch(split, true));
            return new Fragment(split, outputs);
        }

        private Fragment Append(Fragment? left, Fragment right)
        {
            if (left is null)
            {
                return right;
            }
            PatchOutputs(left.Outputs, right.Start);
            return new Fragment(left.Start, right.Outputs);
        }

        private Fragment Concatenate(Fragment left, Fragment right)
        {
            PatchOutputs(left.Outputs, right.Start);
            return new Fragment(left.Start, right.Outputs);
        }

        private int Emit(Instruction instruction)
        {
            if (_instructions.Count >= MaximumInstructions)
            {
                throw Boundary(
                    $"Embedded JSON schema pattern exceeds the {MaximumInstructions}-instruction limit.");
            }
            _instructions.Add(instruction);
            return _instructions.Count - 1;
        }

        private void PatchOutputs(IEnumerable<Patch> outputs, int target)
        {
            foreach (var patch in outputs)
            {
                var instruction = _instructions[patch.Instruction];
                if (patch.Second)
                {
                    instruction.Second = target;
                }
                else
                {
                    instruction.First = target;
                }
            }
        }
    }

    private sealed class StateSet
    {
        private readonly bool[] _visited;
        private readonly List<int> _states = [];
        private readonly Stack<int> _pending = [];

        internal StateSet(int instructionCount)
        {
            _visited = new bool[instructionCount];
        }

        internal IReadOnlyList<int> States => _states;

        internal bool TryVisit(int state)
        {
            if (state < 0 || state >= _visited.Length)
            {
                throw new InvalidOperationException("JSON pattern instruction target is invalid.");
            }
            if (_visited[state])
            {
                return false;
            }
            _visited[state] = true;
            return true;
        }

        internal void Add(int state) => _states.Add(state);

        internal void PushPending(int state) => _pending.Push(state);

        internal bool TryPopPending(out int state) => _pending.TryPop(out state);

        internal void Reset()
        {
            Array.Clear(_visited);
            _states.Clear();
            _pending.Clear();
        }

        internal bool ContainsMatch(IReadOnlyList<Instruction> instructions) =>
            _states.Any(state => instructions[state].Operation == Operation.Match);
    }
}
