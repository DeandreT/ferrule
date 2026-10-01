using System.Text;
using System.Text.RegularExpressions;

namespace Ferrule.Runtime;

internal static partial class FerruleScalarRegex
{
    private const long MaximumBoundaryWork = 100_000_000;
    private const long MaximumBoundaryExecutionBytes = 64 * 1024 * 1024;

    internal sealed class WorkLimitException : Exception
    {
        internal WorkLimitException() : base("word-boundary regex exceeds its work limit") { }
    }

    internal sealed class BoundaryWork
    {
        private long _remaining = MaximumBoundaryWork;
        internal void Charge(long count = 1)
        {
            if (count > _remaining) { throw new WorkLimitException(); }
            _remaining -= count;
        }
    }

    internal readonly record struct ScalarCapture(string Input, int Start, int End)
    {
        internal bool Success => Start >= 0 && End >= Start;
        internal string Value => Success ? Input[Start..End] : string.Empty;
    }

    internal sealed record ScalarMatch(int Index, int Length, ScalarCapture[] Groups);

    internal sealed class ScalarRegexProgram
    {
        private readonly Regex? _host;
        private readonly int[]? _hostGroups;
        private readonly BoundaryInstruction[]? _instructions;
        private readonly int _start;
        internal int CaptureCount { get; }
        internal bool UsesBoundaryMachine => _instructions is not null;

        internal static ScalarRegexProgram Host(Regex host, int[] groups) => new(host, groups);
        internal static ScalarRegexProgram Boundary(string source, RegexOptions options, bool strictUnicodeProfile = false)
        {
            var (root, captures) = new Translator(source, options, strictUnicodeProfile).ParseBoundary();
            return FromBoundary(root, captures);
        }

        internal static ScalarRegexProgram? TryNullableCaptures(string source, RegexOptions options)
        {
            BoundaryNode root;
            int captures;
            try
            {
                // This fresh, bounded parser has no state in common with the
                // completed host translation. Unsupported/deep ordinary host
                // patterns retain their existing host constructor and errors.
                (root, captures) = new Translator(source, options).ParseBoundary();
            }
            catch (Exception error) when (error is ArgumentException or NotSupportedException)
            {
                return null;
            }
            // Ordinary host patterns outside the selected Rust syntax depth
            // keep their legacy acceptance and capture behavior.
            if (!root.HasNullableCaptureLoop || root.SyntaxHeight > 250) { return null; }
            // The eligible trigger is now established. VM capacity failures
            // must stay typed; compiling outside the speculative catch keeps
            // those limits from being bypassed through the host path.
            return FromBoundary(root, captures);
        }

        private static ScalarRegexProgram FromBoundary(BoundaryNode root, int captures)
        {
            var (instructions, start) = new BoundaryCompiler().Compile(root);
            return new(instructions, start, captures);
        }

        private ScalarRegexProgram(Regex host, int[] groups)
        { _host = host; _hostGroups = groups; CaptureCount = groups.Length; }
        private ScalarRegexProgram(BoundaryInstruction[] instructions, int start, int captures)
        {
            _instructions = instructions; _start = start; CaptureCount = captures + 1;
            RequireExecutionCapacity(instructions.Length, CaptureCount * 2);
        }

        internal bool IsMatch(string input, BoundaryWork? work = null)
        {
            if (_host is not null) { return _host.IsMatch(input); }
            return new BoundaryRunner(_instructions!, _start, 2, work ?? new BoundaryWork()).Find(input, 0, true) is not null;
        }

        internal void VisitMatches(string input, Action<int, int> visit, BoundaryWork? work = null)
        {
            if (_host is not null)
            {
                foreach (var match in _host.EnumerateMatches(input)) { visit(match.Index, match.Length); }
                return;
            }
            foreach (var tags in BoundaryMatches(input, 2, work ?? new BoundaryWork())) { visit(tags[0], tags[1] - tags[0]); }
        }

        internal IEnumerable<ScalarMatch> Matches(string input, BoundaryWork? work = null)
        {
            if (_host is not null)
            {
                foreach (Match match in _host.Matches(input))
                {
                    var groups = _hostGroups!.Select(index => {
                        var group = match.Groups[index];
                        return new ScalarCapture(input, group.Success ? group.Index : -1,
                            group.Success ? group.Index + group.Length : -1);
                    }).ToArray();
                    yield return new(match.Index, match.Length, groups);
                }
                yield break;
            }
            foreach (var tags in BoundaryMatches(input, CaptureCount * 2, work ?? new BoundaryWork()))
            {
                var groups = new ScalarCapture[CaptureCount];
                for (var index = 0; index < groups.Length; index++)
                {
                    groups[index] = new(input, tags[index * 2], tags[index * 2 + 1]);
                }
                yield return new(tags[0], tags[1] - tags[0], groups);
            }
        }

