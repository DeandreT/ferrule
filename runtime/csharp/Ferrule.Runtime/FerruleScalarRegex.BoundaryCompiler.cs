namespace Ferrule.Runtime;

internal static partial class FerruleScalarRegex
{
    private const int MaximumBoundaryInstructions = 10 * 1024 * 1024 / 64;
    private enum BoundaryOp { Consume, Split, Jump, Tag, Assert, Match }
    private sealed class BoundaryInstruction
    {
        internal readonly BoundaryOp Op;
        internal readonly ScalarSet? Set;
        internal readonly BoundaryAssertion Assertion;
        internal readonly int Tag;
        internal int Next = -1;
        internal int Other = -1;
        internal BoundaryInstruction(BoundaryOp op, ScalarSet? set = null,
            BoundaryAssertion assertion = default, int tag = 0)
        { Op = op; Set = set; Assertion = assertion; Tag = tag; }
    }
    private readonly record struct BoundaryPatch(int Instruction, bool Other = false);
    private sealed record BoundaryFragment(int Start, List<BoundaryPatch> Exits);

    private sealed class BoundaryCompiler
    {
        private readonly List<BoundaryInstruction> _instructions = new();

        internal (BoundaryInstruction[] Instructions, int Start) Compile(BoundaryNode root)
        {
            // Account for expansion before allocating repeated bodies. The
            // instructions share immutable scalar sets instead of cloning them.
            Estimate(root);
            var fragment = Build(root);
            var terminal = Emit(new(BoundaryOp.Match));
            Patch(fragment.Exits, terminal);
            return (_instructions.ToArray(), fragment.Start);
        }

        private static long Estimate(BoundaryNode node)
        {
            long count = node.Kind switch {
                BoundaryKind.Sequence => node.Children!.Sum(Estimate),
                BoundaryKind.Alternate => node.Children!.Sum(Estimate) + node.Children!.Length - 1,
                BoundaryKind.Capture => Estimate(node.Children![0]) + 2,
                BoundaryKind.Repeat when node.Maximum == 0 => 1,
                BoundaryKind.Repeat => Estimate(node.Children![0])
                    * (node.Maximum.HasValue ? node.Maximum.Value : Math.Max(1L, node.Minimum))
                    + (node.Maximum.HasValue ? (long)node.Maximum.Value - node.Minimum
                        : node.Minimum == 0 && Nullable(node.Children[0]) ? 2 : 1),
                _ => 1,
            };
            if (count >= MaximumBoundaryInstructions)
            {
                throw Invalid("word-boundary regex exceeds its compiled instruction limit");
            }
            return count;
        }

        private int Emit(BoundaryInstruction instruction)
        {
            if (_instructions.Count == MaximumBoundaryInstructions)
            {
                throw Invalid("word-boundary regex exceeds its compiled instruction limit");
            }
            _instructions.Add(instruction);
            return _instructions.Count - 1;
        }

        private void Patch(IEnumerable<BoundaryPatch> patches, int destination)
        {
            foreach (var patch in patches)
            {
                if (patch.Other) { _instructions[patch.Instruction].Other = destination; }
                else { _instructions[patch.Instruction].Next = destination; }
            }
        }

        private BoundaryFragment Single(BoundaryInstruction instruction)
        {
            var index = Emit(instruction);
            return new(index, new() { new(index) });
        }

        private BoundaryFragment Concatenate(BoundaryFragment? left, BoundaryFragment right)
        {
            if (left is null) { return right; }
            Patch(left.Exits, right.Start);
            return new(left.Start, right.Exits);
        }

        private BoundaryFragment Build(BoundaryNode node)
        {
            switch (node.Kind)
            {
                case BoundaryKind.Empty:
                    return Single(new(BoundaryOp.Jump));
                case BoundaryKind.Consume:
                    return Single(new(BoundaryOp.Consume, node.Set));
                case BoundaryKind.Assertion:
                    return Single(new(BoundaryOp.Assert, assertion: node.Assertion));
                case BoundaryKind.Capture:
                    var before = Single(new(BoundaryOp.Tag, tag: node.Capture * 2));
                    var body = Concatenate(before, Build(node.Children![0]));
                    return Concatenate(body, Single(new(BoundaryOp.Tag, tag: node.Capture * 2 + 1)));
                case BoundaryKind.Sequence:
                    BoundaryFragment? sequence = null;
                    foreach (var child in node.Children!) { sequence = Concatenate(sequence, Build(child)); }
                    return sequence ?? Single(new(BoundaryOp.Jump));
                case BoundaryKind.Alternate:
                    var alternatives = node.Children!;
                    var result = Build(alternatives[^1]);
                    for (var index = alternatives.Length - 2; index >= 0; index--)
                    {
                        var left = Build(alternatives[index]);
                        var split = Emit(new(BoundaryOp.Split) { Next = left.Start, Other = result.Start });
                        left.Exits.AddRange(result.Exits);
                        result = new(split, left.Exits);
                    }
                    return result;
                case BoundaryKind.Repeat:
                    return Repetition(node);
                default: throw Invalid("invalid boundary regex AST");
            }
        }

        private static bool Nullable(BoundaryNode node) => node.Kind switch {
            BoundaryKind.Consume => false,
            BoundaryKind.Sequence => node.Children!.All(Nullable),
            BoundaryKind.Alternate => node.Children!.Any(Nullable),
            BoundaryKind.Capture => Nullable(node.Children![0]),
            BoundaryKind.Repeat => node.Minimum == 0 || Nullable(node.Children![0]),
            _ => true,
        };

        private BoundaryFragment Repetition(BoundaryNode node)
        {
            BoundaryFragment? result = null;
            if (!node.Maximum.HasValue)
            {
                for (uint index = 1; index < node.Minimum; index++)
                {
                    result = Concatenate(result, Build(node.Children![0]));
                }
                var body = Build(node.Children![0]);
                var split = Emit(new(BoundaryOp.Split));
                var exit = new BoundaryPatch(split, !node.Lazy);
                if (node.Lazy) { _instructions[split].Other = body.Start; }
                else { _instructions[split].Next = body.Start; }
                Patch(body.Exits, split);
                if (node.Minimum != 0)
                {
                    return Concatenate(result, new(body.Start, new() { exit }));
                }
                if (!Nullable(node.Children[0])) { return new(split, new() { exit }); }
                // A nullable star needs a distinct initial optional branch.
                // Otherwise first-visit epsilon dedup can skip its first empty
                // capture or prefer a lower-ranked consuming alternative.
                var optional = Emit(new(BoundaryOp.Split));
                if (node.Lazy) { _instructions[optional].Other = body.Start; }
                else { _instructions[optional].Next = body.Start; }
                return new(optional, new() { exit, new(optional, !node.Lazy) });
            }
            for (uint index = 0; index < node.Minimum; index++)
            {
                result = Concatenate(result, Build(node.Children![0]));
            }
            for (var index = node.Minimum; index < node.Maximum.Value; index++)
            {
                var body = Build(node.Children![0]);
                var split = Emit(new(BoundaryOp.Split));
                if (node.Lazy) { _instructions[split].Other = body.Start; }
                else { _instructions[split].Next = body.Start; }
                body.Exits.Add(new(split, !node.Lazy));
                result = Concatenate(result, new(split, body.Exits));
            }
            return result ?? Single(new(BoundaryOp.Jump));
        }
    }
}