        private IEnumerable<int[]> BoundaryMatches(string input, int slots, BoundaryWork work)
        {
            var runner = new BoundaryRunner(_instructions!, _start, slots, work);
            var position = 0;
            var lastEnd = -1;
            while (position <= input.Length)
            {
                var tags = runner.Find(input, position, false);
                if (tags is null) { yield break; }
                var first = tags[0];
                var last = tags[1];
                // Native search iteration suppresses an empty match directly
                // adjoining a preceding match, while still searching later.
                if (first != last || first != lastEnd)
                {
                    yield return tags;
                    lastEnd = last;
                }
                if (first != last) { position = last; }
                else if (last == input.Length) { yield break; }
                else { position = last + Rune.GetRuneAt(input, last).Utf16SequenceLength; }
            }
        }
    }

    internal static ScalarRegexProgram CompileProgram(string source, RegexOptions options)
    {
        if (new Translator(source, options).ContainsExplicitUnicode())
        { return ScalarRegexProgram.Boundary(source, options, true); }
        var translator = new Translator(source, options);
        var translated = translator.Translate();
        if (translator.HasWordBoundary || translator.HasConsecutiveRepetition || translator.HasExplicitUnicode)
        {
            return ScalarRegexProgram.Boundary(source, options, translator.HasExplicitUnicode);
        }
        if (translator.CanProbeNullableCaptures)
        {
            var captures = ScalarRegexProgram.TryNullableCaptures(source, options);
            if (captures is not null) { return captures; }
        }
        var host = new Regex("(?m:^|)(?:" + translated + ")", options);
        return ScalarRegexProgram.Host(host, translator.CaptureGroups(host));
    }

    private static void RequireExecutionCapacity(int instructions, int slots)
    {
        // Two tag matrices, epoch/state arrays and the bounded epsilon stack.
        // This is a local execution cap, checked before any matrix allocation.
        var bytes = (long)instructions * (slots * 8L + 64L) + slots * 8L;
        if (bytes > MaximumBoundaryExecutionBytes)
        {
            throw Invalid("word-boundary regex exceeds its capture-memory limit");
        }
    }

    private sealed class BoundaryStates
    {
        internal readonly int[] Tags;
        internal readonly int[] States;
        private readonly int[] _visited;
        private int _epoch;
        internal int Count;

        internal BoundaryStates(int count, int slots)
        {
            Tags = new int[checked(count * slots)];
            States = new int[count];
            _visited = new int[count];
        }

        internal void Reset()
        {
            Count = 0;
            if (_epoch == int.MaxValue) { Array.Clear(_visited); _epoch = 0; }
            _epoch++;
        }
        internal bool Visit(int instruction)
        {
            if (_visited[instruction] == _epoch) { return false; }
            _visited[instruction] = _epoch;
            return true;
        }
    }

    private readonly record struct BoundaryPending(int Instruction, int[] Tags, int Offset);

    private sealed class BoundaryRunner
    {
        private readonly BoundaryInstruction[] _instructions;
        private readonly int _start;
        private readonly int _slots;
        private readonly BoundaryWork _work;
        private BoundaryStates _current;
        private BoundaryStates _next;
        private readonly Stack<BoundaryPending> _pending;
        private readonly int[] _initial;
        private readonly int[] _candidate;

        internal BoundaryRunner(BoundaryInstruction[] instructions, int start, int slots, BoundaryWork work)
        {
            RequireExecutionCapacity(instructions.Length, slots);
            _instructions = instructions; _start = start; _slots = slots; _work = work;
            _current = new(instructions.Length, slots); _next = new(instructions.Length, slots);
            _pending = new Stack<BoundaryPending>(checked(instructions.Length * 2));
            _initial = new int[slots]; _candidate = new int[slots];
            Array.Fill(_initial, -1);
        }

        internal int[]? Find(string input, int from, bool boolean)
        {
            _current.Reset(); _next.Reset();
            var position = from;
            var found = false;
            while (true)
            {
                if (!found)
                {
                    _initial[0] = position;
                    Closure(_current, _start, _initial, 0, input, position);
                }
                var active = _current.Count;
                for (var index = 0; index < active; index++)
                {
                    var state = _current.States[index];
                    if (_instructions[state].Op != BoundaryOp.Match) { continue; }
                    if (boolean) { return _candidate; }
                    Array.Copy(_current.Tags, state * _slots, _candidate, 0, _slots);
                    _work.Charge(_slots);
                    _candidate[1] = position;
                    found = true;
                    active = index; // Keep only paths ranked ahead of this match.
                    break;
                }
                if (position == input.Length || active == 0 && found)
                {
                    return found ? _candidate : null;
                }
                var rune = Rune.GetRuneAt(input, position);
                var following = position + rune.Utf16SequenceLength;
                _next.Reset();
                for (var index = 0; index < active; index++)
                {
                    var state = _current.States[index];
                    var instruction = _instructions[state];
                    _work.Charge();
                    if (instruction.Set!.Contains(rune.Value))
                    {
                        Closure(_next, instruction.Next, _current.Tags, state * _slots, input, following);
                    }
                }
                (_current, _next) = (_next, _current);
                position = following;
            }
        }

        private void Closure(BoundaryStates states, int start, int[] tags, int offset, string input, int position)
        {
            _pending.Push(new(start, tags, offset));
            while (_pending.TryPop(out var pending))
            {
                _work.Charge();
                if (!states.Visit(pending.Instruction)) { continue; }
                var row = pending.Instruction * _slots;
                Array.Copy(pending.Tags, pending.Offset, states.Tags, row, _slots);
                _work.Charge(_slots);
                var instruction = _instructions[pending.Instruction];
                switch (instruction.Op)
                {
                    case BoundaryOp.Jump:
                        _pending.Push(new(instruction.Next, states.Tags, row)); break;
                    case BoundaryOp.Split:
                        _pending.Push(new(instruction.Other, states.Tags, row));
                        _pending.Push(new(instruction.Next, states.Tags, row)); break;
                    case BoundaryOp.Tag:
                        if (instruction.Tag < _slots) { states.Tags[row + instruction.Tag] = position; }
                        _pending.Push(new(instruction.Next, states.Tags, row)); break;
                    case BoundaryOp.Assert:
                        if (Assertion(instruction.Assertion, input, position))
                        {
                            _pending.Push(new(instruction.Next, states.Tags, row));
                        }
                        break;
                    default:
                        states.States[states.Count++] = pending.Instruction; break;
                }
            }
        }

        private static bool Assertion(BoundaryAssertion assertion, string input, int position)
        {
            return assertion switch {
                BoundaryAssertion.Start => position == 0,
                BoundaryAssertion.End => position == input.Length,
                BoundaryAssertion.LineStart => position == 0 || input[position - 1] == '\n',
                BoundaryAssertion.LineEnd => position == input.Length || input[position] == '\n',
                BoundaryAssertion.Word => WordBefore(input, position) != WordAfter(input, position),
                BoundaryAssertion.NotWord => WordBefore(input, position) == WordAfter(input, position),
                BoundaryAssertion.WordStart => !WordBefore(input, position) && WordAfter(input, position),
                BoundaryAssertion.WordEnd => WordBefore(input, position) && !WordAfter(input, position),
                BoundaryAssertion.WordStartHalf => !WordBefore(input, position),
                BoundaryAssertion.WordEndHalf => !WordAfter(input, position),
                BoundaryAssertion.AsciiWord => WordBefore(input, position, true) != WordAfter(input, position, true),
                BoundaryAssertion.AsciiNotWord => WordBefore(input, position, true) == WordAfter(input, position, true),
                BoundaryAssertion.AsciiWordStart => !WordBefore(input, position, true) && WordAfter(input, position, true),
                BoundaryAssertion.AsciiWordEnd => WordBefore(input, position, true) && !WordAfter(input, position, true),
                BoundaryAssertion.AsciiWordStartHalf => !WordBefore(input, position, true),
                BoundaryAssertion.AsciiWordEndHalf => !WordAfter(input, position, true),
                _ => throw Invalid("invalid scalar word assertion"),
            };
        }

        private static bool WordBefore(string input, int position, bool ascii = false)
        {
            if (position == 0) { return false; }
            var start = position - 1;
            if (char.IsLowSurrogate(input[start])) { start--; }
            return (ascii ? AsciiClasses["word"] : Shorthand('w')).Contains(Rune.GetRuneAt(input, start).Value);
        }
        private static bool WordAfter(string input, int position, bool ascii = false) => position < input.Length
            && (ascii ? AsciiClasses["word"] : Shorthand('w')).Contains(Rune.GetRuneAt(input, position).Value);
    }
}
